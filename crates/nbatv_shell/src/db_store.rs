//! Archive-backed catalog for the Shell (#19).
//!
//! The live Shell reads the real SQLite archive through [`DbStore`]: every
//! season list, game ledger, Box Score, and Play dispatch resolves through
//! `nbatv_db` queries. [`Store`] is the seam the views (and headless tests)
//! program against — [`Store::Db`] in production, [`Store::Fixture`] for
//! tests and offline development only. Fixtures seed nothing in the live
//! path.
//!
//! Degradation is honest at every layer: an unreadable query degrades to
//! its empty value (empty season list, `None` box, `Unavailable` dispatch),
//! never a panic. Query-time errors are silent by design — the empty UI
//! state IS the surfacing — while open-time failures log once to stderr.
//!
//! Known Wave-1 limits (later catalog slices own these):
//! - Season slugs derive from the ending year alone (`1947` → `1946-47`),
//!   so two leagues sharing a year would share a slug and their games mix
//!   under it. The current archive is single-league.
//! - List views fetch one `tape_sources` plus one sweep-status query per
//!   game. Local SQLite makes this trivial at archive scale; a later slice
//!   can batch it.
//! - `NBA_CUP` games skate in the Regular-season ledger (they count as
//!   regular-season games bar the final).
//! - Tape banners derive from stored sweep evidence (`game_queries` plus
//!   `tape_sources`, see `nbatv_catalog`): a game with stream tape rows is
//!   Playable whatever the query history says; otherwise the stored sweep
//!   verdict rules (Sweeping / Unavailable / pointer-only).

use std::collections::HashMap;
use std::path::Path;

use crate::handover::CacheEntry as HandoverCacheEntry;
use crate::model::{
    BoxPlayer, BoxScore, BoxTeam, Game, GameType, Season, TapeSource, TapeState, Team,
};
use crate::route::Route;
use crate::store::{FixtureStore, PaletteItem, PaletteKind, SeasonCounts};
use nbatv_catalog::{
    game_context_for, sweep_game, IaProbe, PolitenessConfig, ProbeRegistry, SweepError,
    SweepReport, YoutubeQuota,
};
use nbatv_catalog::{
    review_list as catalog_review_list, sweep_status_for, ReviewItem, SweepStatus,
};

/// Workspace-relative path of the live archive database, resolved against
/// the process working directory (the workspace root in development).
/// Opened with `create_schema` so a brand-new file is a valid empty db.
/// The file is SQLite state, never committed (see `.gitignore`).
pub const ARCHIVE_DB_PATH: &str = "data/archive.db";

/// The live catalog: a SQLite archive connection the Shell renders through.
pub struct DbStore {
    conn: nbatv_db::rusqlite::Connection,
}

impl DbStore {
    /// Open (creating) the archive file at `path` and ensure the schema,
    /// so an empty file is valid. Fails without touching anything else —
    /// callers fall back to an empty in-memory archive.
    pub fn open(path: impl AsRef<Path>) -> nbatv_db::rusqlite::Result<Self> {
        let conn = nbatv_db::open(path)?;
        nbatv_db::create_schema(&conn)?;
        Ok(Self { conn })
    }

    /// Hermetic empty archive: no filesystem. Pure-logic tests use this.
    pub fn in_memory() -> Self {
        let conn = nbatv_db::open_in_memory().expect("in-memory archive db must open");
        if let Err(err) = nbatv_db::create_schema(&conn) {
            eprintln!("archive db: schema setup failed on in-memory db ({err})");
        }
        Self { conn }
    }

    /// Take ownership of an already-opened connection (headless db tests
    /// seed it first). The schema is ensured idempotently; failures log
    /// once and queries then degrade to empty.
    pub fn from_connection(conn: nbatv_db::rusqlite::Connection) -> Self {
        if let Err(err) = nbatv_db::create_schema(&conn) {
            eprintln!("archive db: schema setup failed ({err})");
        }
        Self { conn }
    }

    /// All seasons, oldest first.
    pub fn seasons(&self) -> Vec<Season> {
        nbatv_db::list_seasons(&self.conn)
            .unwrap_or_default()
            .into_iter()
            .map(|row| Season {
                slug: season_slug(row.year),
                label: display_label(&row.label),
                league: row.league,
            })
            .collect()
    }

