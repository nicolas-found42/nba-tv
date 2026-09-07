//! `nbatv_shell`: the app window and all browse screens.
//!
//! Shell-owned [`route::Route`]: Home → Season dashboard → Team → Game.
//! Backed by an in-memory [`store::FixtureStore`]; no network, no media.

pub mod app;
pub mod model;
pub mod route;
pub mod store;

pub use app::ShellApp;
pub use model::{
    cell, playback_class_for_rank, BoxPlayer, BoxScore, BoxTeam, Game, GameType, PlaybackClass,
    Season, TapeSource, TapeState, Team,
};
pub use route::{Crumb, ParseError, Route};
pub use store::{FixtureStore, PaletteItem, PaletteKind, SeasonCounts};
