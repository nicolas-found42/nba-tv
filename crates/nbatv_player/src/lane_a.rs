//! Lane A: ffmpeg-sidecar command builders, progressive-readiness checks,
//! and the frame→texture stage.
//!
//! Lane A is the default lane. A sidecar `ffmpeg` binary decodes
//! progressive-file tape (Cache Tier MP4s, Internet Archive direct MP4s);
//! decoded RGBA frames become `egui` textures. The builders below produce
//! argument vectors only — spawning the process is the shell's job — so
//! they are fully testable with no media, no window, and no download.
//!
//! ## Why raw `std::process::Command`, not the `ffmpeg-sidecar` crate
//!
//! Issues #5 and #10 name the `ffmpeg-sidecar` crate as the binding. The
//! code deliberately does not depend on it: [ADR 0003] keeps the
//! dependency surface small (`nbatv_player` is a dependency-free leaf per
//! ADR 0001) and treats `ffmpeg` as a user-provisioned process that
//! workspace code spawns and never links. The pipe protocol here is three
//! argument builders plus a `read_exact` loop ([`crate::pump`]), which
//! is less code than the crate's event model. Revisit only if PTS control
//! or progress parsing is needed; that would need its own ADR.
//!
//! [ADR 0003]: https://github.com/nicolas-found42/nba-tv/blob/main/docs/adr/0003-c-dependencies-and-sidecars.md
//!
//! ## Pacing and audio
//!
//! The pipe is untimed, so [`crate::paced`] releases frames at the source
//! frame rate ([`parse_probe_report`] reads it from [`probe_args`] output)
//! against a [`crate::clock::MediaClock`]. Audio is a second, independent
//! sidecar built by [`audio_pipe_args`] and played through the optional
//! `audio` cargo feature; see [`crate::clock`] for the master-clock rule.

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
/// the wall clock (see research 10 section 1.1; [`crate::paced`]). Audio,
/// when wanted, is a separate ffmpeg process: [`audio_pipe_args`].
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

/// Arguments that decode only the audio of `src` to raw interleaved
/// little-endian `f32` PCM on stdout, starting at `start_seconds`.
///
/// Shape: `ffmpeg -hide_banner -ss <t> -i <src> -vn -f f32le -acodec
/// pcm_f32le -ar <rate> -ac <channels> -`. The sidecar resamples and
/// remixes to the output device's `sample_rate`/`channels`, so the reader
/// needs no resampler. `-ss` is an input seek, like [`seek_play_args`],
/// so a seeked audio respawn lands on the same offset as the video one.
pub fn audio_pipe_args(
    src: &str,
    start_seconds: f64,
    sample_rate: u32,
    channels: u16,
) -> Vec<String> {
    vec![
        "-hide_banner".to_string(),
        "-ss".to_string(),
        format!("{start_seconds}"),
        "-i".to_string(),
        src.to_string(),
        "-vn".to_string(),
        "-f".to_string(),
        "f32le".to_string(),
        "-acodec".to_string(),
        "pcm_f32le".to_string(),
        "-ar".to_string(),
        sample_rate.to_string(),
        "-ac".to_string(),
        channels.to_string(),
        "-".to_string(),
    ]
}

/// What the ffmpeg stream report says about a tape.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ProbeReport {
    /// Video frame rate, when the report names one.
    pub fps: Option<f64>,
    /// Whether the tape has at least one audio stream.
    pub has_audio: bool,
}

/// Parse the stderr report of `ffmpeg -hide_banner -i <src>` (see
/// [`probe_args`]).
///
/// Reads the first `Video:` stream line for `<n> fps` (falling back to
/// `<n> tbr`) and checks for any `Audio:` stream. Unknown or garbled
/// reports yield the default (no fps, no audio), never a panic.
pub fn parse_probe_report(report: &str) -> ProbeReport {
    let mut out = ProbeReport::default();
    for line in report.lines() {
        let line = line.trim();
        if !line.starts_with("Stream #") {
            continue;
        }
        if line.contains(": Audio:") {
            out.has_audio = true;
        } else if line.contains(": Video:") && out.fps.is_none() {
            out.fps = rate_before(line, " fps").or_else(|| rate_before(line, " tbr"));
        }
    }
    out
}

