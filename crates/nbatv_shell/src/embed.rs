//! Lane B embed host: sanctioned vendor-player embeds in the shell window.
//!
//! The pure half of this module (bounds, the sanction gate, the dispatch
//! mapping, the cue snippet) is always compiled and headless-tested. Only
//! [`EmbedHost`] — the thin `wry` child-webview wrapper — sits behind the
//! `lane-b` feature, because CI has no WebKit and the default build must
//! stay `wry`-free.
//!
//! ## Legality gate (binding; mirrors `nbatv_player::lane_b` module docs)
//!
//! No yt-dlp/extraction path, never a `/watch?v=` page URL, never direct
//! stream/`googlevideo.com` bytes into anything, NBA App always external.
//! [`is_sanctioned_embed`] enforces this on every URL before it can reach a
//! webview, and both [`EmbedHost::open`] and [`EmbedHost::navigate`]
//! re-check it. [`embed_url_for`] maps only [`PlayDispatch::OpenEmbed`]
//! through the same gate; every other dispatch maps to `None`.
//!
//! ## Platform notes (from map research #5 — recorded, not heroed)
//!
//! The child webview is a native overlay parented to the shell window, not
//! an egui widget: egui cannot clip it, modal popups (the ⌘K palette)
//! render *under* it, and keyboard focus follows the platform webview while
//! it is up. Bounds are physical pixels and must be recomputed on
//! resize/DPI change; this slice opens at [`EmbedBounds::PLACEHOLDER`] and
//! leaves resize tracking to the driver slice. `wry` documents that child
//! creation may panic on an invalid/unsupported handle; the shell only ever
//! passes the live `eframe::Frame` handle, so that path stays untriggered.

use crate::handover::PlayDispatch;

/// Child-webview rectangle in physical pixels, relative to the shell window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmbedBounds {
    /// Left edge, physical pixels from the window's left edge.
    pub x: i32,
    /// Top edge, physical pixels from the window's top edge.
    pub y: i32,
    /// Width in physical pixels; must be nonzero.
    pub w: u32,
    /// Height in physical pixels; must be nonzero.
    pub h: u32,
}

impl EmbedBounds {
    /// Placeholder tape rect until the driver slice wires resize tracking:
    /// 960×540 at the window origin.
    pub const PLACEHOLDER: EmbedBounds = EmbedBounds {
        x: 0,
        y: 0,
        w: 960,
        h: 540,
    };

    /// Build bounds, rejecting a zero extent (`None` when `w == 0 || h == 0`:
    /// a zero-area webview is never a real tape rect).
    pub const fn new(x: i32, y: i32, w: u32, h: u32) -> Option<Self> {
        if w == 0 || h == 0 {
            return None;
        }
        Some(Self { x, y, w, h })
    }
}

/// The sanctioned embed URL for a Play dispatch, if it has one.
///
/// Maps [`PlayDispatch::OpenEmbed`] through [`is_sanctioned_embed`] and
/// every other dispatch to `None` — including [`PlayDispatch::OpenExternal`],
/// whose NBA App URLs must always open externally and never embed.
pub fn embed_url_for(dispatch: &PlayDispatch) -> Option<String> {
    match dispatch {
        PlayDispatch::OpenEmbed { url } if is_sanctioned_embed(url) => Some(url.clone()),
        _ => None,
    }
}