    pub fn season_label(&self, slug: &str) -> Option<String> {
        self.seasons()
            .into_iter()
            .find(|s| s.slug == slug)
            .map(|s| s.label)
    }

    fn teams(&self) -> Vec<Team> {
        nbatv_db::list_teams(&self.conn)
            .unwrap_or_default()
            .into_iter()
            .map(|row| Team {
                br_slug: row.br_slug,
                city: row.city,
                name: row.name,
                abbrev: row.abbrev,
                // NULL = era did not record the span; 0 reads as "active in
                // every Season" — show-rather-than-hide, matching the
                // box-score NULL rule's never-punish-the-gap stance.
                active_from: row
                    .active_from
                    .and_then(|y| u16::try_from(y).ok())
                    .unwrap_or(0),
                active_to: row.active_to.and_then(|y| u16::try_from(y).ok()),
            })
            .collect()
    }

    pub fn team(&self, slug: &str) -> Option<Team> {
        self.teams().into_iter().find(|t| t.br_slug == slug)
    }

    pub fn team_name(&self, slug: &str) -> Option<String> {
        self.team(slug).map(|t| t.display_name())
    }

    /// Tape rows for one game, best rank first — the exact rows Play
    /// dispatch consults in ladder order.
    pub fn tape_sources(&self, game_id: &str) -> Vec<TapeSource> {
        nbatv_db::tape_sources_for(&self.conn, game_id)
            .unwrap_or_default()
            .into_iter()
            .map(convert_tape_source)
            .collect()
    }

    /// The playable Cache Tier copy for a game, if any: the best-ranked
    /// `Ready` row, converted field-for-field into the handover shape Play
    /// resolves cache-first. Non-`Ready` rows never surface here. Degrades
    /// to `None` on error — an unreadable cache is a cache miss, never a
    /// crash.
    pub fn ready_cache_entry(&self, game_id: &str) -> Option<HandoverCacheEntry> {
        nbatv_db::ready_cache_entry_for(&self.conn, game_id)
            .ok()
            .flatten()
            .map(|row| HandoverCacheEntry {
                game_id: row.game_id.0,
                location: row.local_path,
            })
    }

    /// Compact cache-status line for the Game view (`None` = no line: no
    /// row recorded for this game). `Ready` plays offline; `Failed`
    /// surfaces honestly; anything in flight names its state.
    pub fn cache_status_line(&self, game_id: &str) -> Option<String> {
        match nbatv_db::cache_state_for(&self.conn, game_id)
            .ok()
            .flatten()?
        {
            nbatv_db::CacheState::Ready => {
                Some("Cache: ready — plays offline, no network needed.".to_owned())
            }
            nbatv_db::CacheState::Failed => {
                Some("Cache: fetch failed — nothing cached, Play falls back to stream.".to_owned())
            }
            nbatv_db::CacheState::Fetching => {
                Some("Cache: fetching… (interrupted downloads resume).".to_owned())
            }
            nbatv_db::CacheState::Verifying => {
                Some("Cache: verifying… (duration check before it can play).".to_owned())
            }
            nbatv_db::CacheState::Pending => Some("Cache: fetch pending….".to_owned()),
        }
    }

    /// Drive mirror input line for the Season view (`None` = no line: the
    /// query failed, so nothing renders rather than a wrong count).
    /// Otherwise names how many `Ready` files await upload, or idle when
    /// none do. Remote presence is NOT probed here (no subprocess in the
    /// view layer); the mirror stage reports `RemoteMissing` with its
    /// one-time sign-in prompt when a run finds no remote.
    pub fn mirror_status_line(&self) -> Option<String> {
        let ready = nbatv_db::ready_cache_entries(&self.conn).ok()?;
        if ready.is_empty() {
            Some("Drive mirror: idle — no Ready files to upload.".to_owned())
        } else {
            let n = ready.len();
            Some(format!(
                "Drive mirror: {n} Ready file{} awaiting upload — dry-run first, then apply.",
                if n == 1 { "" } else { "s" }
            ))
        }
    }

    pub fn game(&self, game_id: &str) -> Option<Game> {
        let row = nbatv_db::game_by_id(&self.conn, game_id).unwrap_or_default()?;
        let sources = self.tape_sources(game_id);
        let tape = tape_state_for_game(&self.conn, game_id, &sources);
        Some(game_with_tape(row, sources, tape))
    }

