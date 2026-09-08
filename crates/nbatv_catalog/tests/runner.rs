//! Headless backfill runner at the port boundary (issue #27).
//!
//! Every test drives [`run_backfill`] through fake ports: zero network in
//! the suite by construction — the only probe/fetcher/mirror
//! implementations linked here are scripted fakes.

use nbatv_catalog::runner::{
    ending_year_to_slug, expand_season_range, parse_argv, run_backfill, season_slug_to_ending_year,
    BackfillConfig, BackfillPorts,
};
use nbatv_catalog::{
    cache_path, src_tag_for_url, MirrorConfig, MirrorOutcome, PolitenessConfig, ProbeCandidate,
    ProbeOutcome, ProbeRegistry, RcloneMirror, RcloneOutput, ScriptStep, ScriptedDuration,
    ScriptedFetcher, ScriptedProbe,
};
use nbatv_db::{CacheState, GameRow, TapeSource};
use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

const NOW: &str = "2026-09-08";

fn memdb() -> Connection {
    let conn = Connection::open_in_memory().expect("in-memory db");
    nbatv_db::create_schema(&conn).expect("create_schema");
    conn
}

fn game(season: i32, game_id: &str, date: &str, home: &str, away: &str) -> GameRow {
    GameRow {
        game_id: game_id.to_owned(),
        nba_game_id: None,
        league: "BAA".to_owned(),
        season,
        date: date.to_owned(),
        game_type: "REGULAR".to_owned(),
        home_team: home.to_owned(),
        away_team: away.to_owned(),
        home_pts: 68,
        away_pts: 66,
        ot: None,
        arena: None,
        attendance: None,
        br_url: format!("https://www.basketball-reference.com/boxscores/{game_id}.html"),
        sources: "[]".to_owned(),
    }
}

/// Three games across two seasons, in manifest order.
fn seeded_two_seasons() -> Connection {
    let conn = memdb();
    nbatv_db::insert_game(
        &conn,
        &game(1947, "194611010TRH", "1946-11-01", "TRH", "NYK"),
    )
    .unwrap();
    nbatv_db::insert_game(
        &conn,
        &game(1947, "194611020CHS", "1946-11-02", "CHS", "NYK"),
    )
    .unwrap();
    nbatv_db::insert_game(
        &conn,
        &game(1948, "194711010TRH", "1947-11-01", "TRH", "CHS"),
    )
    .unwrap();
    conn
}

/// CONFIRMED evidence for `home`/`away`: both teams plus a full-game
/// marker, so the sweep writes a byte-class (rung 1) tape row.
fn confirmed_for(home: &str, away: &str) -> ProbeCandidate {
    ProbeCandidate {
        url_or_pointer: "https://archive.org/download/classic/game.mp4".to_owned(),
        title: format!("{home} vs {away} Full Game"),
        description: format!("full game tape {home} {away}"),
        duration_secs: None,
    }
}

fn reject_registry<'a>(
    p0: &'a ScriptedProbe,
    p1: &'a ScriptedProbe,
    p2: &'a ScriptedProbe,
    p3: &'a ScriptedProbe,
    p4: &'a ScriptedProbe,
) -> ProbeRegistry<'a> {
    let mut registry = ProbeRegistry::new();
    registry
        .register(p0)
        .register(p1)
        .register(p2)
        .register(p3)
        .register(p4);
    registry
}

/// Probes where rung 0 misses honestly and rung 1 wins CONFIRMED, so every
/// fresh game surfaces one byte-class tape row for the fetch stage.
fn winning_probes() -> (
    ScriptedProbe,
    ScriptedProbe,
    ScriptedProbe,
    ScriptedProbe,
    ScriptedProbe,
) {
    (
        ScriptedProbe::responding(0, ProbeOutcome::empty("rung0 inventory query".to_owned())),
        ScriptedProbe::responding(
            1,
            ProbeOutcome::found(
                "rung1 inventory query".to_owned(),
                // One CONFIRMED candidate per team pair in the fixture set:
                // the sweep scores every candidate and keeps the best, so
                // each game wins on its own pair and stops ascending.
                vec![
                    confirmed_for("TRH", "NYK"),
                    confirmed_for("CHS", "NYK"),
                    confirmed_for("TRH", "CHS"),
                ],
            ),
        ),
        ScriptedProbe::responding(2, ProbeOutcome::empty("rung2 query".to_owned())),
        ScriptedProbe::responding(3, ProbeOutcome::empty("rung3 query".to_owned())),
        ScriptedProbe::responding(4, ProbeOutcome::empty("rung4 query".to_owned())),
    )
}

