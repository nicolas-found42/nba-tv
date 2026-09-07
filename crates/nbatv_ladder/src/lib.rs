//! Source Ladder for the nba-tv personal archive.
//!
//! Vocabulary per `CONTEXT.md`: **Source Ladder** (the fixed order in which
//! Tape Sources are searched for one Game), **Tape Source** (one place that
//! holds a Game Tape), **External Surface** (a source that plays in a vendor
//! player, not in the Player Backend).
//!
//! Rung order and playback/legality marks follow research doc 11
//! (Source Ladder v2). Pointers only: this crate never crawls, never
//! downloads media, holds no keys. Grey sources are pointers only.

pub mod collector;
pub mod crosswalk;
pub mod exhaustion;
pub mod quota;
pub mod rung;
pub mod source;

pub use collector::{parse_br_url_to_game_id, parse_gregg_line, GreggNote, UsasdRecord};
pub use crosswalk::{Crosswalk, CrosswalkRow, CHOUCISAN_FIXTURE};
pub use exhaustion::{
    evaluate_sweep, rescan_hint, MatchLevel, RecordedQuery, RescanHint, RungEvaluation, SweepStatus,
};
pub use quota::YoutubeQuota;
pub use rung::{Ladder, Legality, PlaybackClass, Rung};
pub use source::TapeSource;
