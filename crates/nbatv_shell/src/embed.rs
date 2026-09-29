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
//!
//! ## Cookie confinement (ADR-0002)
//!
//! Vendor sign-ins (age gates) complete inside the webview: its cookies
//! and credentials live in the webview profile only. [`EmbedHost::open`]
//! builds a dedicated [`wry::WebContext`] rooted at
//! [`WEBVIEW_PROFILE_DIR`] (`data/webview-profile/`, gitignored), which the
//! Windows/Linux backends honor directly; on macOS/WKWebView the backend
//! ignores the directory and uses the app-scoped OS data store instead.
//! Either way nothing lands in the repo tree or the Cache Tier, and the app
//! itself never sees a credential — the card only notes that a sign-in is
//! wanted (see [`is_sign_in_url`]).
//!
//! ## Card state machine (headless)
//!
//! [`EmbedSession`] is the pure dispatch/card state behind the Game view's
//! embed card: `Pending` (Play recorded an embed) → `Visible` (the child
//! webview hosts it) or `Refused` (framing/platform refused → the session's
//! [`EmbedSession::refused_fallback`] is the existing `OpenExternal`
//! dispatch, rendered as a card rather than an error). The sign-in hint is
//! orthogonal state on the session, set from the live player URL on
//! `lane-b` builds and headlessly in tests.

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

/// Webview profile root: where the Lane B webview keeps cookies, storage,
/// and sign-in state on backends that honor a data directory
/// (Windows/Linux). Gitignored, outside the repo tree's committed paths
/// and outside the Cache Tier. On macOS/WKWebView the backend ignores the
/// directory and uses the app-scoped OS data store — still never the repo.
pub const WEBVIEW_PROFILE_DIR: &str = "data/webview-profile";

/// Where one embed dispatch stands: the pure card state behind the Game
/// view's embed card. Always compiled and headless-testable; the `lane-b`
/// webview only ever realizes the `Visible` branch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LaneBStatus {
    /// Play recorded an embed dispatch; the child webview is opening.
    Pending,
    /// The child webview hosts the vendor player in-window.
    Visible,
    /// Framing (or the platform) refused: the session falls back to
    /// [`EmbedSession::refused_fallback`], the existing `OpenExternal`
    /// dispatch rendered as a card rather than an error.
    Refused,
}

/// One embed dispatch plus its card state: the URL from
/// [`PlayDispatch::OpenEmbed`], its [`LaneBStatus`], and whether the vendor
/// currently wants a sign-in (age gate) completed inside the player.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmbedSession {
    url: String,
    status: LaneBStatus,
    sign_in_hint: bool,
}

impl EmbedSession {
    /// Record a fresh Play outcome: pending, no sign-in hint yet.
    pub fn pending(url: String) -> Self {
        Self {
            url,
            status: LaneBStatus::Pending,
            sign_in_hint: false,
        }
    }

    /// Session for a Play dispatch, if it has an embed URL: `Some(pending)`
    /// for [`PlayDispatch::OpenEmbed`], `None` for every other outcome.
    /// The shell records this on every Play press so the card state
    /// composes with whatever dispatch (cache, ladder) produced it.
    pub fn for_dispatch(dispatch: &PlayDispatch) -> Option<Self> {
        match dispatch {
            PlayDispatch::OpenEmbed { url } => Some(Self::pending(url.clone())),
            _ => None,
        }
    }

    /// The embed URL this session was recorded for.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Current card state.
    pub fn status(&self) -> &LaneBStatus {
        &self.status
    }

    /// Whether the sanction gate would let this URL into a webview.
    /// A non-sanctioned session stays `Pending` forever: the card shows
    /// the gate note and no host ever opens (`mark_visible` enforces it).
    pub fn can_host(&self) -> bool {
        is_sanctioned_embed(&self.url)
    }

    /// The child webview hosts the player now. A non-sanctioned session
    /// refuses the transition (stays `Pending`): only the shell's gated
    /// open path can reach `Visible`.
    pub fn mark_visible(&mut self) {
        if self.can_host() {
            self.status = LaneBStatus::Visible;
        }
    }