/// Scratch dir that removes itself on drop (repo convention: best-effort
/// cleanup, no tempdir crate).
fn temp_dir(name: &str) -> TempDir {
    let dir = std::env::temp_dir().join(format!("nbatv-runner-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    TempDir(dir)
}

struct TempDir(PathBuf);

impl std::ops::Deref for TempDir {
    type Target = PathBuf;
    fn deref(&self) -> &PathBuf {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Fake rclone: `listremotes` answers present, every `copy` is captured and
/// succeeds. The caller owns `calls` and asserts the dry-run argv.
fn fake_mirror(calls: &Arc<Mutex<Vec<Vec<String>>>>) -> RcloneMirror {
    let calls = Arc::clone(calls);
    RcloneMirror::with_runner(Box::new(move |argv: &[String]| {
        calls.lock().expect("mirror calls").push(argv.to_vec());
        if argv.get(1).map(String::as_str) == Some("listremotes") {
            Ok(RcloneOutput::success("nbatv-drive:\n"))
        } else {
            assert_eq!(
                argv.get(1).map(String::as_str),
                Some("copy"),
                "every upload invocation is a copy: {argv:?}"
            );
            Ok(RcloneOutput::success("transferred"))
        }
    }))
}

fn polite() -> PolitenessConfig {
    PolitenessConfig {
        ia_request_min_interval: Duration::from_secs(9),
        ytdlp_request_sleep: Duration::from_secs(11),
        ytdlp_sleep_requests: 3,
        max_concurrent_probes: 1,
        youtube_daily_limit: 50,
    }
}

fn config(start: &str, end: &str) -> BackfillConfig {
    BackfillConfig {
        season_start: start.to_owned(),
        season_end: end.to_owned(),
        now: NOW.to_owned(),
        dry_run: true,
        limit: None,
        max_retries: 3,
    }
}

// ---- Season slug ordering contract ----------------------------------------

#[test]
fn season_slugs_order_by_ending_year() {
    assert_eq!(season_slug_to_ending_year("1946-47"), Some(1947));
    assert_eq!(season_slug_to_ending_year("1999-00"), Some(2000));
    assert_eq!(season_slug_to_ending_year("2000-01"), Some(2001));
    assert_eq!(season_slug_to_ending_year("2024-25"), Some(2025));
    // Garbage never parses: no season silently maps to year zero.
    assert_eq!(season_slug_to_ending_year("1946"), None);
    assert_eq!(season_slug_to_ending_year("1946-48"), None);
    assert_eq!(season_slug_to_ending_year("47"), None);
    assert_eq!(season_slug_to_ending_year(""), None);
}

#[test]
fn season_slug_roundtrips_through_the_ending_year() {
    for year in [1947, 1948, 1977, 2000, 2001, 2025] {
        let slug = ending_year_to_slug(year);
        assert_eq!(
            season_slug_to_ending_year(&slug),
            Some(year),
            "slug {slug} must parse back to {year}"
        );
    }
    assert_eq!(ending_year_to_slug(1947), "1946-47");
    assert_eq!(ending_year_to_slug(2000), "1999-00");
}

#[test]
fn season_range_expands_inclusive_in_year_order() {
    assert_eq!(
        expand_season_range("1946-47", "1948-49").unwrap(),
        vec![1947, 1948, 1949]
    );
    assert_eq!(
        expand_season_range("1946-47", "1946-47").unwrap(),
        vec![1947]
    );
    assert!(
        expand_season_range("1948-49", "1946-47").is_err(),
        "a reversed range is a usage error, never an empty run"
    );
    assert!(
        expand_season_range("1946-47", "bogus").is_err(),
        "an unparseable endpoint is a usage error"
    );
}

// ---- Acceptance: season-ranged backfill end to end, offline ----------------

#[test]
fn backfill_runs_sweep_fetch_and_mirror_offline() {
    let conn = seeded_two_seasons();
    let dir = temp_dir("e2e");
    let manifest = dir.join("files-from.txt");
    let (p0, p1, p2, p3, p4) = winning_probes();
    let registry = reject_registry(&p0, &p1, &p2, &p3, &p4);
    let fetcher = ScriptedFetcher::new(vec![ScriptStep::complete(&vec![7u8; 2048])]);
    let duration = ScriptedDuration::seconds(5400.0);
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mirror = fake_mirror(&calls);
    let politeness = polite();
    let ports = BackfillPorts {
        probes: registry,
        politeness: politeness.clone(),
        fetcher: &fetcher,
        duration: &duration,
        mirror: &mirror,
        mirror_config: MirrorConfig::default(),
        manifest_path: manifest.clone(),
        cache_root: dir.join("cache"),
    };

    let report = run_backfill(&conn, &config("1946-47", "1947-48"), &ports).unwrap();

    assert_eq!(report.games, 3, "both seasons in range are swept");
    assert_eq!(report.swept, 3);
    assert_eq!(report.skipped, 0, "nothing is fresh on the first run");
    assert_eq!(report.found, 3, "one byte-class row per game");
    assert_eq!(report.fetched_ready, 3);
    assert_eq!(report.fetched_failed, 0);
    assert_eq!(
        report.mirror.outcome,
        MirrorOutcome::Completed { files: 3 },
        "one mirror at the end uploads every Ready row"
    );
    assert!(
        report.mirror.dry_run,
        "the config dry-run flag threads through"
    );
    assert_eq!(fetcher.calls(), 3, "one bounded fetch per candidate row");
    // The politeness config governs pacing: every probe saw it untouched.
    for probe in [&p0, &p1] {
        assert_eq!(probe.last_politeness(), Some(politeness.clone()));
    }
    assert!(p0.calls() >= 1, "rung 0 ran before the rung-1 win");
    // The summary names every stage for the overnight log.
    let summary = report.summary();
    for stage in [
        "swept=3",
        "skipped=0",
        "deferred=0",
        "found=3",
        "fetched_ready=3",
        "mirror=",
    ] {
        assert!(summary.contains(stage), "summary names {stage}: {summary}");
    }
    // The manifest covers exactly the fetched set.
    let manifest_text = std::fs::read_to_string(&manifest).unwrap();
    assert_eq!(manifest_text.lines().count(), 3);
}

#[test]
fn season_range_selects_which_games_run() {
    let conn = seeded_two_seasons();
    let dir = temp_dir("range");
    let (p0, p1, p2, p3, p4) = winning_probes();
    let registry = reject_registry(&p0, &p1, &p2, &p3, &p4);
    let fetcher = ScriptedFetcher::new(vec![ScriptStep::complete(&vec![7u8; 64])]);
    let duration = ScriptedDuration::seconds(5400.0);
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mirror = fake_mirror(&calls);
    let ports = BackfillPorts {
        probes: registry,
        politeness: PolitenessConfig::default(),
        fetcher: &fetcher,
        duration: &duration,
        mirror: &mirror,
        mirror_config: MirrorConfig::default(),
        manifest_path: dir.join("files-from.txt"),
        cache_root: dir.join("cache"),
    };

    let report = run_backfill(&conn, &config("1946-47", "1946-47"), &ports).unwrap();
    assert_eq!(report.games, 2, "only the 1946-47 season runs");
    assert_eq!(report.fetched_ready, 2);
    assert_eq!(report.mirror.outcome, MirrorOutcome::Completed { files: 2 });
}

#[test]
fn limit_bounds_the_run_in_season_order() {
    let conn = seeded_two_seasons();
    let dir = temp_dir("limit");
    let (p0, p1, p2, p3, p4) = winning_probes();
    let registry = reject_registry(&p0, &p1, &p2, &p3, &p4);
    let fetcher = ScriptedFetcher::new(vec![ScriptStep::complete(&vec![7u8; 64])]);
    let duration = ScriptedDuration::seconds(5400.0);
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mirror = fake_mirror(&calls);
    let ports = BackfillPorts {
        probes: registry,
        politeness: PolitenessConfig::default(),
        fetcher: &fetcher,
        duration: &duration,
        mirror: &mirror,
        mirror_config: MirrorConfig::default(),
        manifest_path: dir.join("files-from.txt"),
        cache_root: dir.join("cache"),
    };
    let mut cfg = config("1946-47", "1947-48");
    cfg.limit = Some(1);

    let report = run_backfill(&conn, &cfg, &ports).unwrap();
    assert_eq!(report.games, 1, "the oldest game runs first");
    assert_eq!(report.fetched_ready, 1);
    assert_eq!(
        nbatv_db::tape_sources_for(&conn, "194611010TRH")
            .unwrap()
            .len(),
        1,
        "the first game in season order is the one that ran"
    );
    assert!(
        nbatv_db::tape_sources_for(&conn, "194711010TRH")
            .unwrap()
            .is_empty(),
        "later games never ran"
    );
}

// ---- Acceptance: rescan-hinted games are skipped ---------------------------

#[test]
fn rescan_hinted_games_are_skipped_when_probed_is_empty() {
    let conn = seeded_two_seasons();
    let dir = temp_dir("rescan");
    let manifest = dir.join("files-from.txt");
    let (p0, p1, p2, p3, p4) = winning_probes();
    let registry = reject_registry(&p0, &p1, &p2, &p3, &p4);
    let fetcher = ScriptedFetcher::new(vec![ScriptStep::complete(&vec![7u8; 64])]);
    let duration = ScriptedDuration::seconds(5400.0);
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mirror = fake_mirror(&calls);
    let ports = BackfillPorts {
        probes: registry,
        politeness: PolitenessConfig::default(),
        fetcher: &fetcher,
        duration: &duration,
        mirror: &mirror,
        mirror_config: MirrorConfig::default(),
        manifest_path: manifest.clone(),
        cache_root: dir.join("cache"),
    };
    let cfg = config("1946-47", "1947-48");

    let first = run_backfill(&conn, &cfg, &ports).unwrap();
    assert_eq!((first.swept, first.skipped), (3, 0));
    let probe_calls = p0.calls() + p1.calls() + p2.calls() + p3.calls() + p4.calls();
    let fetch_calls = fetcher.calls();

    // Same timestamp: every sweep row is inside its rescan window, so the
    // runner must NOT re-call the sweep — skipped derives from
    // `probed == []`, with no second freshness implementation.
    let second = run_backfill(&conn, &cfg, &ports).unwrap();
    assert_eq!(second.games, 3);
    assert_eq!(second.swept, 0, "no game re-probed inside its window");
    assert_eq!(second.skipped, 3, "skipped counts the empty-probed games");
    assert_eq!(
        p0.calls() + p1.calls() + p2.calls() + p3.calls() + p4.calls(),
        probe_calls,
        "the second run made zero probe calls"
    );
    assert_eq!(
        fetcher.calls(),
        fetch_calls,
        "Ready rows are never re-fetched"
    );
    assert_eq!(second.found, 0, "nothing unfetched remains");
    assert_eq!(
        second.mirror.outcome,
        MirrorOutcome::Completed { files: 3 },
        "the mirror still runs over the stored Ready set"
    );
}

// ---- Fetch stage: only non-Ready byte-class rows ---------------------------

#[test]
fn ready_rows_are_not_refetched_but_failed_rows_retry() {
    let conn = seeded_two_seasons();
    let dir = temp_dir("ready");
    let cache_root = dir.join("cache");
    // One game already Ready (never re-fetched), one Failed (retries).
    for (id, season, state) in [
        ("194611010TRH", "1946-47", CacheState::Ready),
        ("194611020CHS", "1946-47", CacheState::Failed),
    ] {
        let dest = cache_path(
            &cache_root,
            season,
            id,
            "NYK",
            if id == "194611020CHS" { "CHS" } else { "TRH" },
            "ia",
            "mp4",
        );
        nbatv_db::upsert_cache_entry(
            &conn,
            &nbatv_db::CacheEntry {
                game_id: id.to_owned(),
                rank: 1,
                source_class: "internet-archive".to_owned(),
                local_path: dest.to_string_lossy().into_owned(),
                bytes: 64,
                verified_at: (state == CacheState::Ready).then(|| NOW.to_owned()),
                state,
            },
        )
        .unwrap();
    }
    let (p0, p1, p2, p3, p4) = winning_probes();
    let registry = reject_registry(&p0, &p1, &p2, &p3, &p4);
    let fetcher = ScriptedFetcher::new(vec![ScriptStep::complete(&vec![7u8; 64])]);
    let duration = ScriptedDuration::seconds(5400.0);
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mirror = fake_mirror(&calls);
    let ports = BackfillPorts {
        probes: registry,
        politeness: PolitenessConfig::default(),
        fetcher: &fetcher,
        duration: &duration,
        mirror: &mirror,
        mirror_config: MirrorConfig::default(),
        manifest_path: dir.join("files-from.txt"),
        cache_root: cache_root.clone(),
    };

    let report = run_backfill(&conn, &config("1946-47", "1946-47"), &ports).unwrap();
    assert_eq!(report.games, 2);
    assert_eq!(report.found, 1, "only the Failed row's game needs bytes");
    assert_eq!(fetcher.calls(), 1, "the Ready row never re-fetches");
    assert_eq!(report.fetched_ready, 1);

    // A clip-length download fails verification honestly and never reads Ready.
    let conn2 = seeded_two_seasons();
    let dir2 = temp_dir("clipfail");
    let (q0, q1, q2, q3, q4) = winning_probes();
    let registry2 = reject_registry(&q0, &q1, &q2, &q3, &q4);
    let clip_fetcher = ScriptedFetcher::new(vec![ScriptStep::complete(b"clip-bytes")]);
    let clip_duration = ScriptedDuration::seconds(600.0);
    let calls2 = Arc::new(Mutex::new(Vec::new()));
    let mirror2 = fake_mirror(&calls2);
    let ports2 = BackfillPorts {
        probes: registry2,
        politeness: PolitenessConfig::default(),
        fetcher: &clip_fetcher,
        duration: &clip_duration,
        mirror: &mirror2,
        mirror_config: MirrorConfig::default(),
        manifest_path: dir2.join("files-from.txt"),
        cache_root: dir2.join("cache"),
    };
    let report2 = run_backfill(&conn2, &config("1946-47", "1946-47"), &ports2).unwrap();
    assert_eq!(report2.fetched_ready, 0);
    assert_eq!(report2.fetched_failed, 2);
    assert!(
        matches!(
            report2.mirror.outcome,
            MirrorOutcome::Completed { files: 0 }
        ),
        "failed fetches never reach the mirror: {:?}",
        report2.mirror.outcome
    );
}

// ---- Mirror stage: one dry-run/apply call threading the flag ---------------

#[test]
fn mirror_runs_once_with_the_configured_dry_run_flag() {
    for dry_run in [true, false] {
        let conn = seeded_two_seasons();
        let dir = temp_dir(if dry_run { "dry" } else { "apply" });
        let (p0, p1, p2, p3, p4) = winning_probes();
        let registry = reject_registry(&p0, &p1, &p2, &p3, &p4);
        let fetcher = ScriptedFetcher::new(vec![ScriptStep::complete(&vec![7u8; 64])]);
        let duration = ScriptedDuration::seconds(5400.0);
        let calls = Arc::new(Mutex::new(Vec::new()));
        let mirror = fake_mirror(&calls);
        let ports = BackfillPorts {
            probes: registry,
            politeness: PolitenessConfig::default(),
            fetcher: &fetcher,
            duration: &duration,
            mirror: &mirror,
            mirror_config: MirrorConfig::default(),
            manifest_path: dir.join("files-from.txt"),
            cache_root: dir.join("cache"),
        };
        let mut cfg = config("1946-47", "1946-47");
        cfg.dry_run = dry_run;

        let report = run_backfill(&conn, &cfg, &ports).unwrap();
        assert_eq!(report.mirror.dry_run, dry_run);
        let calls = calls.lock().expect("calls");
        let copies: Vec<_> = calls
            .iter()
            .filter(|argv| argv.get(1).map(String::as_str) == Some("copy"))
            .collect();
        assert_eq!(copies.len(), 1, "exactly one mirror copy runs per backfill");
        assert_eq!(
            copies[0].contains(&"--dry-run".to_owned()),
            dry_run,
            "the copy argv carries --dry-run exactly when previewing"
        );
    }
}

#[test]
fn remote_missing_surfaces_honestly_without_failing_the_run() {
    let conn = seeded_two_seasons();
    let dir = temp_dir("missing");
    let (p0, p1, p2, p3, p4) = winning_probes();
    let registry = reject_registry(&p0, &p1, &p2, &p3, &p4);
    let fetcher = ScriptedFetcher::new(vec![ScriptStep::complete(&vec![7u8; 64])]);
    let duration = ScriptedDuration::seconds(5400.0);
    // No remote configured: listremotes answers empty.
    let mirror = RcloneMirror::with_runner(Box::new(|argv: &[String]| {
        assert_eq!(argv.get(1).map(String::as_str), Some("listremotes"));
        Ok(RcloneOutput::success(""))
    }));
    let ports = BackfillPorts {
        probes: registry,
        politeness: PolitenessConfig::default(),
        fetcher: &fetcher,
        duration: &duration,
        mirror: &mirror,
        mirror_config: MirrorConfig::default(),
        manifest_path: dir.join("files-from.txt"),
        cache_root: dir.join("cache"),
    };

    let report = run_backfill(&conn, &config("1946-47", "1946-47"), &ports).unwrap();
    assert_eq!(report.fetched_ready, 2, "sweep and fetch still complete");
    assert!(
        matches!(report.mirror.outcome, MirrorOutcome::RemoteMissing { .. }),
        "the missing remote is named, not failed: {:?}",
        report.mirror.outcome
    );
    assert!(
        report.summary().contains("RemoteMissing"),
        "the summary says what to do next: {}",
        report.summary()
    );
}

// ---- Argv: the tiny clap-free surface --------------------------------------

fn argv(words: &[&str]) -> Vec<String> {
    words.iter().map(|w| w.to_string()).collect()
}

#[test]
fn argv_parses_the_minimal_surface() {
    let args = parse_argv(&argv(&["nbatv-catalog-runner", "1946-47", "1947-48"])).unwrap();
    assert_eq!(args.season_start, "1946-47");
    assert_eq!(args.season_end, "1947-48");
    assert!(
        args.dry_run,
        "preview by default: bytes never leave the machine"
    );
    assert_eq!(args.limit, None);
    assert_eq!(args.max_retries, 3);
    assert_eq!(args.db_path, PathBuf::from("data/archive.db"));

    let apply = parse_argv(&argv(&[
        "nbatv-catalog-runner",
        "1946-47",
        "1947-48",
        "--apply",
        "--limit",
        "5",
        "--db",
        "/tmp/a.db",
        "--now",
        "2026-09-01",
    ]))
    .unwrap();
    assert!(!apply.dry_run);
    assert_eq!(apply.limit, Some(5));
    assert_eq!(apply.db_path, PathBuf::from("/tmp/a.db"));
    assert_eq!(apply.now, "2026-09-01");

    // Last flag wins, so scripts can append `--dry-run` defensively.
    let back = parse_argv(&argv(&[
        "nbatv-catalog-runner",
        "1946-47",
        "1947-48",
        "--apply",
        "--dry-run",
    ]))
    .unwrap();
    assert!(back.dry_run);
}

#[test]
fn argv_rejects_usage_errors() {
    assert!(
        parse_argv(&argv(&["nbatv-catalog-runner"])).is_err(),
        "the season range is required"
    );
    assert!(
        parse_argv(&argv(&["nbatv-catalog-runner", "1946-47", "bogus"])).is_err(),
        "an unparseable season is a usage error"
    );
    assert!(
        parse_argv(&argv(&["nbatv-catalog-runner", "1947-48", "1946-47"])).is_err(),
        "a reversed range is a usage error"
    );
    assert!(
        parse_argv(&argv(&[
            "nbatv-catalog-runner",
            "1946-47",
            "1947-48",
            "--limit",
            "0"
        ]))
        .is_err(),
        "a zero limit runs nothing: say so loudly"
    );
    assert!(
        parse_argv(&argv(&[
            "nbatv-catalog-runner",
            "1946-47",
            "1947-48",
            "--frobnicate"
        ]))
        .is_err(),
        "unknown flags are usage errors, never ignored"
    );
}

// ---- Fetch destination honesty ----------------------------------------------

#[test]
fn fetch_destinations_use_the_research_15_naming() {
    let conn = seeded_two_seasons();
    let dir = temp_dir("naming");
    let cache_root = dir.join("cache");
    let (p0, p1, p2, p3, p4) = winning_probes();
    let registry = reject_registry(&p0, &p1, &p2, &p3, &p4);
    let fetcher = ScriptedFetcher::new(vec![ScriptStep::complete(&vec![7u8; 64])]);
    let duration = ScriptedDuration::seconds(5400.0);
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mirror = fake_mirror(&calls);
    let ports = BackfillPorts {
        probes: registry,
        politeness: PolitenessConfig::default(),
        fetcher: &fetcher,
        duration: &duration,
        mirror: &mirror,
        mirror_config: MirrorConfig::default(),
        manifest_path: dir.join("files-from.txt"),
        cache_root: cache_root.clone(),
    };

    run_backfill(&conn, &config("1946-47", "1946-47"), &ports).unwrap();
    let entries = nbatv_db::cache_entries_for(&conn, "194611010TRH").unwrap();
    assert_eq!(entries.len(), 1);
    let expected = cache_path(
        &cache_root,
        "1946-47",
        "194611010TRH",
        "NYK",
        "TRH",
        src_tag_for_url("https://archive.org/download/classic/game.mp4"),
        "mp4",
    );
    assert_eq!(
        PathBuf::from(&entries[0].local_path),
        expected,
        "the fetch destination is the agreed cache naming"
    );
    assert_eq!(entries[0].state, CacheState::Ready, "the row reads Ready");
}

#[test]
fn non_byte_class_tape_rows_are_never_fetched() {
    // A rank-0 (External Surface) win is a pointer, not bytes: the fetch
    // stage must ignore it entirely, whatever its state claims.
    let conn = seeded_two_seasons();
    let mut external = TapeSource {
        game_id: "194611010TRH".to_owned(),
        rank: 0,
        source_class: "official-nba-free-tier".to_owned(),
        url_or_pointer: "https://www.nba.com/watch/featured".to_owned(),
        match_confidence: 1.0,
        verified_at: "2026-09-08".to_owned(),
    };
    // A rogue state column cannot make an External Surface row fetchable:
    // the filter is the playback class, not the state.
    external.match_confidence = 0.0;
    nbatv_db::insert_tape_source(&conn, &external).unwrap();

    let dir = temp_dir("nonfile");
    let (p0, p1, p2, p3, p4) = winning_probes();
    let registry = reject_registry(&p0, &p1, &p2, &p3, &p4);
    let fetcher = ScriptedFetcher::new(vec![ScriptStep::complete(&vec![7u8; 2048])]);
    let duration = ScriptedDuration::seconds(5400.0);
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mirror = fake_mirror(&calls);
    let ports = BackfillPorts {
        probes: registry,
        politeness: PolitenessConfig::default(),
        fetcher: &fetcher,
        duration: &duration,
        mirror: &mirror,
        mirror_config: MirrorConfig::default(),
        manifest_path: dir.join("files-from.txt"),
        cache_root: dir.join("cache"),
    };
    let report = run_backfill(&conn, &config("1946-47", "1947-48"), &ports).unwrap();
    assert_eq!(
        report.found, 3,
        "the rank-0 row adds nothing to the fetch set"
    );
    for game in ["194611010TRH", "194611020CHS", "194711010TRH"] {
        let entries = nbatv_db::cache_entries_for(&conn, game).unwrap();
        assert!(
            entries
                .iter()
                .all(|e| e.source_class != external.source_class),
            "no cache row may reference the External Surface pointer"
        );
    }
}

#[test]
fn one_quota_is_shared_across_every_game_in_the_run() {
    // The runner mints ONE YoutubeQuota per run; probes capture the
    // remaining budget at call time, so equal readings across games prove
    // the budget was shared, not re-minted per game.
    let conn = seeded_two_seasons();
    let dir = temp_dir("quota");
    let (p0, p1, p2, p3, p4) = winning_probes();
    let registry = reject_registry(&p0, &p1, &p2, &p3, &p4);
    let fetcher = ScriptedFetcher::new(vec![ScriptStep::complete(&vec![1u8; 8])]);
    let duration = ScriptedDuration::seconds(5400.0);
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mirror = fake_mirror(&calls);
    let ports = BackfillPorts {
        probes: registry,
        politeness: PolitenessConfig::default(),
        fetcher: &fetcher,
        duration: &duration,
        mirror: &mirror,
        mirror_config: MirrorConfig::default(),
        manifest_path: dir.join("files-from.txt"),
        cache_root: dir.join("cache"),
    };
    run_backfill(&conn, &config("1946-47", "1947-48"), &ports).unwrap();
    let spendings: Vec<Option<u32>> = [&p0, &p1, &p2, &p3, &p4]
        .iter()
        .map(|p| p.last_quota_remaining())
        .collect();
    let seen: Vec<Option<u32>> = spendings
        .iter()
        .filter(|q| q.is_some())
        .map(|q| *q)
        .collect();
    assert!(
        !seen.is_empty(),
        "at least one probe must observe the quota"
    );
    for pair in seen.windows(2) {
        assert_eq!(pair[0], pair[1], "one budget shared across the whole run");
    }
}
