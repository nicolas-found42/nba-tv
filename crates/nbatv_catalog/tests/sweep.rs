//! TapeCatalog sweep pipeline at the port boundary (issue #20).
//!
//! Every test drives [`sweep_game`] through a fake [`SourceProbe`]: zero
//! network in the suite by construction — the only probe implementations
//! linked here are [`ScriptedProbe`] and the quota-gating fake below.

use nbatv_catalog::{
    game_context_for, rank_candidates, reconcile_candidate_level, review_list,
    review_list_with_judge, sweep_game, sweep_game_with_judge, sweep_status_for, CandidateVerdict,
    CollectorNoteInput, CrawlFailureChoice, DisabledJevJudge, FileSelectionInput, GameContext,
    GameTypeChoice, HtmlRowChoice, JevError, JevJudge, PolitenessConfig, ProbeCandidate,
    ProbeOutcome, ProbeRegistry, ScriptedProbe, SearchTemplateSelection, SourceProbe, SweepStatus,
    TeamChoice,
};
use nbatv_db::{create_schema, insert_game, GameQuery, GameRow};
use nbatv_ladder::YoutubeQuota;
use rusqlite::Connection;
use std::time::Duration;

const GAME_ID: &str = "194611010TRH";
const T0: &str = "2026-01-01";

fn game_row() -> GameRow {
    GameRow {
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
    }
}

fn seeded_conn() -> Connection {
    let conn = Connection::open_in_memory().expect("in-memory archive db");
    create_schema(&conn).expect("create_schema");
    insert_game(&conn, &game_row()).unwrap();
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

fn polite() -> PolitenessConfig {
    PolitenessConfig::default()
}

fn quota() -> YoutubeQuota {
    YoutubeQuota::new()
}

/// Evidence scoring CONFIRMED against TRH/NYK: both teams, full-game marker,
/// full-length duration.
fn confirmed_evidence() -> ProbeCandidate {
    ProbeCandidate {
        url_or_pointer: "https://archive.org/details/194611010TRH".to_owned(),
        title: "NYK at TRH Full Game 1946".to_owned(),
        description: "Complete broadcast".to_owned(),
        duration_secs: Some(7_200),
    }
}

/// Evidence scoring LIKELY: both teams, no full-game marker, unknown length.
fn likely_evidence() -> ProbeCandidate {
    ProbeCandidate {
        url_or_pointer: "https://www.youtube.com/watch?v=ABCDEFGHIJK".to_owned(),
        title: "NYK at TRH 1946-11-01".to_owned(),
        description: "Archive upload".to_owned(),
        duration_secs: None,
    }
}

/// Evidence scoring REVIEW: one team only, short clip.
fn review_evidence() -> ProbeCandidate {
    ProbeCandidate {
        url_or_pointer: "https://example.com/clip/194611010TRH".to_owned(),
        title: "NYK highlights 1946".to_owned(),
        description: "Short reel".to_owned(),
        duration_secs: Some(600),
    }
}

/// Evidence scoring Reject: names no team, claims no tape.
fn reject_evidence() -> ProbeCandidate {
    ProbeCandidate {
        url_or_pointer: "https://example.com/unrelated".to_owned(),
        title: "Cats playing piano compilation".to_owned(),
        description: "Unrelated upload".to_owned(),
        duration_secs: None,
    }
}

fn outcome_for(evidence: ProbeCandidate) -> ProbeOutcome {
    ProbeOutcome::found("rung query".to_owned(), vec![evidence])
}

fn scripted(rung: u8, evidence: ProbeCandidate) -> ScriptedProbe {
    ScriptedProbe::responding(rung, outcome_for(evidence))
}

fn all_reject_registry<'a>(
    p0: &'a ScriptedProbe,
    p1: &'a ScriptedProbe,
    p2: &'a ScriptedProbe,
    p3: &'a ScriptedProbe,
    p4: &'a ScriptedProbe,
) -> ProbeRegistry<'a> {
    let mut reg = ProbeRegistry::new();
    reg.register(p0)
        .register(p1)
        .register(p2)
        .register(p3)
        .register(p4);
    reg
}

fn reject_probes() -> (
    ScriptedProbe,
    ScriptedProbe,
    ScriptedProbe,
    ScriptedProbe,
    ScriptedProbe,
) {
    (
        scripted(0, reject_evidence()),
        scripted(1, reject_evidence()),
        scripted(2, reject_evidence()),
        scripted(3, reject_evidence()),
        scripted(4, reject_evidence()),
    )
}

