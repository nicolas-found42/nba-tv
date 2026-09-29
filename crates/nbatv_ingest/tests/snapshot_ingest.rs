//! Seam test for the snapshot builder (`ingest_snapshot_dir`).
//!
//! A minimal on-disk crawl — one season dir holding a schedule page, one box
//! snapshot, and a totals page — must fill the archive schema through the
//! real `nbatv_db` writers, and a second run over the same dir must be a
//! no-op (resume), not a duplicate pass.

use nbatv_ingest::{ingest_snapshot_dir, parse_games_page, parse_totals_page};
use rusqlite::Connection;
use std::path::Path;

/// Same shape as the e2e fixture: three rows — the played opener (box file
/// present), a second played game (no box file), and a future slugless row.
const GAMES_HTML: &str = "\
<table class=\"stats_table\" id=\"games\">\
<thead><tr>\
<th data-stat=\"date_game\">Date</th>\
<th data-stat=\"visitor_team_name\">Visitor/Neutral</th>\
<th data-stat=\"home_team_name\">Home/Neutral</th>\
<th data-stat=\"box_score_text\">Box Score</th>\
</tr></thead><tbody>\
<tr>\
<th data-stat=\"date_game\" csk=\"1946-11-01\">Fri, Nov 1, 1946</th>\
<td data-stat=\"visitor_team_name\"><a href=\"/teams/NYK/1947.html\">New York Knicks</a></td>\
<td data-stat=\"home_team_name\"><a href=\"/teams/TRH/1947.html\">Toronto Huskies</a></td>\
<td data-stat=\"box_score_text\"><a href=\"/boxscores/194611010TRH.html\">Box Score</a></td>\
</tr>\
<tr>\
<th data-stat=\"date_game\" csk=\"1946-11-02\">Sat, Nov 2, 1946</th>\
<td data-stat=\"visitor_team_name\"><a href=\"/teams/TRH/1947.html\">Toronto Huskies</a></td>\
<td data-stat=\"home_team_name\"><a href=\"/teams/PHI/1947.html\">Philadelphia 76ers</a></td>\
<td data-stat=\"box_score_text\"><a href=\"/boxscores/194611020PHI.html\">Box Score</a></td>\
</tr>\
<tr>\
<th data-stat=\"date_game\" csk=\"1946-11-04\">Mon, Nov 4, 1946</th>\
<td data-stat=\"visitor_team_name\"><a href=\"/teams/TRH/1947.html\">Toronto Huskies</a></td>\
<td data-stat=\"home_team_name\"><a href=\"/teams/PIT/1947.html\">Pittsburgh Ironmen</a></td>\
<td data-stat=\"box_score_text\"></td>\
</tr>\
</tbody></table>";

const BOX_HTML: &str = "\
<div class=\"scorebox_meta\"><div><strong>November 1, 1946</strong>, Maple Leaf Gardens</div></div>\
<table class=\"stats_table\" id=\"box-NYK-game-basic\">\
<tbody>\
<tr>\
<th data-stat=\"player\"><a href=\"/players/s/sadowed01.html\">Ed Sadowski</a></th>\
<td data-stat=\"fg\">7</td><td data-stat=\"fga\">15</td>\
<td data-stat=\"ft\">6</td><td data-stat=\"fta\">8</td>\
<td data-stat=\"pf\">4</td><td data-stat=\"pts\">20</td>\
</tr>\
<tr>\
<th data-stat=\"player\">Team Totals</th>\
<td data-stat=\"mp\">240</td>\
<td data-stat=\"fg\">22</td><td data-stat=\"fga\">60</td>\
<td data-stat=\"ft\">24</td><td data-stat=\"fta\">30</td>\
<td data-stat=\"pf\">22</td><td data-stat=\"pts\">68</td>\
</tr>\
</tbody></table>\
<table class=\"stats_table\" id=\"box-TRH-game-basic\">\
<tbody>\
<tr>\
<th data-stat=\"player\"><a href=\"/players/m/mogusle01.html\">Leo Mogus</a></th>\
<td data-stat=\"fg\">6</td><td data-stat=\"fga\">14</td>\
<td data-stat=\"ft\">5</td><td data-stat=\"fta\">7</td>\
<td data-stat=\"pf\">3</td><td data-stat=\"pts\">17</td>\
</tr>\
<tr>\
<th data-stat=\"player\">Team Totals</th>\
<td data-stat=\"mp\">240</td>\
<td data-stat=\"fg\">20</td><td data-stat=\"fga\">55</td>\
<td data-stat=\"ft\">26</td><td data-stat=\"fta\">34</td>\
<td data-stat=\"pf\">20</td><td data-stat=\"pts\">66</td>\
</tr>\
</tbody></table>";