    /// Framing (or the platform) refused in-window hosting.
    pub fn mark_refused(&mut self) {
        self.status = LaneBStatus::Refused;
    }

    /// Whether the vendor currently wants a sign-in completed inside the
    /// player. Set from the live player URL on `lane-b` builds (see
    /// [`is_sign_in_url`]), headlessly in tests.
    pub fn set_sign_in_hint(&mut self, needed: bool) {
        self.sign_in_hint = needed;
    }

    /// Whether the embed card should show the session/sign-in hint.
    pub fn sign_in_hint(&self) -> bool {
        self.sign_in_hint
    }

    /// The fallback when [`LaneBStatus::Refused`]: the existing
    /// open-external path for the same URL. `Some` only once refused, so
    /// the shell can only fall back from a genuine refusal.
    pub fn refused_fallback(&self) -> Option<PlayDispatch> {
        match self.status {
            LaneBStatus::Refused => Some(PlayDispatch::OpenExternal {
                url: self.url.clone(),
            }),
            _ => None,
        }
    }

    /// The external-open URL once refused, for the fallback card.
    pub fn fallback_url(&self) -> Option<&str> {
        match self.status {
            LaneBStatus::Refused => Some(&self.url),
            _ => None,
        }
    }
}

/// Whether a player URL is a vendor sign-in surface rather than the tape
/// itself: Google's account sign-in or YouTube's sign-in interstitial,
/// where age-gated players land when they want a login. The shell polls
/// the hosted webview's current URL against this and raises the card's
/// session hint — the sign-in itself still completes inside the webview,
/// with cookies confined to the webview profile.
pub fn is_sign_in_url(url: &str) -> bool {
    let rest = match url.strip_prefix("https://") {
        Some(rest) => rest,
        None => return false,
    };
    let (raw_host, path) = match rest.find('/') {
        Some(idx) => (&rest[..idx], &rest[idx..]),
        None => (rest, "/"),
    };
    let host = raw_host
        .split('@')
        .next_back()
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    if host == "accounts.google.com" || host.ends_with(".accounts.google.com") {
        return true;
    }
    if host == "accounts.youtube.com" || host.ends_with(".accounts.youtube.com") {
        return true;
    }
    if host == "youtube.com"
        || host == "www.youtube.com"
        || host == "m.youtube.com"
        || host == "youtube-nocookie.com"
        || host == "www.youtube-nocookie.com"
    {
        return path == "/signin" || path.starts_with("/signin?");
    }
    false
}