    /// All games of one Season's Schedule, in date order.
    pub fn games_for_season(&self, season: &str) -> Vec<Game> {
        let Some(year) = season_year(season) else {
            return Vec::new();
        };
        nbatv_db::games_in_season(&self.conn, year)
            .unwrap_or_default()
            .into_iter()
            .map(|row| {
                let sources = self.tape_sources(&row.game_id);
                let tape = tape_state_for_game(&self.conn, &row.game_id, &sources);
                game_with_tape(row, sources, tape)
            })
            .collect()
    }

    /// Stored sweep verdict for one game (Playable / Sweeping / Unavailable
    /// / Exists-not-streamable), derived live from `game_queries` plus
    /// `tape_sources`. Degrades to Sweeping on error — an unreadable sweep
    /// keeps looking, never claims absence.
    pub fn sweep_status(&self, game_id: &str) -> SweepStatus {
        sweep_status_for(&self.conn, game_id).unwrap_or(SweepStatus::Sweeping)
    }

    /// REVIEW candidates for one game, in rung order: the Game view's
    /// review card. Degrades to empty on error.
    pub fn review_list(&self, game_id: &str) -> Vec<ReviewItem> {
        catalog_review_list(&self.conn, game_id).unwrap_or_default()
    }

    /// Clubs on a Season dashboard: every club whose span covers the
    /// season's start year, in slug order.
    pub fn teams_for_season(&self, season: &str) -> Vec<Team> {
        let Some(start) = season_year(season).map(|end| end - 1) else {
            return Vec::new();
        };
        let Ok(start) = u16::try_from(start) else {
            return Vec::new();
        };
        let mut teams: Vec<Team> = self
            .teams()
            .into_iter()
            .filter(|t| t.active_in(start))
            .collect();
        teams.sort_by(|a, b| a.br_slug.cmp(&b.br_slug));
        teams
    }

    /// Dashboard filter box: substring over city, name, slug, abbrev
    /// (case-insensitive). Empty query returns the whole club grid.
    pub fn filter_teams(&self, season: &str, query: &str) -> Vec<Team> {
        let q = query.trim().to_lowercase();
        self.teams_for_season(season)
            .into_iter()
            .filter(|t| {
                q.is_empty()
                    || t.city.to_lowercase().contains(&q)
                    || t.name.to_lowercase().contains(&q)
                    || t.br_slug.to_lowercase().contains(&q)
                    || t.abbrev.to_lowercase().contains(&q)
            })
            .collect()
    }

    /// One Team's games as a view over the Season Schedule, split into
    /// the Regular-season ledger and the Playoffs ledger.
    pub fn team_games(&self, season: &str, team: &str) -> (Vec<Game>, Vec<Game>) {
        let mut regular = Vec::new();
        let mut playoffs = Vec::new();
        for game in self.games_for_season(season) {
            if game.home_team != team && game.away_team != team {
                continue;
            }
            match game.game_type {
                GameType::Regular => regular.push(game),
                GameType::Playoffs => playoffs.push(game),
            }
        }
        (regular, playoffs)
    }

    /// Seeded + playable counts for a Season dashboard.
    pub fn season_counts(&self, season: &str) -> SeasonCounts {
        let games = self.games_for_season(season);
        SeasonCounts {
            seeded: games.len(),
            playable: games
                .iter()
                .filter(|g| g.tape == TapeState::Playable)
                .count(),
        }
    }

    /// Box Score for a Game, when its rows are archived. `None` renders the
    /// view's honest "pending" note — a missing box never blocks the tape
    /// banner, and tape presence never depends on it.
    pub fn box_for(&self, game_id: &str) -> Option<BoxScore> {
        let teams = nbatv_db::box_teams_for(&self.conn, game_id).unwrap_or_default();
        let players = nbatv_db::box_players_for(&self.conn, game_id).unwrap_or_default();
        if teams.is_empty() && players.is_empty() {
            return None;
        }
        let names: HashMap<String, String> = nbatv_db::player_names_for_game(&self.conn, game_id)
            .unwrap_or_default()
            .into_iter()
            .collect();
        // Away first, box-score convention; unknown sides keep slug order.
        let sides = nbatv_db::game_by_id(&self.conn, game_id).unwrap_or_default();
        let rank_of = |br: &str| -> u8 {
            match &sides {
                Some(g) if g.away_team == br => 0,
                Some(g) if g.home_team == br => 1,
                _ => 2,
            }
        };
        let mut team_rows: Vec<BoxTeam> = teams.into_iter().map(convert_box_team).collect();
        team_rows.sort_by(|a, b| {
            rank_of(&a.team_br)
                .cmp(&rank_of(&b.team_br))
                .then(a.team_br.cmp(&b.team_br))
        });
        let mut player_rows: Vec<BoxPlayer> = players
            .into_iter()
            .map(|row| convert_box_player(row, &names))
            .collect();
        // Same away-first convention as team totals, so the players grid
        // leads with the same side the totals row does; within a team the
        // slug order (starters block) is preserved.
        player_rows.sort_by(|a, b| {
            rank_of(&a.team_br)
                .cmp(&rank_of(&b.team_br))
                .then(a.team_br.cmp(&b.team_br))
                .then(a.player_br.cmp(&b.player_br))
        });
        Some(BoxScore {
            game_id: game_id.to_string(),
            teams: team_rows,
            players: player_rows,
        })
    }

