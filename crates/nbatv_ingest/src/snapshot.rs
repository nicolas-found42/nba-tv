//! Snapshot ingest: `data/raw/br` -> the archive db.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use crate::br_html::{
    parse_box_page, parse_games_page, parse_totals_page, BoxPlayerInput, BoxTeamInput,
    GameIndexRow, SeasonTotalInput,
};
use crate::csv::validate_game_id;
use crate::fetch::read_snapshot_gz;

/// Playoff marker on a BR box page: the "other scores" section opens with
/// `game_summaries playoffs …` on every postseason page and with plain
/// `game_summaries` on every regular-season page (verified across the 2026-09
/// crawl waves: 350/350 pages of 1946-47 — 19 playoff, 331 regular — plus the
/// single archived postseason boxes of 1964-65/1974-75/1984-85/1994-95/
/// 2004-05/2014-15/2024-25 and regular openers of every season). The round
/// label (`h2`, e.g. `1947 BAA Finals`) is kept for future per-round typing;
/// only the binary regular/postseason decision feeds `game_type` today.
fn page_is_playoffs(html: &str) -> bool {
    html.contains("game_summaries playoffs")
}

/// `game_type` for one ingested game: the schedule pages carry no round
/// marker, so the box snapshot decides — playoffs marker present ⇒
/// `PLAYOFFS`, anything else (bare schedule row, regular box) ⇒ `REGULAR`.
fn game_type_for(has_box_html: bool, box_is_playoffs: bool) -> &'static str {
    if has_box_html && box_is_playoffs {
        "PLAYOFFS"
    } else {
        "REGULAR"
    }
}

// ---------------------------------------------------------------------------
// Snapshot ingest: `data/raw/br` -> the archive db
// ---------------------------------------------------------------------------

use nbatv_db::rusqlite::Connection;
use nbatv_db::{
    insert_box_player, insert_box_team, insert_season, insert_season_total, upsert_game,
    upsert_team, BoxPlayerRow, BoxTeamRow, GameRow, SeasonRow, SeasonTotalRow, TeamRow,
};

/// One run of the snapshot builder. State counters (`seasons`..`games_*`)
/// describe what the archive holds after the run and are stable across
/// re-ingests; the `inserted_*`/`upgraded_*` counters describe the writes
/// this run actually performed and drop back to zero on a resume pass.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct IngestReport {
    pub seasons: usize,
    pub teams: usize,
    pub games: usize,
    pub games_with_box: usize,
    pub games_without_box: usize,
    /// Box snapshot present but its team sides disagree with the schedule:
    /// the game is stored as a bare schedule row and the box is skipped.
    pub games_mismatched: usize,
    /// Schedule rows dropped for a missing/invalid box slug (summed over
    /// every page parsed this run).
    pub skipped_bad_slugs: usize,
    /// Box snapshots whose file name passes validation but has no schedule
    /// row: the schedule is the authority, so the page is ignored.
    pub skipped_orphan_box_pages: usize,
    /// Schedule rows whose game-id date disagrees with the season dir they
    /// sit in (a mislabeled page in the crawl): dropped, never stored under
    /// a wrong season.
    pub skipped_season_mismatch: usize,
    /// Team slugs not in [`TEAM_CITY_NAME`]: stored with slug-shaped names
    /// so games stay browsable, and reported for the crosswalk to grow.
    pub unknown_team_slugs: Vec<String>,
    pub inserted_box_teams: usize,
    pub inserted_box_players: usize,
    pub inserted_season_total_rows: usize,
    /// Games whose scores went from bare 0-0 to box-derived this run.
    pub upgraded_games: usize,
}

#[derive(Debug)]
pub enum IngestError {
    Io(std::io::Error),
    Db(nbatv_db::rusqlite::Error),
    /// A snapshot failed the gzip/UTF-8 decode the crawl format guarantees.
    Snapshot(String),
}

impl fmt::Display for IngestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IngestError::Io(e) => write!(f, "snapshot read failed: {e}"),
            IngestError::Db(e) => write!(f, "archive db write failed: {e}"),
            IngestError::Snapshot(e) => write!(f, "snapshot decode failed: {e}"),
        }
    }
}

impl std::error::Error for IngestError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            IngestError::Io(e) => Some(e),
            IngestError::Db(e) => Some(e),
            IngestError::Snapshot(_) => None,
        }
    }
}

impl From<std::io::Error> for IngestError {
    fn from(e: std::io::Error) -> Self {
        IngestError::Io(e)
    }
}

