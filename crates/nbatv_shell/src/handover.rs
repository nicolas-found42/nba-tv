//! Shell → Player Backend handover: the pure resolve-dispatch path behind
//! the Game view's Play button.
//!
//! The button never plays anything itself. It calls [`dispatch_for`], which
//! feeds the game's Cache Tier entry plus its [`TapeSource`] rows through the
//! real [`nbatv_player::resolve`] and maps the outcome to a [`PlayDispatch`]
//! the (later) effect layer can act on. Box Score is never consulted: this is
//! a tape-only signal.
//!
//! This module is pure: no `egui`/`eframe` import, so every path stays
//! headless-testable.

use crate::model::{playback_class_for_rank, PlaybackClass, TapeSource};

/// A Cache Tier entry as the shell sees it: the normalized MP4 location
/// (local path or progressive URL) held for one game.
///
/// The shell owns this shape locally; [`dispatch_for`] converts it
/// field-for-field into the player's contract-identical copy. `None` means
/// "no Cache Tier lookup yet" — the cache is an accelerator, never a
/// dependency.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CacheEntry {
    pub game_id: String,
    pub location: String,
}

/// What pressing Play resolved to. A pure outcome: recording it spawns
/// nothing, opens nothing, and touches no network. The Lane B webview
/// hosts the OpenEmbed outcome (`lane-b` feature builds); the Lane A
/// pump spawn is the remaining later slice.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlayDispatch {
    /// Play a progressive file in Lane A (Cache Tier location or a
    /// rank-1/rank-4 ladder URL).
    PlayProgressive { src: String },
    /// Load a sanctioned vendor embed in the Lane B webview.
    OpenEmbed { url: String },
    /// Hand a URL to the OS (NBA App / watch.nba.com pages).
    OpenExternal { url: String },
    /// No playable bytes: show the pointer view naming where tape lives.
    ShowPointer { pointer: String },
    /// Cache missed and the ladder is consumed: honestly unavailable.
    Unavailable,
}

/// Resolve what pressing Play means for `game_id`.
///
/// Order comes from [`nbatv_player::resolve`]: the cache entry (when it names
/// this game) first, then the lowest-`rank` source row naming this game
/// (rank ties keep the caller's order), then [`PlayDispatch::Unavailable`].
/// Rows naming other games are ignored. The winning tape row maps through
/// [`playback_class_for_rank`]:
///
/// - `ProgressiveFile` (rungs 1+4) → [`PlayDispatch::PlayProgressive`].
/// - `ExternalSurface` (rungs 0+2+3) → URL convention (see
///   [`dispatch_external`]).
/// - `Pointer` (rungs 5+6+7) or an unknown rung → [`PlayDispatch::ShowPointer`].
pub fn dispatch_for(
    game_id: &str,
    cache: Option<CacheEntry>,
    sources: &[TapeSource],
) -> PlayDispatch {
    let player_cache = cache.map(|entry| nbatv_player::CacheEntry {
        game_id: entry.game_id,
        location: entry.location,
    });
    let player_sources: Vec<nbatv_player::TapeSource> = sources
        .iter()
        .map(|source| nbatv_player::TapeSource {
            game_id: source.game_id.clone(),
            rank: source.rank,
            source_class: source.source_class.clone(),
            url_or_pointer: source.url_or_pointer.clone(),
            match_confidence: source.match_confidence,
            verified_at: source.verified_at.clone(),
        })
        .collect();
    match nbatv_player::resolve(game_id, player_cache, &player_sources) {
        nbatv_player::Resolved::CacheTier { location, .. } => {
            PlayDispatch::PlayProgressive { src: location }
        }
        nbatv_player::Resolved::Tape(source) => dispatch_tape(&source),
        nbatv_player::Resolved::Unavailable { .. } => PlayDispatch::Unavailable,
    }
}

/// Map one winning ladder row to its dispatch via its ladder rung.
fn dispatch_tape(source: &nbatv_player::TapeSource) -> PlayDispatch {
    match playback_class_for_rank(source.rank) {
        Some(PlaybackClass::ProgressiveFile) => PlayDispatch::PlayProgressive {
            src: source.url_or_pointer.clone(),
        },
        Some(PlaybackClass::ExternalSurface) => dispatch_external(&source.url_or_pointer),
        Some(PlaybackClass::Pointer) | None => PlayDispatch::ShowPointer {
            pointer: source.url_or_pointer.clone(),
        },
    }
}