    /// ⌘K palette: substring over seasons, teams, and games.
    /// Empty query lists everything (the archive is tiny for now).
    pub fn palette_search(&self, query: &str) -> Vec<PaletteItem> {
        let q = query.trim().to_lowercase();
        let mut out = Vec::new();
        let hit = |hay: &str| q.is_empty() || hay.to_lowercase().contains(&q);
        for season in self.seasons() {
            if hit(&format!(
                "{} {} {}",
                season.slug, season.label, season.league
            )) {
                out.push(PaletteItem {
                    kind: PaletteKind::Season,
                    label: format!("Season {}", season.label),
                    detail: season.league.clone(),
                    route: Route::Season {
                        season: season.slug.clone(),
                    },
                });
            }
        }
        for team in self.teams() {
            if hit(&format!(
                "{} {} {} {}",
                team.city, team.name, team.br_slug, team.abbrev
            )) {
                out.push(PaletteItem {
                    kind: PaletteKind::Team,
                    label: team.display_name(),
                    detail: team.status_tag().to_string(),
                    route: Route::Team {
                        season: self.first_season_for(&team),
                        team: team.br_slug.clone(),
                    },
                });
            }
        }
        for season in self.seasons() {
            for game in self.games_for_season(&season.slug) {
                if hit(&format!(
                    "{} {} {} {}",
                    game.label(),
                    game.game_id,
                    game.season,
                    game.scoreline()
                )) {
                    out.push(PaletteItem {
                        kind: PaletteKind::Game,
                        label: format!("{} — {}", game.date, game.label()),
                        detail: format!("{} {}", game.scoreline(), game.tape.glyph()),
                        route: Route::Game {
                            game_id: game.game_id.clone(),
                        },
                    });
                }
            }
        }
        out
    }

    fn first_season_for(&self, team: &Team) -> String {
        self.seasons()
            .iter()
            .find(|s| {
                season_year(&s.slug)
                    .map(|end| end - 1)
                    .and_then(|start| u16::try_from(start).ok())
                    .map_or(false, |y| team.active_in(y))
            })
            .map(|s| s.slug.clone())
            .unwrap_or_else(|| "1946-47".to_string())
    }
}

/// Rung-1 live sweep entry point (issue #21): runs the Internet Archive
/// probe for one archived game and persists the evidence through the
/// catalog sweep. Other rungs stay unswept until their probes land. The
/// probe carries its own transport (live `curl` in production, stubs in
/// tests), so this seam stays offline-safe under injection.
impl DbStore {
    /// Sweep `game_id` through the rung-1 Internet Archive probe.
    ///
    /// Returns `None` when the game is unknown to the archive (nothing to
    /// probe, nothing recorded). A found full game becomes a rank-1
    /// `tape_sources` row, and Play then streams it in-window through the
    /// existing Lane A session — no further wiring.
    pub fn sweep_ia_rung(
        &self,
        probe: &IaProbe,
        game_id: &str,
        politeness: &PolitenessConfig,
        quota: &mut YoutubeQuota,
        now: &str,
    ) -> Result<Option<SweepReport>, SweepError> {
        let Some(game) = game_context_for(&self.conn, game_id)? else {
            return Ok(None);
        };
        let mut registry = ProbeRegistry::new();
        registry.register(probe);
        sweep_game(&self.conn, &game, &registry, politeness, quota, now).map(Some)
    }
}

