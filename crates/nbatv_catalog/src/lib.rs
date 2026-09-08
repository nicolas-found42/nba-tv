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

pub mod ia_probe;
pub mod nba_probe;
pub mod politeness;
pub mod probe;
pub mod rung4_probe;
pub mod scorer;
pub mod sweep;
pub mod ytdlp_probe;

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