const TOTALS_HTML: &str = "\
<table class=\"stats_table\" id=\"totals_stats\">\
<thead><tr><th data-stat=\"player\">Player</th><th data-stat=\"team\">Tm</th>\
<th data-stat=\"g\">G</th></tr></thead><tbody>\
<tr>\
<th data-stat=\"player\"><a href=\"/players/s/sadowed01.html\">Ed Sadowski</a></th>\
<td data-stat=\"team\">NYK</td>\
<td data-stat=\"g\">60</td><td data-stat=\"mp\">None</td>\
<td data-stat=\"fg\"></td><td data-stat=\"fga\"></td>\
<td data-stat=\"ft\"></td><td data-stat=\"fta\"></td>\
<td data-stat=\"pf\"></td><td data-stat=\"pts\">550</td>\
</tr>\
</tbody></table>";

fn write(dir: &Path, name: &str, body: &str) {
    std::fs::write(dir.join(name), nbatv_ingest::gzip_encode(body.as_bytes()))
        .expect("fixture file");
}

/// The earliest crawl waves stored plain UTF-8: cover both shapes.
fn write_plain(dir: &Path, name: &str, body: &str) {
    std::fs::write(dir.join(name), body).expect("fixture file");
}

fn fixture_crawl(name: &str) -> temp::TempDir {
    let root = temp::temp_dir(name);
    let season = root.path().join("1946-47");
    std::fs::create_dir_all(&season).expect("season dir");
    write(&season, "_games.html", GAMES_HTML);
    // Monthly splits overlap the full page: dedup by game id is load-bearing
    // (the real 1946-47 dir holds the full page plus seven monthly splits).
    write(&season, "_games-november.html", GAMES_HTML);
    write(&season, "194611010TRH.html", BOX_HTML);
    write_plain(&season, "_totals.html", TOTALS_HTML);
    root
}

fn count(conn: &Connection, table: &str) -> i64 {
    conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
        row.get(0)
    })
    .expect("count")
}