/// The catalog behind every Shell screen: the archive database in
/// production, fixed fixtures for tests and offline development only.
pub enum Store {
    Fixture(FixtureStore),
    Db(DbStore),
}

impl Store {
    pub fn seasons(&self) -> Vec<Season> {
        match self {
            Store::Fixture(s) => s.seasons().to_vec(),
            Store::Db(s) => s.seasons(),
        }
    }

    pub fn season_label(&self, slug: &str) -> Option<String> {
        match self {
            Store::Fixture(s) => s.season_label(slug),
            Store::Db(s) => s.season_label(slug),
        }
    }

    pub fn team_name(&self, slug: &str) -> Option<String> {
        match self {
            Store::Fixture(s) => s.team_name(slug),
            Store::Db(s) => s.team_name(slug),
        }
    }

    pub fn game(&self, game_id: &str) -> Option<Game> {
        match self {
            Store::Fixture(s) => s.game(game_id).cloned(),
            Store::Db(s) => s.game(game_id),
        }
    }

    pub fn games_for_season(&self, season: &str) -> Vec<Game> {
        match self {
            Store::Fixture(s) => s.games_for_season(season).into_iter().cloned().collect(),
            Store::Db(s) => s.games_for_season(season),
        }
    }

    pub fn teams_for_season(&self, season: &str) -> Vec<Team> {
        match self {
            Store::Fixture(s) => s.teams_for_season(season).into_iter().cloned().collect(),
            Store::Db(s) => s.teams_for_season(season),
        }
    }

    pub fn filter_teams(&self, season: &str, query: &str) -> Vec<Team> {
        match self {
            Store::Fixture(s) => s.filter_teams(season, query).into_iter().cloned().collect(),
            Store::Db(s) => s.filter_teams(season, query),
        }
    }

    pub fn team_games(&self, season: &str, team: &str) -> (Vec<Game>, Vec<Game>) {
        match self {
            Store::Fixture(s) => {
                let (regular, playoffs) = s.team_games(season, team);
                (
                    regular.into_iter().cloned().collect(),
                    playoffs.into_iter().cloned().collect(),
                )
            }
            Store::Db(s) => s.team_games(season, team),
        }
    }

    pub fn season_counts(&self, season: &str) -> SeasonCounts {
        match self {
            Store::Fixture(s) => s.season_counts(season),
            Store::Db(s) => s.season_counts(season),
        }
    }

    pub fn box_for(&self, game_id: &str) -> Option<BoxScore> {
        match self {
            Store::Fixture(s) => s.box_for(game_id).cloned(),
            Store::Db(s) => s.box_for(game_id),
        }
    }

    pub fn palette_search(&self, query: &str) -> Vec<PaletteItem> {
        match self {
            Store::Fixture(s) => s.palette_search(query),
            Store::Db(s) => s.palette_search(query),
        }
    }

    pub fn sweep_status(&self, game_id: &str) -> SweepStatus {
        match self {
            Store::Fixture(s) => s
                .game(game_id)
                .map(sweep_status_for_fixture)
                .unwrap_or(SweepStatus::Sweeping),
            Store::Db(s) => s.sweep_status(game_id),
        }
    }

    pub fn review_list(&self, game_id: &str) -> Vec<ReviewItem> {
        match self {
            Store::Fixture(_) => Vec::new(),
            Store::Db(s) => s.review_list(game_id),
        }
    }

    /// The playable Cache Tier copy for a game, if any. Fixtures hold no
    /// cache rows (offline dev plays streams); the live database serves its
    /// best-ranked `Ready` row.
    pub fn ready_cache_entry(&self, game_id: &str) -> Option<HandoverCacheEntry> {
        match self {
            Store::Fixture(_) => None,
            Store::Db(s) => s.ready_cache_entry(game_id),
        }
    }

    /// Compact cache-status line for the Game view, if a row is recorded.
    pub fn cache_status_line(&self, game_id: &str) -> Option<String> {
        match self {
            Store::Fixture(_) => None,
            Store::Db(s) => s.cache_status_line(game_id),
        }
    }

    /// Drive mirror input line for the Season view, if the store can say.
    /// Fixtures hold no cache rows (offline dev mirrors nothing).
    pub fn mirror_status_line(&self) -> Option<String> {
        match self {
            Store::Fixture(_) => None,
            Store::Db(s) => s.mirror_status_line(),
        }
    }
}

