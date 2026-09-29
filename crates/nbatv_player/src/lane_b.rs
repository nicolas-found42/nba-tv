//! Lane B: sanctioned vendor-player embeds plus External Surface openers.
//!
//! Lane B covers every [`crate::playback::PlaybackClass::ExternalSurface`]
//! tape: YouTube fan archives, the official NBA Classic Games YouTube
//! mirror, and the non-Anglo-rehost cluster (Dailymotion/Vimeo-class).
//! The vendor's own player is hosted in a `wry` `build_as_child` webview
//! bounded to the shell's tape rect; transport controls drive it through
//! the platform's player API (YouTube: IFrame Player API).
//!
//! ## Legality gate — refusal of stream extraction (read before extending)
//!
//! This module provides **no yt-dlp/extraction path and never will**.
//! Feeding a YouTube watch/stream URL (or any extractor-resolved
//! `googlevideo.com` media URL) into ffmpeg is gate-failing as an app
//! feature per research 10 section 3.1: the YouTube Terms of Service permit
//! showing videos only "through the embeddable YouTube player", the API
//! Developer Policies (section III.E.1) prohibit API clients from
//! downloading/caching audiovisual content without prior written approval,
//! and extraction tooling is the subject of active DMCA enforcement (RIAA
//! notice to GitHub, 2020-10-23). The NBA App likewise has no in-window
//! decode lane per research 10 section 3.2: its streams are
//! Widevine-encrypted, so decrypting them for ffmpeg would be circumvention
//! under 17 U.S.C. section 1201(a)(1)(A); NBA App URLs open externally via
//! [`nba_app_opener`]. Do not add helpers that produce, accept, or forward
//! direct stream URLs for these surfaces — that is the gate this refusal
//! holds.

/// Build the sanctioned YouTube IFrame Player API embed URL for `video_id`.
///
/// Emits `https://www.youtube.com/embed/<id>?enablejsapi=1&rel=0`:
/// an `/embed/` player URL with the JS API enabled, never a
/// `/watch?v=` page URL and never a direct stream URL. `video_id` is the
/// YouTube Data API `videoId` (the catalog key), passed through verbatim.
pub fn youtube_embed_url(video_id: &str) -> String {
    std::format!("https://www.youtube.com/embed/{video_id}?enablejsapi=1&rel=0")
}

/// Embed URL variant that also cues a start offset.
///
/// Appends the IFrame API `start` parameter (seconds) so the player opens
/// cued at the right point. Still an `/embed/` URL with `enablejsapi=1`.
pub fn youtube_embed_url_with_start(video_id: &str, start_seconds: u64) -> String {
    std::format!(
        "https://www.youtube.com/embed/{video_id}?enablejsapi=1&rel=0&start={start_seconds}"
    )
}

/// JavaScript snippet driving the embedded player to `video_id`.
///
/// Calls the IFrame Player API `player.loadVideoById({videoId, startSeconds})`
/// (see research 10 section 1.2), which is how the shell's uniform
/// transport controls seek the Lane B player. Returns script text for the
/// `wry` webview to evaluate — not a URL, and never stream bytes.
pub fn load_video_by_id_snippet(video_id: &str, start_seconds: u64) -> String {
    std::format!("player.loadVideoById({{videoId:\"{video_id}\",startSeconds:{start_seconds}}});")
}

/// A vendor surface whose pixels live outside the Player Backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExternalSurface {
    /// An NBA App / watch.nba.com page. Widevine-encrypted: there is no
    /// lawful in-window decode lane, so this always opens externally.
    NbaApp { url: String },
    /// Any other vendor embed page already suitable for the Lane B webview
    /// (e.g. a Dailymotion/Vimeo embed URL supplied by the ladder).
    VendorEmbed { url: String },
}

/// What the shell should do with an [`ExternalSurface`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenAction {
    /// Hand the URL to the OS (vendor app / system browser).
    OpenExternally { url: String },
    /// Load the URL in the Lane B child webview (sanctioned embed pages).
    OpenInWebview { url: String },
}

/// Open an [`ExternalSurface`]: NBA App URLs always go external; vendor
/// embed pages load in the Lane B webview.
pub fn open_external(surface: &ExternalSurface) -> OpenAction {
    match surface {
        ExternalSurface::NbaApp { url } => OpenAction::OpenExternally { url: url.clone() },
        ExternalSurface::VendorEmbed { url } => OpenAction::OpenInWebview { url: url.clone() },
    }
}

/// Convenience opener for NBA App URLs: always
/// [`OpenAction::OpenExternally`]. There is deliberately no embed/webview
/// variant — see the module-level legality gate.
pub fn nba_app_opener(url: &str) -> OpenAction {
    open_external(&ExternalSurface::NbaApp {
        url: url.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embed_url_is_sanctioned_iframe_player() {
        let url = youtube_embed_url("dQw4w9WgXcQ");
        assert!(
            url.contains("/embed/"),
            "must be an /embed/ player URL, got {url}"
        );
        assert!(
            url.contains("enablejsapi=1"),
            "must enable the IFrame JS API, got {url}"
        );
        assert!(url.starts_with("https://www.youtube.com/embed/dQw4w9WgXcQ"));
    }

    #[test]
    fn embed_url_is_never_a_watch_or_stream_url() {
        for url in [
            youtube_embed_url("dQw4w9WgXcQ"),
            youtube_embed_url_with_start("dQw4w9WgXcQ", 90),
        ] {
            assert!(!url.contains("watch?v="), "never a watch URL: {url}");
            assert!(!url.contains("googlevideo"), "never a stream URL: {url}");
            assert!(!url.contains("/v/"), "never a legacy stream path: {url}");
            assert!(url.contains("/embed/"), "always an embed URL: {url}");
        }
    }

    #[test]
    fn embed_with_start_cues_the_offset() {
        let url = youtube_embed_url_with_start("dQw4w9WgXcQ", 90);
        assert!(url.contains("start=90"), "got {url}");
        assert!(url.contains("enablejsapi=1"), "got {url}");
    }

    #[test]
    fn load_video_snippet_names_player_api_params() {
        let js = load_video_by_id_snippet("dQw4w9WgXcQ", 90);
        assert!(js.contains("loadVideoById"), "got {js}");
        assert!(js.contains("dQw4w9WgXcQ"), "got {js}");
        assert!(js.contains("startSeconds"), "got {js}");
        assert!(js.contains('9') && js.contains("90"), "got {js}");
        assert!(!js.contains("googlevideo"), "snippet carries no stream URL");
    }

    #[test]
    fn nba_app_urls_open_externally_never_in_window() {
        let action = nba_app_opener("https://watch.nba.com/game/abc");
        assert_eq!(
            action,
            OpenAction::OpenExternally {
                url: "https://watch.nba.com/game/abc".to_string(),
            }
        );
    }

    #[test]
    fn vendor_embeds_open_in_the_lane_b_webview() {
        let action = open_external(&ExternalSurface::VendorEmbed {
            url: "https://www.youtube.com/embed/dQw4w9WgXcQ?enablejsapi=1&rel=0".to_string(),
        });
        match action {
            OpenAction::OpenInWebview { url } => assert!(url.contains("/embed/")),
            OpenAction::OpenExternally { .. } => panic!("vendor embed must stay in-window"),
        }
    }
}