impl From<nbatv_db::rusqlite::Error> for IngestError {
    fn from(e: nbatv_db::rusqlite::Error) -> Self {
        IngestError::Db(e)
    }
}

/// Slug -> (city, name) for every franchise the crawl can surface, BAA
/// 1946-47 through the present. Same shape as [`franchise_crosswalk`]: a
/// checked-in domain constant, not a scrape. BR's league schedule pages
/// label most modern clubs by city alone, so the split is curated here
/// rather than guessed from link text; an unknown slug still gets a team
/// row (slug-shaped name) and is reported via
/// `IngestReport::unknown_team_slugs`.
const TEAM_CITY_NAME: &[(&str, &str, &str)] = &[
    ("ATL", "Atlanta", "Hawks"),
    ("BAL", "Baltimore", "Bullets"),
    ("BOS", "Boston", "Celtics"),
    ("BRK", "Brooklyn", "Nets"),
    ("BUF", "Buffalo", "Braves"),
    ("CHA", "Charlotte", "Bobcats"),
    ("CHH", "Charlotte", "Hornets"),
    ("CHO", "Charlotte", "Hornets"),
    ("CHI", "Chicago", "Bulls"),
    ("CHS", "Chicago", "Stags"),
    ("CIN", "Cincinnati", "Royals"),
    ("CLE", "Cleveland", "Cavaliers"),
    ("CLR", "Cleveland", "Rebels"),
    ("DAL", "Dallas", "Mavericks"),
    ("DEN", "Denver", "Nuggets"),
    ("DET", "Detroit", "Pistons"),
    ("DTF", "Detroit", "Falcons"),
    ("GSW", "Golden State", "Warriors"),
    ("HOU", "Houston", "Rockets"),
    ("IND", "Indiana", "Pacers"),
    ("KCK", "Kansas City", "Kings"),
    ("KCO", "Kansas City-Omaha", "Kings"),
    ("LAC", "Los Angeles", "Clippers"),
    ("LAL", "Los Angeles", "Lakers"),
    ("MEM", "Memphis", "Grizzlies"),
    ("MIA", "Miami", "Heat"),
    ("MIL", "Milwaukee", "Bucks"),
    ("MIN", "Minnesota", "Timberwolves"),
    ("NJN", "New Jersey", "Nets"),
    ("NOH", "New Orleans", "Hornets"),
    ("NOJ", "New Orleans", "Jazz"),
    ("NOP", "New Orleans", "Pelicans"),
    ("NYK", "New York", "Knicks"),
    ("OKC", "Oklahoma City", "Thunder"),
    ("ORL", "Orlando", "Magic"),
    ("PHI", "Philadelphia", "76ers"),
    ("PHO", "Phoenix", "Suns"),
    ("PHW", "Philadelphia", "Warriors"),
    ("PIT", "Pittsburgh", "Ironmen"),
    ("POR", "Portland", "Trail Blazers"),
    ("PRO", "Providence", "Steamrollers"),
    ("SAC", "Sacramento", "Kings"),
    ("SAS", "San Antonio", "Spurs"),
    ("SEA", "Seattle", "SuperSonics"),
    ("SFW", "San Francisco", "Warriors"),
    ("STB", "St. Louis", "Bombers"),
    ("STL", "St. Louis", "Hawks"),
    ("TOR", "Toronto", "Raptors"),
    ("TRH", "Toronto", "Huskies"),
    ("UTA", "Utah", "Jazz"),
    ("WAS", "Washington", "Wizards"),
    ("WSB", "Washington", "Bullets"),
    ("WSC", "Washington", "Capitols"),
];

/// `1946-47` -> 1947, with the century rollover (`1999-00` -> 2000). NBA
/// endings run `47`..`99` then `00`..: a short year below `46` belongs to
/// the 2000s.
pub(crate) fn season_slug_to_ending_year(slug: &str) -> Option<i32> {
    let bytes = slug.as_bytes();
    if bytes.len() != 7 || bytes[4] != b'-' {
        return None;
    }
    let numeric = |slice: &[u8]| slice.iter().all(|b| b.is_ascii_digit());
    if !numeric(&bytes[..4]) || !numeric(&bytes[5..]) {
        return None;
    }
    let short: i32 = slug[5..].parse().ok()?;
    let century = if short <= 45 { 2000 } else { 1900 };
    Some(century + short)
}