#[test]
fn snapshot_crawl_fills_the_archive_and_reingest_is_a_noop() {
    let conn = Connection::open_in_memory().expect("in-memory db");
    let crawl = fixture_crawl("seam");

    let first = ingest_snapshot_dir(&conn, crawl.path()).expect("first ingest");

    // State counters: schedule rows dedup across the full and monthly pages.
    assert_eq!(first.seasons, 1);
    assert_eq!(
        first.teams, 3,
        "NYK, TRH, PHI — PIT's slugless row is skipped"
    );
    assert_eq!(first.games, 2);
    assert_eq!(first.games_with_box, 1);
    assert_eq!(first.games_without_box, 1);
    assert_eq!(first.games_mismatched, 0);
    assert_eq!(
        first.skipped_bad_slugs,
        2 * parse_games_page(GAMES_HTML).skipped_bad_slugs,
        "each parsed page contributes its own skip count (rows dedup, skips sum)"
    );
    assert_eq!(first.skipped_orphan_box_pages, 0);
    assert!(
        first.unknown_team_slugs.is_empty(),
        "all fixture slugs in the crosswalk"
    );
    assert_eq!(first.inserted_box_teams, 2);
    assert_eq!(first.inserted_box_players, 2);
    assert_eq!(
        first.inserted_season_total_rows,
        parse_totals_page(TOTALS_HTML, 1947).len()
    );

    // The opener's scores come from the box snapshot's team totals, mapped
    // through the schedule's home/away sides; the bare row stays 0-0.
    let (home_pts, away_pts): (i32, i32) = conn
        .query_row(
            "SELECT home_pts, away_pts FROM games WHERE game_id = '194611010TRH'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!((home_pts, away_pts), (66, 68), "TRH hosted; NYK visited");
    let bare_pts: i64 = conn
        .query_row(
            "SELECT home_pts FROM games WHERE game_id = '194611020PHI'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(bare_pts, 0, "no snapshot yet: the schedule row stays bare");

    assert_eq!(count(&conn, "seasons"), 1);
    assert_eq!(count(&conn, "teams"), 3);
    assert_eq!(count(&conn, "games"), 2);
    assert_eq!(count(&conn, "box_team"), 2);
    assert_eq!(count(&conn, "box_player"), 2);
    // Every fixture club reaches the crawl frontier, so none carries an
    // observed end: active_to stays NULL (the Shell reads that as active).
    let defunct: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM teams WHERE active_to IS NOT NULL",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(defunct, 0, "frontier clubs have no recorded end");

    // A grown totals page lands only its new rows on the next pass.
    write(
        &crawl.path().join("1946-47"),
        "_totals.html",
        &TOTALS_HTML.replace(
            "</tbody></table>",
            "<tr><th data-stat=\"player\"><a href=\"/players/k/kaploso01.html\">Sonny Kaplow</a></th>\n\
             <td data-stat=\"team\">NYK</td>\n\
             <td data-stat=\"g\">54</td><td data-stat=\"pts\">431</td></tr></tbody></table>",
        ),
    );

    // Resume: the second pass inserts nothing and reports the same state.
    let second = ingest_snapshot_dir(&conn, crawl.path()).expect("second ingest");
    assert_eq!(
        second.seasons, first.seasons,
        "state counters are stable across runs"
    );
    assert_eq!(second.games, first.games);
    assert_eq!(second.games_with_box, first.games_with_box);
    assert_eq!(second.inserted_box_teams, 0, "box rows already present");
    assert_eq!(second.inserted_box_players, 0);
    assert_eq!(
        second.inserted_season_total_rows, 1,
        "the grown totals page lands exactly its new row"
    );
    assert_eq!(second.upgraded_games, 0);

    assert_eq!(count(&conn, "games"), 2, "no duplicate games on resume");
    assert_eq!(
        count(&conn, "box_team"),
        2,
        "no duplicate box rows on resume"
    );
    assert_eq!(count(&conn, "box_player"), 2);
    assert_eq!(
        count(&conn, "player_season_totals"),
        parse_totals_page(TOTALS_HTML, 1947).len() as i64 + 1
    );
}

#[test]
fn orphan_box_page_is_counted_and_ignored() {
    let conn = Connection::open_in_memory().expect("in-memory db");
    let crawl = fixture_crawl("orphan");
    // A valid-looking snapshot with no schedule row: the schedule is the
    // authority, so the page counts as skipped and inserts nothing.
    write(&crawl.path().join("1946-47"), "194611030PRO.html", BOX_HTML);

    // A mislabeled schedule page (1975 rows inside the 1946-47 dir) is
    // dropped row-by-row instead of poisoning the season.
    write(
        &crawl.path().join("1946-47"),
        "_games-december.html",
        &GAMES_HTML
            .replace("194611010TRH", "197511010TRH")
            .replace("194611020PHI", "197511020PHI")
            .replace("194611040", "197511040"),
    );

    let report = ingest_snapshot_dir(&conn, crawl.path()).expect("ingest");
    assert_eq!(report.skipped_orphan_box_pages, 1);
    assert_eq!(
        report.skipped_season_mismatch, 2,
        "every slugged row of the mislabeled page is dropped"
    );
    assert_eq!(count(&conn, "games"), 2, "the orphan adds no game row");
}

/// Scratch dir that removes itself on drop (repo convention: best-effort
/// cleanup, no tempdir crate).
mod temp {
    use std::path::{Path, PathBuf};

    pub struct TempDir(PathBuf);

    impl TempDir {
        pub fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    pub fn temp_dir(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("nbatv-ingest-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        TempDir(dir)
    }
}