fn no_priority(_: &[String]) -> Result<Option<u8>, JevError> {
    Ok(None)
}

fn prioritize_second(items: &[String]) -> Result<Option<u8>, JevError> {
    Ok(Some(
        items
            .first()
            .is_some_and(|item| item.contains("second"))
            .then_some(3)
            .unwrap_or(1),
    ))
}

fn fail_priority(_: &[String]) -> Result<Option<u8>, JevError> {
    Err(JevError::Transport("review test failure".to_owned()))
}

fn invalid_priority(_: &[String]) -> Result<Option<u8>, JevError> {
    Ok(Some(4))
}

type PriorityResult = Result<Option<u8>, JevError>;

struct CandidatePolicyJudge {
    fail: bool,
    priority: fn(&[String]) -> PriorityResult,
}

impl JevJudge for CandidatePolicyJudge {
    fn select_file(
        &self,
        _: &FileSelectionInput,
    ) -> Result<Option<nbatv_catalog::FileSelection>, JevError> {
        Ok(None)
    }

    fn match_candidate(
        &self,
        _: &GameContext,
        candidate: &ProbeCandidate,
    ) -> Result<Option<CandidateVerdict>, JevError> {
        if self.fail {
            return Err(JevError::Transport("candidate test failure".to_owned()));
        }
        Ok(Some(CandidateVerdict {
            same_game: Some(candidate.url_or_pointer != "contradiction"),
            both_teams: Some(true),
            date_or_round: Some(true),
            full_game: Some(true),
            quality: Some(3.0),
            decisive: true,
        }))
    }

    fn classify_game_type(&self, _: &GameTypeChoice) -> Result<Option<String>, JevError> {
        Ok(None)
    }

    fn align_team(&self, _: &TeamChoice) -> Result<Option<String>, JevError> {
        Ok(None)
    }

    fn match_collector_note(&self, _: &CollectorNoteInput) -> Result<Option<String>, JevError> {
        Ok(None)
    }

    fn choose_search_template(
        &self,
        _: &GameContext,
        _: &str,
    ) -> Result<Option<SearchTemplateSelection>, JevError> {
        Ok(None)
    }

    fn classify_crawl_failure(&self, _: &str) -> Result<Option<CrawlFailureChoice>, JevError> {
        Ok(None)
    }

    fn classify_html_row(&self, _: &str, _: &str) -> Result<Option<HtmlRowChoice>, JevError> {
        Ok(None)
    }

    fn route_shell_command(&self, _: &str) -> Result<Option<String>, JevError> {
        Ok(None)
    }

    fn prioritize_review(&self, items: &[String], _: &str) -> Result<Option<u8>, JevError> {
        (self.priority)(items)
    }
}

#[test]
fn candidate_contradiction_demotes_but_never_promotes() {
    let mut likely = likely_evidence();
    likely.url_or_pointer = "contradiction".to_owned();
    let reject = reject_evidence();
    let candidates = vec![likely.clone(), reject.clone()];

    let ranked = rank_candidates(
        &ctx(),
        &candidates,
        &CandidatePolicyJudge {
            fail: false,
            priority: no_priority,
        },
    );

    assert_eq!(ranked[0].index, 0);
    assert_eq!(ranked[0].level, nbatv_ladder::MatchLevel::Review);
    assert_eq!(ranked[1].index, 1);
    assert_eq!(ranked[1].level, nbatv_ladder::MatchLevel::Reject);
    assert_eq!(
        reconcile_candidate_level(nbatv_ladder::MatchLevel::Confirmed, None),
        nbatv_ladder::MatchLevel::Confirmed
    );
}