/// Season slug (`1946-47`) for an archive ending year (`1947`).
fn season_slug(year: i32) -> String {
    format!("{}-{:02}", year - 1, year.rem_euclid(100))
}

/// Ending year for a season slug (`1946-47` → `1947`): one past the leading
/// start year. `None` for slugs with no leading year.
fn season_year(slug: &str) -> Option<i32> {
    slug.split(['-', '–'])
        .next()?
        .trim()
        .parse::<i32>()
        .ok()
        .map(|start| start + 1)
}

/// Display label for a stored season label: the archive stores `1946-47`,
/// the Shell renders the en-dash fixture convention `1946–47`.
fn display_label(db_label: &str) -> String {
    db_label.replace('-', "–")
}

/// Tape availability for one game: verified stream rows are ground truth —
/// tape exists and Play dispatch plays it, whatever the query history says
/// (hand-seeded and pre-sweep rows stay honest) — otherwise the derived
/// sweep verdict rules: an unconsumed ladder is Sweeping, a fully recorded
/// empty sweep is Unavailable, and pointer-only rows with no sweep history
/// are pointer-only. A Playable verdict with no stream rows degrades to
/// Sweeping (stale win, keep looking) rather than showing Play with
/// nothing to play. One tape_sources read plus one game_queries read per
/// game: the rows are preloaded once and the verdict derived from them.
fn tape_state_for_game(
    conn: &nbatv_db::rusqlite::Connection,
    game_id: &str,
    sources: &[TapeSource],
) -> TapeState {
    if sources.iter().any(|s| s.rank <= 4) {
        return TapeState::Playable;
    }
    // Past the early return every row is rung 5+, so any row at all means
    // pointer-only.
    let queries = nbatv_db::game_queries_for(conn, game_id).unwrap_or_default();
    match nbatv_catalog::sweep_status_from(game_id, queries, !sources.is_empty()) {
        SweepStatus::Playable { .. } => TapeState::Sweeping,
        SweepStatus::Sweeping => TapeState::Sweeping,
        SweepStatus::Unavailable { .. } => TapeState::Unavailable,
        SweepStatus::ExistsNotStreamable => TapeState::Pointer,
    }
}

/// Fixture games carry their verdict directly: map it onto the sweep
/// vocabulary so the [`Store`] seam reads uniformly.
fn sweep_status_for_fixture(game: &Game) -> SweepStatus {
    match game.tape {
        TapeState::Playable => SweepStatus::Playable {
            rank: game.sources.iter().map(|s| s.rank).min().unwrap_or(0),
        },
        TapeState::Sweeping => SweepStatus::Sweeping,
        TapeState::Unavailable => SweepStatus::Unavailable {
            consumed_label: format!("ladder consumed {}", game.game_id),
        },
        TapeState::Pointer => SweepStatus::ExistsNotStreamable,
    }
}

fn convert_tape_source(row: nbatv_db::TapeSource) -> TapeSource {
    TapeSource {
        game_id: row.game_id,
        rank: row.rank,
        source_class: row.source_class,
        url_or_pointer: row.url_or_pointer,
        match_confidence: row.match_confidence,
        verified_at: row.verified_at,
    }
}

fn game_with_tape(row: nbatv_db::GameRow, sources: Vec<TapeSource>, tape: TapeState) -> Game {
    Game {
        game_id: row.game_id,
        season: season_slug(row.season),
        date: row.date,
        game_type: match row.game_type.as_str() {
            "PLAYOFFS" => GameType::Playoffs,
            // REGULAR and NBA_CUP (in-season) skate in the Regular ledger.
            _ => GameType::Regular,
        },
        home_team: row.home_team,
        away_team: row.away_team,
        home_pts: u32::try_from(row.home_pts).unwrap_or(0),
        away_pts: u32::try_from(row.away_pts).unwrap_or(0),
        tape,
        sources,
    }
}

/// Nullable integer stat: negative archive values degrade to unrecorded
/// rather than wrapping.
fn uopt(value: Option<i32>) -> Option<u32> {
    value.and_then(|v| u32::try_from(v).ok())
}

/// Archive minutes are text (`"240"` team totals, `"32:00"` player-style):
/// keep the leading whole minutes, `None` when unparseable.
fn parse_minutes(text: &str) -> Option<u32> {
    text.trim().split(':').next()?.trim().parse().ok()
}