/// Whether `url` may load in the Lane B webview.
///
/// Accepts only `http(s)` URLs, and on a YouTube host only the `/embed/`
/// player over TLS (the ladder normalizes real YouTube tapes to
/// `www.youtube.com/embed/<id>` via [`nbatv_player::youtube_embed_url`], so
/// only that host pair plus the `youtube-nocookie` privacy pair pass).
/// Anything else `http(s)` — e.g. a Dailymotion/Vimeo-class embed URL the
/// ladder supplied already in [`PlayDispatch::OpenEmbed`] form — passes as
/// supplied.
///
/// Refuses: non-web schemes (`javascript:`, `data:`, `file:`, …); YouTube
/// `/watch` page URLs, `/shorts/`, and `youtu.be` short links (share links,
/// not players); `googlevideo.com` direct-stream bytes; and anything on
/// `nba.com` (Widevine: always external, never embedded).
pub fn is_sanctioned_embed(url: &str) -> bool {
    let (is_https, rest) = if let Some(rest) = url.strip_prefix("https://") {
        (true, rest)
    } else if let Some(rest) = url.strip_prefix("http://") {
        (false, rest)
    } else {
        return false;
    };
    let (raw_host, path) = match rest.find('/') {
        Some(idx) => (&rest[..idx], &rest[idx..]),
        None => (rest, "/"),
    };
    // Drop any userinfo/port before matching so `host:port` spellings of a
    // sanctioned host still match and `youtu.be:443` cannot slip past the
    // short-link refusal below.
    let host = raw_host
        .split('@')
        .next_back()
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    if host.is_empty() {
        return false;
    }
    // Same `nba.com` convention as `handover::dispatch_external`: NBA App /
    // watch.nba.com pages always open externally, never embedded.
    if host.contains("nba.com") {
        return false;
    }
    // Direct stream bytes must never load into anything.
    if host.contains("googlevideo.com") {
        return false;
    }
    // Short links are share links, not embed players.
    if host == "youtu.be" || host.ends_with(".youtu.be") {
        return false;
    }
    if host.contains("youtube") {
        let embed_host = host == "youtube.com"
            || host == "www.youtube.com"
            || host == "youtube-nocookie.com"
            || host == "www.youtube-nocookie.com";
        return embed_host
            && is_https
            && (path == "/embed" || path.starts_with("/embed/") || path.starts_with("/embed?"));
    }
    true
}

/// JavaScript snippet cueing the embedded player to `video_id` at `start`.
///
/// Delegates to [`nbatv_player::load_video_by_id_snippet`] (IFrame Player API
/// `player.loadVideoById`): script text for the webview to evaluate, never
/// stream bytes.
///
/// Uniform transport chrome (egui play/pause/seek/rate controls driving
/// [`EmbedHost::cue`]) is a deferred driver slice: `cue`/`cue_snippet` is
/// the seam it will drive, and this slice ships no transport UI.
pub fn cue_snippet(video_id: &str, start_seconds: u64) -> String {
    nbatv_player::load_video_by_id_snippet(video_id, start_seconds)
}

/// Failure modes for opening/driving the Lane B webview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmbedError {
    /// The platform cannot parent a child webview (e.g. mobile targets,
    /// Wayland without the GTK embedding path).
    UnsupportedPlatform,
    /// The webview failed to build, navigate, or evaluate; carries the
    /// backend message. Also covers sanction-gate refusals on
    /// [`EmbedHost::open`]/[`EmbedHost::navigate`] with a `refused …`
    /// message.
    CreationFailed(String),
    /// The `lane-b` feature is off: this build has no webview, so the shell
    /// shows the OpenEmbed URL plus the external fallback instead.
    FeatureDisabled,
}

impl std::fmt::Display for EmbedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EmbedError::UnsupportedPlatform => {
                write!(f, "child webview unsupported on this platform")
            }
            EmbedError::CreationFailed(msg) => write!(f, "embed webview failed: {msg}"),
            EmbedError::FeatureDisabled => write!(f, "lane-b feature is disabled in this build"),
        }
    }
}

impl std::error::Error for EmbedError {}

/// The Lane B child webview, parented to the shell window.
#[cfg(feature = "lane-b")]
pub struct EmbedHost {
    webview: wry::WebView,
}

#[cfg(feature = "lane-b")]
impl EmbedHost {
    /// Parent a child webview at `bounds` loading `url`, gated by
    /// [`is_sanctioned_embed`].
    ///
    /// `parent` is the live `eframe::Frame` handle (it implements
    /// `HasWindowHandle` on native targets). The webview lives exactly as
    /// long as this host: dropping it destroys the native child. Callers
    /// must keep one host per URL — opening per-frame would stack native
    /// child windows above the egui UI with no z-order control.
    pub fn open(
        parent: &impl raw_window_handle::HasWindowHandle,
        bounds: EmbedBounds,
        url: &str,
    ) -> Result<Self, EmbedError> {
        if !is_sanctioned_embed(url) {
            return Err(EmbedError::CreationFailed(format!(
                "refused non-sanctioned embed URL: {url}"
            )));
        }
        let rect = wry::Rect {
            position: wry::dpi::PhysicalPosition::new(bounds.x, bounds.y).into(),
            size: wry::dpi::PhysicalSize::new(bounds.w, bounds.h).into(),
        };
        let webview = wry::WebViewBuilder::new_as_child(parent)
            .with_url(url)
            .with_bounds(rect)
            .build()
            .map_err(|err| match err {
                wry::Error::UnsupportedWindowHandle => EmbedError::UnsupportedPlatform,
                other => EmbedError::CreationFailed(other.to_string()),
            })?;
        Ok(Self { webview })
    }