#[test]
fn resweep_demoting_a_candidate_removes_its_stale_tape_source() {
    let conn = seeded_conn();
    let first_p0 = scripted(0, confirmed_evidence());
    let (_, first_p1, first_p2, first_p3, first_p4) = reject_probes();
    let first_reg = all_reject_registry(&first_p0, &first_p1, &first_p2, &first_p3, &first_p4);
    let mut quota = quota();

    let first = sweep_game(&conn, &ctx(), &first_reg, &polite(), &mut quota, T0).unwrap();
    assert!(matches!(first.status, SweepStatus::Playable { .. }));
    assert_eq!(nbatv_db::tape_sources_for(&conn, GAME_ID).unwrap().len(), 1);
    nbatv_db::upsert_cache_entry(
        &conn,
        &nbatv_db::CacheEntry {
            game_id: GAME_ID.to_owned(),
            rank: 0,
            source_class: "catalog".to_owned(),
            local_path: "tape/194611010TRH.mp4".to_owned(),
            bytes: 123,
            verified_at: Some(T0.to_owned()),
            state: nbatv_db::CacheState::Ready,
        },
    )
    .unwrap();

    let mut contradicted = confirmed_evidence();
    contradicted.url_or_pointer = "contradiction".to_owned();
    let p0 = scripted(0, contradicted);
    let (_, p1, p2, p3, p4) = reject_probes();
    let reg = all_reject_registry(&p0, &p1, &p2, &p3, &p4);
    let second = sweep_game_with_judge(
        &conn,
        &ctx(),
        &reg,
        &polite(),
        &mut quota,
        "2026-04-02",
        &CandidatePolicyJudge {
            fail: false,
            priority: no_priority,
        },
    )
    .unwrap();

    assert!(!matches!(second.status, SweepStatus::Playable { .. }));
    assert!(nbatv_db::tape_sources_for(&conn, GAME_ID)
        .unwrap()
        .is_empty());
    assert!(nbatv_db::cache_entries_for(&conn, GAME_ID)
        .unwrap()
        .is_empty());
}

#[test]
fn sweep_demotes_an_explicitly_contradicted_candidate() {
    let conn = seeded_conn();
    let mut contradicted = confirmed_evidence();
    contradicted.url_or_pointer = "contradiction".to_owned();
    let p0 = scripted(0, contradicted);
    let (_, p1, p2, p3, p4) = reject_probes();
    let reg = all_reject_registry(&p0, &p1, &p2, &p3, &p4);
    let mut quota = quota();

    let report = sweep_game_with_judge(
        &conn,
        &ctx(),
        &reg,
        &polite(),
        &mut quota,
        T0,
        &CandidatePolicyJudge {
            fail: false,
            priority: no_priority,
        },
    )
    .unwrap();

    assert!(matches!(report.status, SweepStatus::Unavailable { .. }));
    let queries = nbatv_db::game_queries_for(&conn, GAME_ID).unwrap();
    assert_eq!(queries[0].best_match_level, "review");
    assert_eq!(queries[0].review_url.as_deref(), Some("contradiction"));
    assert!(nbatv_db::tape_sources_for(&conn, GAME_ID)
        .unwrap()
        .is_empty());
}

#[test]
fn candidate_jev_error_preserves_deterministic_sweep() {
    let conn = seeded_conn();
    let p0 = scripted(0, likely_evidence());
    let mut reg = ProbeRegistry::new();
    reg.register(&p0);
    let mut quota = quota();

    let report = sweep_game_with_judge(
        &conn,
        &ctx(),
        &reg,
        &polite(),
        &mut quota,
        T0,
        &CandidatePolicyJudge {
            fail: true,
            priority: no_priority,
        },
    )
    .unwrap();

    assert_eq!(report.status, SweepStatus::Playable { rank: 0 });
    let queries = nbatv_db::game_queries_for(&conn, GAME_ID).unwrap();
    assert_eq!(queries[0].best_match_level, "likely");
    assert_eq!(nbatv_db::tape_sources_for(&conn, GAME_ID).unwrap().len(), 1);
}

#[test]
fn review_priority_reorders_in_memory_without_changing_queries() {
    let conn = seeded_conn();
    for (rung, query_text) in [(0, "first"), (1, "second")] {
        nbatv_db::upsert_game_query(
            &conn,
            &GameQuery {
                game_id: GAME_ID.to_owned(),
                rung,
                query_text: query_text.to_owned(),
                queried_at: T0.to_owned(),
                best_match_level: "review".to_owned(),
                review_url: Some(format!("https://example.com/{query_text}")),
                review_title: Some(query_text.to_owned()),
            },
        )
        .unwrap();
    }
    let before = nbatv_db::game_queries_for(&conn, GAME_ID).unwrap();

    let ranked = review_list_with_judge(
        &conn,
        GAME_ID,
        &CandidatePolicyJudge {
            fail: false,
            priority: prioritize_second,
        },
        "which candidate deserves review first?",
    )
    .unwrap();

    assert_eq!(
        ranked.iter().map(|item| item.rung).collect::<Vec<_>>(),
        vec![1, 0]
    );
    assert_eq!(nbatv_db::game_queries_for(&conn, GAME_ID).unwrap(), before);
}