fn convert_box_team(row: nbatv_db::BoxTeamRow) -> BoxTeam {
    BoxTeam {
        game_id: row.game_id,
        team_br: row.team_br,
        mp: row.mp.as_deref().and_then(parse_minutes),
        fg: uopt(row.fg),
        fga: uopt(row.fga),
        ft: uopt(row.ft),
        fta: uopt(row.fta),
        oreb: uopt(row.oreb),
        dreb: uopt(row.dreb),
        reb: uopt(row.reb),
        ast: uopt(row.ast),
        stl: uopt(row.stl),
        blk: uopt(row.blk),
        pf: uopt(row.pf),
        pts: uopt(row.pts),
        plus_minus: row.plus_minus.map(|v| v.round() as i32),
    }
}

fn convert_box_player(row: nbatv_db::BoxPlayerRow, names: &HashMap<String, String>) -> BoxPlayer {
    BoxPlayer {
        player_name: names
            .get(&row.player_br)
            .cloned()
            .unwrap_or_else(|| row.player_br.clone()),
        game_id: row.game_id,
        team_br: row.team_br,
        player_br: row.player_br,
        starter: row.starter,
        position: row.position,
        mp: row.mp.as_deref().and_then(parse_minutes),
        fg: uopt(row.fg),
        fga: uopt(row.fga),
        ft: uopt(row.ft),
        fta: uopt(row.fta),
        pts: uopt(row.pts),
        plus_minus: row.plus_minus.map(|v| v.round() as i32),
        dnp_reason: row.dnp_reason,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn season_slug_round_trips_through_year() {
        assert_eq!(season_slug(1947), "1946-47");
        assert_eq!(season_slug(2001), "2000-01");
        assert_eq!(season_year("1946-47"), Some(1947));
        assert_eq!(season_year("1946–47"), Some(1947));
        assert_eq!(season_year("nope"), None);
    }

    fn memdb() -> nbatv_db::rusqlite::Connection {
        let conn = nbatv_db::open_in_memory().expect("in-memory archive db");
        nbatv_db::create_schema(&conn).expect("create_schema");
        conn
    }

    fn stream_source(rank: u8) -> TapeSource {
        TapeSource {
            game_id: nbatv_db::GameId("194611010TRH".to_owned()),
            rank,
            source_class: nbatv_db::SourceClass::InternetArchive,
            url_or_pointer: "https://archive.org/details/x".to_owned(),
            match_confidence: 1.0,
            verified_at: "2026-01-01".to_owned(),
        }
    }

    fn sweep_query(rung: u8, level: &str) -> nbatv_db::GameQuery {
        nbatv_db::GameQuery {
            game_id: nbatv_db::GameId("194611010TRH".to_owned()),
            rung,
            query_text: format!("query for rung {rung}"),
            queried_at: "2026-01-01".to_owned(),
            best_match_level: level.to_owned(),
            review_url: None,
            review_title: None,
        }
    }

    #[test]
    fn tape_state_prefers_stream_rows_then_stored_verdict() {
        let conn = memdb();
        // Verified stream rows win whatever the query history says.
        assert_eq!(
            tape_state_for_game(&conn, "194611010TRH", &[stream_source(1)]),
            TapeState::Playable
        );
        // No rows, no history: still sweeping, never absent.
        assert_eq!(
            tape_state_for_game(&conn, "194611010TRH", &[]),
            TapeState::Sweeping
        );
        // Fully recorded empty sweep: unavailable.
        for rung in 0u8..=4 {
            nbatv_db::upsert_game_query(&conn, &sweep_query(rung, "reject")).unwrap();
        }
        assert_eq!(
            tape_state_for_game(&conn, "194611010TRH", &[]),
            TapeState::Unavailable
        );
    }

    #[test]
    fn playable_verdict_without_stream_rows_degrades_to_sweeping() {
        let conn = memdb();
        // A recorded LIKELY win with zero tape rows is a stale win: the
        // state must not offer Play with nothing to play.
        nbatv_db::upsert_game_query(&conn, &sweep_query(1, "likely")).unwrap();
        assert_eq!(
            tape_state_for_game(&conn, "194611010TRH", &[]),
            TapeState::Sweeping
        );
    }

    #[test]
    fn tape_state_maps_pointer_only_sweeps_to_pointer() {
        let conn = memdb();
        // Mirror the real call path: the slice and the db agree, because
        // callers load sources from the same connection.
        nbatv_db::insert_tape_source(
            &conn,
            &nbatv_db::TapeSource {
                game_id: nbatv_db::GameId("194611010TRH".to_owned()),
                rank: 5,
                source_class: nbatv_db::SourceClass::Collector,
                url_or_pointer: "pointer:collector/x".to_owned(),
                match_confidence: 1.0,
                verified_at: "2026-01-01".to_owned(),
            },
        )
        .unwrap();
        let pointer = TapeSource {
            rank: 5,
            ..stream_source(5)
        };
        assert_eq!(
            tape_state_for_game(&conn, "194611010TRH", &[pointer]),
            TapeState::Pointer
        );
    }

    #[test]
    fn minutes_parse_keeps_leading_whole_minutes() {
        assert_eq!(parse_minutes("240"), Some(240));
        assert_eq!(parse_minutes("38:00"), Some(38));
        assert_eq!(parse_minutes(""), None);
        assert_eq!(parse_minutes("Did Not Play"), None);
    }

    #[test]
    fn negative_stats_degrade_to_unrecorded() {
        assert_eq!(uopt(Some(-1)), None);
        assert_eq!(uopt(Some(68)), Some(68));
    }

    fn cache_row(state: nbatv_db::CacheState) -> nbatv_db::CacheEntry {
        nbatv_db::CacheEntry {
            game_id: nbatv_db::GameId("194611010TRH".to_owned()),
            rank: 1,
            source_class: nbatv_db::SourceClass::InternetArchive,
            local_path: "data/cache/tape/1946-47/194611010TRH__NYK-at-TRH__ia.mp4".to_owned(),
            bytes: 100,
            verified_at: None,
            state,
        }
    }

    #[test]
    fn cache_status_line_names_each_state_and_hides_missing_rows() {
        let conn = memdb();
        let store = DbStore::from_connection(conn);
        assert_eq!(store.cache_status_line("194611010TRH"), None);
        assert_eq!(store.ready_cache_entry("194611010TRH"), None);
    }

    #[test]
    fn cache_status_line_covers_in_flight_states() {
        for (state, fragment) in [
            (nbatv_db::CacheState::Pending, "pending"),
            (nbatv_db::CacheState::Fetching, "fetching"),
            (nbatv_db::CacheState::Verifying, "verifying"),
        ] {
            let conn = memdb();
            nbatv_db::upsert_cache_entry(&conn, &cache_row(state)).unwrap();
            let store = DbStore::from_connection(conn);
            let line = store
                .cache_status_line("194611010TRH")
                .expect("a recorded row always yields a line");
            assert!(
                line.contains(fragment),
                "{state:?} line names its state: {line}"
            );
            assert_eq!(
                store.ready_cache_entry("194611010TRH"),
                None,
                "{state:?} rows never surface as playable"
            );
        }
    }

    #[test]
    fn mirror_status_line_is_idle_when_nothing_is_ready() {
        let conn = memdb();
        let store = DbStore::from_connection(conn);
        assert_eq!(
            store.mirror_status_line().as_deref(),
            Some("Drive mirror: idle — no Ready files to upload.")
        );
        // A non-Ready row is not mirror input either.
        let conn = memdb();
        nbatv_db::upsert_cache_entry(&conn, &cache_row(nbatv_db::CacheState::Failed)).unwrap();
        let store = DbStore::from_connection(conn);
        assert_eq!(
            store.mirror_status_line().as_deref(),
            Some("Drive mirror: idle — no Ready files to upload.")
        );
        // Fixtures hold no cache rows: no line in offline dev.
        assert_eq!(
            Store::Fixture(FixtureStore::fixture()).mirror_status_line(),
            None
        );
    }

    #[test]
    fn mirror_status_line_counts_ready_files() {
        let conn = memdb();
        let mut ready = cache_row(nbatv_db::CacheState::Ready);
        nbatv_db::upsert_cache_entry(&conn, &ready).unwrap();
        ready.rank = 4;
        ready.game_id = nbatv_db::GameId("194611020CHS".to_owned());
        nbatv_db::upsert_cache_entry(&conn, &ready).unwrap();
        let store = DbStore::from_connection(conn);
        let line = store
            .mirror_status_line()
            .expect("Ready rows are mirror input");
        assert!(
            line.contains("2 Ready"),
            "the line counts Ready files: {line}"
        );
        assert!(
            line.contains("dry-run"),
            "the line points at preview-before-apply: {line}"
        );
    }
}
