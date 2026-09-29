//! `nbatv_shell`: the app window and all browse screens.
//!
//! Shell-owned [`route::Route`]: Home → Season dashboard → Team → Game.
//! Backed by the archive database through [`db_store::DbStore`] (opened at
//! [`db_store::ARCHIVE_DB_PATH`]); the fixture store remains for tests and
//! offline development only. No network, no media.

pub mod app;
pub mod db_store;
pub mod embed;
pub mod handover;
pub mod model;
pub mod route;
pub mod store;
pub use app::{LaneAStatus, ShellApp};
pub use db_store::{DbStore, Store, ARCHIVE_DB_PATH};
pub use embed::{
    cue_snippet, embed_url_for, is_sanctioned_embed, is_sign_in_url, EmbedBounds, EmbedError,
    EmbedHost, EmbedSession, LaneBStatus, WEBVIEW_PROFILE_DIR,
};
pub use handover::{dispatch_for, CacheEntry, PlayDispatch};
pub use model::{
    cell, playback_class_for_rank, BoxPlayer, BoxScore, BoxTeam, Game, GameType, PlaybackClass,
    Season, TapeSource, TapeState, Team,
};
pub use nbatv_catalog::{ReviewItem, SweepStatus};
pub use route::{Crumb, ParseError, Route};
pub use store::{FixtureStore, PaletteItem, PaletteKind, SeasonCounts};
