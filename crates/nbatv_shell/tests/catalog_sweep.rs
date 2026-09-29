//! Wave 2 proof (#20): the Shell shows honest sweep verdicts and the
//! review card from stored sweep evidence.
//!
//! Each test seeds a minimal archive, drives one offline sweep through
//! scripted fake probes (zero network), then reads the verdicts through
//! the same [`ShellApp`] seams the views use: tape banner state, the
//! review list behind the Game view's review card, and Play dispatch.

use nbatv_catalog::{
    sweep_game, GameContext, ProbeCandidate, ProbeOutcome, ProbeRegistry, ScriptedProbe,
    SweepStatus, YoutubeQuota,
};
use nbatv_db::rusqlite::Connection;
use nbatv_db::{
    create_schema, insert_game, insert_season, insert_team, GameRow, SeasonRow, TeamRow,
};
use nbatv_shell::{PlayDispatch, ShellApp, TapeState};

const GAME_ID: &str = "194611010TRH";
const T0: &str = "2026-01-01";

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
    for (slug, city, name) in [("TRH", "Toronto", "Huskies"), ("NYK", "New York", "Knicks")] {
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
    conn
}

fn ctx() -> GameContext {
    GameContext {
        game_id: GAME_ID.to_owned(),
        home_team: "TRH".to_owned(),
        away_team: "NYK".to_owned(),
        date: "1946-11-01".to_owned(),
    }
}

fn evidence(url: &str, title: &str, duration_secs: Option<u64>) -> ProbeCandidate {
    ProbeCandidate {
        url_or_pointer: url.to_owned(),
        title: title.to_owned(),
        description: "scripted fake evidence".to_owned(),
        duration_secs,
    }
}

fn confirmed() -> ProbeCandidate {
    evidence(
        "https://archive.org/details/194611010TRH",
        "NYK at TRH Full Game 1946",
        Some(7_200),
    )
}

fn reject() -> ProbeCandidate {
    evidence("https://example.com/unrelated", "Cats playing piano", None)
}

fn review() -> ProbeCandidate {
    evidence(
        "https://example.com/clip/194611010TRH",
        "NYK highlights 1946",
        Some(600),
    )
}

fn scripted(rung: u8, candidate: ProbeCandidate) -> ScriptedProbe {
    ScriptedProbe::responding(
        rung,
        ProbeOutcome::found(format!("query for rung {rung}"), vec![candidate]),
    )
}

fn sweep(conn: &Connection, probes: &ProbeRegistry<'_>) {
    sweep_game(
        conn,
        &ctx(),
        probes,
        &nbatv_catalog::PolitenessConfig::default(),
        &mut YoutubeQuota::new(),
        T0,
    )
    .unwrap();
}

#[test]
fn swept_confirmed_game_is_playable_and_plays() {
    let conn = seeded_conn();
    let p0 = scripted(0, reject());
    let p1 = scripted(1, confirmed());
    let mut reg = ProbeRegistry::new();
    reg.register(&p0).register(&p1);
    sweep(&conn, &reg);

    let mut app = ShellApp::from_connection(conn);
    let game = app.store().game(GAME_ID).expect("seeded game");
    assert_eq!(game.tape, TapeState::Playable);
    assert_eq!(
        app.store().sweep_status(GAME_ID),
        SweepStatus::Playable { rank: 1 }
    );
    assert!(app.store().review_list(GAME_ID).is_empty());

    app.press_play(GAME_ID);
    assert!(
        matches!(
            app.last_dispatch(),
            Some(PlayDispatch::PlayProgressive { .. })
        ),
        "rung-1 tape is a progressive file behind the Player Backend"
    );
}

#[test]
fn review_candidate_surfaces_but_never_dispatches() {
    let conn = seeded_conn();
    let p0 = scripted(0, review());
    let mut reg = ProbeRegistry::new();
    reg.register(&p0);
    sweep(&conn, &reg);

    let mut app = ShellApp::from_connection(conn);
    let reviews = app.store().review_list(GAME_ID);
    assert_eq!(reviews.len(), 1, "the review card has one item");
    assert_eq!(reviews[0].rung, 0);
    assert_eq!(reviews[0].title, "NYK highlights 1946");
    // Only the probed rung was consumed: the verdict stays Sweeping.
    assert_eq!(app.store().game(GAME_ID).unwrap().tape, TapeState::Sweeping);

    app.press_play(GAME_ID);
    assert_eq!(
        app.last_dispatch(),
        Some(&PlayDispatch::Unavailable),
        "REVIEW never reaches Play dispatch"
    );
}

#[test]
fn exhausted_sweep_shows_unavailable() {
    let conn = seeded_conn();
    let probes: Vec<ScriptedProbe> = (0u8..=4).map(|rung| scripted(rung, reject())).collect();
    let mut reg = ProbeRegistry::new();
    for probe in &probes {
        reg.register(probe);
    }
    sweep(&conn, &reg);

    let mut app = ShellApp::from_connection(conn);
    assert!(matches!(
        app.store().sweep_status(GAME_ID),
        SweepStatus::Unavailable { .. }
    ));
    assert_eq!(
        app.store().game(GAME_ID).unwrap().tape,
        TapeState::Unavailable
    );
    assert!(app.store().review_list(GAME_ID).is_empty());

    app.press_play(GAME_ID);
    assert_eq!(app.last_dispatch(), Some(&PlayDispatch::Unavailable));
}

#[test]
fn pointer_only_game_shows_pointer() {
    let conn = seeded_conn();
    nbatv_db::insert_tape_source(
        &conn,
        &nbatv_db::TapeSource {
            game_id: nbatv_db::GameId(GAME_ID.to_owned()),
            rank: 5,
            source_class: nbatv_db::SourceClass::Collector,
            url_or_pointer: "pointer:collector/194611010TRH".to_owned(),
            match_confidence: 1.0,
            verified_at: T0.to_owned(),
        },
    )
    .unwrap();

    let mut app = ShellApp::from_connection(conn);
    assert_eq!(
        app.store().sweep_status(GAME_ID),
        SweepStatus::ExistsNotStreamable
    );
    assert_eq!(app.store().game(GAME_ID).unwrap().tape, TapeState::Pointer);

    app.press_play(GAME_ID);
    assert!(matches!(
        app.last_dispatch(),
        Some(PlayDispatch::ShowPointer { .. })
    ));
}

#[test]
fn fresh_game_with_no_history_shows_sweeping() {
    let app = ShellApp::from_connection(seeded_conn());
    assert_eq!(app.store().sweep_status(GAME_ID), SweepStatus::Sweeping);
    assert_eq!(
        app.store().game(GAME_ID).unwrap().tape,
        TapeState::Sweeping,
        "a never-swept game may become playable — never mark it absent"
    );
}
