//! Wave 1 proof (#19): the Shell renders from the real archive database.
//!
//! - A seeded in-memory db (one season, one game, two tape rows) drives
//!   seasons, games, Box Score, and Play dispatch through the rank-1 row.
//! - An empty db degrades honestly: empty season list, no crash,
//!   Play on a missing game resolves to Unavailable.

use nbatv_db::rusqlite::Connection;
use nbatv_db::{
    create_schema, insert_box_player, insert_box_team, insert_game, insert_player, insert_season,
    insert_tape_source, insert_team, BoxPlayerRow, BoxTeamRow, GameRow, PlayerRow, SeasonRow,
    TapeSource, TeamRow,
};
use nbatv_shell::{PlayDispatch, ShellApp, TapeState};

const GAME_ID: &str = "194611010TRH";
const RANK1_URL: &str = "https://archive.org/details/194611010TRH";
const RANK3_POINTER: &str = "pointer:fan-cluster/194611010TRH";

/// In-memory archive with one BAA season, three clubs, one game, its Box
/// Score rows, and two tape rows inserted worst-rank-first (insertion order
/// must not decide dispatch — ascending rank must).
fn seeded_conn() -> Connection {
    let conn = nbatv_db::open_in_memory().expect("in-memory archive db");
    create_schema(&conn).expect("create_schema");

    insert_season(
        &conn,
        &SeasonRow {
            league: "BAA".to_owned(),
            year: 1947,
            label: "1946-47".to_owned(),
        },
    )
    .unwrap();
    for (slug, city, name, to) in [
        ("BOS", "Boston", "Celtics", None),
        ("NYK", "New York", "Knicks", None),
        ("TRH", "Toronto", "Huskies", Some(1946)),
    ] {
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
                active_to: to,
            },
        )
        .unwrap();
    }
    insert_game(
        &conn,
        &GameRow {
            game_id: GAME_ID.to_owned(),
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
            arena: None,
            attendance: None,
            br_url: "https://www.basketball-reference.com/boxscores/194611010TRH.html".to_owned(),
            sources: "[]".to_owned(),
        },
    )
    .unwrap();

    // Team totals mirror the 1946 live-data rule: TRH's fga is unrecorded
    // (None renders as "—", never an invented zero).
    for (team, fg, fga, ft, fta, pf, pts) in [
        (
            "NYK",
            Some(24),
            Some(28),
            Some(20),
            Some(25),
            Some(14),
            Some(68),
        ),
        (
            "TRH",
            Some(22),
            None,
            Some(22),
            Some(27),
            Some(16),
            Some(66),
        ),
    ] {
        insert_box_team(
            &conn,
            &BoxTeamRow {
                game_id: GAME_ID.to_owned(),
                team_br: team.to_owned(),
                mp: Some("240".to_owned()),
                fg,
                fga,
                fg3: None,
                fg3a: None,
                ft,
                fta,
                oreb: None,
                dreb: None,
                reb: None,
                ast: None,
                stl: None,
                blk: None,
                tov: None,
                pf,
                pts,
                plus_minus: None,
            },
        )
        .unwrap();
    }
    insert_player(
        &conn,
        &PlayerRow {
            br_slug: "sadowsk01".to_owned(),
            nba_person_id: None,
            name: "Ed Sadowski".to_owned(),
            first_season: Some(1947),
            last_season: Some(1947),
        },
    )
    .unwrap();
    insert_box_player(
        &conn,
        &BoxPlayerRow {
            game_id: GAME_ID.to_owned(),
            team_br: "NYK".to_owned(),
            player_br: "sadowsk01".to_owned(),
            starter: Some(true),
            position: Some("C".to_owned()),
            mp: Some("38:00".to_owned()),
            fg: Some(7),
            fga: Some(15),
            fg3: None,
            fg3a: None,
            ft: Some(5),
            fta: Some(7),
            oreb: None,
            dreb: None,
            reb: None,
            ast: None,
            stl: None,
            blk: None,
            tov: None,
            pf: Some(4),
            pts: Some(19),
            plus_minus: None,
            dnp_reason: None,
        },
    )
    .unwrap();
    // DNP row with no players-directory entry: name falls back to the slug.
    insert_box_player(
        &conn,
        &BoxPlayerRow {
            game_id: GAME_ID.to_owned(),
            team_br: "TRH".to_owned(),
            player_br: "teammate01".to_owned(),
            starter: Some(false),
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
            dnp_reason: Some("Coach's decision".to_owned()),
        },
    )
    .unwrap();

    // Worst rank first on purpose: dispatch must still pick rank 1.
    insert_tape_source(
        &conn,
        &TapeSource {
            game_id: nbatv_db::GameId(GAME_ID.to_owned()),
            rank: 3,
            source_class: "fan-rehost".into(),
            url_or_pointer: RANK3_POINTER.to_owned(),
            match_confidence: 0.4,
            verified_at: "2026-01-02".to_owned(),
        },
    )
    .unwrap();
    insert_tape_source(
        &conn,
        &TapeSource {
            game_id: nbatv_db::GameId(GAME_ID.to_owned()),
            rank: 1,
            source_class: nbatv_db::SourceClass::InternetArchive,
            url_or_pointer: RANK1_URL.to_owned(),
            match_confidence: 0.9,
            verified_at: "2026-01-01".to_owned(),
        },
    )
    .unwrap();
    conn
}

