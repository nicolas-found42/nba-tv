//! Lane A: ffmpeg-sidecar command builders, progressive-readiness checks,
//! and the frame→texture stage.
//!
//! Lane A is the default lane. A sidecar `ffmpeg` binary decodes
//! progressive-file tape (Cache Tier MP4s, Internet Archive direct MP4s);
//! decoded RGBA frames become `egui` textures. The builders below produce
//! argument vectors only — spawning the process is the shell's job — so
//! they are fully testable with no media, no window, and no download.

/// Arguments that ask ffmpeg to describe `src` without decoding it.
///
/// Shape: `ffmpeg -hide_banner -i <src>`. ffmpeg prints the stream report
/// to stderr and exits; the caller parses formats/codecs from that report.
pub fn probe_args(src: &str) -> Vec<String> {
    vec![
        "-hide_banner".to_string(),
        "-i".to_string(),
        src.to_string(),
    ]
}

/// Arguments that decode `src` to raw RGBA frames on stdout.
///
/// Shape: `ffmpeg -hide_banner -i <src> -f rawvideo -pix_fmt rgba -`.
/// The shell reads the pipe as a frame iterator and syncs video against
/// the wall clock (see research 10 section 1.1). Audio, when wanted, is a
/// second `-f s16le` pipe on the same design.
pub fn play_pipe_args(src: &str) -> Vec<String> {
    vec![
        "-hide_banner".to_string(),
        "-i".to_string(),
        src.to_string(),
        "-f".to_string(),
        "rawvideo".to_string(),
        "-pix_fmt".to_string(),
        "rgba".to_string(),
        "-".to_string(),
    ]
}

/// Arguments that restart playback at `seconds` via a fast input seek.
///
/// `-ss` goes BEFORE `-i` (input seek: ffmpeg seeks on the index without
/// decoding up to the target, O(1) for moov-first MP4s). Placing `-ss`
/// after `-i` would be an accurate-but-slow output seek; the builder never
/// emits that shape.
pub fn seek_play_args(src: &str, seconds: f64) -> Vec<String> {
    vec![
        "-hide_banner".to_string(),
        "-ss".to_string(),
        format!("{seconds}"),
        "-i".to_string(),
        src.to_string(),
        "-f".to_string(),
        "rawvideo".to_string(),
        "-pix_fmt".to_string(),
        "rgba".to_string(),
        "-".to_string(),
    ]
}

/// Cache Tier normalizer arguments (byte-exact contract).
///
/// Runs once per tape at cache-fill time, never at play time. Emits exactly:
///
/// ```text
/// ffmpeg -i <src> -c:v libx264 -crf 20 -preset medium -c:a aac -b:a 160k -movflags +faststart out.mp4
/// ```
///
/// `+faststart` moves the moov atom to the front so the copy streams
/// progressively over `Range` with no HLS. `libx264` stays the
/// compatibility default (every Lane A build decodes H.264); a macOS build
/// may substitute `h264_videotoolbox` for throughput outside this contract.
pub fn normalize_args(src: &str, out: &str) -> Vec<String> {
    vec![
        "-i".to_string(),
        src.to_string(),
        "-c:v".to_string(),
        "libx264".to_string(),
        "-crf".to_string(),
        "20".to_string(),
        "-preset".to_string(),
        "medium".to_string(),
        "-c:a".to_string(),
        "aac".to_string(),
        "-b:a".to_string(),
        "160k".to_string(),
        "-movflags".to_string(),
        "+faststart".to_string(),
        out.to_string(),
    ]
}

/// Whether one MP4 is progressively streamable over HTTP `Range`.
///
/// Both halves are required: the moov index must precede the media
/// (`moov_first`) and the server must honor range requests
/// (`accepts_range`, from the `Accept-Ranges` response header).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mp4RangeReadiness {
    pub moov_first: bool,
    pub accepts_range: bool,
}

impl Mp4RangeReadiness {
    pub fn is_progressive_ready(&self) -> bool {
        self.moov_first && self.accepts_range
    }
}

/// Report whether an `Accept-Ranges` header value offers byte ranges.
///
/// Accepts the header value (e.g. `"bytes"`) or `None` when the header is
/// absent. Matching is ASCII case-insensitive and tolerates surrounding
/// whitespace; `"none"` (or anything without the `bytes` token) is false.
pub fn supports_range(accept_ranges: Option<&str>) -> bool {
    match accept_ranges {
        Some(value) => value
            .split(',')
            .any(|token| token.trim().eq_ignore_ascii_case("bytes")),
        None => false,
    }
}

/// Report whether the moov atom precedes the mdat atom.
///
/// Takes parsed atom offsets (bytes from the file start). `None` on either
/// side means "offset unknown" and is not moov-first: the caller must not
/// treat an unexamined file as stream-ready.
pub fn is_moov_first(moov_offset: Option<u64>, mdat_offset: Option<u64>) -> bool {
    match (moov_offset, mdat_offset) {
        (Some(moov), Some(mdat)) => moov < mdat,
        _ => false,
    }
}

/// One decoded RGBA frame, straight off the sidecar pipe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawFrame {
    pub width: u32,
    pub height: u32,
    /// Row-major RGBA bytes; must hold `width * height * 4` entries.
    pub rgba: Vec<u8>,
}

