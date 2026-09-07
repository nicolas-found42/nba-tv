//! Player Backend lanes for the nba-tv archive.
//!
//! Glossary (CONTEXT.md): the **Player Backend** turns Game Tape bytes into
//! moving pictures. Two lanes, per research 10 (in-window playback):
//!
//! - **Lane A (ffmpeg-sidecar, default):** progressive-file tape (Cache Tier
//!   files, Internet Archive direct MP4s, any progressive file URL) is
//!   decoded by a sidecar `ffmpeg` binary into frames that become `egui`
//!   textures. Workspace code stays pure Rust; C decode lives only behind
//!   the ffmpeg-sidecar binding.
//! - **Lane B (sanctioned vendor embed):** YouTube-class and other platform
//!   surfaces play in the vendor's own player hosted in a `wry`
//!   `build_as_child` webview. The pixels never enter an `egui` texture.
//!
//! The NBA App stays an **External Surface** (opened externally, never
//! decoded in-window). YouTube-via-ffmpeg and NBA-App-via-ffmpeg are
//! gate-failing and are never implemented here; see [`lane_b`] for the
//! documented refusal.
//!
//! Play-time resolution order (per research 02, section 3.3): Cache Tier
//! entry (if any) first, then `tape_sources` rows by ascending rank, then
//! honestly unavailable. The Box Score always renders regardless; only tape
//! can be unavailable.
//!
//! No media is ever downloaded by this crate. Tests use tiny inline fixtures
//! and never touch the network (except one optional `ffmpeg -version`
//! presence probe that skips gracefully when ffmpeg is absent).

pub mod lane_a;
pub mod lane_b;
pub mod playback;
pub mod pump;
pub mod resolve;

pub use lane_a::{
    is_moov_first, normalize_args, play_pipe_args, probe_args, seek_play_args, supports_range,
    EguiTextureStage, FrameToTexture, Mp4RangeReadiness, RawFrame, StubTextureStage, TextureImage,
};
pub use lane_b::{
    load_video_by_id_snippet, nba_app_opener, open_external, youtube_embed_url,
    youtube_embed_url_with_start, ExternalSurface, OpenAction,
};
pub use playback::{lane_for, Lane, PlaybackClass};
pub use pump::{Pump, PumpError};
pub use resolve::{resolve, CacheEntry, Resolved, TapeSource};