#[test]
fn disabled_failed_or_invalid_review_priority_preserves_order() {
    let conn = seeded_conn();
    for rung in 0..2 {
        nbatv_db::upsert_game_query(
            &conn,
            &GameQuery {
                game_id: GAME_ID.to_owned(),
                rung,
                query_text: format!("query-{rung}"),
                queried_at: T0.to_owned(),
                best_match_level: "review".to_owned(),
                review_url: Some(format!("https://example.com/{rung}")),
                review_title: Some(format!("candidate-{rung}")),
            },
        )
        .unwrap();
    }
    let expected = vec![0, 1];
    let order = |judge: &dyn JevJudge| {
        review_list_with_judge(&conn, GAME_ID, judge, "")
            .unwrap()
            .into_iter()
            .map(|item| item.rung)
            .collect::<Vec<_>>()
    };

    assert_eq!(order(&DisabledJevJudge), expected);
    assert_eq!(
        order(&CandidatePolicyJudge {
            fail: false,
            priority: fail_priority,
        }),
        expected
    );
    assert_eq!(
        order(&CandidatePolicyJudge {
            fail: false,
            priority: invalid_priority,
        }),
        expected
    );
}

// ---- Acceptance: first LIKELY-or-better wins, sweep stops ascending --------

#[test]
fn first_likely_wins_and_stops_ascending() {
    let conn = seeded_conn();
    let p0 = scripted(0, likely_evidence());
    let p1 = scripted(1, confirmed_evidence());
    let p2 = scripted(2, reject_evidence());
    let p3 = scripted(3, reject_evidence());
    let p4 = scripted(4, reject_evidence());
    let reg = all_reject_registry(&p0, &p1, &p2, &p3, &p4);
    let mut quota = quota();

    let report = sweep_game(&conn, &ctx(), &reg, &polite(), &mut quota, T0).unwrap();

    assert_eq!(report.status, SweepStatus::Playable { rank: 0 });
    assert_eq!(
        report.probed,
        vec![0],
        "sweep stops at the first LIKELY+ rung"
    );
    assert_eq!(p0.calls(), 1);
    assert_eq!(p1.calls(), 0, "higher rungs are never probed after a win");

    let tapes = nbatv_db::tape_sources_for(&conn, GAME_ID).unwrap();
    assert_eq!(tapes.len(), 1);
    assert_eq!(tapes[0].rank, 0);
    assert_eq!(
        tapes[0].url_or_pointer,
        "https://www.youtube.com/watch?v=ABCDEFGHIJK"
    );

    // Only the probed rung records a query row.
    let queries = nbatv_db::game_queries_for(&conn, GAME_ID).unwrap();
    assert_eq!(queries.len(), 1);
    assert_eq!(queries[0].rung, 0);
    assert_eq!(queries[0].best_match_level, "likely");
}

#[test]
fn confirmed_flows_to_tape_with_full_confidence() {
    let conn = seeded_conn();
    let p0 = scripted(0, reject_evidence());
    let p1 = scripted(1, confirmed_evidence());
    let mut reg = ProbeRegistry::new();
    reg.register(&p0).register(&p1);
    let mut quota = quota();

    let report = sweep_game(&conn, &ctx(), &reg, &polite(), &mut quota, T0).unwrap();

    assert_eq!(report.status, SweepStatus::Playable { rank: 1 });
    assert_eq!(report.probed, vec![0, 1]);
    let tapes = nbatv_db::tape_sources_for(&conn, GAME_ID).unwrap();
    assert_eq!(tapes.len(), 1);
    assert_eq!(tapes[0].rank, 1);
    assert_eq!(tapes[0].source_class, "internet-archive");
    assert_eq!(tapes[0].match_confidence, 1.0);
    assert_eq!(tapes[0].verified_at, T0);
}

// ---- Acceptance: missing queries mean Sweeping -----------------------------

#[test]
fn missing_queries_mean_sweeping() {
    let conn = seeded_conn();
    let p0 = scripted(0, reject_evidence());
    let mut reg = ProbeRegistry::new();
    reg.register(&p0);
    let mut quota = quota();

    let report = sweep_game(&conn, &ctx(), &reg, &polite(), &mut quota, T0).unwrap();

    assert_eq!(report.status, SweepStatus::Sweeping);
    assert!(nbatv_db::tape_sources_for(&conn, GAME_ID)
        .unwrap()
        .is_empty());
}

