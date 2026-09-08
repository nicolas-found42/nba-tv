//! `nbatv_shell`: the app window and all browse screens.
//!
//! Shell-owned [`route::Route`]: Home → Season dashboard → Team → Game.
//! Backed by an in-memory [`store::FixtureStore`]; no network, no media.

pub mod app;
pub mod embed;
pub mod handover;
pub mod model;
pub mod route;
pub mod store;
pub use app::{LaneAStatus, ShellApp};
pub use embed::{
    cue_snippet, embed_url_for, is_sanctioned_embed, EmbedBounds, EmbedError, EmbedHost,
};
pub use handover::{dispatch_for, CacheEntry, PlayDispatch};
pub use model::{
    cell, playback_class_for_rank, BoxPlayer, BoxScore, BoxTeam, Game, GameType, PlaybackClass,
    Season, TapeSource, TapeState, Team,
};
pub use route::{Crumb, ParseError, Route};
pub use store::{FixtureStore, PaletteItem, PaletteKind, SeasonCounts};
