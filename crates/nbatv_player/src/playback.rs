//! [`PlaybackClass`] and the Lane A / Lane B mapping.
//!
//! Playback classes come from the Source Ladder integration contract: rungs
//! 1+4 (direct progressive files) are `ProgressiveFile`, rungs 0+2+3
//! (vendor-player surfaces) are `ExternalSurface`, and rungs 5+6+7
//! (catalog/purchase/institutional pointers) are `Pointer`. This crate owns
//! the enum shape locally; integration dedups onto `nbatv_db` later.

/// How one game tape can be played.
///
/// - `ProgressiveFile`: bytes are directly decodable (Cache Tier file,
///   Internet Archive direct MP4, any progressive HTTP file). Lane A.
/// - `ExternalSurface`: bytes live behind a vendor player (YouTube,
///   Dailymotion/Vimeo-class rehost cluster). Lane B pulls the sanctioned
///   player in-window via a webview; the NBA App variant opens externally.
/// - `Pointer`: no playable bytes exist (collector catalogs, purchase-only,
///   institutional pointers). No playback; the shell shows a pointer view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlaybackClass {
    ProgressiveFile,
    ExternalSurface,
    Pointer,
}

/// Player Backend lane that serves a [`PlaybackClass`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Lane {
    /// ffmpeg-sidecar decode into `egui` textures (default lane).
    A,
    /// Sanctioned vendor-player embed in a `wry` child webview.
    B,
}

/// Map a [`PlaybackClass`] to its [`Lane`].
///
/// - `ProgressiveFile` → `Some(Lane::A)`
/// - `ExternalSurface` → `Some(Lane::B)`
/// - `Pointer` → `None` (no playback; the shell renders a pointer view)
///   instead.
pub fn lane_for(class: PlaybackClass) -> Option<Lane> {
    match class {
        PlaybackClass::ProgressiveFile => Some(Lane::A),
        PlaybackClass::ExternalSurface => Some(Lane::B),
        PlaybackClass::Pointer => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progressive_file_maps_to_lane_a() {
        assert_eq!(lane_for(PlaybackClass::ProgressiveFile), Some(Lane::A));
    }

    #[test]
    fn external_surface_maps_to_lane_b() {
        assert_eq!(lane_for(PlaybackClass::ExternalSurface), Some(Lane::B));
    }

    #[test]
    fn pointer_maps_to_no_playback() {
        assert_eq!(lane_for(PlaybackClass::Pointer), None);
    }

    #[test]
    fn all_classes_have_a_defined_mapping() {
        let classes = [
            PlaybackClass::ProgressiveFile,
            PlaybackClass::ExternalSurface,
            PlaybackClass::Pointer,
        ];
        let lanes: Vec<Option<Lane>> = classes.iter().map(|c| lane_for(*c)).collect();
        assert_eq!(lanes, vec![Some(Lane::A), Some(Lane::B), None]);
    }
}