    /// Load a new sanctioned embed URL into the existing child webview.
    pub fn navigate(&self, url: &str) -> Result<(), EmbedError> {
        if !is_sanctioned_embed(url) {
            return Err(EmbedError::CreationFailed(format!(
                "refused non-sanctioned embed URL: {url}"
            )));
        }
        self.webview
            .load_url(url)
            .map_err(|err| EmbedError::CreationFailed(err.to_string()))
    }

    /// Drive the embedded player to `video_id` at `start_seconds` via the
    /// IFrame Player API snippet from [`cue_snippet`].
    pub fn cue(&self, video_id: &str, start_seconds: u64) -> Result<(), EmbedError> {
        self.webview
            .evaluate_script(&cue_snippet(video_id, start_seconds))
            .map_err(|err| EmbedError::CreationFailed(err.to_string()))
    }

    /// Reposition the child webview (resize/DPI tracking calls this; the
    /// driver slice owns that wiring).
    pub fn set_bounds(&self, bounds: EmbedBounds) -> Result<(), EmbedError> {
        let rect = wry::Rect {
            position: wry::dpi::PhysicalPosition::new(bounds.x, bounds.y).into(),
            size: wry::dpi::PhysicalSize::new(bounds.w, bounds.h).into(),
        };
        self.webview
            .set_bounds(rect)
            .map_err(|err| EmbedError::CreationFailed(err.to_string()))
    }
}

/// Feature-off stand-in: the type exists so shell code paths name it in both
/// builds, but no webview can exist and [`EmbedHost::open`] always reports
/// [`EmbedError::FeatureDisabled`]. The shell then shows the OpenEmbed URL
/// plus the external fallback — never a blank lie.
#[cfg(not(feature = "lane-b"))]
pub struct EmbedHost {
    _never: std::convert::Infallible,
}