/// BAA through 1948-49, NBA from 1949-50 on.
pub(crate) fn league_for_ending_year(year: i32) -> &'static str {
    if year <= 1949 {
        "BAA"
    } else {
        "NBA"
    }
}

/// Snapshots are stored gzip (see [`write_snapshot_gz`]); the earliest
/// crawl waves predate that and stored plain UTF-8, so the magic bytes
/// decide which decoder runs. Anything else is a hard decode error.
pub(crate) fn read_snapshot_page(path: &Path) -> Result<String, IngestError> {
    let bytes = std::fs::read(path)?;
    let decode = |r: Result<String, IngestError>| {
        r.map_err(|e| match e {
            IngestError::Snapshot(detail) => {
                IngestError::Snapshot(format!("{}: {detail}", path.display()))
            }
            other => other,
        })
    };
    if bytes.starts_with(&[0x1f, 0x8b]) {
        decode(read_snapshot_gz(path).map_err(|e| IngestError::Snapshot(e.to_string())))
    } else {
        decode(
            String::from_utf8(bytes)
                .map_err(|e| IngestError::Snapshot(format!("snapshot is not UTF-8: {e}"))),
        )
    }
}

fn sorted_entries(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .collect();
    entries.sort();
    Ok(entries)
}

/// Ingest a snapshot crawl (`data/raw/br`, one directory per season slug)
/// into the archive db. Idempotent: safe to re-run after the crawl grows.
///
/// Honesty notes, all visible in [`IngestReport`]:
/// - `game_type` is `PLAYOFFS` exactly when the game's box snapshot carries
///   BR's postseason marker (`game_summaries playoffs`); bare schedule rows
///   stay `REGULAR` until their snapshot lands (the schedule pages carry no
///   round marker).
/// - `ot`/`arena`/`attendance` are NULL: the schedule parser does not
///   extract them yet.
/// - Games whose box snapshot is absent stay as bare schedule rows (0-0)
///   and upgrade automatically when a later crawl adds the snapshot.
///
/// The whole run commits atomically; teams are re-upserted from the full
/// crawl's observed span so a grown crawl widens the spans wholesale.
pub fn ingest_snapshot_dir(conn: &Connection, root: &Path) -> Result<IngestReport, IngestError> {
    nbatv_db::create_schema(conn)?;
    let tx = conn.unchecked_transaction()?;
    let mut report = IngestReport::default();
    let mut team_spans: BTreeMap<String, (i32, i32)> = BTreeMap::new();

    for entry in sorted_entries(root)? {
        let Some(slug) = entry.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let Some(year) = season_slug_to_ending_year(slug) else {
            continue;
        };
        if !entry.is_dir() {
            continue;
        }
        ingest_season_dir(&tx, &mut report, &mut team_spans, &entry, slug, year)?;
    }

    // A club observed in the crawl's latest season has no recorded end:
    // its `active_to` stays NULL (the Shell reads that as "active"), so
    // only clubs whose slug stops before the frontier get the defunct tag.
    let crawl_frontier = team_spans.values().map(|(_, to)| *to).max();
    for (slug, (from, to)) in &team_spans {
        let active_to = if Some(*to) == crawl_frontier {
            None
        } else {
            Some(*to)
        };
        let (city, name) = match TEAM_CITY_NAME.iter().find(|(s, _, _)| s == slug) {
            Some((_, city, name)) => ((*city).to_owned(), (*name).to_owned()),
            None => {
                report.unknown_team_slugs.push(slug.clone());
                (slug.clone(), slug.clone())
            }
        };
        upsert_team(
            &tx,
            &TeamRow {
                br_slug: slug.clone(),
                nba_team_id: None,
                franchise_id: None,
                city,
                name,
                abbrev: slug.clone(),
                active_from: Some(*from),
                active_to,
            },
        )?;
    }
    report.teams = team_spans.len();

    tx.commit()?;
    Ok(report)
}