// ---- Acceptance: fully recorded empty sweep means Unavailable --------------

#[test]
fn fully_recorded_empty_sweep_means_unavailable() {
    let conn = seeded_conn();
    let (p0, p1, p2, p3, p4) = reject_probes();
    let reg = all_reject_registry(&p0, &p1, &p2, &p3, &p4);
    let mut quota = quota();

    let report = sweep_game(&conn, &ctx(), &reg, &polite(), &mut quota, T0).unwrap();

    assert!(matches!(report.status, SweepStatus::Unavailable { .. }));
    assert_eq!(report.probed, vec![0, 1, 2, 3, 4]);
    let queries = nbatv_db::game_queries_for(&conn, GAME_ID).unwrap();
    assert_eq!(queries.len(), 5);
    assert!(
        queries.iter().all(|q| q.best_match_level == "reject"),
        "every outbound query is recorded, even failures"
    );
}

// ---- Acceptance: REVIEW lists for humans, never becomes tape --------------

#[test]
fn review_never_becomes_tape_but_lists_for_humans() {
    let conn = seeded_conn();
    let p0 = scripted(0, review_evidence());
    let (_, p1, p2, p3, p4) = reject_probes();
    let reg = all_reject_registry(&p0, &p1, &p2, &p3, &p4);
    let mut quota = quota();

    let report = sweep_game(&conn, &ctx(), &reg, &polite(), &mut quota, T0).unwrap();

    assert!(
        matches!(report.status, SweepStatus::Unavailable { .. }),
        "REVIEW-best full sweep is still unavailable (needs LIKELY+)"
    );
    assert!(
        nbatv_db::tape_sources_for(&conn, GAME_ID)
            .unwrap()
            .is_empty(),
        "REVIEW rows NEVER become tape_sources"
    );
    let reviews = review_list(&conn, GAME_ID).unwrap();
    assert_eq!(reviews.len(), 1);
    assert_eq!(reviews[0].rung, 0);
    assert_eq!(
        reviews[0].url_or_pointer,
        "https://example.com/clip/194611010TRH"
    );
    assert_eq!(reviews[0].query_text, "rung query");
}

// ---- Acceptance: rescan window skips, stale rows get replaced --------------

#[test]
fn fresh_window_skips_reprobe_and_stale_rows_are_replaced() {
    let conn = seeded_conn();
    let (p0, p1, p2, p3, p4) = reject_probes();
    let reg = all_reject_registry(&p0, &p1, &p2, &p3, &p4);
    let mut quota = quota();

    sweep_game(&conn, &ctx(), &reg, &polite(), &mut quota, T0).unwrap();
    assert_eq!(p0.calls(), 1);

    // Inside the 90-day window: no re-probe, same verdict from stored rows.
    let report = sweep_game(&conn, &ctx(), &reg, &polite(), &mut quota, "2026-01-15").unwrap();
    assert!(matches!(report.status, SweepStatus::Unavailable { .. }));
    assert_eq!(report.probed, Vec::<u8>::new());
    assert_eq!(p0.calls(), 1, "games inside the window are not re-probed");

    // Past the window: the sweep re-probes and replaces the stale rows.
    let report = sweep_game(&conn, &ctx(), &reg, &polite(), &mut quota, "2026-04-02").unwrap();
    assert!(matches!(report.status, SweepStatus::Unavailable { .. }));
    assert_eq!(report.probed, vec![0, 1, 2, 3, 4]);
    let queries = nbatv_db::game_queries_for(&conn, GAME_ID).unwrap();
    assert_eq!(queries.len(), 5, "stale rows are replaced, not duplicated");
    assert!(queries.iter().all(|q| q.queried_at == "2026-04-02"));
}

// ---- Acceptance: rows survive restart --------------------------------------