#[cfg(not(feature = "lane-b"))]
impl EmbedHost {
    /// Always `Err(EmbedError::FeatureDisabled)`: `wry` is compiled out.
    pub fn open(
        _parent: &impl raw_window_handle::HasWindowHandle,
        _bounds: EmbedBounds,
        _url: &str,
    ) -> Result<Self, EmbedError> {
        Err(EmbedError::FeatureDisabled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanction_gate_accepts_sanctioned_embeds() {
        // Ladder-normalized YouTube embeds (cf. handover::dispatch_external).
        assert!(is_sanctioned_embed(&nbatv_player::youtube_embed_url(
            "dQw4w9WgXcQ"
        )));
        assert!(is_sanctioned_embed(
            "https://www.youtube.com/embed/dQw4w9WgXcQ?enablejsapi=1&rel=0&start=42"
        ));
        assert!(is_sanctioned_embed(
            "https://youtube-nocookie.com/embed/dQw4w9WgXcQ"
        ));
        // Non-YouTube vendor embeds arrive in OpenEmbed form already.
        assert!(is_sanctioned_embed(
            "https://www.dailymotion.com/embed/video/x8abc12"
        ));
        assert!(is_sanctioned_embed("https://player.vimeo.com/video/12345"));
    }

    #[test]
    fn sanction_gate_refuses_watch_shorts_stream_and_nba_app() {
        // Never a /watch?v= page URL, even with a well-formed video id.
        assert!(!is_sanctioned_embed(
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ"
        ));
        assert!(!is_sanctioned_embed(
            "https://www.youtube.com/shorts/dQw4w9WgXcQ"
        ));
        // youtu.be is a share link, not an embed player (also with a port).
        assert!(!is_sanctioned_embed("https://youtu.be/dQw4w9WgXcQ"));
        assert!(!is_sanctioned_embed("http://youtu.be:443/dQw4w9WgXcQ"));
        // Lookalike YouTube hosts are not the embed player.
        assert!(!is_sanctioned_embed(
            "https://www.youtube.com.evil.com/embed/dQw4w9WgXcQ"
        ));
        assert!(!is_sanctioned_embed(
            "https://evilyoutube.com/embed/dQw4w9WgXcQ"
        ));
        // YouTube embeds require TLS.
        assert!(!is_sanctioned_embed(
            "http://www.youtube.com/embed/dQw4w9WgXcQ"
        ));
        // Never direct stream bytes.
        assert!(!is_sanctioned_embed(
            "https://rr1---sn-xyz.googlevideo.com/videoplayback?expire=1"
        ));
        // NBA App always opens externally, never embedded.
        assert!(!is_sanctioned_embed(
            "https://watch.nba.com/game/0022400001"
        ));
        assert!(!is_sanctioned_embed("https://www.nba.com/watch/game/1"));
        // Non-web schemes and degenerate inputs.
        assert!(!is_sanctioned_embed("javascript:alert(1)"));
        assert!(!is_sanctioned_embed("data:text/html,<h1>x</h1>"));
        assert!(!is_sanctioned_embed("file:///tmp/game.mp4"));
        assert!(!is_sanctioned_embed(""));
        assert!(!is_sanctioned_embed("https://"));
    }

    #[test]
    fn embed_url_for_maps_only_open_embed() {
        let url = nbatv_player::youtube_embed_url("dQw4w9WgXcQ");
        assert_eq!(
            embed_url_for(&PlayDispatch::OpenEmbed { url: url.clone() }),
            Some(url)
        );
        // An OpenEmbed carrying a hostile URL is still gated: sanction
        // is re-checked here, not just trusted from the dispatch.
        assert_eq!(
            embed_url_for(&PlayDispatch::OpenEmbed {
                url: "https://www.youtube.com/watch?v=dQw4w9WgXcQ".to_string(),
            }),
            None
        );
        assert_eq!(
            embed_url_for(&PlayDispatch::OpenExternal {
                url: "https://watch.nba.com/game/1".to_string(),
            }),
            None
        );
        assert_eq!(
            embed_url_for(&PlayDispatch::PlayProgressive {
                src: "https://archive.org/download/x/y.mp4".to_string(),
            }),
            None
        );
        assert_eq!(
            embed_url_for(&PlayDispatch::ShowPointer {
                pointer: "Catalog ref FTE-1 (pointer only)".to_string(),
            }),
            None
        );
        assert_eq!(embed_url_for(&PlayDispatch::Unavailable), None);
    }

    #[test]
    fn bounds_rejects_zero_extent() {
        assert_eq!(EmbedBounds::new(0, 0, 0, 100), None);
        assert_eq!(EmbedBounds::new(0, 0, 100, 0), None);
        assert_eq!(EmbedBounds::new(0, 0, 0, 0), None);
        let bounds = EmbedBounds::new(10, 20, 960, 540).expect("nonzero extent");
        assert_eq!((bounds.x, bounds.y, bounds.w, bounds.h), (10, 20, 960, 540));
    }

    #[test]
    fn cue_snippet_carries_video_id() {
        let snippet = cue_snippet("dQw4w9WgXcQ", 42);
        assert!(snippet.contains("dQw4w9WgXcQ"));
        assert_eq!(
            snippet,
            nbatv_player::load_video_by_id_snippet("dQw4w9WgXcQ", 42)
        );
    }

    #[cfg(test)]
    struct DummyWindow;

    #[cfg(test)]
    impl raw_window_handle::HasWindowHandle for DummyWindow {
        fn window_handle(
            &self,
        ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
            Err(raw_window_handle::HandleError::NotSupported)
        }
    }

    #[cfg(not(feature = "lane-b"))]
    #[test]
    fn open_without_feature_reports_feature_disabled() {
        // wry is compiled out, so even a (dummy) parent handle and a
        // sanctioned URL cannot open: the shell must show the URL plus
        // the external fallback instead.
        let bounds = EmbedBounds::new(0, 0, 960, 540).expect("nonzero extent");
        let err = EmbedHost::open(
            &DummyWindow,
            bounds,
            "https://www.youtube.com/embed/dQw4w9WgXcQ?enablejsapi=1&rel=0",
        )
        .err()
        .expect("feature off: open must fail");
        assert_eq!(err, EmbedError::FeatureDisabled);
    }
}
