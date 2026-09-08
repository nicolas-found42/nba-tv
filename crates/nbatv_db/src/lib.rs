//! SQLite storage for the nba-tv personal NBA archive.
//!
//! Schema mirrors the batch contract: `seasons`, `teams`, `players`,
//! `games`, `box_team`, `box_player`, `player_season_totals`,
//! `tape_sources`, plus `game_queries` (TapeCatalog sweep evidence). The
//! `game_id` primary key is the Basketball-Reference box-score slug (e.g.
//! `194611010TRH`).
//!
//! Era rule: box-score columns for stats the era did not record are
//! NULLABLE. NULL means "era did not record" and the UI renders "—".
//! Box score is never unavailable; only tape can be unavailable.

use rusqlite::{Connection, Result as SqlResult, Row};

// ---------------------------------------------------------------------------
// Model types (contract shapes)
// ---------------------------------------------------------------------------

/// Basketball-Reference box-score slug, e.g. `194611010TRH`.
/// Shape: 9 ASCII digits + 3 uppercase ASCII letters.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GameId(pub String);

impl GameId {
    /// Validate the slug shape and wrap it. Returns `None` on garbage.
    pub fn parse(s: &str) -> Option<Self> {
        if is_valid_game_id(s) {
            Some(GameId(s.to_owned()))
        } else {
            None
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Canonical Basketball-Reference box-score URL for this game.
    pub fn br_url(&self) -> String {
        format!(
            "https://www.basketball-reference.com/boxscores/{}.html",
            self.0
        )
    }
}

impl std::fmt::Display for GameId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Shared shape check: `^\d{9}[A-Z]{3}$`.
pub fn is_valid_game_id(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 12
        && b[..9].iter().all(|c| c.is_ascii_digit())
        && b[9..].iter().all(|c| c.is_ascii_uppercase())
}

/// How a tape source plays. Determined by ladder rung:
/// rungs 1+4 → file, 0+2+3 → vendor surface, 5+6+7 → pointer only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackClass {
    ProgressiveFile,
    ExternalSurface,
    Pointer,
}

impl PlaybackClass {
    /// Map a source-ladder rung (0–7) to its playback class.
    /// Returns `None` for out-of-range rungs.
    pub fn of_rank(rank: u8) -> Option<Self> {
        match rank {
            1 | 4 => Some(PlaybackClass::ProgressiveFile),
            0 | 2 | 3 => Some(PlaybackClass::ExternalSurface),
            5 | 6 | 7 => Some(PlaybackClass::Pointer),
            _ => None,
        }
    }
}

/// One ranked tape-source pointer for a game (grey sources are pointers
/// only; media is never downloaded by this crate).
#[derive(Debug, Clone, PartialEq)]
pub struct TapeSource {
    pub game_id: String,
    pub rank: u8,
    pub source_class: String,
    pub url_or_pointer: String,
    pub match_confidence: f32,
    pub verified_at: String,
}

impl TapeSource {
    pub fn playback_class(&self) -> Option<PlaybackClass> {
        PlaybackClass::of_rank(self.rank)
    }
}

/// One recorded outbound sweep query for a game rung (TapeCatalog evidence,
/// issue #20). One row per `(game_id, rung)`: a re-sweep replaces the stale
/// row, so restart resumes from what is stored. `best_match_level` is one
/// of `confirmed|likely|review|reject`. `review_url`/`review_title` carry
/// the top REVIEW candidate evidence when `best_match_level` is `review`
/// (else NULL); REVIEW rows never become `tape_sources`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameQuery {
    pub game_id: String,
    pub rung: u8,
    pub query_text: String,
    pub queried_at: String,
    pub best_match_level: String,
    pub review_url: Option<String>,
    pub review_title: Option<String>,
}

/// Cache Tier download state for one ranked byte-class tape row (issue #25).
/// One row per `(game_id, rank)`: a re-fetch refreshes the row instead of
/// failing on the primary key. `Pending` → `Fetching` → `Verifying` →
/// `Ready`; any failure lands on `Failed` (never `Ready`), so a `Ready`
/// row always names a verified local file Play can decode offline.
/// Stored as `TEXT` under a `CHECK` constraint, mirroring
/// `game_queries.best_match_level`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheState {
    Pending,
    Fetching,
    Verifying,
    Ready,
    Failed,
}

impl CacheState {
    /// The stored string form.
    pub fn as_str(self) -> &'static str {
        match self {
            CacheState::Pending => "Pending",
            CacheState::Fetching => "Fetching",
            CacheState::Verifying => "Verifying",
            CacheState::Ready => "Ready",
            CacheState::Failed => "Failed",
        }
    }

    /// Parse a stored string; `None` for anything the `CHECK` rejects.
    pub fn parse(s: &str) -> Option<CacheState> {
        match s {
            "Pending" => Some(CacheState::Pending),
            "Fetching" => Some(CacheState::Fetching),
            "Verifying" => Some(CacheState::Verifying),
            "Ready" => Some(CacheState::Ready),
            "Failed" => Some(CacheState::Failed),
            _ => None,
        }
    }
}

impl std::fmt::Display for CacheState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One Cache Tier download row: the local copy of one ranked byte-class
/// tape source. `local_path` is the `tape/…` cache path (never committed);
/// `bytes` is the on-disk size at the last write; `verified_at` is the
/// `YYYY-MM-DD` verification date, set only on `Ready` rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheEntry {
    pub game_id: String,
    pub rank: u8,
    pub source_class: String,
    pub local_path: String,
    pub bytes: i64,
    pub verified_at: Option<String>,
    pub state: CacheState,
}

