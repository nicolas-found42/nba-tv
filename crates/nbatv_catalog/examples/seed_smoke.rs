//! Seed the tiny archive the smoke log in `runner.rs` documents, so the
//! monthly bounded smoke re-runs from the repo as written. Created on
//! demand (the smoke used a throwaway that was deleted — this checked-in
//! example is the durable replacement):
//!
//! ```sh
//! cargo run -q -p nbatv_catalog --example seed_smoke -- /tmp/nbatv-smoke/archive.db
//! cargo run -q -p nbatv_catalog --bin nbatv-catalog-runner -- 1989-90 1989-90 \
//!   --db /tmp/nbatv-smoke/archive.db --manifest /tmp/nbatv-smoke/files-from.txt \
//!   --now 2026-09-08
//! cargo run -q -p nbatv_catalog --bin nbatv-catalog-runner -- 1946-47 1946-47 \
//!   --db /tmp/nbatv-smoke/archive.db --manifest /tmp/nbatv-smoke/files-from.txt \
//!   --now 2026-09-08
//! ```
//!
//! Seeds exactly two games (no seasons/teams rows needed — the runner reads
//! `games` and `game_context_for` needs only the games row): the 1946-47
//! opener (NYK @ TRH) and the 1990 Finals G5 (POR @ DET), the two the
//! smoke log walks.

use nbatv_catalog::GameContext;
use nbatv_db::{create_schema, insert_game, GameRow};
use rusqlite::Connection;

fn game(season: i32, game_id: &str, date: &str, home: &str, away: &str) -> GameRow {
    GameRow {
        game_id: game_id.to_owned(),
        nba_game_id: None,
        league: "NBA".to_owned(),
        season,
        date: date.to_owned(),
        game_type: if game_id.ends_with("POR") {
            "PLAYOFFS".to_owned()
        } else {
            "REGULAR".to_owned()
        },
        home_team: home.to_owned(),
        away_team: away.to_owned(),
        home_pts: 1,
        away_pts: 1,
        ot: None,
        arena: None,
        attendance: None,
        br_url: format!("https://www.basketball-reference.com/boxscores/{game_id}.html"),
        sources: "[]".to_owned(),
    }
}

fn main() {
    let Some(db_path) = std::env::args().nth(1) else {
        eprintln!("usage: seed_smoke <archive.db>");
        std::process::exit(2);
    };
    let conn = Connection::open(&db_path).expect("open archive db");
    create_schema(&conn).expect("create_schema");
    insert_game(
        &conn,
        &game(1947, "194611010TRH", "1946-11-01", "TRH", "NYK"),
    )
    .expect("seed 1946-47 opener");
    insert_game(
        &conn,
        &game(1990, "199006140POR", "1990-06-14", "POR", "DET"),
    )
    .expect("seed 1990 Finals G5");
    let _ = GameContext {
        game_id: "194611010TRH".to_owned(),
        home_team: "TRH".to_owned(),
        away_team: "NYK".to_owned(),
        date: "1946-11-01".to_owned(),
    }; // shape reference; the runner builds contexts itself
    println!("seeded {} (194611010TRH, 199006140POR)", db_path);
}
