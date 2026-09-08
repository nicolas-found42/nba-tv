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

pub mod politeness;
pub mod probe;
pub mod scorer;
pub mod sweep;

pub use nbatv_ladder::{MatchLevel, SweepStatus, YoutubeQuota};
pub use politeness::PolitenessConfig;
pub use probe::{
    GameContext, ProbeCandidate, ProbeOutcome, ProbeRegistry, ScriptedProbe, SourceProbe,
};
pub use scorer::{confidence_for, level_to_str, parse_level, score_candidate};
pub use sweep::{
    game_context_for, review_list, review_list_all, rung_name, sweep_game, sweep_status_for,
    sweep_status_from, ReviewItem, SweepError, SweepReport,
};