// ---------------------------------------------------------------------------
// Row types for inserts
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct SeasonRow {
    pub league: String,
    pub year: i32,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TeamRow {
    pub br_slug: String,
    pub nba_team_id: Option<i64>,
    pub franchise_id: Option<String>,
    pub city: String,
    pub name: String,
    pub abbrev: String,
    pub active_from: Option<i32>,
    pub active_to: Option<i32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlayerRow {
    pub br_slug: String,
    pub nba_person_id: Option<i64>,
    pub name: String,
    pub first_season: Option<i32>,
    pub last_season: Option<i32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GameRow {
    pub game_id: String,
    pub nba_game_id: Option<String>,
    pub league: String,
    pub season: i32,
    pub date: String,
    /// `REGULAR` | `PLAYOFFS` | `NBA_CUP`.
    pub game_type: String,
    pub home_team: String,
    pub away_team: String,
    pub home_pts: i32,
    pub away_pts: i32,
    pub ot: Option<String>,
    pub arena: Option<String>,
    pub attendance: Option<i64>,
    pub br_url: String,
    pub sources: String,
}

/// Team box score. Era-absent stats are `None` (NULL = not recorded).
/// Minutes are text (`"240"` team totals, `"32:00"` player-style).
/// Widens #3 §4's enumerated columns with `fg3`/`fg3a`/`tov`: BR publishes
/// threes from 1979-80 and turnovers in the modern era, so they are stored
/// when recorded and `None` for earlier seasons — same NULL rule as above.
///
/// Live-data correction (1946-47 crawl): even the "always recorded" core —
/// `fg`/`fga`/`ft`/`fta`/`pf`/`pts` — goes unrecorded in the earliest
/// seasons (November 1946 team totals blank `fga`, and variously `fta` and
/// `pf`). All six are therefore nullable: blank means era-did-not-record,
/// never zero. `pts` is present on every observed totals row and is the
/// minimum for a usable result.
#[derive(Debug, Clone, PartialEq)]
pub struct BoxTeamRow {
    pub game_id: String,
    pub team_br: String,
    pub mp: Option<String>,
    pub fg: Option<i32>,
    pub fga: Option<i32>,
    pub fg3: Option<i32>,
    pub fg3a: Option<i32>,
    pub ft: Option<i32>,
    pub fta: Option<i32>,
    pub oreb: Option<i32>,
    pub dreb: Option<i32>,
    pub reb: Option<i32>,
    pub ast: Option<i32>,
    pub stl: Option<i32>,
    pub blk: Option<i32>,
    pub tov: Option<i32>,
    pub pf: Option<i32>,
    pub pts: Option<i32>,
    pub plus_minus: Option<f64>,
}

/// Player box score. All stats nullable: era-absent stats AND DNP rows
/// (which carry `dnp_reason` instead) store NULL.
#[derive(Debug, Clone, PartialEq)]
pub struct BoxPlayerRow {
    pub game_id: String,
    pub team_br: String,
    pub player_br: String,
    pub starter: Option<bool>,
    pub position: Option<String>,
    pub mp: Option<String>,
    pub fg: Option<i32>,
    pub fga: Option<i32>,
    pub fg3: Option<i32>,
    pub fg3a: Option<i32>,
    pub ft: Option<i32>,
    pub fta: Option<i32>,
    pub oreb: Option<i32>,
    pub dreb: Option<i32>,
    pub reb: Option<i32>,
    pub ast: Option<i32>,
    pub stl: Option<i32>,
    pub blk: Option<i32>,
    pub tov: Option<i32>,
    pub pf: Option<i32>,
    pub pts: Option<i32>,
    pub plus_minus: Option<f64>,
    pub dnp_reason: Option<String>,
}

/// One (player, season, team) totals row; 2TM/3TM seasons keep splits.
#[derive(Debug, Clone, PartialEq)]
pub struct SeasonTotalRow {
    pub player_br: String,
    pub season: i32,
    pub team_br: String,
    pub g: i32,
    pub mp: Option<i32>,
    pub fg: Option<i32>,
    pub fga: Option<i32>,
    pub fg3: Option<i32>,
    pub fg3a: Option<i32>,
    pub ft: Option<i32>,
    pub fta: Option<i32>,
    pub oreb: Option<i32>,
    pub dreb: Option<i32>,
    pub reb: Option<i32>,
    pub ast: Option<i32>,
    pub stl: Option<i32>,
    pub blk: Option<i32>,
    pub tov: Option<i32>,
    pub pf: Option<i32>,
    pub pts: Option<i32>,
}

// ---------------------------------------------------------------------------
// Schema
// ---------------------------------------------------------------------------

/// Create all 10 archive tables. Idempotent (`IF NOT EXISTS`).
pub fn create_schema(conn: &Connection) -> SqlResult<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS seasons (
            league TEXT NOT NULL,
            year   INTEGER NOT NULL,
            label  TEXT NOT NULL,
            PRIMARY KEY (league, year)
        );
        CREATE TABLE IF NOT EXISTS teams (
            br_slug      TEXT PRIMARY KEY,
            nba_team_id  INTEGER NULL,
            franchise_id TEXT NULL,
            city         TEXT NOT NULL,
            name         TEXT NOT NULL,
            abbrev       TEXT NOT NULL,
            active_from  INTEGER NULL,
            active_to    INTEGER NULL
        );
        CREATE TABLE IF NOT EXISTS players (
            br_slug       TEXT PRIMARY KEY,
            nba_person_id INTEGER NULL,
            name          TEXT NOT NULL,
            first_season  INTEGER NULL,
            last_season   INTEGER NULL
        );
        CREATE TABLE IF NOT EXISTS games (
            game_id    TEXT PRIMARY KEY,
            nba_game_id TEXT NULL,
            league     TEXT NOT NULL,
            season     INTEGER NOT NULL,
            date       TEXT NOT NULL,
            game_type  TEXT NOT NULL
                       CHECK (game_type IN ('REGULAR', 'PLAYOFFS', 'NBA_CUP')),
            home_team  TEXT NOT NULL,
            away_team  TEXT NOT NULL,
            home_pts   INTEGER NOT NULL,
            away_pts   INTEGER NOT NULL,
            ot         TEXT NULL,
            arena      TEXT NULL,
            attendance INTEGER NULL,
            br_url     TEXT NOT NULL,
            sources    TEXT NOT NULL DEFAULT '[]'
        );
        CREATE TABLE IF NOT EXISTS box_team (
            game_id    TEXT NOT NULL,
            team_br    TEXT NOT NULL,
            mp         TEXT NULL,
            fg         INTEGER NULL,
            fga        INTEGER NULL,
            fg3        INTEGER NULL,
            fg3a       INTEGER NULL,
            ft         INTEGER NULL,
            fta        INTEGER NULL,
            oreb       INTEGER NULL,
            dreb       INTEGER NULL,
            reb        INTEGER NULL,
            ast        INTEGER NULL,
            stl        INTEGER NULL,
            blk        INTEGER NULL,
            tov        INTEGER NULL,
            pf         INTEGER NULL,
            pts        INTEGER NULL,
            plus_minus REAL NULL,
            PRIMARY KEY (game_id, team_br)
        );
        CREATE TABLE IF NOT EXISTS box_player (
            game_id    TEXT NOT NULL,
            team_br    TEXT NOT NULL,
            player_br  TEXT NOT NULL,
            starter    INTEGER NULL,
            position   TEXT NULL,
            mp         TEXT NULL,
            fg         INTEGER NULL,
            fga        INTEGER NULL,
            fg3        INTEGER NULL,
            fg3a       INTEGER NULL,
            ft         INTEGER NULL,
            fta        INTEGER NULL,
            oreb       INTEGER NULL,
            dreb       INTEGER NULL,
            reb        INTEGER NULL,
            ast        INTEGER NULL,
            stl        INTEGER NULL,
            blk        INTEGER NULL,
            tov        INTEGER NULL,
            pf         INTEGER NULL,
            pts        INTEGER NULL,
            plus_minus REAL NULL,
            dnp_reason TEXT NULL,
            PRIMARY KEY (game_id, team_br, player_br)
        );
        CREATE TABLE IF NOT EXISTS player_season_totals (
            player_br TEXT NOT NULL,
            season    INTEGER NOT NULL,
            team_br   TEXT NOT NULL,
            g         INTEGER NOT NULL,
            mp        INTEGER NULL,
            fg        INTEGER NULL,
            fga       INTEGER NULL,
            fg3       INTEGER NULL,
            fg3a      INTEGER NULL,
            ft        INTEGER NULL,
            fta       INTEGER NULL,
            oreb      INTEGER NULL,
            dreb      INTEGER NULL,
            reb       INTEGER NULL,
            ast       INTEGER NULL,
            stl       INTEGER NULL,
            blk       INTEGER NULL,
            tov       INTEGER NULL,
            pf        INTEGER NULL,
            pts       INTEGER NULL,
            PRIMARY KEY (player_br, season, team_br)
        );
        CREATE TABLE IF NOT EXISTS tape_sources (
            game_id           TEXT NOT NULL,
            rank              INTEGER NOT NULL,
            source_class      TEXT NOT NULL,
            url_or_pointer    TEXT NOT NULL,
            match_confidence  REAL NOT NULL,
            verified_at       TEXT NOT NULL,
            PRIMARY KEY (game_id, rank)
        );
        CREATE TABLE IF NOT EXISTS game_queries (
            game_id          TEXT NOT NULL,
            rung             INTEGER NOT NULL,
            query_text       TEXT NOT NULL,
            queried_at       TEXT NOT NULL,
            best_match_level TEXT NOT NULL
                             CHECK (best_match_level IN ('confirmed','likely','review','reject')),
            review_url       TEXT NULL,
            review_title     TEXT NULL,
            PRIMARY KEY (game_id, rung)
        );
        CREATE TABLE IF NOT EXISTS cache_entries (
            game_id      TEXT NOT NULL,
            rank         INTEGER NOT NULL,
            source_class TEXT NOT NULL,
            local_path   TEXT NOT NULL,
            bytes        INTEGER NOT NULL DEFAULT 0,
            verified_at  TEXT NULL,
            state        TEXT NOT NULL
                         CHECK (state IN ('Pending','Fetching','Verifying','Ready','Failed')),
            PRIMARY KEY (game_id, rank)
        );
        ",
    )
}

// ---------------------------------------------------------------------------
// Inserts
// ---------------------------------------------------------------------------

pub fn insert_season(conn: &Connection, s: &SeasonRow) -> SqlResult<()> {
    conn.execute(
        "INSERT INTO seasons (league, year, label) VALUES (?1, ?2, ?3)",
        rusqlite::params![s.league, s.year, s.label],
    )?;
    Ok(())
}

pub fn insert_team(conn: &Connection, t: &TeamRow) -> SqlResult<()> {
    conn.execute(
        "INSERT INTO teams
            (br_slug, nba_team_id, franchise_id, city, name, abbrev, active_from, active_to)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        rusqlite::params![
            t.br_slug,
            t.nba_team_id,
            t.franchise_id,
            t.city,
            t.name,
            t.abbrev,
            t.active_from,
            t.active_to
        ],
    )?;
    Ok(())
}

/// Ingest-convergence write for the snapshot builder: the crawl recomputes
/// the observed span wholesale, so a re-ingest over a grown crawl replaces
/// the row with the crawl's current truth (no MIN/MAX accumulation).
pub fn upsert_team(conn: &Connection, t: &TeamRow) -> SqlResult<()> {
    conn.execute(
        "INSERT INTO teams
            (br_slug, nba_team_id, franchise_id, city, name, abbrev, active_from, active_to)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT (br_slug) DO UPDATE SET
            nba_team_id = excluded.nba_team_id,
            franchise_id = excluded.franchise_id,
            city = excluded.city,
            name = excluded.name,
            abbrev = excluded.abbrev,
            active_from = excluded.active_from,
            active_to = excluded.active_to",
        rusqlite::params![
            t.br_slug,
            t.nba_team_id,
            t.franchise_id,
            t.city,
            t.name,
            t.abbrev,
            t.active_from,
            t.active_to
        ],
    )?;
    Ok(())
}

pub fn insert_player(conn: &Connection, p: &PlayerRow) -> SqlResult<()> {
    conn.execute(
        "INSERT INTO players (br_slug, nba_person_id, name, first_season, last_season)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![
            p.br_slug,
            p.nba_person_id,
            p.name,
            p.first_season,
            p.last_season
        ],
    )?;
    Ok(())
}