/// Whether the webview still sits on the sanctioned embed location the
/// shell opened: same scheme, host, and path — query-string and fragment
/// churn (token refreshes, player params) do not count. A vendor that
/// bounces the iframe to an interstitial changes the path, which is the
/// redirect-refusal signal the shell polls for. Silent in-frame render
/// refusal keeps this URL unchanged and cannot be detected without the
/// vendor's cooperation.
pub fn same_embed_location(opened: &str, current: &str) -> bool {
    fn strip(url: &str) -> &str {
        let no_query = url.split(['?', '#']).next().unwrap_or(url);
        no_query.trim_end_matches('/')
    }
    strip(opened) == strip(current)
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
///
/// Owns its [`wry::WebContext`] alongside the [`wry::WebView`] so the
/// cookie profile rooted at [`WEBVIEW_PROFILE_DIR`] lives exactly as long
/// as the hosted player: dropping the host destroys both the native child
/// and its claim on the profile. Sign-ins complete inside the webview and
/// the app never sees a credential.
#[cfg(feature = "lane-b")]
pub struct EmbedHost {
    webview: wry::WebView,
    // Kept alive with the webview: `wry` documents that dropping the
    // context can cost the webview backend behavior (custom protocols on
    // macOS), so both drop together here.
    _context: wry::WebContext,
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
    ///
    /// Cookies and sign-in state stay in the dedicated profile rooted at
    /// [`WEBVIEW_PROFILE_DIR`] (honored on Windows/Linux; the app-scoped
    /// OS store on macOS) — never the repo tree or the Cache Tier.
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
        let mut context = wry::WebContext::new(Some(WEBVIEW_PROFILE_DIR.into()));
        let webview = wry::WebViewBuilder::new_as_child(parent)
            .with_url(url)
            .with_bounds(rect)
            .with_web_context(&mut context)
            .build()
            .map_err(|err| match err {
                wry::Error::UnsupportedWindowHandle => EmbedError::UnsupportedPlatform,
                other => EmbedError::CreationFailed(other.to_string()),
            })?;
        Ok(Self {
            webview,
            _context: context,
        })
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

    /// The player URL the webview currently shows. The shell polls this to
    /// raise the card's sign-in hint when the vendor navigates to a login
    /// surface (see [`is_sign_in_url`]) — observation only, never a
    /// credential read.
    pub fn current_url(&self) -> Result<String, EmbedError> {
        self.webview
            .url()
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
    fn session_starts_pending_and_tracks_hosting() {
        let url = nbatv_player::youtube_embed_url("dQw4w9WgXcQ");
        let mut session = EmbedSession::pending(url.clone());
        assert_eq!(session.url(), url);
        assert_eq!(session.status(), &LaneBStatus::Pending);
        assert!(session.can_host());
        assert!(!session.sign_in_hint());
        assert_eq!(session.fallback_url(), None);
        assert_eq!(session.refused_fallback(), None);
        session.mark_visible();
        assert_eq!(session.status(), &LaneBStatus::Visible);
        assert_eq!(session.refused_fallback(), None);
    }

    #[test]
    fn session_refusal_falls_back_to_open_external() {
        let url = nbatv_player::youtube_embed_url("dQw4w9WgXcQ");
        let mut session = EmbedSession::pending(url.clone());
        session.mark_refused();
        assert_eq!(session.status(), &LaneBStatus::Refused);
        assert_eq!(session.fallback_url(), Some(url.as_str()));
        assert_eq!(
            session.refused_fallback(),
            Some(PlayDispatch::OpenExternal { url })
        );
    }

    #[test]
    fn session_for_dispatch_maps_only_open_embed() {
        let url = nbatv_player::youtube_embed_url("dQw4w9WgXcQ");
        let session = EmbedSession::for_dispatch(&PlayDispatch::OpenEmbed { url: url.clone() })
            .expect("OpenEmbed records a session");
        assert_eq!(session.url(), url);
        assert_eq!(session.status(), &LaneBStatus::Pending);
        assert_eq!(
            EmbedSession::for_dispatch(&PlayDispatch::OpenExternal { url }),
            None
        );
        assert_eq!(EmbedSession::for_dispatch(&PlayDispatch::Unavailable), None);
    }

    #[test]
    fn unsanctioned_session_never_hosts() {
        // A dispatch carrying a non-player URL still records a session (so
        // the card renders), but the gate pins it: no host ever opens.
        let mut session =
            EmbedSession::pending("https://www.youtube.com/watch?v=dQw4w9WgXcQ".to_string());
        assert!(!session.can_host());
        session.mark_visible();
        assert!(!session.can_host());
    }

    #[test]
    fn sign_in_hint_flags_only_login_surfaces() {
        assert!(is_sign_in_url(
            "https://accounts.google.com/signin/v2/identifier?service=youtube"
        ));
        assert!(is_sign_in_url(
            "https://sub.accounts.google.com/o/oauth2/auth?client_id=x"
        ));
        assert!(is_sign_in_url(
            "https://www.youtube.com/signin?next=/embed/x"
        ));
        assert!(is_sign_in_url(
            "https://accounts.youtube.com/accounts/SetSID"
        ));
        // The player itself, vendor embeds, and junk never hint.
        assert!(!is_sign_in_url(&nbatv_player::youtube_embed_url(
            "dQw4w9WgXcQ"
        )));
        assert!(!is_sign_in_url(
            "https://www.dailymotion.com/embed/video/x8abc12"
        ));
        assert!(!is_sign_in_url("http://accounts.google.com/signin"));
        assert!(!is_sign_in_url("javascript:alert(1)"));
        assert!(!is_sign_in_url(""));
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
