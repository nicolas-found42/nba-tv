//! Rung-2 yt-dlp sidecar probe (issue #22).
//!
//! Every test drives [`YtdlpProbe`] through a fake yt-dlp binary: a small
//! `sh` script written to a temp dir at test time with its behavior baked in
//! (no env passing, so tests stay parallel-safe). Zero network in the suite
//! by construction — the real binary appears only in the documented manual
//! smoke run.
//!
//! Fakes are installed via [`common::install_fake`] and every spawn is
//! wrapped in [`common::with_spawn_lock`]: on Linux a concurrently forked
//! child inherits every open fd (O_CLOEXEC affects exec, not fork), so a
//! write window overlapping a fork makes `execve` of the script fail with
//! ETXTBSY — the exact CI flake this suite used to hit (see
//! `tests/common/mod.rs`).

use common::{argv_of, install_fake, with_spawn_lock};
use nbatv_catalog::{
    registry_with_ytdlp, sweep_game, GameContext, PolitenessConfig, ProbeRegistry, SourceProbe,
    YtdlpProbe,
};
use nbatv_db::rusqlite::Connection;
use nbatv_db::{create_schema, game_queries_for, insert_game, tape_sources_for, GameRow};
use nbatv_ladder::YoutubeQuota;
use std::time::Duration;

mod common;

const GAME_ID: &str = "194611010TRH";
const T0: &str = "2026-01-01";

fn ctx() -> GameContext {
    GameContext {
        game_id: GAME_ID.to_owned(),
        home_team: "TRH".to_owned(),
        away_team: "NYK".to_owned(),
        date: "1946-11-01".to_owned(),
    }
}

fn full_entry() -> &'static str {
    r#"{"id":"ABCDEFGHIJK","title":"NYK vs TRH Full Game 1946-11-01","description":"Complete broadcast","duration":7500,"webpage_url":"https://www.youtube.com/watch?v=ABCDEFGHIJK"}"#
}

#[test]
fn parallel_fake_binaries_get_distinct_temp_dirs() {
    let workers = (0..32)
        .map(|_| {
            std::thread::spawn(|| {
                let (bin, _, guard) = install_fake("ytdlp", ":");
                (bin.parent().expect("binary parent").to_path_buf(), guard)
            })
        })
        .collect::<Vec<_>>();
    let dirs = workers
        .into_iter()
        .map(|worker| worker.join().expect("install fake binary").0)
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(dirs.len(), 32, "parallel fakes must not share temp dirs");
}

#[test]
fn search_pattern_uses_teams_and_date() {
    assert_eq!(
        YtdlpProbe::search_pattern(&ctx()),
        "NYK vs TRH Full Game 1946-11-01"
    );
    assert_eq!(
        YtdlpProbe::search_query(&ctx()),
        "ytsearch10:NYK vs TRH Full Game 1946-11-01"
    );
}

#[test]
fn probe_reports_rung_2_youtube() {
    let probe = YtdlpProbe::new();
    assert_eq!(probe.rung(), 2);
    assert_eq!(probe.name(), "youtube");
}

#[test]
fn full_game_candidate_parsed_with_watch_url_and_duration() {
    let (bin, _log, _guard) = install_fake(
        "ytdlp",
        &format!("printf '%s\\n' '{full}'", full = full_entry()),
    );
    let probe = YtdlpProbe::with_binary(&bin);
    let mut quota = YoutubeQuota::new();
    let out = with_spawn_lock(|| probe.probe(&ctx(), &PolitenessConfig::default(), &mut quota));
    assert!(!out.deferred);
    assert_eq!(out.query_text, "ytsearch10:NYK vs TRH Full Game 1946-11-01");
    assert_eq!(out.candidates.len(), 1);
    let c = &out.candidates[0];
    assert_eq!(
        c.url_or_pointer,
        "https://www.youtube.com/watch?v=ABCDEFGHIJK"
    );
    assert_eq!(c.title, "NYK vs TRH Full Game 1946-11-01");
    assert_eq!(c.description, "Complete broadcast");
    assert_eq!(c.duration_secs, Some(7500));
    assert_eq!(quota.used(), 1);
}

#[test]
fn argv_carries_politeness_sleep_flags_match_filter_and_flat_json() {
    let (bin, log, _guard) = install_fake("ytdlp", ":");
    let probe = YtdlpProbe::with_binary(&bin);
    let mut quota = YoutubeQuota::new();
    with_spawn_lock(|| probe.probe(&ctx(), &PolitenessConfig::default(), &mut quota));
    let argv = argv_of(&log);
    let flag = |name: &str| {
        argv.iter()
            .position(|a| a == name)
            .unwrap_or_else(|| panic!("argv missing {name}: {argv:?}"))
    };
    assert!(argv.contains(&"--flat-playlist".to_owned()));
    assert!(argv.contains(&"--dump-json".to_owned()));
    let m = flag("--match-filters");
    assert_eq!(argv[m + 1], "duration > 4200");
    let s = flag("--sleep-requests");
    assert_eq!(argv[s + 1], "5");
    assert_eq!(
        argv.last().unwrap(),
        "ytsearch10:NYK vs TRH Full Game 1946-11-01"
    );
}

#[test]
fn custom_sleep_seconds_reach_the_sidecar() {
    let (bin, log, _guard) = install_fake("ytdlp", ":");
    let probe = YtdlpProbe::with_binary(&bin);
    let mut quota = YoutubeQuota::new();
    let politeness = PolitenessConfig {
        ytdlp_request_sleep: Duration::from_secs(9),
        ..PolitenessConfig::default()
    };
    with_spawn_lock(|| probe.probe(&ctx(), &politeness, &mut quota));
    let argv = argv_of(&log);
    let s = argv
        .iter()
        .position(|a| a == "--sleep-requests")
        .expect("sleep flag present");
    assert_eq!(argv[s + 1], "9");
}

