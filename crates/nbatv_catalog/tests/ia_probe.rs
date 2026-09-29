//! Internet Archive SourceProbe at the port boundary (issue #21).
//!
//! Every test drives [`IaProbe`] through a stub [`IaHttp`] serving committed
//! JSON snapshots of the IA `advancedsearch` + `metadata` responses (plain
//! API metadata, never media): zero network in the suite by construction.
//! Fixture shapes mirror the live responses quoted in research note 15 §1.

use nbatv_catalog::{
    score_candidate, sweep_game, GameContext, IaError, IaHttp, IaProbe, MatchLevel,
    PolitenessConfig, ProbeRegistry, SourceProbe, SweepStatus, YoutubeQuota,
};
use nbatv_db::rusqlite::Connection;
use nbatv_db::{
    create_schema, game_queries_for, insert_game, insert_season, insert_team, tape_sources_for,
    GameRow, SeasonRow, TeamRow,
};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
// 1990 Finals Game 5 (Portland home, Detroit away, 1990-06-14): the note-15
// identifier `detroit-pistons-vs-portland-trailblazers-1990-nba-finals-game-5`
// is a single-game item, so the tape row crosswalk is unambiguous.
const GAME_ID: &str = "199006140POR";
const T0: &str = "2026-01-01";

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!("tests/fixtures/{name}"))
        .unwrap_or_else(|err| panic!("read fixture {name}: {err}"))
}

/// Stub IA transport: routes by URL substring, records every outbound URL.
/// The log is reference-shared so tests can read it after the probe owns
/// the stub.
struct StubHttp {
    routes: HashMap<String, Result<String, String>>,
    calls: Arc<Mutex<Vec<String>>>,
}