#[test]
fn queries_survive_restart_and_resume_without_reprobing() {
    let path = std::env::temp_dir().join(format!(
        "nbatv-catalog-restart-{}-{}.db",
        std::process::id(),
        GAME_ID
    ));
    let _ = std::fs::remove_file(&path);
    {
        let conn = Connection::open(&path).expect("open archive file");
        create_schema(&conn).expect("create_schema");
        insert_game(&conn, &game_row()).unwrap();
        let (p0, p1, p2, p3, p4) = reject_probes();
        let reg = all_reject_registry(&p0, &p1, &p2, &p3, &p4);
        sweep_game(&conn, &ctx(), &reg, &polite(), &mut quota(), T0).unwrap();
    }
    // "Restart": a fresh connection to the same file, no probes registered.
    {
        let conn = Connection::open(&path).expect("reopen archive file");
        let queries = nbatv_db::game_queries_for(&conn, GAME_ID).unwrap();
        assert_eq!(queries.len(), 5, "game_queries rows survive restart");
        let empty = ProbeRegistry::new();
        let report =
            sweep_game(&conn, &ctx(), &empty, &polite(), &mut quota(), "2026-01-15").unwrap();
        assert!(
            matches!(report.status, SweepStatus::Unavailable { .. }),
            "stored sweep resumes without re-probing"
        );
        assert_eq!(report.probed, Vec::<u8>::new());
    }
    let _ = std::fs::remove_file(&path);
}

// ---- Acceptance: pointers-only means Exists-not-streamable -----------------

#[test]
fn pointers_only_means_exists_not_streamable() {
    let conn = seeded_conn();
    nbatv_db::insert_tape_source(
        &conn,
        &nbatv_db::TapeSource {
            game_id: GAME_ID.to_owned(),
            rank: 5,
            source_class: "collector-catalogs".to_owned(),
            url_or_pointer: "pointer:collector/194611010TRH".to_owned(),
            match_confidence: 1.0,
            verified_at: T0.to_owned(),
        },
    )
    .unwrap();

    let status = sweep_status_for(&conn, GAME_ID).unwrap();
    assert_eq!(status, SweepStatus::ExistsNotStreamable);
}

// ---- Acceptance: politeness is the single source probes read ---------------

#[test]
fn politeness_config_reaches_probes_untouched() {
    let conn = seeded_conn();
    let p0 = scripted(0, reject_evidence());
    let mut reg = ProbeRegistry::new();
    reg.register(&p0);
    let mut quota = quota();
    let cfg = PolitenessConfig {
        ia_request_min_interval: Duration::from_secs(9),
        ytdlp_request_sleep: Duration::from_secs(3),
        ytdlp_sleep_requests: 4,
        max_concurrent_probes: 1,
        youtube_daily_limit: 100,
    };

    sweep_game(&conn, &ctx(), &reg, &cfg, &mut quota, T0).unwrap();

    assert_eq!(p0.last_politeness(), Some(cfg));
    assert_eq!(PolitenessConfig::default().youtube_daily_limit, 100);
}

/// The YouTube quota contract later probes implement: the probe spends one
/// budget unit per query and defers (records nothing) when the day is spent.
struct QuotaProbe {
    rung: u8,
    outcome: ProbeOutcome,
}

impl SourceProbe for QuotaProbe {
    fn rung(&self) -> u8 {
        self.rung
    }

    fn name(&self) -> &'static str {
        "test-quota-probe"
    }

    fn probe(
        &self,
        _game: &GameContext,
        _politeness: &PolitenessConfig,
        quota: &mut YoutubeQuota,
    ) -> ProbeOutcome {
        if quota.schedule(1) == 0 {
            return ProbeOutcome::deferred("youtube: daily quota spent, deferred".to_owned());
        }
        self.outcome.clone()
    }
}

#[test]
fn quota_exhaustion_defers_without_recording() {
    let conn = seeded_conn();
    let p0 = scripted(0, reject_evidence());
    let spent = YoutubeQuota::with_limit(0);
    let yt = QuotaProbe {
        rung: 2,
        outcome: outcome_for(likely_evidence()),
    };
    let p4 = scripted(4, reject_evidence());
    let mut reg = ProbeRegistry::new();
    reg.register(&p0).register(&yt).register(&p4);
    let mut quota = spent;

    let report = sweep_game(&conn, &ctx(), &reg, &polite(), &mut quota, T0).unwrap();

    assert_eq!(report.status, SweepStatus::Sweeping);
    assert_eq!(report.probed, vec![0, 4], "deferred rungs record nothing");
    let queries = nbatv_db::game_queries_for(&conn, GAME_ID).unwrap();
    assert!(
        queries.iter().all(|q| q.rung != 2),
        "a deferred query leaves no evidence row to block retry"
    );
}

// ---- Game context helper ----------------------------------------------------

#[test]
fn game_context_loads_from_the_archive() {
    let conn = seeded_conn();
    let loaded = game_context_for(&conn, GAME_ID)
        .unwrap()
        .expect("seeded game");
    assert_eq!(loaded, ctx());
    assert!(game_context_for(&conn, "190001010AAA").unwrap().is_none());
}