#[test]
fn zero_quota_grant_defers_without_running_the_binary() {
    let (bin, log, _guard) = install_fake("ytdlp", ":");
    let probe = YtdlpProbe::with_binary(&bin);
    let mut quota = YoutubeQuota::with_limit(1);
    assert_eq!(quota.schedule(1), 1);
    let out = with_spawn_lock(|| probe.probe(&ctx(), &PolitenessConfig::default(), &mut quota));
    assert!(out.deferred);
    assert!(out.candidates.is_empty());
    assert!(!log.exists(), "spent quota must not invoke the sidecar");
}

#[test]
fn missing_binary_defers() {
    let probe = YtdlpProbe::with_binary("/nonexistent/yt-dlp-sidecar");
    let mut quota = YoutubeQuota::new();
    let out = with_spawn_lock(|| probe.probe(&ctx(), &PolitenessConfig::default(), &mut quota));
    assert!(out.deferred);
    assert!(out.candidates.is_empty());
    assert_eq!(quota.used(), 1, "the attempt still spends one unit");
}

#[test]
fn nonzero_exit_defers() {
    let (bin, _log, _guard) = install_fake("ytdlp", "exit 1");
    let probe = YtdlpProbe::with_binary(&bin);
    let mut quota = YoutubeQuota::new();
    let out = with_spawn_lock(|| probe.probe(&ctx(), &PolitenessConfig::default(), &mut quota));
    assert!(out.deferred);
    assert!(out.candidates.is_empty());
}

#[test]
fn empty_output_records_an_empty_query() {
    let (bin, _log, _guard) = install_fake("ytdlp", ":");
    let probe = YtdlpProbe::with_binary(&bin);
    let mut quota = YoutubeQuota::new();
    let out = with_spawn_lock(|| probe.probe(&ctx(), &PolitenessConfig::default(), &mut quota));
    assert!(!out.deferred, "a ran query records, even with no hits");
    assert!(out.candidates.is_empty());
}

#[test]
fn unparseable_lines_are_skipped_not_fatal() {
    let (bin, _log, _guard) = install_fake(
        "ytdlp",
        &format!(
            "printf '%s\\n' 'not json at all' '{full}'",
            full = full_entry()
        ),
    );
    let probe = YtdlpProbe::with_binary(&bin);
    let mut quota = YoutubeQuota::new();
    let out = with_spawn_lock(|| probe.probe(&ctx(), &PolitenessConfig::default(), &mut quota));
    assert!(!out.deferred);
    assert_eq!(out.candidates.len(), 1);
    assert_eq!(out.candidates[0].duration_secs, Some(7500));
}

#[test]
fn short_and_unknown_duration_entries_are_kept_as_evidence() {
    // The probe is evidence-only: yt-dlp's --match-filters gate applies in
    // production, but whatever the sidecar returns is recorded with honest
    // durations for the scorer to judge — never silently dropped here.
    let (bin, _log, _guard) = install_fake(
        "ytdlp",
        r#"printf '%s\n' '{"id":"SHORTSHORT1","title":"NYK vs TRH highlights","duration":600}' '{"id":"UNKNOWNUNK1","title":"NYK vs TRH Full Game tape","duration":null}'"#,
    );
    let probe = YtdlpProbe::with_binary(&bin);
    let mut quota = YoutubeQuota::new();
    let out = with_spawn_lock(|| probe.probe(&ctx(), &PolitenessConfig::default(), &mut quota));
    assert!(!out.deferred);
    assert_eq!(out.candidates.len(), 2);
    assert_eq!(out.candidates[0].duration_secs, Some(600));
    assert_eq!(out.candidates[1].duration_secs, None);
}

fn seeded_conn() -> Connection {
    let conn = Connection::open_in_memory().expect("in-memory archive db");
    create_schema(&conn).expect("create_schema");
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

#[test]
fn sweep_end_to_end_writes_rung_2_embed_class_row() {
    let (bin, _log, _guard) = install_fake(
        "ytdlp",
        &format!("printf '%s\\n' '{full}'", full = full_entry()),
    );
    let probe = YtdlpProbe::with_binary(&bin);
    let mut registry = ProbeRegistry::new();
    registry.register(&probe);
    // The constructor wires the same registration in one call.
    let via_helper = registry_with_ytdlp(&probe);
    assert!(via_helper.get(2).is_some());

    let conn = seeded_conn();
    let mut quota = YoutubeQuota::new();
    let report = with_spawn_lock(|| {
        sweep_game(
            &conn,
            &ctx(),
            &registry,
            &PolitenessConfig::default(),
            &mut quota,
            T0,
        )
        .expect("sweep")
    });
    assert!(report.probed.contains(&2));

    let tapes = tape_sources_for(&conn, GAME_ID).expect("tapes");
    assert_eq!(tapes.len(), 1);
    assert_eq!(tapes[0].rank, 2, "rung 2 is the embed-class ladder rung");
    assert_eq!(tapes[0].source_class, nbatv_db::SourceClass::YouTube);
    assert_eq!(
        tapes[0].url_or_pointer, "https://www.youtube.com/watch?v=ABCDEFGHIJK",
        "watch URL: the shell converts to the sanctioned /embed/ player"
    );

    let queries = game_queries_for(&conn, GAME_ID).expect("queries");
    assert_eq!(queries.len(), 1);
    assert_eq!(
        queries[0].query_text, "ytsearch10:NYK vs TRH Full Game 1946-11-01",
        "the query is recorded as sweep evidence"
    );
}