fn ingest_season_dir(
    tx: &Connection,
    report: &mut IngestReport,
    team_spans: &mut BTreeMap<String, (i32, i32)>,
    dir: &Path,
    slug: &str,
    year: i32,
) -> Result<(), IngestError> {
    let league = league_for_ending_year(year);

    // Schedule first: the league index pages are the authority for which
    // games exist; full-season and monthly pages dedup by game id.
    let mut schedule: BTreeMap<String, GameIndexRow> = BTreeMap::new();
    let mut schedule_pages = 0;
    for path in sorted_entries(dir)? {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !name.starts_with("_games") || !name.ends_with(".html") {
            continue;
        }
        let html = read_snapshot_page(&path)?;
        let page = parse_games_page(&html);
        report.skipped_bad_slugs += page.skipped_bad_slugs;
        for row in page.rows {
            let id_year: i32 = row.game_id[..4].parse().unwrap_or(0);
            if id_year != year - 1 && id_year != year {
                report.skipped_season_mismatch += 1;
                continue;
            }
            schedule.entry(row.game_id.clone()).or_insert(row);
        }
        schedule_pages += 1;
    }
    if schedule_pages == 0 {
        return Ok(());
    }

    let season_known: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM seasons WHERE league = ?1 AND year = ?2)",
        nbatv_db::rusqlite::params![league, year],
        |row| row.get(0),
    )?;
    if !season_known {
        insert_season(
            tx,
            &SeasonRow {
                league: league.to_owned(),
                year,
                label: slug.to_owned(),
            },
        )?;
    }
    report.seasons += 1;

    for row in schedule.values() {
        for team in [row.home_br.as_str(), row.away_br.as_str()] {
            let span = team_spans
                .entry(team.to_owned())
                .or_insert((year - 1, year - 1));
            span.0 = span.0.min(year - 1);
            span.1 = span.1.max(year - 1);
        }
    }

    for (game_id, row) in &schedule {
        let box_path = dir.join(format!("{game_id}.html"));
        if !box_path.exists() {
            // No box snapshot: no playoff signal (schedule pages never carry
            // one), so the row stays REGULAR until the crawl adds the page.
            upsert_game(tx, &bare_game_row(game_id, row, year, league, "REGULAR"))?;
            report.games_without_box += 1;
            continue;
        }
        let html = read_snapshot_page(&box_path)?;
        let game_type = game_type_for(true, page_is_playoffs(&html));
        let (teams, players) = parse_box_page(&html);
        let side_pts = |team: &str| -> Option<i32> {
            teams.iter().find(|t| t.team_br == team).and_then(|t| t.pts)
        };
        match (side_pts(&row.home_br), side_pts(&row.away_br)) {
            (Some(home_pts), Some(away_pts)) => {
                let prior: Option<(i32, i32)> = tx
                    .query_row(
                        "SELECT home_pts, away_pts FROM games WHERE game_id = ?1",
                        nbatv_db::rusqlite::params![game_id],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                    .map(Some)
                    .or_else(|e| {
                        if e == nbatv_db::rusqlite::Error::QueryReturnedNoRows {
                            Ok(None)
                        } else {
                            Err(e)
                        }
                    })?;
                let mut game = bare_game_row(game_id, row, year, league, game_type);
                game.home_pts = home_pts;
                game.away_pts = away_pts;
                upsert_game(tx, &game)?;
                if prior == Some((0, 0)) {
                    report.upgraded_games += 1;
                }
                let has_box: bool = tx.query_row(
                    "SELECT EXISTS(SELECT 1 FROM box_team WHERE game_id = ?1)",
                    nbatv_db::rusqlite::params![game_id],
                    |r| r.get(0),
                )?;
                if !has_box {
                    for team in teams {
                        insert_box_team(tx, &box_team_row(game_id, team))?;
                        report.inserted_box_teams += 1;
                    }
                    for player in players {
                        insert_box_player(tx, &box_player_row(game_id, player))?;
                        report.inserted_box_players += 1;
                    }
                }
                report.games_with_box += 1;
            }
            _ => {
                upsert_game(tx, &bare_game_row(game_id, row, year, league, game_type))?;
                report.games_mismatched += 1;
            }
        }
    }
    report.games += schedule.len();

    for path in sorted_entries(dir)? {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !name.ends_with(".html") || name.starts_with('_') {
            continue;
        }
        let stem = name.strip_suffix(".html").unwrap_or(name);
        if validate_game_id(stem) && !schedule.contains_key(stem) {
            report.skipped_orphan_box_pages += 1;
        }
    }

    let totals_path = dir.join("_totals.html");
    if totals_path.exists() {
        let html = read_snapshot_page(&totals_path)?;
        for total in parse_totals_page(&html, year) {
            let known: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM player_season_totals \
                 WHERE player_br = ?1 AND season = ?2 AND team_br = ?3)",
                nbatv_db::rusqlite::params![total.player_br, total.season, total.team_br],
                |row| row.get(0),
            )?;
            if known {
                continue;
            }
            insert_season_total(tx, &season_total_row(total))?;
            report.inserted_season_total_rows += 1;
        }
    }
    Ok(())
}