pub fn insert_game(conn: &Connection, g: &GameRow) -> SqlResult<()> {
    conn.execute(
        "INSERT INTO games
            (game_id, nba_game_id, league, season, date, game_type,
             home_team, away_team, home_pts, away_pts, ot, arena,
             attendance, br_url, sources)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
        rusqlite::params![
            g.game_id,
            g.nba_game_id,
            g.league,
            g.season,
            g.date,
            g.game_type,
            g.home_team,
            g.away_team,
            g.home_pts,
            g.away_pts,
            g.ot,
            g.arena,
            g.attendance,
            g.br_url,
            g.sources
        ],
    )?;
    Ok(())
}

/// Ingest-convergence write for the snapshot builder: re-ingesting a game
/// with a box snapshot upgrades the schedule-only row (0-0) to real scores
/// and refreshes parsed fields. `sources` is deliberately absent from the
/// UPDATE set — the catalog owns that JSON and must survive re-ingests.
pub fn upsert_game(conn: &Connection, g: &GameRow) -> SqlResult<()> {
    conn.execute(
        "INSERT INTO games
            (game_id, nba_game_id, league, season, date, game_type,
             home_team, away_team, home_pts, away_pts, ot, arena,
             attendance, br_url, sources)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
         ON CONFLICT (game_id) DO UPDATE SET
            league = excluded.league,
            season = excluded.season,
            date = excluded.date,
            game_type = excluded.game_type,
            home_team = excluded.home_team,
            away_team = excluded.away_team,
            home_pts = excluded.home_pts,
            away_pts = excluded.away_pts,
            ot = excluded.ot,
            arena = excluded.arena,
            attendance = excluded.attendance,
            br_url = excluded.br_url",
        rusqlite::params![
            g.game_id,
            g.nba_game_id,
            g.league,
            g.season,
            g.date,
            g.game_type,
            g.home_team,
            g.away_team,
            g.home_pts,
            g.away_pts,
            g.ot,
            g.arena,
            g.attendance,
            g.br_url,
            g.sources
        ],
    )?;
    Ok(())
}

pub fn insert_box_team(conn: &Connection, b: &BoxTeamRow) -> SqlResult<()> {
    conn.execute(
        "INSERT INTO box_team
            (game_id, team_br, mp, fg, fga, fg3, fg3a, ft, fta,
             oreb, dreb, reb, ast, stl, blk, tov, pf, pts, plus_minus)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9,
                 ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)",
        rusqlite::params![
            b.game_id,
            b.team_br,
            b.mp,
            b.fg,
            b.fga,
            b.fg3,
            b.fg3a,
            b.ft,
            b.fta,
            b.oreb,
            b.dreb,
            b.reb,
            b.ast,
            b.stl,
            b.blk,
            b.tov,
            b.pf,
            b.pts,
            b.plus_minus
        ],
    )?;
    Ok(())
}

pub fn insert_box_player(conn: &Connection, b: &BoxPlayerRow) -> SqlResult<()> {
    let starter: Option<i64> = b.starter.map(|s| if s { 1 } else { 0 });
    conn.execute(
        "INSERT INTO box_player
            (game_id, team_br, player_br, starter, position, mp,
             fg, fga, fg3, fg3a, ft, fta, oreb, dreb, reb, ast,
             stl, blk, tov, pf, pts, plus_minus, dnp_reason)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10,
                 ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20,
                 ?21, ?22, ?23)",
        rusqlite::params![
            b.game_id,
            b.team_br,
            b.player_br,
            starter,
            b.position,
            b.mp,
            b.fg,
            b.fga,
            b.fg3,
            b.fg3a,
            b.ft,
            b.fta,
            b.oreb,
            b.dreb,
            b.reb,
            b.ast,
            b.stl,
            b.blk,
            b.tov,
            b.pf,
            b.pts,
            b.plus_minus,
            b.dnp_reason
        ],
    )?;
    Ok(())
}

pub fn insert_season_total(conn: &Connection, t: &SeasonTotalRow) -> SqlResult<()> {
    conn.execute(
        "INSERT INTO player_season_totals
            (player_br, season, team_br, g, mp, fg, fga, fg3, fg3a,
             ft, fta, oreb, dreb, reb, ast, stl, blk, tov, pf, pts)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10,
                 ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20)",
        rusqlite::params![
            t.player_br,
            t.season,
            t.team_br,
            t.g,
            t.mp,
            t.fg,
            t.fga,
            t.fg3,
            t.fg3a,
            t.ft,
            t.fta,
            t.oreb,
            t.dreb,
            t.reb,
            t.ast,
            t.stl,
            t.blk,
            t.tov,
            t.pf,
            t.pts
        ],
    )?;
    Ok(())
}