impl StubHttp {
    fn new() -> Self {
        Self {
            routes: HashMap::new(),
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// A live handle to the call log, readable after the probe owns the stub.
    fn calls_handle(&self) -> Arc<Mutex<Vec<String>>> {
        Arc::clone(&self.calls)
    }

    fn route(mut self, pattern: &str, body: String) -> Self {
        self.routes.insert(pattern.to_owned(), Ok(body));
        self
    }

    fn failing(mut self, pattern: &str, message: &str) -> Self {
        self.routes
            .insert(pattern.to_owned(), Err(message.to_owned()));
        self
    }
}

impl IaHttp for StubHttp {
    fn get(&self, url: &str) -> Result<String, IaError> {
        self.calls.lock().push(url.to_owned());
        for (pattern, outcome) in &self.routes {
            if url.contains(pattern) {
                return outcome.clone().map_err(IaError::Transport);
            }
        }
        Err(IaError::Transport(format!("stub has no route for {url}")))
    }
}

fn search_stub() -> StubHttp {
    StubHttp::new()
        .route("advancedsearch.php", fixture("ia_search_1990_finals.json"))
        .route(
            "/metadata/detroit-pistons-vs-portland-trailblazers-1990-nba-finals-game-5",
            fixture("ia_metadata_1990_g5.json"),
        )
}

fn ctx() -> GameContext {
    GameContext {
        game_id: GAME_ID.to_owned(),
        home_team: "POR".to_owned(),
        away_team: "DET".to_owned(),
        date: "1990-06-14".to_owned(),
    }
}

fn instant_politeness() -> PolitenessConfig {
    PolitenessConfig {
        ia_request_min_interval: Duration::ZERO,
        ..PolitenessConfig::default()
    }
}

fn seeded_conn() -> Connection {
    let conn = nbatv_db::open_in_memory().expect("in-memory archive db");
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
    conn
}

#[test]
fn probe_answers_rung_1_as_internet_archive() {
    let probe = IaProbe::with_http(Box::new(search_stub()));
    assert_eq!(probe.rung(), 1);
    assert_eq!(probe.name(), "internet-archive");
}

#[test]
fn query_text_uses_the_note15_identifier_shape() {
    let probe = IaProbe::with_http(Box::new(search_stub()));
    let outcome = probe.probe(&ctx(), &instant_politeness(), &mut YoutubeQuota::new());
    assert!(
        !outcome.deferred,
        "canned fixtures must produce a recorded query"
    );
    assert!(
        outcome
            .query_text
            .contains("identifier:(*nba*finals*game*)"),
        "inventory query keeps the note-15 verbatim shape: {}",
        outcome.query_text
    );
    assert!(
        outcome.query_text.contains("mediatype:(movies)"),
        "inventory query stays fielded on movies: {}",
        outcome.query_text
    );
}

#[test]
fn every_outbound_request_follows_the_politeness_interval() {
    let http = search_stub();
    let sleeps: Arc<Mutex<Vec<Duration>>> = Arc::new(Mutex::new(Vec::new()));
    let logged = Arc::clone(&sleeps);
    let probe =
        IaProbe::with_http_and_sleep(Box::new(http), Box::new(move |d| logged.lock().push(d)));
    let politeness = PolitenessConfig {
        ia_request_min_interval: Duration::from_secs(2),
        ..PolitenessConfig::default()
    };
    let outcome = probe.probe(&ctx(), &politeness, &mut YoutubeQuota::new());
    assert!(!outcome.deferred);
    let sleeps = sleeps.lock().clone();
    // One search plus one metadata fetch: a pacing sleep precedes each.
    assert_eq!(sleeps.len(), 2, "sleeps: {sleeps:?}");
    assert!(sleeps.iter().all(|d| *d == Duration::from_secs(2)));
}

#[test]
fn full_game_file_becomes_a_direct_download_candidate() {
    let probe = IaProbe::with_http(Box::new(search_stub()));
    let outcome = probe.probe(&ctx(), &instant_politeness(), &mut YoutubeQuota::new());
    assert_eq!(outcome.candidates.len(), 1);
    let candidate = &outcome.candidates[0];
    assert_eq!(
        candidate.url_or_pointer,
        "https://archive.org/download/detroit-pistons-vs-portland-trailblazers-1990-nba-finals-game-5/Detroit%20Pistons%20Vs%20Portland%20Trailblazers%201990%20NBA%20Finals%20Game%205.mp4"
    );
    assert_eq!(candidate.duration_secs, Some(7_621));
    assert!(
        candidate.title.contains("1990 NBA Finals Game 5"),
        "title stays the source title: {}",
        candidate.title
    );
}

#[test]
fn year_matching_identifier_is_preferred_over_undated_noise() {
    let stub = StubHttp::new()
        .route("advancedsearch.php", fixture("ia_search_1990_finals.json"))
        .route(
            "/metadata/detroit-pistons-vs-portland-trailblazers-1990-nba-finals-game-5",
            fixture("ia_metadata_1990_g5.json"),
        );
    let calls = stub.calls_handle();
    let probe = IaProbe::with_http(Box::new(stub));
    let outcome = probe.probe(&ctx(), &instant_politeness(), &mut YoutubeQuota::new());
    assert!(
        outcome
            .candidates
            .iter()
            .any(|c| c.url_or_pointer.contains("1990-nba-finals-game-5")),
        "the dated Finals item wins over the undated promo: {:?}",
        outcome.candidates
    );
    let calls = calls.lock().clone();
    assert_eq!(calls.len(), 2, "one search, one metadata fetch: {calls:?}");
    assert!(calls[0].contains("advancedsearch.php"), "{calls:?}");
    assert!(calls[1].contains("1990-nba-finals-game-5"), "{calls:?}");
    assert!(
        !calls.iter().any(|url| url.contains("classic-promo")),
        "undated noise never costs a request: {calls:?}"
    );
}

#[test]
fn scorer_rejects_highlights_and_confirms_the_full_game() {
    let highlight_http = StubHttp::new()
        .route(
            "advancedsearch.php",
            r#"{"response": {"numFound": 1, "docs": [{"identifier": "1996-nba-finals-highlights", "title": "1996 NBA Finals Highlights"}]}}"#.to_owned(),
        )
        .route(
            "/metadata/1996-nba-finals-highlights",
            fixture("ia_metadata_highlights.json"),
        );
    let game = GameContext {
        game_id: "199606090SEA".to_owned(),
        home_team: "SEA".to_owned(),
        away_team: "CHI".to_owned(),
        date: "1996-06-09".to_owned(),
    };
    let probe = IaProbe::with_http(Box::new(highlight_http));
    let highlight = probe.probe(&game, &instant_politeness(), &mut YoutubeQuota::new());
    assert_eq!(highlight.candidates.len(), 1);
    assert_eq!(
        score_candidate(&game, &highlight.candidates[0]),
        MatchLevel::Reject,
        "a 10-minute highlight names no team and stays rejected"
    );

    let probe = IaProbe::with_http(Box::new(search_stub()));
    let full = probe.probe(&ctx(), &instant_politeness(), &mut YoutubeQuota::new());
    assert_eq!(full.candidates.len(), 1);
    assert!(
        score_candidate(&ctx(), &full.candidates[0]) >= MatchLevel::Likely,
        "a two-hour Pistons/Blazers file scores LIKELY or better"
    );
}

#[test]
fn sweep_populates_a_rung1_row_with_the_br_slug_crosswalk() {
    let conn = seeded_conn();
    let probe = IaProbe::with_http(Box::new(search_stub()));
    let mut registry = ProbeRegistry::new();
    registry.register(&probe);
    let report = sweep_game(
        &conn,
        &ctx(),
        &registry,
        &instant_politeness(),
        &mut YoutubeQuota::new(),
        T0,
    )
    .expect("sweep");
    assert_eq!(report.status, SweepStatus::Playable { rank: 1 });
    assert_eq!(report.probed, vec![1]);

    let rows = tape_sources_for(&conn, GAME_ID).expect("tape rows");
    assert_eq!(rows.len(), 1, "one rung-1 tape row: {rows:?}");
    assert_eq!(rows[0].rank, 1);
    assert_eq!(
        rows[0].game_id,
        nbatv_db::GameId(GAME_ID.to_owned()),
        "BR-slug crosswalk on the row"
    );
    assert_eq!(rows[0].source_class, nbatv_db::SourceClass::InternetArchive);
    assert!(
        rows[0]
            .url_or_pointer
            .starts_with("https://archive.org/download/"),
        "direct progressive file bytes: {}",
        rows[0].url_or_pointer
    );

    let queries = game_queries_for(&conn, GAME_ID).expect("query rows");
    assert_eq!(queries.len(), 1);
    assert_eq!(queries[0].rung, 1);
    assert!(
        queries[0]
            .query_text
            .contains("identifier:(*nba*finals*game*)"),
        "the exact query is recorded as evidence: {}",
        queries[0].query_text
    );
}

#[test]
fn rung1_leaves_the_youtube_quota_untouched() {
    let probe = IaProbe::with_http(Box::new(search_stub()));
    let mut quota = YoutubeQuota::with_limit(7);
    probe.probe(&ctx(), &instant_politeness(), &mut quota);
    assert_eq!(quota.used(), 0);
    assert_eq!(quota.remaining(), 7);
}

#[test]
fn empty_search_records_a_found_nothing_query() {
    let probe = IaProbe::with_http(Box::new(
        StubHttp::new().route("advancedsearch.php", fixture("ia_search_empty.json")),
    ));
    let outcome = probe.probe(&ctx(), &instant_politeness(), &mut YoutubeQuota::new());
    assert!(!outcome.deferred, "an empty answer still ran");
    assert!(outcome.candidates.is_empty());

    let conn = seeded_conn();
    let mut registry = ProbeRegistry::new();
    registry.register(&probe);
    let report = sweep_game(
        &conn,
        &ctx(),
        &registry,
        &instant_politeness(),
        &mut YoutubeQuota::new(),
        T0,
    )
    .expect("sweep");
    assert!(tape_sources_for(&conn, GAME_ID).expect("rows").is_empty());
    let queries = game_queries_for(&conn, GAME_ID).expect("queries");
    assert_eq!(queries.len(), 1);
    assert_eq!(queries[0].best_match_level, "reject");
    assert!(matches!(
        report.status,
        SweepStatus::Sweeping | SweepStatus::Unavailable { .. }
    ));
}

#[test]
fn title_query_is_the_fallback_when_the_identifier_query_is_empty() {
    let probe = IaProbe::with_http(Box::new(
        StubHttp::new()
            .route("identifier%3A", fixture("ia_search_empty.json"))
            .route("title%3A", fixture("ia_search_1990_finals.json"))
            .route(
                "/metadata/detroit-pistons-vs-portland-trailblazers-1990-nba-finals-game-5",
                fixture("ia_metadata_1990_g5.json"),
            ),
    ));
    let outcome = probe.probe(&ctx(), &instant_politeness(), &mut YoutubeQuota::new());
    assert_eq!(outcome.candidates.len(), 1, "title fallback found the game");
    assert!(
        outcome.query_text.contains("title:(NBA Finals)"),
        "both queries are recorded: {}",
        outcome.query_text
    );
}

#[test]
fn transport_failure_defers_so_the_sweep_records_nothing() {
    let probe = IaProbe::with_http(Box::new(
        StubHttp::new().failing("advancedsearch.php", "connection reset"),
    ));
    let outcome = probe.probe(&ctx(), &instant_politeness(), &mut YoutubeQuota::new());
    assert!(outcome.deferred, "a failed query retries, never rejects");

    let conn = seeded_conn();
    let mut registry = ProbeRegistry::new();
    registry.register(&probe);
    sweep_game(
        &conn,
        &ctx(),
        &registry,
        &instant_politeness(),
        &mut YoutubeQuota::new(),
        T0,
    )
    .expect("sweep");
    assert!(
        game_queries_for(&conn, GAME_ID)
            .expect("queries")
            .is_empty(),
        "deferred rungs burn no rescan window"
    );
    assert!(tape_sources_for(&conn, GAME_ID).expect("rows").is_empty());
}

#[test]
fn metadata_failure_defers_even_after_a_good_search() {
    let probe = IaProbe::with_http(Box::new(
        StubHttp::new()
            .route("advancedsearch.php", fixture("ia_search_1990_finals.json"))
            .failing("/metadata/", "connection reset"),
    ));
    let outcome = probe.probe(&ctx(), &instant_politeness(), &mut YoutubeQuota::new());
    assert!(outcome.deferred);
    assert!(outcome.candidates.is_empty());
}

#[test]
fn item_without_playable_files_yields_no_candidates() {
    let probe = IaProbe::with_http(Box::new(
        StubHttp::new()
            .route(
                "advancedsearch.php",
                r#"{"response": {"numFound": 1, "docs": [{"identifier": "1990-g5-stills", "title": "1990 Finals stills"}]}}"#.to_owned(),
            )
            .route(
                "/metadata/1990-g5-stills",
                r#"{"metadata": {"identifier": "1990-g5-stills", "title": "1990 Finals stills"}, "files": [{"name": "1990-g5-stills_files.xml", "format": "Metadata", "source": "derivative"}, {"name": "1990-g5-stills.thumbs/still_000001.jpg", "format": "Thumbnail", "source": "derivative"}]}"#.to_owned(),
            ),
    ));
    let outcome = probe.probe(&ctx(), &instant_politeness(), &mut YoutubeQuota::new());
    assert!(!outcome.deferred);
    assert!(outcome.candidates.is_empty(), "stills are not tape");
}

#[test]
fn multi_file_item_reports_its_longest_original_video_file() {
    let probe = IaProbe::with_http(Box::new(
        StubHttp::new()
            .route(
                "advancedsearch.php",
                r#"{"response": {"numFound": 1, "docs": [{"identifier": "1996-nba-finals-game-3", "title": "1996 NBA Finals"}]}}"#.to_owned(),
            )
            .route(
                "/metadata/1996-nba-finals-game-3",
                fixture("ia_metadata_1996_game3.json"),
            ),
    ));
    let game = GameContext {
        game_id: "199606070CHI".to_owned(),
        home_team: "CHI".to_owned(),
        away_team: "SEA".to_owned(),
        date: "1996-06-07".to_owned(),
    };
    let outcome = probe.probe(&game, &instant_politeness(), &mut YoutubeQuota::new());
    assert_eq!(outcome.candidates.len(), 1);
    let candidate = &outcome.candidates[0];
    assert!(
        candidate
            .url_or_pointer
            .ends_with("/1996%20NBA%20Finals%20Game%202.mp4"),
        "longest original wins, thumbnails never: {}",
        candidate.url_or_pointer
    );
    assert_eq!(candidate.duration_secs, Some(8_494));
    assert!(
        candidate.description.contains("Chicago Bulls")
            && candidate.description.contains("Seattle SuperSonics"),
        "the source description (both teams, full names) is kept: {}",
        candidate.description
    );
}