/// The positive finite number written right before `unit` in `line`
/// (e.g. `"29.97"` in `"..., 29.97 fps, ..."`); `k` suffix means x1000.
fn rate_before(line: &str, unit: &str) -> Option<f64> {
    let end = line.find(unit)?;
    let head = &line[..end];
    let start = head
        .rfind(|c: char| !(c.is_ascii_digit() || c == '.' || c == 'k'))
        .map_or(0, |i| i + 1);
    let token = &head[start..];
    let (digits, scale) = match token.strip_suffix('k') {
        Some(d) => (d, 1000.0),
        None => (token, 1.0),
    };
    let value = digits.parse::<f64>().ok()? * scale;
    (value.is_finite() && value > 0.0).then_some(value)
}

/// Normalized Lane A decode extent, in pixels.
///
/// The rawvideo pipe carries no header: the byte count of every frame is
/// `width * height * 4`, so the caller-declared extent must match the bytes
/// on the wire exactly. Every game's native tape size differs, so Lane A
/// decodes through `-vf scale=<W>:<H>` (see [`play_scaled_args`]) and every
/// consumer decodes at exactly this extent. 640x360 (16:9) keeps one frame
/// (~900 KiB RGBA) cheap while staying recognizable on an archive browse
/// screen.
pub const LANE_A_WIDTH: u32 = 640;
pub const LANE_A_HEIGHT: u32 = 360;

/// Arguments that decode `src` to raw RGBA frames at a fixed extent.
///
/// Shape: `ffmpeg -hide_banner -i <src> -vf scale=<W>:<H> -f rawvideo
/// -pix_fmt rgba -`. The scale filter forces the output extent so the
/// reader can size frames as `width * height * 4` for any input size.
/// Prefer this over [`play_pipe_args`] whenever the input dimensions are
/// not known up front (i.e. every ladder tape); the unscaled builder stays
/// for callers that already know the extent (tests, Cache Tier copies with
/// recorded dimensions).
pub fn play_scaled_args(src: &str, width: u32, height: u32) -> Vec<String> {
    vec![
        "-hide_banner".to_string(),
        "-i".to_string(),
        src.to_string(),
        "-vf".to_string(),
        format!("scale={width}:{height}"),
        "-f".to_string(),
        "rawvideo".to_string(),
        "-pix_fmt".to_string(),
        "rgba".to_string(),
        "-".to_string(),
    ]
}