pub fn insert_tape_source(conn: &Connection, t: &TapeSource) -> SqlResult<()> {
    conn.execute(
        "INSERT INTO tape_sources
            (game_id, rank, source_class, url_or_pointer, match_confidence, verified_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![
            t.game_id,
            t.rank,
            t.source_class,
            t.url_or_pointer,
            t.match_confidence,
            t.verified_at
        ],
    )?;
    Ok(())
}

fn tape_source_from_row(row: &Row<'_>) -> rusqlite::Result<TapeSource> {
    Ok(TapeSource {
        game_id: row.get(0)?,
        rank: row.get(1)?,
        source_class: row.get(2)?,
        url_or_pointer: row.get(3)?,
        match_confidence: row.get(4)?,
        verified_at: row.get(5)?,
    })
}

/// All tape sources for a game, best rank first.
pub fn tape_sources_for(conn: &Connection, game_id: &str) -> SqlResult<Vec<TapeSource>> {
    let mut stmt = conn.prepare(
        "SELECT game_id, rank, source_class, url_or_pointer, match_confidence, verified_at
         FROM tape_sources WHERE game_id = ?1 ORDER BY rank ASC",
    )?;
    let rows = stmt.query_map([game_id], tape_source_from_row)?;
    rows.collect()
}

/// Record one sweep query, replacing the stale row for `(game_id, rung)`
/// when the rung is re-swept after the rescan window.
pub fn upsert_game_query(conn: &Connection, q: &GameQuery) -> SqlResult<()> {
    conn.execute(
        "INSERT INTO game_queries
            (game_id, rung, query_text, queried_at, best_match_level, review_url, review_title)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT (game_id, rung) DO UPDATE SET
            query_text = excluded.query_text,
            queried_at = excluded.queried_at,
            best_match_level = excluded.best_match_level,
            review_url = excluded.review_url,
            review_title = excluded.review_title",
        rusqlite::params![
            q.game_id,
            q.rung,
            q.query_text,
            q.queried_at,
            q.best_match_level,
            q.review_url,
            q.review_title
        ],
    )?;
    Ok(())
}

fn game_query_from_row(row: &Row<'_>) -> rusqlite::Result<GameQuery> {
    Ok(GameQuery {
        game_id: row.get(0)?,
        rung: row.get(1)?,
        query_text: row.get(2)?,
        queried_at: row.get(3)?,
        best_match_level: row.get(4)?,
        review_url: row.get(5)?,
        review_title: row.get(6)?,
    })
}

/// All recorded sweep queries for a game, in rung order.
pub fn game_queries_for(conn: &Connection, game_id: &str) -> SqlResult<Vec<GameQuery>> {
    let mut stmt = conn.prepare(
        "SELECT game_id, rung, query_text, queried_at, best_match_level, review_url, review_title
         FROM game_queries WHERE game_id = ?1 ORDER BY rung ASC",
    )?;
    let rows = stmt.query_map([game_id], game_query_from_row)?;
    rows.collect()
}

/// All recorded sweep queries across games, in game then rung order (backs
/// the catalog's all-games review list).
pub fn game_queries_all(conn: &Connection) -> SqlResult<Vec<GameQuery>> {
    let mut stmt = conn.prepare(
        "SELECT game_id, rung, query_text, queried_at, best_match_level, review_url, review_title
         FROM game_queries ORDER BY game_id ASC, rung ASC",
    )?;
    let rows = stmt.query_map([], game_query_from_row)?;
    rows.collect()
}

/// Record-or-replace one ranked tape source: a re-sweep that re-verifies a
/// rung refreshes its row instead of failing on the primary key.
pub fn upsert_tape_source(conn: &Connection, t: &TapeSource) -> SqlResult<()> {
    conn.execute(
        "INSERT INTO tape_sources
            (game_id, rank, source_class, url_or_pointer, match_confidence, verified_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT (game_id, rank) DO UPDATE SET
            source_class = excluded.source_class,
            url_or_pointer = excluded.url_or_pointer,
            match_confidence = excluded.match_confidence,
            verified_at = excluded.verified_at",
        rusqlite::params![
            t.game_id,
            t.rank,
            t.source_class,
            t.url_or_pointer,
            t.match_confidence,
            t.verified_at
        ],
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Cache Tier entries (issue #25)
// ---------------------------------------------------------------------------

/// Record-or-replace one Cache Tier download row: a re-fetch refreshes the
/// row (new path, byte count, state) instead of failing on the primary key.
/// `verified_at` is `Some` only on `Ready` rows; callers clear it when a
/// re-fetch moves the row back out of `Ready`.
pub fn upsert_cache_entry(conn: &Connection, e: &CacheEntry) -> SqlResult<()> {
    conn.execute(
        "INSERT INTO cache_entries
            (game_id, rank, source_class, local_path, bytes, verified_at, state)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT (game_id, rank) DO UPDATE SET
            source_class = excluded.source_class,
            local_path = excluded.local_path,
            bytes = excluded.bytes,
            verified_at = excluded.verified_at,
            state = excluded.state",
        rusqlite::params![
            e.game_id,
            e.rank,
            e.source_class,
            e.local_path,
            e.bytes,
            e.verified_at,
            e.state.as_str()
        ],
    )?;
    Ok(())
}

fn cache_entry_from_row(row: &Row<'_>) -> rusqlite::Result<CacheEntry> {
    let state_text: String = row.get(6)?;
    let state = CacheState::parse(&state_text).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            6,
            rusqlite::types::Type::Text,
            format!("unknown cache state {state_text:?}").into(),
        )
    })?;
    Ok(CacheEntry {
        game_id: row.get(0)?,
        rank: row.get(1)?,
        source_class: row.get(2)?,
        local_path: row.get(3)?,
        bytes: row.get(4)?,
        verified_at: row.get(5)?,
        state,
    })
}

/// All Cache Tier rows for a game, best rank first (whatever their state).
pub fn cache_entries_for(conn: &Connection, game_id: &str) -> SqlResult<Vec<CacheEntry>> {
    let mut stmt = conn.prepare(
        "SELECT game_id, rank, source_class, local_path, bytes, verified_at, state
         FROM cache_entries WHERE game_id = ?1 ORDER BY rank ASC",
    )?;
    let rows = stmt.query_map([game_id], cache_entry_from_row)?;
    rows.collect()
}

/// The playable local copy for a game, if any: the best-ranked `Ready` row.
/// Non-`Ready` rows (pending, fetching, failed) never surface here, so Play
/// resolves cache-first without ever decoding an unverified file.
pub fn ready_cache_entry_for(conn: &Connection, game_id: &str) -> SqlResult<Option<CacheEntry>> {
    let mut stmt = conn.prepare(
        "SELECT game_id, rank, source_class, local_path, bytes, verified_at, state
         FROM cache_entries WHERE game_id = ?1 AND state = 'Ready'
         ORDER BY rank ASC LIMIT 1",
    )?;
    let mut rows = stmt.query_map([game_id], cache_entry_from_row)?;
    match rows.next() {
        None => Ok(None),
        Some(row) => row.map(Some),
    }
}

/// Every `Ready` Cache Tier row across games, in mirror order (season, then
/// game, then rank) so a dry-run preview and its apply cannot diverge.
/// Backs the Drive mirror stage (`nbatv_catalog::drive`): only `Ready` rows
/// ever reach a manifest, and the order is deterministic whatever the
/// insertion history was.
pub fn ready_cache_entries(conn: &Connection) -> SqlResult<Vec<CacheEntry>> {
    let mut stmt = conn.prepare(
        "SELECT ce.game_id, ce.rank, ce.source_class, ce.local_path, ce.bytes, ce.verified_at, ce.state
         FROM cache_entries ce LEFT JOIN games g ON g.game_id = ce.game_id
         WHERE ce.state = 'Ready'
         ORDER BY COALESCE(g.season, 0), ce.game_id, ce.rank",
    )?;
    let rows = stmt.query_map([], cache_entry_from_row)?;
    rows.collect()
}

/// The cache state behind the Game view's compact status line: the row
/// Play would take — the best-ranked Ready entry when one exists (a
/// lower-rank failure must not hide a playable local file), else the
/// best-ranked row in any state.
pub fn cache_state_for(conn: &Connection, game_id: &str) -> SqlResult<Option<CacheState>> {
    let mut stmt = conn.prepare(
        "SELECT state FROM cache_entries WHERE game_id = ?1 \
         ORDER BY CASE WHEN state = 'Ready' THEN 0 ELSE 1 END, rank ASC LIMIT 1",
    )?;
    let mut rows = stmt.query_map([game_id], |row| row.get::<_, String>(0))?;
    match rows.next() {
        None => Ok(None),
        Some(text) => {
            let text = text?;
            Ok(CacheState::parse(&text))
        }
    }
}

// ---------------------------------------------------------------------------
// Reads: the catalog queries the Shell renders through (#19)
// ---------------------------------------------------------------------------
//
// Every list arrives in the order the Shell shows it (seasons oldest-first,
// games in date order), so the Shell maps rows to its model without
// re-sorting. Callers degrade to empty on error — an unreadable archive is
// an empty season list, never a panic.

/// All seasons, oldest first.
pub fn list_seasons(conn: &Connection) -> SqlResult<Vec<SeasonRow>> {
    let mut stmt =
        conn.prepare("SELECT league, year, label FROM seasons ORDER BY year ASC, league ASC")?;
    let rows = stmt.query_map([], |row| {
        Ok(SeasonRow {
            league: row.get(0)?,
            year: row.get(1)?,
            label: row.get(2)?,
        })
    })?;
    rows.collect()
}

/// All teams, in slug order.
pub fn list_teams(conn: &Connection) -> SqlResult<Vec<TeamRow>> {
    let mut stmt = conn.prepare(
        "SELECT br_slug, nba_team_id, franchise_id, city, name, abbrev, active_from, active_to
         FROM teams ORDER BY br_slug ASC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(TeamRow {
            br_slug: row.get(0)?,
            nba_team_id: row.get(1)?,
            franchise_id: row.get(2)?,
            city: row.get(3)?,
            name: row.get(4)?,
            abbrev: row.get(5)?,
            active_from: row.get(6)?,
            active_to: row.get(7)?,
        })
    })?;
    rows.collect()
}

const GAME_COLS: &str = "game_id, nba_game_id, league, season, date, game_type, \
    home_team, away_team, home_pts, away_pts, ot, arena, attendance, br_url, sources";

fn game_row_from_row(row: &Row<'_>) -> rusqlite::Result<GameRow> {
    Ok(GameRow {
        game_id: row.get(0)?,
        nba_game_id: row.get(1)?,
        league: row.get(2)?,
        season: row.get(3)?,
        date: row.get(4)?,
        game_type: row.get(5)?,
        home_team: row.get(6)?,
        away_team: row.get(7)?,
        home_pts: row.get(8)?,
        away_pts: row.get(9)?,
        ot: row.get(10)?,
        arena: row.get(11)?,
        attendance: row.get(12)?,
        br_url: row.get(13)?,
        sources: row.get(14)?,
    })
}

/// All games of one season (ending year, e.g. 1947 for 1946-47), in date
/// order. The `games.sources` JSON column is ingest bookkeeping, not the
/// live tape path — tape always comes from `tape_sources_for`.
pub fn games_in_season(conn: &Connection, season: i32) -> SqlResult<Vec<GameRow>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {GAME_COLS} FROM games WHERE season = ?1 ORDER BY date ASC, game_id ASC"
    ))?;
    let rows = stmt.query_map([season], game_row_from_row)?;
    rows.collect()
}