/// The `egui`-ready image for one frame: same layout contract as
/// `egui::ColorImage::from_rgba_unmultiplied`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextureImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Pure-data frame→texture stage: validates and converts decoded frames
/// without touching a window, GPU context, or `egui`.
///
/// The production shell implements this trait against a real
/// `egui::TextureHandle` (`load_texture` once, `set` per frame); the stub
/// below implements the same contract over plain bytes for tests.
pub trait FrameToTexture {
    /// Convert one frame, or return `None` when the frame is malformed
    /// (wrong byte count, zero extent). Implementations must never panic
    /// on adversarial input.
    fn convert(&mut self, frame: &RawFrame) -> Option<TextureImage>;
}

/// Test/placeholder stage: validates dimensions and clones the bytes.
///
/// Counts accepted frames in [`StubTextureStage::frames_converted`] and
/// remembers the last accepted extent in `last_dims`.
#[derive(Debug, Default)]
pub struct StubTextureStage {
    pub frames_converted: u64,
    pub last_dims: Option<(u32, u32)>,
}

impl FrameToTexture for StubTextureStage {
    fn convert(&mut self, frame: &RawFrame) -> Option<TextureImage> {
        if frame.width == 0 || frame.height == 0 {
            return None;
        }
        let expected = (frame.width as usize)
            .checked_mul(frame.height as usize)?
            .checked_mul(4)?;
        if frame.rgba.len() != expected {
            return None;
        }
        self.frames_converted += 1;
        self.last_dims = Some((frame.width, frame.height));
        Some(TextureImage {
            width: frame.width,
            height: frame.height,
            rgba: frame.rgba.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_args_are_byte_exact() {
        let args = normalize_args("in.mkv", "out.mp4");
        let rendered = std::format!("ffmpeg {}", args.join(" "));
        assert_eq!(
            rendered,
            "ffmpeg -i in.mkv -c:v libx264 -crf 20 -preset medium -c:a aac -b:a 160k -movflags +faststart out.mp4"
        );
        assert!(args.iter().any(|a| a == "+faststart"));
    }

    #[test]
    fn seek_places_ss_before_input() {
        let args = seek_play_args("tape.mp4", 75.5);
        let ss = args.iter().position(|a| a == "-ss").expect("-ss present");
        let input = args.iter().position(|a| a == "-i").expect("-i present");
        assert!(
            ss + 2 == input,
            "-ss <t> must sit directly before -i, got {args:?}"
        );
        assert_eq!(args[ss + 1], "75.5");
        assert!(args.contains(&"tape.mp4".to_string()));
    }

    #[test]
    fn seek_zero_is_still_an_input_seek() {
        let args = seek_play_args("tape.mp4", 0.0);
        let ss = args.iter().position(|a| a == "-ss").expect("-ss present");
        let input = args.iter().position(|a| a == "-i").expect("-i present");
        assert!(ss < input);
    }

    #[test]
    fn play_pipe_streams_raw_rgba_to_stdout() {
        let args = play_pipe_args("tape.mp4");
        assert_eq!(args.last().map(String::as_str), Some("-"));
        assert!(args.windows(2).any(|w| w == ["-pix_fmt", "rgba"]));
        assert!(!args.iter().any(|a| a == "-ss"));
    }

    #[test]
    fn readiness_needs_moov_first_and_range() {
        assert!(Mp4RangeReadiness {
            moov_first: true,
            accepts_range: true,
        }
        .is_progressive_ready());
        assert!(!Mp4RangeReadiness {
            moov_first: true,
            accepts_range: false,
        }
        .is_progressive_ready());
        assert!(!Mp4RangeReadiness {
            moov_first: false,
            accepts_range: true,
        }
        .is_progressive_ready());
    }

    #[test]
    fn range_header_parsing() {
        assert!(supports_range(Some("bytes")));
        assert!(supports_range(Some("Bytes")));
        assert!(!supports_range(Some("none")));
        assert!(!supports_range(None));
    }

    #[test]
    fn moov_offset_ordering() {
        assert!(is_moov_first(Some(32), Some(1024)));
        assert!(!is_moov_first(Some(4096), Some(1024)));
        assert!(!is_moov_first(None, Some(1024)));
        assert!(!is_moov_first(Some(32), None));
    }

    #[test]
    fn stub_stage_accepts_a_well_formed_frame() {
        let mut stage = StubTextureStage::default();
        let frame = RawFrame {
            width: 2,
            height: 1,
            rgba: vec![255, 0, 0, 255, 0, 0, 255, 255],
        };
        let image = stage.convert(&frame).expect("valid frame converts");
        assert_eq!(image.width, 2);
        assert_eq!(image.height, 1);
        assert_eq!(image.rgba, frame.rgba);
        assert_eq!(stage.frames_converted, 1);
        assert_eq!(stage.last_dims, Some((2, 1)));
    }

    #[test]
    fn stub_stage_rejects_malformed_frames_without_panicking() {
        let mut stage = StubTextureStage::default();
        assert!(stage
            .convert(&RawFrame {
                width: 2,
                height: 1,
                rgba: vec![0; 7],
            })
            .is_none());
        assert!(stage
            .convert(&RawFrame {
                width: 0,
                height: 1,
                rgba: vec![],
            })
            .is_none());
        assert_eq!(stage.frames_converted, 0);
    }

    /// Presence probe only: passes whether or not ffmpeg is installed.
    /// Never downloads media, never decodes; just reports `ffmpeg -version`.
    #[test]
    fn ffmpeg_version_probe_skips_gracefully_when_absent() {
        let probe = std::process::Command::new("ffmpeg")
            .arg("-version")
            .output();
        match probe {
            Ok(output) => {
                assert!(
                    output.status.success(),
                    "ffmpeg -version should exit 0 when ffmpeg exists"
                );
            }
            Err(_) => {
                // ffmpeg not installed in this environment: graceful skip.
            }
        }
    }
}