/// Arguments that restart scaled playback at `seconds` via a fast input seek.
///
/// Same input-seek contract as [`seek_play_args`] (`-ss` before `-i`) plus
/// the [`play_scaled_args`] extent filter, so a seeked respawn emits the
/// same frame size as the original spawn.
pub fn seek_scaled_args(src: &str, seconds: f64, width: u32, height: u32) -> Vec<String> {
    vec![
        "-hide_banner".to_string(),
        "-ss".to_string(),
        format!("{seconds}"),
        "-i".to_string(),
        src.to_string(),
        "-vf".to_string(),
        format!("scale={width}:{height}"),
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

/// Scan the top-level MP4 atoms of `bytes` for `moov`/`mdat` offsets.
///
/// Returns `(moov_offset, mdat_offset)`, each `None` when absent. Feed the
/// pair to [`is_moov_first`]. Handles 32-bit sizes, `size == 1` 64-bit
/// largesize, and `size == 0` (extends to EOF); a truncated or corrupt
/// atom header ends the scan instead of panicking.
pub fn mp4_top_level_offsets(bytes: &[u8]) -> (Option<u64>, Option<u64>) {
    let mut moov = None;
    let mut mdat = None;
    let mut pos = 0usize;
    while pos + 8 <= bytes.len() {
        let size32 =
            u32::from_be_bytes([bytes[pos], bytes[pos + 1], bytes[pos + 2], bytes[pos + 3]]) as u64;
        let tag = &bytes[pos + 4..pos + 8];
        let (header, size) = if size32 == 1 {
            if pos + 16 > bytes.len() {
                break;
            }
            let large = u64::from_be_bytes([
                bytes[pos + 8],
                bytes[pos + 9],
                bytes[pos + 10],
                bytes[pos + 11],
                bytes[pos + 12],
                bytes[pos + 13],
                bytes[pos + 14],
                bytes[pos + 15],
            ]);
            (16u64, large)
        } else {
            (8u64, size32)
        };
        if size == 0 {
            // Extends to EOF: record a trailing mdat tag, then stop.
            if tag == b"mdat" && mdat.is_none() {
                mdat = Some(pos as u64);
            }
            break;
        }
        if size < header || pos as u64 + size > bytes.len() as u64 {
            break;
        }
        if tag == b"moov" && moov.is_none() {
            moov = Some(pos as u64);
        } else if tag == b"mdat" && mdat.is_none() {
            mdat = Some(pos as u64);
        }
        if moov.is_some() && mdat.is_some() {
            break;
        }
        pos += size as usize;
    }
    (moov, mdat)
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

/// Shared pure-data validation behind both stages: reject zero extents and
/// byte-count mismatches without panicking, clone the bytes on success.
fn convert_validated(frame: &RawFrame) -> Option<TextureImage> {
    if frame.width == 0 || frame.height == 0 {
        return None;
    }
    let expected = (frame.width as usize)
        .checked_mul(frame.height as usize)?
        .checked_mul(4)?;
    if frame.rgba.len() != expected {
        return None;
    }
    Some(TextureImage {
        width: frame.width,
        height: frame.height,
        rgba: frame.rgba.clone(),
    })
}

impl FrameToTexture for StubTextureStage {
    fn convert(&mut self, frame: &RawFrame) -> Option<TextureImage> {
        let image = convert_validated(frame)?;
        self.frames_converted += 1;
        self.last_dims = Some((frame.width, frame.height));
        Some(image)
    }
}

/// Production frame→texture stage: the type the shell wires to its real
/// `egui::TextureHandle`.
///
/// Same pure-data contract as [`StubTextureStage`] — validate, clone, count
/// in `frames_converted`, remember `last_dims` — but this is the
/// shell-facing name: in T4 the shell wraps it with `load_texture` once and
/// `set` per frame. It stays window/GPU-free here so it remains
/// unit-testable with no display. [`StubTextureStage`] is kept for tests.
#[derive(Debug, Default)]
pub struct EguiTextureStage {
    pub frames_converted: u64,
    pub last_dims: Option<(u32, u32)>,
}

impl FrameToTexture for EguiTextureStage {
    fn convert(&mut self, frame: &RawFrame) -> Option<TextureImage> {
        let image = convert_validated(frame)?;
        self.frames_converted += 1;
        self.last_dims = Some((frame.width, frame.height));
        Some(image)
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
    fn audio_pipe_is_video_free_f32le_at_the_requested_format() {
        let args = audio_pipe_args("tape.mp4", 12.5, 48_000, 2);
        let rendered = args.join(" ");
        assert_eq!(
            rendered,
            "-hide_banner -ss 12.5 -i tape.mp4 -vn -f f32le -acodec pcm_f32le -ar 48000 -ac 2 -"
        );
        let ss = args.iter().position(|a| a == "-ss").expect("-ss");
        let input = args.iter().position(|a| a == "-i").expect("-i");
        assert_eq!(ss + 2, input, "input seek shape must hold");
    }

    #[test]
    fn probe_report_reads_fps_and_audio() {
        let report = "Input #0, mov,mp4, from 't.mp4':\n  Duration: 00:00:01.00, start: 0.000000, bitrate: 98 kb/s\n  Stream #0:0[0x1](und): Video: h264 (High 4:4:4 Predictive) (avc1 / 0x31637661), yuv444p(progressive), 32x32 [SAR 1:1 DAR 1:1], 12 kb/s, 5 fps, 5 tbr, 10240 tbn (default)\n  Stream #0:1[0x2](und): Audio: aac (LC) (mp4a / 0x6134706D), 44100 Hz, mono, fltp, 70 kb/s (default)\n";
        let probe = parse_probe_report(report);
        assert_eq!(probe.fps, Some(5.0));
        assert!(probe.has_audio);
    }

    #[test]
    fn probe_report_handles_fractional_video_only_and_tbr_fallback() {
        let ntsc = "  Stream #0:0: Video: h264, yuv420p, 640x360, 29.97 fps, 29.97 tbr, 90k tbn";
        assert_eq!(parse_probe_report(ntsc).fps, Some(29.97));
        assert!(!parse_probe_report(ntsc).has_audio);

        let tbr_only = "  Stream #0:0: Video: mpeg4, yuv420p, 320x240, 25 tbr, 25 tbn";
        assert_eq!(parse_probe_report(tbr_only).fps, Some(25.0));

        let kilo = "  Stream #0:0: Video: h264, yuv420p, 640x360, 1k tbr, 1k tbn";
        assert_eq!(parse_probe_report(kilo).fps, Some(1000.0));
    }

    #[test]
    fn probe_report_ignores_garbage() {
        for junk in [
            "",
            "no streams here",
            "Stream #0:0: Video: h264, 0 fps",
            "Stream #0:0: Video: x, nan fps",
        ] {
            let probe = parse_probe_report(junk);
            assert_eq!(probe.fps, None, "junk {junk:?}");
            assert!(!probe.has_audio);
        }
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

    #[test]
    fn production_stage_accepts_a_well_formed_frame() {
        let mut stage = EguiTextureStage::default();
        let frame = RawFrame {
            width: 3,
            height: 2,
            rgba: vec![9; 3 * 2 * 4],
        };
        let image = stage.convert(&frame).expect("valid frame converts");
        assert_eq!(image.width, 3);
        assert_eq!(image.height, 2);
        assert_eq!(image.rgba, frame.rgba);
        assert_eq!(stage.frames_converted, 1);
        assert_eq!(stage.last_dims, Some((3, 2)));
    }

    #[test]
    fn production_stage_rejects_extent_mismatch_without_panicking() {
        let mut stage = EguiTextureStage::default();
        // Declared 32x32 (4096 bytes) but handed a truncated buffer.
        assert!(stage
            .convert(&RawFrame {
                width: 32,
                height: 32,
                rgba: vec![0; 4095],
            })
            .is_none());
        // Empty buffer against a nonzero extent, and a zero extent outright.
        assert!(stage
            .convert(&RawFrame {
                width: 32,
                height: 32,
                rgba: vec![],
            })
            .is_none());
        assert!(stage
            .convert(&RawFrame {
                width: 0,
                height: 0,
                rgba: vec![],
            })
            .is_none());
        assert_eq!(stage.frames_converted, 0);
        assert_eq!(stage.last_dims, None);
    }

    #[test]
    fn normalize_tail_requests_faststart_out() {
        let args = normalize_args("tape.mkv", "out.mp4");
        assert_eq!(
            args[args.len() - 2..],
            ["+faststart".to_string(), "out.mp4".to_string()],
            "normalizer must end with -movflags +faststart <out>, got {args:?}"
        );
    }

    #[test]
    fn normalized_output_models_a_moov_first_round_trip() {
        // The normalizer emits +faststart, whose post-condition is moov
        // before mdat. Model that file layout (no re-encode in tests) and
        // prove the readiness helpers accept exactly that shape.
        let args = normalize_args("tape.mkv", "out.mp4");
        assert!(args.iter().any(|a| a == "+faststart"));
        assert!(is_moov_first(Some(32), Some(4096)));
        assert!(!is_moov_first(Some(4096), Some(32)));
        assert!(!is_moov_first(None, Some(4096)));
        let readiness = Mp4RangeReadiness {
            moov_first: is_moov_first(Some(32), Some(4096)),
            accepts_range: supports_range(Some("bytes")),
        };
        assert!(readiness.is_progressive_ready());
        assert!(!Mp4RangeReadiness {
            moov_first: false,
            accepts_range: true,
        }
        .is_progressive_ready());
    }

    #[test]
    fn top_level_offsets_spots_moov_before_mdat() {
        // ftyp(24) + moov(8) + mdat(8): fabricated atoms, real layout rule.
        let mut bytes = vec![0u8; 40];
        bytes[0..4].copy_from_slice(&24u32.to_be_bytes());
        bytes[4..8].copy_from_slice(b"ftyp");
        bytes[24..28].copy_from_slice(&8u32.to_be_bytes());
        bytes[28..32].copy_from_slice(b"moov");
        bytes[32..36].copy_from_slice(&8u32.to_be_bytes());
        bytes[36..40].copy_from_slice(b"mdat");
        let (moov, mdat) = mp4_top_level_offsets(&bytes);
        assert_eq!((moov, mdat), (Some(24), Some(32)));
        assert!(is_moov_first(moov, mdat));
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