/// One game by its Basketball-Reference slug, if archived.
pub fn game_by_id(conn: &Connection, game_id: &str) -> SqlResult<Option<GameRow>> {
    let mut stmt = conn.prepare(&format!("SELECT {GAME_COLS} FROM games WHERE game_id = ?1"))?;
    let mut rows = stmt.query_map([game_id], game_row_from_row)?;
    match rows.next() {
        None => Ok(None),
        Some(row) => row.map(Some),
    }
}

fn box_team_from_row(row: &Row<'_>) -> rusqlite::Result<BoxTeamRow> {
    Ok(BoxTeamRow {
        game_id: row.get(0)?,
        team_br: row.get(1)?,
        mp: row.get(2)?,
        fg: row.get(3)?,
        fga: row.get(4)?,
        fg3: row.get(5)?,
        fg3a: row.get(6)?,
        ft: row.get(7)?,
        fta: row.get(8)?,
        oreb: row.get(9)?,
        dreb: row.get(10)?,
        reb: row.get(11)?,
        ast: row.get(12)?,
        stl: row.get(13)?,
        blk: row.get(14)?,
        tov: row.get(15)?,
        pf: row.get(16)?,
        pts: row.get(17)?,
        plus_minus: row.get(18)?,
    })
}

/// Team totals for one game, in slug order.
pub fn box_teams_for(conn: &Connection, game_id: &str) -> SqlResult<Vec<BoxTeamRow>> {
    let mut stmt = conn.prepare(
        "SELECT game_id, team_br, mp, fg, fga, fg3, fg3a, ft, fta,
                oreb, dreb, reb, ast, stl, blk, tov, pf, pts, plus_minus
         FROM box_team WHERE game_id = ?1 ORDER BY team_br ASC",
    )?;
    let rows = stmt.query_map([game_id], box_team_from_row)?;
    rows.collect()
}

fn box_player_from_row(row: &Row<'_>) -> rusqlite::Result<BoxPlayerRow> {
    Ok(BoxPlayerRow {
        game_id: row.get(0)?,
        team_br: row.get(1)?,
        player_br: row.get(2)?,
        starter: row.get(3)?,
        position: row.get(4)?,
        mp: row.get(5)?,
        fg: row.get(6)?,
        fga: row.get(7)?,
        fg3: row.get(8)?,
        fg3a: row.get(9)?,
        ft: row.get(10)?,
        fta: row.get(11)?,
        oreb: row.get(12)?,
        dreb: row.get(13)?,
        reb: row.get(14)?,
        ast: row.get(15)?,
        stl: row.get(16)?,
        blk: row.get(17)?,
        tov: row.get(18)?,
        pf: row.get(19)?,
        pts: row.get(20)?,
        plus_minus: row.get(21)?,
        dnp_reason: row.get(22)?,
    })
}

/// Player totals for one game, grouped by team.
pub fn box_players_for(conn: &Connection, game_id: &str) -> SqlResult<Vec<BoxPlayerRow>> {
    let mut stmt = conn.prepare(
        "SELECT game_id, team_br, player_br, starter, position, mp,
                fg, fga, fg3, fg3a, ft, fta, oreb, dreb, reb, ast,
                stl, blk, tov, pf, pts, plus_minus, dnp_reason
         FROM box_player WHERE game_id = ?1 ORDER BY team_br ASC, player_br ASC",
    )?;
    let rows = stmt.query_map([game_id], box_player_from_row)?;
    rows.collect()
}

