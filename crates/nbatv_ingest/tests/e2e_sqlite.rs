//! End-to-end proof for T1 (#13): parser output inserts into SQLite.
//!
//! The production crate stays dependency-free (contract mirrors, not imports),
//! so this integration test — dev-dependencies only — is where the seam meets
//! the schema: `parse_games_page` output plus parser-shaped box/totals rows go
//! through the real `nbatv_db::insert_*` fns into an in-memory DB and read back.
//!
//! Live-run procedure (1946-47 BAA, the small season), documented here because
//! CI has no network: fetch `_games.html` for season `1946-47` with
//! `fetch_season` (etiquette >= 3.5 s/request, gz snapshots under
//! `data/raw/br/1946-47/`, resume skips files already on disk), parse with
//! `parse_games_page` / `parse_box_page` / `parse_totals_page`, attach
//! `game_id` from each snapshot file name at insert time, insert via
//! `nbatv_db::insert_*`, and re-run to observe resume-skips plus
//! `recrawl_hint` output for `meta-revised` stamps.

use nbatv_db::{
    insert_box_player, insert_box_team, insert_game, insert_season, insert_season_total,
    insert_team, BoxPlayerRow, BoxTeamRow, GameRow, SeasonRow, SeasonTotalRow, TeamRow,
};
use nbatv_ingest::{parse_games_page, validate_game_id};

/// Minimal BR `_games.html` shape: one played opener, one slugless future game.
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
<th data-stat=\"date_game\" csk=\"1946-11-04\">Mon, Nov 4, 1946</th>\
<td data-stat=\"visitor_team_name\"><a href=\"/teams/TRH/1947.html\">Toronto Huskies</a></td>\
<td data-stat=\"home_team_name\"><a href=\"/teams/PIT/1947.html\">Pittsburgh Ironmen</a></td>\
<td data-stat=\"box_score_text\"></td>\
</tr>\
</tbody></table>";

#[test]
fn parsed_season_inserts_end_to_end_into_sqlite() {
    // 1. Parse: the opener yields one valid slug, the future game is skipped.
    let page = parse_games_page(GAMES_HTML);
    assert_eq!(page.rows.len(), 1);
    assert_eq!(page.skipped_bad_slugs, 1);
    let game = &page.rows[0];
    assert_eq!(game.game_id, "194611010TRH");
    assert!(validate_game_id(&game.game_id));
    assert!(nbatv_db::is_valid_game_id(&game.game_id));

    // 2. Insert the full chain for that game (season + teams + game).
    let conn = nbatv_db::open_in_memory().expect("in-memory DB");
    nbatv_db::create_schema(&conn).expect("schema");
    insert_season(
        &conn,
        &SeasonRow {
            league: "BAA".to_owned(),
            year: 1947,
            label: "1946-47 BAA".to_owned(),
        },
    )
    .expect("season");
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
                active_to: None,
            },
        )
        .expect("team");
    }
    insert_game(
        &conn,
        &GameRow {
            game_id: game.game_id.clone(),
            nba_game_id: None,
            league: "BAA".to_owned(),
            season: 1947,
            date: game.date.clone(),
            game_type: "REGULAR".to_owned(),
            home_team: game.home_br.clone(),
            away_team: game.away_br.clone(),
            home_pts: 66,
            away_pts: 68,
            ot: None,
            arena: None,
            attendance: None,
            br_url: "https://www.basketball-reference.com/boxscores/194611010TRH.html".to_owned(),
            sources: "[]".to_owned(),
        },
    )
    .expect("game");

    // 3. Parser-shaped box rows: 1946-47 era NULLs (no threes/steals/blocks),
    //    mirroring `parse_box_page` output for a pre-modern box page. TRH
    //    carries the live-November-1946 shape: team `fga` unrecorded (None).
    for (team, pts, fga) in [("NYK", 68, Some(60)), ("TRH", 66, None)] {
        insert_box_team(
            &conn,
            &BoxTeamRow {
                game_id: game.game_id.clone(),
                team_br: team.to_owned(),
                mp: Some("240".to_owned()),
                fg: Some(20),
                fga,
                fg3: None,
                fg3a: None,
                ft: Some(28),
                fta: Some(40),
                oreb: None,
                dreb: None,
                reb: None,
                ast: None,
                stl: None,
                blk: None,
                tov: None,
                pf: Some(15),
                pts: Some(pts),
                plus_minus: None,
            },
        )
        .expect("box team");
    }
    insert_box_player(
        &conn,
        &BoxPlayerRow {
            game_id: game.game_id.clone(),
            team_br: "NYK".to_owned(),
            player_br: "doejo01".to_owned(),
            starter: Some(true),
            position: None,
            mp: None,
            fg: Some(5),
            fga: Some(12),
            fg3: None,
            fg3a: None,
            ft: Some(4),
            fta: Some(6),
            oreb: None,
            dreb: None,
            reb: None,
            ast: None,
            stl: None,
            blk: None,
            tov: None,
            pf: Some(2),
            pts: Some(14),
            plus_minus: None,
            dnp_reason: None,
        },
    )
    .expect("box player");
    insert_season_total(
        &conn,
        &SeasonTotalRow {
            player_br: "doejo01".to_owned(),
            season: 1947,
            team_br: "NYK".to_owned(),
            g: 60,
            mp: None,
            fg: Some(200),
            fga: Some(600),
            fg3: None,
            fg3a: None,
            ft: Some(150),
            fta: Some(220),
            oreb: None,
            dreb: None,
            reb: None,
            ast: Some(80),
            stl: None,
            blk: None,
            tov: None,
            pf: Some(120),
            pts: Some(550),
        },
    )
    .expect("totals");

    // 4. Read back: every table holds exactly what the parser chain produced.
    let count = |table: &str| -> i64 {
        conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .expect("count")
    };
    assert_eq!(count("games"), 1);
    assert_eq!(count("box_team"), 2);
    assert_eq!(count("box_player"), 1);
    assert_eq!(count("player_season_totals"), 1);
    let pts: i32 = conn
        .query_row(
            "SELECT pts FROM box_team WHERE game_id = ?1 AND team_br = 'NYK'",
            [&game.game_id],
            |row| row.get(0),
        )
        .expect("box read-back");
    assert_eq!(pts, 68);
    // Era NULLs survive the round-trip (NULL = era did not record, not zero).
    let fg3: Option<i32> = conn
        .query_row(
            "SELECT fg3 FROM box_team WHERE game_id = ?1 AND team_br = 'NYK'",
            [&game.game_id],
            |row| row.get(0),
        )
        .expect("null read-back");
    assert_eq!(fg3, None);
}
