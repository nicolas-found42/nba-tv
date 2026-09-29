//! Rung-1 live sweep through the Shell seam (issue #21).
//!
//! Seeds a file-backed archive (so the sweep and the app open the same
//! database, as in production), drives [`DbStore::sweep_ia_rung`] with a
//! stub [`IaHttp`] transport (zero network), then proves the found row
//! plays: Play dispatch resolves progressive and the Lane A session starts
//! on the IA file URL — the existing in-window path, no shell changes.

use nbatv_catalog::{IaError, IaHttp, IaProbe, PolitenessConfig, SweepStatus, YoutubeQuota};
use nbatv_db::{
    create_schema, game_queries_for, insert_game, insert_season, insert_team, tape_sources_for,
    GameRow, SeasonRow, TeamRow,
};
use nbatv_shell::{DbStore, PlayDispatch, ShellApp, TapeState};
use std::path::PathBuf;
use std::time::Duration;

const GAME_ID: &str = "199006140POR";
const T0: &str = "2026-01-01";

const SEARCH_BODY: &str = r#"{"response": {"numFound": 1, "start": 0, "docs": [{"identifier": "detroit-pistons-vs-portland-trailblazers-1990-nba-finals-game-5", "title": "Detroit Pistons Vs Portland Trailblazers 1990 NBA Finals Game 5", "date": "1990-06-14"}]}}"#;
const META_BODY: &str = r#"{"metadata": {"identifier": "detroit-pistons-vs-portland-trailblazers-1990-nba-finals-game-5", "title": "Detroit Pistons Vs Portland Trailblazers 1990 NBA Finals Game 5", "description": "1990 NBA Finals Game 5 between the Detroit Pistons and Portland Trail Blazers."}, "files": [{"name": "1990 Finals Game 5.mp4", "format": "MPEG4", "size": "640123456", "length": "7621.50", "source": "original"}]}"#;

/// Offline IA transport: canned search + metadata bodies, nothing else.
struct StubIa;

impl IaHttp for StubIa {
    fn get(&self, url: &str) -> Result<String, IaError> {
        if url.contains("advancedsearch.php") {
            Ok(SEARCH_BODY.to_owned())
        } else if url.contains("/metadata/") {
            Ok(META_BODY.to_owned())
        } else {
            Err(IaError::Transport(format!("stub has no route for {url}")))
        }
    }
}

fn archive_path(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!("nbatv-ia-rung-{tag}-{}.db", std::process::id()))
}

fn seed(path: &std::path::Path) {
    let _ = std::fs::remove_file(path);
    let conn = nbatv_db::open(path).expect("open archive file");
    create_schema(&conn).expect("create_schema");
    insert_season(
        &conn,
        &SeasonRow {
            league: "NBA".to_owned(),
            year: 1990,
            label: "1989-90".to_owned(),
        },
    )
    .unwrap();
    for (slug, city, name) in [
        ("POR", "Portland", "Trail Blazers"),
        ("DET", "Detroit", "Pistons"),
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
                active_from: Some(1970),
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
            league: "NBA".to_owned(),
            season: 1990,
            date: "1990-06-14".to_owned(),
            game_type: "PLAYOFFS".to_owned(),
            home_team: "POR".to_owned(),
            away_team: "DET".to_owned(),
            home_pts: 94,
            away_pts: 92,
            ot: None,
            arena: None,
            attendance: None,
            br_url: "https://www.basketball-reference.com/boxscores/199006140POR.html".to_owned(),
            sources: "[]".to_owned(),
        },
    )
    .unwrap();
}

fn instant_politeness() -> PolitenessConfig {
    PolitenessConfig {
        ia_request_min_interval: Duration::ZERO,
        ..PolitenessConfig::default()
    }
}

#[test]
fn ia_sweep_populates_the_row_and_play_streams_lane_a() {
    let path = archive_path("play");
    seed(&path);

    let probe = IaProbe::with_http(Box::new(StubIa));
    let report = DbStore::open(&path)
        .expect("open store")
        .sweep_ia_rung(
            &probe,
            GAME_ID,
            &instant_politeness(),
            &mut YoutubeQuota::new(),
            T0,
        )
        .expect("sweep")
        .expect("known game sweeps");
    assert_eq!(report.status, SweepStatus::Playable { rank: 1 });

    let conn = nbatv_db::open(&path).expect("reopen archive");
    let rows = tape_sources_for(&conn, GAME_ID).expect("tape rows");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].rank, 1);
    assert_eq!(rows[0].source_class, nbatv_db::SourceClass::InternetArchive);
    assert!(
        rows[0]
            .url_or_pointer
            .starts_with("https://archive.org/download/"),
        "direct file bytes: {}",
        rows[0].url_or_pointer
    );
    let queries = game_queries_for(&conn, GAME_ID).expect("query rows");
    assert_eq!(queries.len(), 1);
    assert!(queries[0]
        .query_text
        .contains("identifier:(*nba*finals*game*)"));
    drop(conn);

    let mut app = ShellApp::open_archive(&path);
    let game = app.store().game(GAME_ID).expect("seeded game");
    assert_eq!(game.tape, TapeState::Playable);
    app.press_play(GAME_ID);
    let src = assert_matches_play_progressive(app.last_dispatch());
    assert!(
        src.contains("archive.org/download/"),
        "plays IA bytes: {src}"
    );
    assert_eq!(app.lane_a_src(), Some(src.as_str()));

    let _ = std::fs::remove_file(&path);
}

#[test]
fn ia_sweep_of_an_unknown_game_records_nothing() {
    let path = archive_path("unknown");
    seed(&path);
    let probe = IaProbe::with_http(Box::new(StubIa));
    let report = DbStore::open(&path)
        .expect("open store")
        .sweep_ia_rung(
            &probe,
            "199006140XXX",
            &instant_politeness(),
            &mut YoutubeQuota::new(),
            T0,
        )
        .expect("sweep");
    assert!(report.is_none(), "unknown games stay untouched");
    let _ = std::fs::remove_file(&path);
}

fn assert_matches_play_progressive(dispatch: Option<&PlayDispatch>) -> String {
    match dispatch {
        Some(PlayDispatch::PlayProgressive { src }) => src.clone(),
        other => panic!("rung-1 tape must dispatch progressive, got {other:?}"),
    }
}