/// Display names for one game's box players, joined to the players
/// directory. Players with no directory row are absent — callers fall back
/// to the slug so a missing name never hides the row.
pub fn player_names_for_game(conn: &Connection, game_id: &str) -> SqlResult<Vec<(String, String)>> {
    let mut stmt = conn.prepare(
        "SELECT players.br_slug, players.name FROM players
         JOIN box_player ON players.br_slug = box_player.player_br
         WHERE box_player.game_id = ?1",
    )?;
    let rows = stmt.query_map([game_id], |row| {
        let slug: String = row.get(0)?;
        let name: String = row.get(1)?;
        Ok((slug, name))
    })?;
    rows.collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn memdb() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        create_schema(&conn).expect("create_schema");
        conn
    }

    fn first_game() -> GameRow {
        GameRow {
            game_id: "194611010TRH".to_owned(),
            nba_game_id: None,
            league: "BAA".to_owned(),
            season: 1947,
            date: "1946-11-01".to_owned(),
            game_type: "REGULAR".to_owned(),
            home_team: "TRH".to_owned(),
            away_team: "NYK".to_owned(),
            home_pts: 66,
            away_pts: 68,
            ot: None,
            arena: Some("Maple Leaf Gardens".to_owned()),
            attendance: Some(7090),
            br_url: "https://www.basketball-reference.com/boxscores/194611010TRH.html".to_owned(),
            sources: "[]".to_owned(),
        }
    }

    #[test]
    fn schema_creates_all_ten_tables() {
        let conn = memdb();
        let mut names: Vec<String> = conn
            .prepare(
                "SELECT name FROM sqlite_master
                 WHERE type = 'table' AND name NOT LIKE 'sqlite_%'
                 ORDER BY name",
            )
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        names.sort();
        assert_eq!(
            names,
            vec![
                "box_player",
                "box_team",
                "cache_entries",
                "game_queries",
                "games",
                "player_season_totals",
                "players",
                "seasons",
                "tape_sources",
                "teams",
            ]
        );
    }

    #[test]
    fn schema_is_idempotent() {
        let conn = memdb();
        create_schema(&conn).expect("second create_schema must succeed");
    }

    #[test]
    fn nullable_era_columns_accept_1946_row() {
        let conn = memdb();
        insert_season(
            &conn,
            &SeasonRow {
                league: "BAA".to_owned(),
                year: 1947,
                label: "1946-47".to_owned(),
            },
        )
        .unwrap();
        for (slug, city, name) in [("NYK", "New York", "Knicks"), ("TRH", "Toronto", "Huskies")] {
            insert_team(
                &conn,
                &TeamRow {
                    br_slug: slug.to_owned(),
                    nba_team_id: None,
                    franchise_id: None,
                    city: city.to_owned(),
                    name: name.to_owned(),
                    abbrev: slug.to_owned(),
                    active_from: Some(1946),
                    active_to: if slug == "TRH" { Some(1947) } else { None },
                },
            )
            .unwrap();
        }
        insert_game(&conn, &first_game()).unwrap();

        // 1946-style team box: only FG/FT/PF/PTS recorded; everything
        // post-1950 (rebounds, assists, steals, blocks, threes, +/-) is NULL.
        // NYK 68 @ TRH 66, first game ever.
        for (team, fg, fga, ft, fta, pf, pts) in [
            ("NYK", 22, 60, 24, 30, 22, 68),
            ("TRH", 20, 55, 26, 34, 20, 66),
        ] {
            insert_box_team(
                &conn,
                &BoxTeamRow {
                    game_id: "194611010TRH".to_owned(),
                    team_br: team.to_owned(),
                    mp: Some("240".to_owned()),
                    fg: Some(fg),
                    fga: Some(fga),
                    fg3: None,
                    fg3a: None,
                    ft: Some(ft),
                    fta: Some(fta),
                    oreb: None,
                    dreb: None,
                    reb: None,
                    ast: None,
                    stl: None,
                    blk: None,
                    tov: None,
                    pf: Some(pf),
                    pts: Some(pts),
                    plus_minus: None,
                },
            )
            .unwrap();
        }

        // Player row with only era stats; DNP row with stats NULL + reason.
        insert_box_player(
            &conn,
            &BoxPlayerRow {
                game_id: "194611010TRH".to_owned(),
                team_br: "NYK".to_owned(),
                player_br: "ed-so".to_owned(),
                starter: None,
                position: Some("C".to_owned()),
                mp: None,
                fg: Some(7),
                fga: Some(15),
                fg3: None,
                fg3a: None,
                ft: Some(6),
                fta: Some(8),
                oreb: None,
                dreb: None,
                reb: None,
                ast: None,
                stl: None,
                blk: None,
                tov: None,
                pf: Some(4),
                pts: Some(20),
                plus_minus: None,
                dnp_reason: None,
            },
        )
        .unwrap();
        insert_box_player(
            &conn,
            &BoxPlayerRow {
                game_id: "194611010TRH".to_owned(),
                team_br: "TRH".to_owned(),
                player_br: "did-not".to_owned(),
                starter: None,
                position: None,
                mp: None,
                fg: None,
                fga: None,
                fg3: None,
                fg3a: None,
                ft: None,
                fta: None,
                oreb: None,
                dreb: None,
                reb: None,
                ast: None,
                stl: None,
                blk: None,
                tov: None,
                pf: None,
                pts: None,
                plus_minus: None,
                dnp_reason: Some("Did Not Play".to_owned()),
            },
        )
        .unwrap();

        let (pts, reb, ast): (i32, Option<i32>, Option<i32>) = conn
            .query_row(
                "SELECT pts, reb, ast FROM box_team
                 WHERE game_id = '194611010TRH' AND team_br = 'NYK'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!((pts, reb, ast), (68, None, None));

        let dnp: Option<String> = conn
            .query_row(
                "SELECT dnp_reason FROM box_player WHERE player_br = 'did-not'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(dnp.as_deref(), Some("Did Not Play"));

        // Totals row with only era columns.
        insert_player(
            &conn,
            &PlayerRow {
                br_slug: "ed-so".to_owned(),
                nba_person_id: None,
                name: "Ed S.".to_owned(),
                first_season: Some(1947),
                last_season: Some(1947),
            },
        )
        .unwrap();
        insert_season_total(
            &conn,
            &SeasonTotalRow {
                player_br: "ed-so".to_owned(),
                season: 1947,
                team_br: "NYK".to_owned(),
                g: 60,
                mp: None,
                fg: Some(200),
                fga: Some(700),
                fg3: None,
                fg3a: None,
                ft: Some(150),
                fta: Some(220),
                oreb: None,
                dreb: None,
                reb: None,
                ast: Some(30),
                stl: None,
                blk: None,
                tov: None,
                pf: Some(180),
                pts: Some(550),
            },
        )
        .unwrap();
    }

    #[test]
    fn tape_source_roundtrip_orders_by_rank() {
        let conn = memdb();
        insert_game(&conn, &first_game()).unwrap();
        for (rank, class, ptr) in [
            (3u8, "fan-rehost", "pointer:fan-cluster/194611010TRH"),
            (
                1u8,
                "internet-archive",
                "https://archive.org/details/194611010TRH",
            ),
            (
                6u8,
                "purchase-only",
                "pointer:classic-sports-catalog#194611010TRH",
            ),
        ] {
            insert_tape_source(
                &conn,
                &TapeSource {
                    game_id: "194611010TRH".to_owned(),
                    rank,
                    source_class: class.to_owned(),
                    url_or_pointer: ptr.to_owned(),
                    match_confidence: 0.9,
                    verified_at: "2026-09-07".to_owned(),
                },
            )
            .unwrap();
        }
        let got = tape_sources_for(&conn, "194611010TRH").unwrap();
        let ranks: Vec<u8> = got.iter().map(|t| t.rank).collect();
        assert_eq!(ranks, vec![1, 3, 6]);
        assert_eq!(
            got[0].playback_class(),
            Some(PlaybackClass::ProgressiveFile)
        );
        assert_eq!(
            got[1].playback_class(),
            Some(PlaybackClass::ExternalSurface)
        );
        assert_eq!(got[2].playback_class(), Some(PlaybackClass::Pointer));
    }

    #[test]
    fn playback_class_covers_all_rungs() {
        use PlaybackClass::*;
        let expected = [
            ExternalSurface, // 0 official-NBA-free-tier
            ProgressiveFile, // 1 Internet-Archive
            ExternalSurface, // 2 YouTube
            ExternalSurface, // 3 non-Anglo-rehost-cluster
            ProgressiveFile, // 4 standing-empty-corpus
            Pointer,         // 5 collector-catalogs
            Pointer,         // 6 purchase-only
            Pointer,         // 7 institutional
        ];
        for (rank, want) in expected.iter().enumerate() {
            assert_eq!(PlaybackClass::of_rank(rank as u8), Some(*want));
        }
        assert_eq!(PlaybackClass::of_rank(8), None);
    }

    #[test]
    fn game_id_parse_accepts_slug_rejects_garbage() {
        assert_eq!(
            GameId::parse("194611010TRH").map(|g| g.0),
            Some("194611010TRH".to_owned())
        );
        assert!(GameId::parse("").is_none());
        assert!(GameId::parse("194611010TR").is_none()); // too short
        assert!(GameId::parse("194611010TRHX").is_none()); // too long
        assert!(GameId::parse("19461101OTRH").is_none()); // letter in digits
        assert!(GameId::parse("194611010trh").is_none()); // lowercase
        assert!(GameId::parse("0024600001").is_none()); // NBA 10-digit id
        assert_eq!(
            GameId::parse("194611010TRH").unwrap().br_url(),
            "https://www.basketball-reference.com/boxscores/194611010TRH.html"
        );
    }

    #[test]
    fn game_type_check_rejects_unknown() {
        let conn = memdb();
        let mut bad = first_game();
        bad.game_id = "194611020CHS".to_owned();
        bad.game_type = "EXHIBITION".to_owned();
        assert!(insert_game(&conn, &bad).is_err());
    }

    #[test]
    fn catalog_reads_serve_the_shell() {
        let conn = memdb();
        insert_season(
            &conn,
            &SeasonRow {
                league: "BAA".to_owned(),
                year: 1947,
                label: "1946-47".to_owned(),
            },
        )
        .unwrap();
        insert_team(
            &conn,
            &TeamRow {
                br_slug: "TRH".to_owned(),
                nba_team_id: None,
                franchise_id: None,
                city: "Toronto".to_owned(),
                name: "Huskies".to_owned(),
                abbrev: "TRH".to_owned(),
                active_from: Some(1946),
                active_to: Some(1947),
            },
        )
        .unwrap();
        insert_game(&conn, &first_game()).unwrap();
        let mut second = first_game();
        second.game_id = "194611020CHS".to_owned();
        second.date = "1946-11-02".to_owned();
        insert_game(&conn, &second).unwrap();

        let seasons = list_seasons(&conn).unwrap();
        assert_eq!(seasons.len(), 1);
        assert_eq!(seasons[0].year, 1947);

        let teams = list_teams(&conn).unwrap();
        assert_eq!(teams.len(), 1);
        assert_eq!(teams[0].br_slug, "TRH");

        let games = games_in_season(&conn, 1947).unwrap();
        assert_eq!(games.len(), 2);
        assert_eq!(games[0].game_id, "194611010TRH");
        assert!(games_in_season(&conn, 1948).unwrap().is_empty());

        assert_eq!(
            game_by_id(&conn, "194611010TRH")
                .unwrap()
                .map(|g| g.away_team),
            Some("NYK".to_owned())
        );
        assert_eq!(game_by_id(&conn, "000000000AAA").unwrap(), None);
    }

    #[test]
    fn box_reads_join_player_names() {
        let conn = memdb();
        insert_game(&conn, &first_game()).unwrap();
        insert_box_team(
            &conn,
            &BoxTeamRow {
                game_id: "194611010TRH".to_owned(),
                team_br: "NYK".to_owned(),
                mp: Some("240".to_owned()),
                fg: Some(22),
                fga: None,
                fg3: None,
                fg3a: None,
                ft: Some(24),
                fta: Some(30),
                oreb: None,
                dreb: None,
                reb: None,
                ast: None,
                stl: None,
                blk: None,
                tov: None,
                pf: Some(22),
                pts: Some(68),
                plus_minus: None,
            },
        )
        .unwrap();
        insert_box_player(
            &conn,
            &BoxPlayerRow {
                game_id: "194611010TRH".to_owned(),
                team_br: "NYK".to_owned(),
                player_br: "ed-so".to_owned(),
                starter: Some(true),
                position: Some("C".to_owned()),
                mp: Some("38:00".to_owned()),
                fg: Some(7),
                fga: Some(15),
                fg3: None,
                fg3a: None,
                ft: Some(6),
                fta: Some(8),
                oreb: None,
                dreb: None,
                reb: None,
                ast: None,
                stl: None,
                blk: None,
                tov: None,
                pf: Some(4),
                pts: Some(20),
                plus_minus: None,
                dnp_reason: None,
            },
        )
        .unwrap();
        insert_player(
            &conn,
            &PlayerRow {
                br_slug: "ed-so".to_owned(),
                nba_person_id: None,
                name: "Ed S.".to_owned(),
                first_season: Some(1947),
                last_season: Some(1947),
            },
        )
        .unwrap();

        let teams = box_teams_for(&conn, "194611010TRH").unwrap();
        assert_eq!(teams.len(), 1);
        assert_eq!(teams[0].pts, Some(68));
        assert_eq!(teams[0].fga, None);
        assert!(box_teams_for(&conn, "000000000AAA").unwrap().is_empty());

        let players = box_players_for(&conn, "194611010TRH").unwrap();
        assert_eq!(players.len(), 1);
        assert_eq!(players[0].starter, Some(true));

        let names = player_names_for_game(&conn, "194611010TRH").unwrap();
        assert_eq!(names, vec![("ed-so".to_owned(), "Ed S.".to_owned())]);
        assert!(player_names_for_game(&conn, "000000000AAA")
            .unwrap()
            .is_empty());
    }

    #[test]
    fn game_queries_round_trip_in_rung_order() {
        let conn = memdb();
        for (rung, level) in [(2u8, "likely"), (0u8, "reject")] {
            upsert_game_query(
                &conn,
                &GameQuery {
                    game_id: "194611010TRH".to_owned(),
                    rung,
                    query_text: format!("query for rung {rung}"),
                    queried_at: "2026-01-01".to_owned(),
                    best_match_level: level.to_owned(),
                    review_url: None,
                    review_title: None,
                },
            )
            .unwrap();
        }
        let queries = game_queries_for(&conn, "194611010TRH").unwrap();
        assert_eq!(queries.len(), 2);
        assert_eq!(queries[0].rung, 0, "queries arrive in rung order");
        assert_eq!(queries[1].rung, 2);
        assert_eq!(queries[1].best_match_level, "likely");
        assert!(game_queries_for(&conn, "000000000AAA").unwrap().is_empty());
    }

    #[test]
    fn game_query_upsert_replaces_the_stale_row() {
        let conn = memdb();
        let query = |at: &str, level: &str| GameQuery {
            game_id: "194611010TRH".to_owned(),
            rung: 1,
            query_text: "ia sweep".to_owned(),
            queried_at: at.to_owned(),
            best_match_level: level.to_owned(),
            review_url: None,
            review_title: None,
        };
        upsert_game_query(&conn, &query("2026-01-01", "reject")).unwrap();
        upsert_game_query(&conn, &query("2026-04-02", "likely")).unwrap();
        let queries = game_queries_for(&conn, "194611010TRH").unwrap();
        assert_eq!(queries.len(), 1, "a rescan replaces, never duplicates");
        assert_eq!(queries[0].queried_at, "2026-04-02");
        assert_eq!(queries[0].best_match_level, "likely");
    }

    #[test]
    fn game_query_carries_review_evidence_and_rejects_bad_levels() {
        let conn = memdb();
        upsert_game_query(
            &conn,
            &GameQuery {
                game_id: "194611010TRH".to_owned(),
                rung: 2,
                query_text: "youtube sweep".to_owned(),
                queried_at: "2026-01-01".to_owned(),
                best_match_level: "review".to_owned(),
                review_url: Some("https://example.com/clip".to_owned()),
                review_title: Some("NYK highlights".to_owned()),
            },
        )
        .unwrap();
        let queries = game_queries_for(&conn, "194611010TRH").unwrap();
        assert_eq!(
            queries[0].review_url.as_deref(),
            Some("https://example.com/clip")
        );
        let bad = GameQuery {
            best_match_level: "maybe".to_owned(),
            ..queries[0].clone()
        };
        assert!(
            upsert_game_query(&conn, &bad).is_err(),
            "levels outside confirmed|likely|review|reject are rejected"
        );
    }

    #[test]
    fn tape_source_upsert_refreshes_the_verified_row() {
        let conn = memdb();
        let source = |at: &str| TapeSource {
            game_id: "194611010TRH".to_owned(),
            rank: 1,
            source_class: "internet-archive".to_owned(),
            url_or_pointer: "https://archive.org/details/194611010TRH".to_owned(),
            match_confidence: 1.0,
            verified_at: at.to_owned(),
        };
        upsert_tape_source(&conn, &source("2026-01-01")).unwrap();
        upsert_tape_source(&conn, &source("2026-04-02")).unwrap();
        let tapes = tape_sources_for(&conn, "194611010TRH").unwrap();
        assert_eq!(tapes.len(), 1);
        assert_eq!(tapes[0].verified_at, "2026-04-02");
    }

    fn cache_entry(rank: u8, state: CacheState) -> CacheEntry {
        CacheEntry {
            game_id: "194611010TRH".to_owned(),
            rank,
            source_class: "internet-archive".to_owned(),
            local_path: "data/cache/tape/1946-47/194611010TRH__NYK-at-TRH__ia.mp4".to_owned(),
            bytes: 714_000_000,
            verified_at: None,
            state,
        }
    }

    #[test]
    fn cache_entry_round_trips_through_all_five_states() {
        let conn = memdb();
        for (rank, state) in [
            CacheState::Pending,
            CacheState::Fetching,
            CacheState::Verifying,
            CacheState::Ready,
            CacheState::Failed,
        ]
        .into_iter()
        .enumerate()
        {
            let rank = (rank + 1) as u8;
            let mut entry = cache_entry(rank, state);
            if state == CacheState::Ready {
                entry.verified_at = Some("2026-09-08".to_owned());
            }
            upsert_cache_entry(&conn, &entry).unwrap();
        }
        let entries = cache_entries_for(&conn, "194611010TRH").unwrap();
        assert_eq!(entries.len(), 5, "one row per (game_id, rank)");
        let states: Vec<CacheState> = entries.iter().map(|e| e.state).collect();
        assert_eq!(
            states,
            vec![
                CacheState::Pending,
                CacheState::Fetching,
                CacheState::Verifying,
                CacheState::Ready,
                CacheState::Failed,
            ],
            "rows come back best rank first with states intact"
        );
        assert_eq!(
            entries[3].verified_at.as_deref(),
            Some("2026-09-08"),
            "verified_at survives the round trip"
        );
    }

    #[test]
    fn cache_state_check_rejects_unknown_strings() {
        let conn = memdb();
        let rejected = conn.execute(
            "INSERT INTO cache_entries
                (game_id, rank, source_class, local_path, bytes, verified_at, state)
             VALUES ('194611010TRH', 1, 'internet-archive', 'x.mp4', 0, NULL, 'Downloaded')",
            [],
        );
        assert!(
            rejected.is_err(),
            "states outside Pending|Fetching|Verifying|Ready|Failed are rejected"
        );
        assert!(CacheState::parse("Downloaded").is_none());
    }

    #[test]
    fn cache_upsert_refreshes_the_row_on_refetch() {
        let conn = memdb();
        upsert_cache_entry(&conn, &cache_entry(1, CacheState::Failed)).unwrap();
        let mut retry = cache_entry(1, CacheState::Ready);
        retry.bytes = 715_000_000;
        retry.verified_at = Some("2026-09-08".to_owned());
        upsert_cache_entry(&conn, &retry).unwrap();
        let entries = cache_entries_for(&conn, "194611010TRH").unwrap();
        assert_eq!(entries.len(), 1, "a re-fetch refreshes, never duplicates");
        assert_eq!(entries[0].state, CacheState::Ready);
        assert_eq!(entries[0].bytes, 715_000_000);
        assert_eq!(entries[0].verified_at.as_deref(), Some("2026-09-08"));
    }

    #[test]
    fn ready_lookup_skips_non_ready_rows() {
        let conn = memdb();
        upsert_cache_entry(&conn, &cache_entry(1, CacheState::Failed)).unwrap();
        upsert_cache_entry(&conn, &cache_entry(4, CacheState::Fetching)).unwrap();
        assert_eq!(
            ready_cache_entry_for(&conn, "194611010TRH").unwrap(),
            None,
            "no Ready row means no cache hit, whatever else is stored"
        );
        assert_eq!(
            cache_state_for(&conn, "194611010TRH").unwrap(),
            Some(CacheState::Failed),
            "the status line still sees the best-ranked row"
        );
        let mut ready = cache_entry(4, CacheState::Ready);
        ready.verified_at = Some("2026-09-08".to_owned());
        upsert_cache_entry(&conn, &ready).unwrap();
        assert_eq!(
            ready_cache_entry_for(&conn, "194611010TRH").unwrap(),
            Some(ready),
            "the best-ranked Ready row is the playable copy"
        );
        // The status line must agree with the row Play takes: a lower-rank
        // failure must not read "fetch failed" while the local file plays.
        assert_eq!(
            cache_state_for(&conn, "194611010TRH").unwrap(),
            Some(CacheState::Ready),
            "the Ready row outranks the Failed row for the status line"
        );
    }

    #[test]
    fn cache_lookups_are_empty_for_unknown_games() {
        let conn = memdb();
        assert_eq!(ready_cache_entry_for(&conn, "000000000AAA").unwrap(), None);
        assert_eq!(cache_state_for(&conn, "000000000AAA").unwrap(), None);
        assert!(cache_entries_for(&conn, "000000000AAA").unwrap().is_empty());
    }

    #[test]
    fn upsert_game_upgrades_scores_without_touching_sources() {
        let conn = memdb();
        // First ingest pass: schedule-only row, scores unknown (0), while
        // the catalog has already attached a tape source to the game.
        let mut bare = first_game();
        bare.home_pts = 0;
        bare.away_pts = 0;
        bare.sources = r#"["ia"]"#.to_owned();
        insert_game(&conn, &bare).unwrap();

        // A later pass with the box snapshot refreshes every parsed field,
        // but the catalog-owned sources JSON must survive the rewrite.
        upsert_game(&conn, &first_game()).unwrap();
        let (home_pts, away_pts, sources): (i32, i32, String) = conn
            .query_row(
                "SELECT home_pts, away_pts, sources FROM games WHERE game_id = '194611010TRH'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(home_pts, 66, "the box-derived score lands");
        assert_eq!(away_pts, 68);
        assert_eq!(
            sources, r#"["ia"]"#,
            "re-ingest never clobbers tape sources"
        );
        assert_eq!(game_count(&conn), 1, "upsert replaces, never duplicates");
    }

    #[test]
    fn upsert_team_replaces_the_observed_span() {
        let conn = memdb();
        let mut team = TeamRow {
            br_slug: "NYK".to_owned(),
            nba_team_id: None,
            franchise_id: None,
            city: "New York".to_owned(),
            name: "Knicks".to_owned(),
            abbrev: "NYK".to_owned(),
            active_from: Some(1946),
            active_to: None,
        };
        insert_team(&conn, &team).unwrap();
        // A later ingest over a grown crawl recomputes the span wholesale:
        // the row equals the crawl's current truth, not an accumulation.
        team.active_to = Some(2025);
        upsert_team(&conn, &team).unwrap();
        let (from, to): (i32, i32) = conn
            .query_row(
                "SELECT active_from, active_to FROM teams WHERE br_slug = 'NYK'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!((from, to), (1946, 2025));
        assert_eq!(team_count(&conn), 1, "upsert replaces, never duplicates");
    }

    fn game_count(conn: &Connection) -> i64 {
        count(conn, "games")
    }

    fn team_count(conn: &Connection) -> i64 {
        count(conn, "teams")
    }

    fn count(conn: &Connection, table: &str) -> i64 {
        conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .unwrap()
    }
}