/// URL convention for [`PlaybackClass::ExternalSurface`] tapes:
///
/// - Contains `nba.com` → [`PlayDispatch::OpenExternal`] as-is. NBA App /
///   watch.nba.com pages are Widevine-encrypted with no lawful in-window
///   decode lane, so they always open externally (never embedded, never
///   fed to ffmpeg).
/// - A YouTube watch / shorts / youtu.be / embed URL → extract the 11-char
///   video id and [`PlayDispatch::OpenEmbed`] the sanctioned
///   [`nbatv_player::youtube_embed_url`]. Never the `/watch` page URL and
///   never a direct stream URL (stream extraction is gate-failing).
/// - Anything else (e.g. a Dailymotion/Vimeo-class embed URL supplied by the
///   ladder) → [`PlayDispatch::OpenEmbed`] as-is for the Lane B webview.
fn dispatch_external(url: &str) -> PlayDispatch {
    if url.contains("nba.com") {
        PlayDispatch::OpenExternal {
            url: url.to_string(),
        }
    } else if let Some(video_id) = youtube_video_id(url) {
        PlayDispatch::OpenEmbed {
            url: nbatv_player::youtube_embed_url(&video_id),
        }
    } else {
        PlayDispatch::OpenEmbed {
            url: url.to_string(),
        }
    }
}

