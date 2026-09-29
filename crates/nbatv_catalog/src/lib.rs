//! TapeCatalog sweep engine (issue #20): the sweep pipeline over the Source
//! Ladder with an injected [`SourceProbe`] port.
//!
//! Vocabulary per `CONTEXT.md`: **TapeCatalog** here is the sweep pipeline
//! (per-game ordered rung evaluation + evidence persistence), **SourceProbe**
//! is the port every network lookup sits behind (fakes in tests, real probes
//! in later tickets), **Game Tape** rows appear only for CONFIRMED/LIKELY
//! matches while REVIEW candidates surface in the review list and never
//! dispatch to playback.
//!
//! Start at [`sweep_game`]: hand it an archive connection, a [`GameContext`],
//! a [`ProbeRegistry`] with one probe per rung 0–4, the [`PolitenessConfig`],
//! a [`YoutubeQuota`] tracker, and a `YYYY-MM-DD` timestamp. Everything is
//! offline by construction — the suite links no network implementation.

pub mod civil;
pub mod drive;
pub mod fetch;
pub mod ia_probe;
pub mod nba_probe;
pub mod politeness;
pub mod probe;
pub mod rung4_probe;
pub mod runner;
pub mod scorer;
pub mod sweep;
pub mod ytdlp_probe;

pub use drive::{
    drive_sign_in_prompt, mirror_ready_entries, MirrorConfig, MirrorEntry, MirrorError,
    MirrorOutcome, MirrorReport, RcloneMirror, RcloneOutput,
};
pub use runner::{
    ending_year_to_slug, exit_code_for_report, expand_season_range, parse_argv, run_backfill,
    season_slug_to_ending_year, usage, BackfillConfig, BackfillPorts, BackfillReport, RunnerArgs,
    RunnerError, DEFAULT_DB_PATH, DEFAULT_MANIFEST_PATH, DEFAULT_MAX_RETRIES, EXIT_OK, EXIT_RUN,
    EXIT_USAGE,
};

pub use fetch::{
    cache_path, cache_root, fetch_to_cache, src_tag_for_url, verify, CacheFetchReport, CurlFetcher,
    DurationProbe, FetchOutcome, FetchSpec, FfprobeDuration, ScriptStep, ScriptedDuration,
    ScriptedFetcher, TapeFetcher, VerifyOutcome, CACHE_GITIGNORE_PATTERN, MAX_GAME_SECS,
    MIN_GAME_SECS, RETRY_PAUSE,
};
pub use ia_probe::{IaError, IaHttp, IaProbe};
pub use nba_probe::{
    finals_for_season, finals_month_ok, nba_catalog_due, nba_rescan_hint, season_for_date,
    FinalsEntry, NbaProbe, FINALS_CATALOG, NBA_CATALOG_REFRESH_DAYS, NBA_CATALOG_VERIFIED,
    NBA_HELP_URL, NBA_WATCH_URL,
};
pub use nbatv_ladder::{MatchLevel, SweepStatus, YoutubeQuota};
pub use politeness::PolitenessConfig;
pub use probe::{
    GameContext, ProbeCandidate, ProbeOutcome, ProbeRegistry, ScriptedProbe, SourceProbe,
};
pub use rung4_probe::{rung4_query_text, Rung4Probe};
pub use scorer::{confidence_for, level_to_str, parse_level, score_candidate};
pub use sweep::{
    game_context_for, review_list, review_list_all, rung_name, sweep_game, sweep_status_for,
    sweep_status_from, ReviewItem, SweepError, SweepReport,
};
pub use ytdlp_probe::{registry_with_ytdlp, YtdlpProbe};