#[test]
fn shell_renders_seasons_games_and_box_from_the_archive_db() {
    let app = ShellApp::from_connection(seeded_conn());

    let seasons = app.store().seasons();
    assert_eq!(seasons.len(), 1);
    assert_eq!(seasons[0].slug, "1946-47");
    assert_eq!(seasons[0].league, "BAA");

    let clubs: Vec<_> = app
        .store()
        .teams_for_season("1946-47")
        .iter()
        .map(|t| t.br_slug.clone())
        .collect();
    assert_eq!(clubs, vec!["BOS", "NYK", "TRH"]);

    let games = app.store().games_for_season("1946-47");
    assert_eq!(games.len(), 1);
    assert_eq!(games[0].game_id, GAME_ID);
    assert_eq!(games[0].tape, TapeState::Playable);
    let ranks: Vec<u8> = games[0].sources.iter().map(|s| s.rank).collect();
    assert_eq!(ranks, vec![1, 3], "tape rows must arrive best-rank-first");

    let counts = app.store().season_counts("1946-47");
    assert_eq!((counts.seeded, counts.playable), (1, 1));

    let bx = app
        .store()
        .box_for(GAME_ID)
        .expect("box score renders from the db when present");
    assert_eq!(bx.teams.len(), 2);
    // Away first, box-score convention.
    assert_eq!(bx.teams[0].team_br, "NYK");
    assert_eq!(bx.teams[0].pts, Some(68));
    assert_eq!(bx.teams[1].team_br, "TRH");
    assert_eq!(bx.teams[1].pts, Some(66));
    assert_eq!(
        bx.teams[1].fga, None,
        "unrecorded 1946 fga stays None, never zero"
    );
    let ed = bx
        .players
        .iter()
        .find(|p| p.player_br == "sadowsk01")
        .expect("seeded player row");
    assert_eq!(ed.player_name, "Ed Sadowski");
    assert_eq!(ed.pts, Some(19));
    let dnp = bx
        .players
        .iter()
        .find(|p| p.player_br == "teammate01")
        .expect("seeded DNP row");
    assert_eq!(dnp.player_name, "teammate01");
    assert_eq!(dnp.dnp_reason.as_deref(), Some("Coach's decision"));
}

#[test]
fn press_play_resolves_through_the_rank_1_tape_row() {
    let mut app = ShellApp::from_connection(seeded_conn());
    app.press_play(GAME_ID);
    assert_eq!(
        app.last_dispatch(),
        Some(&PlayDispatch::PlayProgressive {
            src: RANK1_URL.to_string(),
        }),
        "dispatch must consult db tape_sources by ascending rank"
    );
    assert_eq!(app.lane_a_src(), Some(RANK1_URL));
}

#[test]
fn empty_db_degrades_honestly() {
    let conn = nbatv_db::open_in_memory().expect("in-memory db");
    create_schema(&conn).expect("create_schema");
    let mut app = ShellApp::from_connection(conn);

    assert!(
        app.store().seasons().is_empty(),
        "empty db renders an empty season list"
    );
    assert!(app.store().games_for_season("1946-47").is_empty());
    assert_eq!(app.store().box_for(GAME_ID), None);
    app.press_play(GAME_ID);
    assert_eq!(app.last_dispatch(), Some(&PlayDispatch::Unavailable));
    assert_eq!(app.lane_a_status(), None);
}

#[test]
fn missing_db_file_degrades_without_panic() {
    // A path whose parent directory does not exist can never open, so the
    // shell must fall back to the empty archive instead of panicking.
    let missing = std::env::temp_dir().join(format!(
        "nbatv-no-such-dir-{}/archive.db",
        std::process::id()
    ));
    let mut app = ShellApp::open_archive(&missing);
    assert!(app.store().seasons().is_empty());
    app.press_play(GAME_ID);
    assert_eq!(app.last_dispatch(), Some(&PlayDispatch::Unavailable));
}

#[test]
fn open_archive_creates_schema_for_a_brand_new_file() {
    // A missing file is created with the schema, so it is a valid empty db.
    let dir = std::env::temp_dir().join(format!("nbatv-archive-new-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch temp dir");
    let path = dir.join("archive.db");
    let app = ShellApp::open_archive(&path);
    assert!(app.store().seasons().is_empty());
    assert!(path.exists(), "open creates the db file");
    let _ = std::fs::remove_dir_all(&dir);
}