/// The schedule-authority row: full identity, scores unknown (0-0) until a
/// box snapshot upgrades them. `game_type` comes from the box page's playoff
/// marker when the snapshot exists; bare schedule rows stay `REGULAR` (no
/// signal — the schedule pages never carry one).
fn bare_game_row(
    game_id: &str,
    row: &GameIndexRow,
    year: i32,
    league: &str,
    game_type: &str,
) -> GameRow {
    GameRow {
        game_id: game_id.to_owned(),
        nba_game_id: None,
        league: league.to_owned(),
        season: year,
        date: row.date.clone(),
        game_type: game_type.to_owned(),
        home_team: row.home_br.clone(),
        away_team: row.away_br.clone(),
        home_pts: 0,
        away_pts: 0,
        ot: None,
        arena: None,
        attendance: None,
        br_url: format!("https://www.basketball-reference.com/boxscores/{game_id}.html"),
        sources: "[]".to_owned(),
    }
}

fn box_team_row(game_id: &str, input: BoxTeamInput) -> BoxTeamRow {
    let BoxTeamInput {
        team_br,
        mp,
        fg,
        fga,
        fg3,
        fg3a,
        ft,
        fta,
        oreb,
        dreb,
        reb,
        ast,
        stl,
        blk,
        tov,
        pf,
        pts,
        plus_minus,
    } = input;
    BoxTeamRow {
        game_id: game_id.to_owned(),
        team_br,
        mp,
        fg,
        fga,
        fg3,
        fg3a,
        ft,
        fta,
        oreb,
        dreb,
        reb,
        ast,
        stl,
        blk,
        tov,
        pf,
        pts,
        plus_minus,
    }
}

fn box_player_row(game_id: &str, input: BoxPlayerInput) -> BoxPlayerRow {
    let BoxPlayerInput {
        team_br,
        player_br,
        starter,
        position,
        mp,
        fg,
        fga,
        fg3,
        fg3a,
        ft,
        fta,
        oreb,
        dreb,
        reb,
        ast,
        stl,
        blk,
        tov,
        pf,
        pts,
        plus_minus,
        dnp_reason,
    } = input;
    BoxPlayerRow {
        game_id: game_id.to_owned(),
        team_br,
        player_br,
        starter,
        position,
        mp,
        fg,
        fga,
        fg3,
        fg3a,
        ft,
        fta,
        oreb,
        dreb,
        reb,
        ast,
        stl,
        blk,
        tov,
        pf,
        pts,
        plus_minus,
        dnp_reason,
    }
}

fn season_total_row(input: SeasonTotalInput) -> SeasonTotalRow {
    let SeasonTotalInput {
        player_br,
        season,
        team_br,
        g,
        mp,
        fg,
        fga,
        fg3,
        fg3a,
        ft,
        fta,
        oreb,
        dreb,
        reb,
        ast,
        stl,
        blk,
        tov,
        pf,
        pts,
    } = input;
    SeasonTotalRow {
        player_br,
        season,
        team_br,
        g,
        mp,
        fg,
        fga,
        fg3,
        fg3a,
        ft,
        fta,
        oreb,
        dreb,
        reb,
        ast,
        stl,
        blk,
        tov,
        pf,
        pts,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playoff_marker_matches_the_real_page_classes() {
        // Real shape, Finals page: `game_summaries playoffs compressed`.
        assert!(page_is_playoffs(
            "<div class=\"game_summaries playoffs compressed\"><h2>1947 BAA Finals</h2></div>"
        ));
        // Real shape, regular page: `game_summaries compressed`.
        assert!(!page_is_playoffs(
            "<div class=\"game_summaries compressed\"><h2>BAA Scores</h2></div>"
        ));
        // No summaries div at all (e.g. a stripped fixture): regular.
        assert!(!page_is_playoffs("<html><body>box</body></html>"));
        // The plain word "playoffs" in nav text is not the marker.
        assert!(!page_is_playoffs("<a href=\"/playoffs/\">Playoffs</a>"));
    }

    #[test]
    fn game_type_never_guesses_without_a_snapshot() {
        assert_eq!(game_type_for(true, true), "PLAYOFFS");
        assert_eq!(game_type_for(true, false), "REGULAR");
        // No box page: no signal, regular — even in June.
        assert_eq!(game_type_for(false, false), "REGULAR");
    }
}