/// Extract the 11-char YouTube video id (`[A-Za-z0-9_-]` × 11) from a watch
/// (`?v=`/`&v=`), shorts (`/shorts/`), short-link (`youtu.be/`), or embed
/// (`/embed/`) URL. Returns `None` when the URL is not a YouTube URL or
/// carries no well-formed id (e.g. a fixture placeholder longer than 11
/// id-chars, which then embeds as-is via [`dispatch_external`]).
fn youtube_video_id(url: &str) -> Option<String> {
    if !url.contains("youtube.com") && !url.contains("youtu.be") {
        return None;
    }
    for marker in ["v=", "/shorts/", "youtu.be/", "/embed/"] {
        let Some(pos) = url.find(marker) else {
            continue;
        };
        let tail = &url[pos + marker.len()..];
        let id: String = tail
            .chars()
            .take_while(|c| is_youtube_id_char(*c))
            .take(11)
            .collect();
        if id.len() == 11 {
            // Reject an 11-char prefix of a longer id-char run so fixture
            // placeholders (e.g. `?v=fixture-sweep`) do not masquerade as
            // real video ids.
            let boundary_ok = tail
                .chars()
                .nth(11)
                .is_none_or(|next| !is_youtube_id_char(next));
            if boundary_ok {
                return Some(id);
            }
        }
    }
    None
}
fn is_youtube_id_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-' || c == '_'
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(game_id: &str, rank: u8, url: &str) -> TapeSource {
        TapeSource {
            game_id: game_id.to_string(),
            rank,
            source_class: "test".to_string(),
            url_or_pointer: url.to_string(),
            match_confidence: 1.0,
            verified_at: "2026-09-07".to_string(),
        }
    }

    #[test]
    fn cache_hit_dispatches_cache_location() {
        let cache = Some(CacheEntry {
            game_id: "194611010TRH".to_string(),
            location: "/cache/194611010TRH.mp4".to_string(),
        });
        let sources = vec![source(
            "194611010TRH",
            1,
            "https://archive.org/details/194611010TRH",
        )];
        assert_eq!(
            dispatch_for("194611010TRH", cache, &sources),
            PlayDispatch::PlayProgressive {
                src: "/cache/194611010TRH.mp4".to_string(),
            }
        );
    }

    #[test]
    fn rank_1_progressive_source_plays_directly() {
        let sources = vec![source(
            "194611010TRH",
            1,
            "https://archive.org/details/194611010TRH",
        )];
        assert_eq!(
            dispatch_for("194611010TRH", None, &sources),
            PlayDispatch::PlayProgressive {
                src: "https://archive.org/details/194611010TRH".to_string(),
            }
        );
    }

    #[test]
    fn rank_2_vendor_embed_opens_in_webview_as_is() {
        let sources = vec![source(
            "194612070BOS",
            2,
            "https://www.dailymotion.com/embed/video/x8abc12",
        )];
        assert_eq!(
            dispatch_for("194612070BOS", None, &sources),
            PlayDispatch::OpenEmbed {
                url: "https://www.dailymotion.com/embed/video/x8abc12".to_string(),
            }
        );
    }

    #[test]
    fn youtube_watch_url_embeds_sanctioned_player() {
        let sources = vec![source(
            "194612070BOS",
            2,
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
        )];
        let dispatch = dispatch_for("194612070BOS", None, &sources);
        match dispatch {
            PlayDispatch::OpenEmbed { url } => {
                assert!(
                    url.contains("dQw4w9WgXcQ"),
                    "embed URL must carry the video id, got {url}"
                );
                assert!(url.contains("/embed/"), "must be an embed URL, got {url}");
                assert!(
                    !url.contains("/watch"),
                    "never the watch page URL, got {url}"
                );
            }
            other => panic!("expected OpenEmbed, got {other:?}"),
        }
    }

    #[test]
    fn youtube_shorts_and_short_links_embed() {
        for url in [
            "https://www.youtube.com/shorts/dQw4w9WgXcQ",
            "https://youtu.be/dQw4w9WgXcQ",
        ] {
            let sources = vec![source("194612070BOS", 3, url)];
            match dispatch_for("194612070BOS", None, &sources) {
                PlayDispatch::OpenEmbed { url } => {
                    assert!(url.contains("dQw4w9WgXcQ"), "got {url}");
                    assert!(url.contains("/embed/"), "got {url}");
                }
                other => panic!("expected OpenEmbed for {url}, got {other:?}"),
            }
        }
    }

    #[test]
    fn nba_com_url_opens_externally_as_is() {
        let url = "https://www.nba.com/watch/video/194611010TRH-classic";
        let sources = vec![source("194611010TRH", 0, url)];
        assert_eq!(
            dispatch_for("194611010TRH", None, &sources),
            PlayDispatch::OpenExternal {
                url: url.to_string(),
            }
        );
    }

    #[test]
    fn rank_6_pointer_shows_pointer_view() {
        let pointer = "Catalog ref FTE-194711150BOS (pointer only)";
        let sources = vec![source("194711150BOS", 6, pointer)];
        assert_eq!(
            dispatch_for("194711150BOS", None, &sources),
            PlayDispatch::ShowPointer {
                pointer: pointer.to_string(),
            }
        );
    }

    #[test]
    fn unknown_rank_shows_pointer_view() {
        let sources = vec![source("194611010TRH", 9, "mystery rung pointer")];
        assert_eq!(
            dispatch_for("194611010TRH", None, &sources),
            PlayDispatch::ShowPointer {
                pointer: "mystery rung pointer".to_string(),
            }
        );
    }

    #[test]
    fn empty_sources_are_honestly_unavailable() {
        assert_eq!(
            dispatch_for("194704160BOS", None, &[]),
            PlayDispatch::Unavailable
        );
    }

    #[test]
    fn foreign_game_rows_are_ignored() {
        let sources = vec![source(
            "OTHER00000AAA",
            1,
            "https://archive.org/details/OTHER00000AAA",
        )];
        assert_eq!(
            dispatch_for("194611010TRH", None, &sources),
            PlayDispatch::Unavailable
        );
    }

    #[test]
    fn rank_ties_keep_caller_order() {
        let sources = vec![
            source("194611010TRH", 1, "https://archive.org/details/first"),
            source("194611010TRH", 1, "https://archive.org/details/second"),
        ];
        assert_eq!(
            dispatch_for("194611010TRH", None, &sources),
            PlayDispatch::PlayProgressive {
                src: "https://archive.org/details/first".to_string(),
            }
        );
    }

    #[test]
    fn lowest_rank_wins_regardless_of_order() {
        let sources = vec![
            source("194611010TRH", 2, "https://example.com/rehost"),
            source(
                "194611010TRH",
                1,
                "https://archive.org/details/194611010TRH",
            ),
        ];
        assert_eq!(
            dispatch_for("194611010TRH", None, &sources),
            PlayDispatch::PlayProgressive {
                src: "https://archive.org/details/194611010TRH".to_string(),
            }
        );
    }
}
