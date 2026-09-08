//! SQLite storage for the nba-tv personal NBA archive.
//!
//! Schema mirrors the batch contract: `seasons`, `teams`, `players`,
//! `games`, `box_team`, `box_player`, `player_season_totals`, plus
//! `tape_sources`. The `game_id` primary key is the Basketball-Reference
//! box-score slug (e.g. `194611010TRH`).
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

/// Create all 8 archive tables. Idempotent (`IF NOT EXISTS`).
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
    fn schema_creates_all_eight_tables() {
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
}
