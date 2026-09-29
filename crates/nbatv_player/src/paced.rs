//! Lane A frame pacing: release decoded frames at the source frame rate.
//!
//! The ffmpeg rawvideo pipe is headerless and untimed: ffmpeg writes frames
//! as fast as it decodes them, so an unpaced reader plays a game at
//! whatever speed the CPU allows. [`PacedPump`] fixes that. It wraps any
//! [`FrameSource`] (the real [`Pump`] in production, a fake in tests) and
//! a [`MediaClock`], and releases frame `n` only when the clock reaches
//! `base + n / fps`.
//!
//! - **Early frame:** [`Poll::Wait`] with the remaining time; the frame is
//!   held, not lost.
//! - **Late frame** (clock more than `drop_threshold` past its slot):
//!   dropped, so video catches up instead of drifting behind the clock.
//!   At most [`MAX_DROPS_PER_POLL`] frames drop per poll so one call never
//!   spins through a whole backlog.
//! - **Paused:** [`Poll::Paused`], nothing is consumed from the source.
//! - **Seek:** the source respawns at the offset, the clock jumps there,
//!   and the frame index restarts at zero.
//!
//! [`PacedPump::poll`] never sleeps, so the shell can call it once per
//! repaint and schedule the next repaint after the returned wait.
//! [`PacedPump::next_paced`] is the blocking convenience form with an
//! injected sleeper. The clock is injected too ([`crate::clock`]), so the
//! pacing logic is tested with a fake clock and no real waiting.
//!
//! Frame index `n` maps to `n / fps` assuming a constant frame rate, which
//! holds for the Cache Tier's normalized H.264 copies. Variable-frame-rate
//! tape drifts by the difference; the master clock (audio, when active)
//! still bounds that drift because late frames are dropped.

use std::time::Duration;

use crate::clock::{MediaClock, MonotonicTime, WallClock};
use crate::lane_a::{parse_probe_report, probe_args, RawFrame};
use crate::pump::{Pump, PumpError};

/// Frame rate assumed when the probe report names none.
pub const DEFAULT_FPS: f64 = 30.0;

/// Most frames one [`PacedPump::poll`] drops while catching up.
pub const MAX_DROPS_PER_POLL: u32 = 8;

/// Anything that yields decoded frames and can restart at an offset.
///
/// [`Pump`] is the production implementation; tests supply a fake so the
/// pacing logic runs with no ffmpeg.
pub trait FrameSource {
    /// The next frame, or `None` at end of stream.
    fn next_frame(&mut self) -> Option<RawFrame>;
    /// Restart decoding at `seconds`.
    fn seek(&mut self, seconds: f64) -> Result<(), PumpError>;
}

impl FrameSource for Pump {
    fn next_frame(&mut self) -> Option<RawFrame> {
        Pump::next_frame(self)
    }

    fn seek(&mut self, seconds: f64) -> Result<(), PumpError> {
        Pump::seek(self, seconds)
    }
}

/// What [`PacedPump::poll`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Poll {
    /// A frame that is due now; show it.
    Frame(RawFrame),
    /// The next frame is not due yet; call again after this long.
    Wait(Duration),
    /// The clock is paused; no frame is consumed.
    Paused,
    /// The source has no more frames.
    End,
}

/// Which clock drives a [`PacedPump`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClockKind {
    /// Monotonic wall clock (silent playback).
    Wall,
    /// The audio output position (audio is the master clock).
    Audio,
}

/// A [`FrameSource`] released at the source frame rate against a
/// [`MediaClock`].
pub struct PacedPump<S: FrameSource> {
    source: S,
    clock: Box<dyn MediaClock>,
    kind: ClockKind,
    fps: f64,
    /// Media position of frame index 0 (nonzero after a seek).
    base: Duration,
    /// Index of the next frame to read from the source.
    next_index: u64,
    /// A frame read but not yet due, with its presentation time.
    pending: Option<(Duration, RawFrame)>,
    drop_threshold: Duration,
    dropped: u64,
    ended: bool,
    fallback_reason: Option<String>,
}

impl<S: FrameSource> PacedPump<S> {
    /// Pace `source` at `fps` against `clock`.
    ///
    /// `fps` must be finite and positive, else
    /// [`PumpError::BadFrameRate`]. The clock decides the start position;
    /// a fresh clock at zero matches a fresh source.
    pub fn new(source: S, fps: f64, clock: Box<dyn MediaClock>) -> Result<Self, PumpError> {
        Self::with_kind(source, fps, clock, ClockKind::Wall)
    }

    /// Like [`PacedPump::new`], recording which kind of clock was passed.
    pub fn with_kind(
        source: S,
        fps: f64,
        clock: Box<dyn MediaClock>,
        kind: ClockKind,
    ) -> Result<Self, PumpError> {
        if !fps.is_finite() || fps <= 0.0 {
            return Err(PumpError::BadFrameRate);
        }
        Ok(PacedPump {
            source,
            clock,
            kind,
            fps,
            base: Duration::ZERO,
            next_index: 0,
            pending: None,
            drop_threshold: Duration::from_secs_f64(2.0 / fps),
            dropped: 0,
            ended: false,
            fallback_reason: None,
        })
    }

    /// Lateness beyond which a frame is dropped (default: two frame
    /// periods).
    pub fn with_drop_threshold(mut self, threshold: Duration) -> Self {
        self.drop_threshold = threshold;
        self
    }

    /// Record why a preferred clock was not used (surfaced to the shell).
    pub fn with_fallback_reason(mut self, reason: Option<String>) -> Self {
        self.fallback_reason = reason;
        self
    }

    /// Ask for the next frame without ever sleeping.
    pub fn poll(&mut self) -> Poll {
        if self.clock.is_paused() {
            return Poll::Paused;
        }
        let mut drops = 0;
        loop {
            let (pts, frame) = match self.pending.take() {
                Some(pending) => pending,
                None => {
                    if self.ended {
                        return Poll::End;
                    }
                    let Some(frame) = self.source.next_frame() else {
                        self.ended = true;
                        return Poll::End;
                    };
                    let pts =
                        self.base + Duration::from_secs_f64(self.next_index as f64 / self.fps);
                    self.next_index += 1;
                    (pts, frame)
                }
            };
            let position = self.clock.position();
            if pts > position {
                let wait = pts - position;
                self.pending = Some((pts, frame));
                return Poll::Wait(wait);
            }
            if position - pts > self.drop_threshold && drops < MAX_DROPS_PER_POLL {
                self.dropped += 1;
                drops += 1;
                continue;
            }
            return Poll::Frame(frame);
        }
    }

    /// Block until the next frame is due, sleeping through `sleep`.
    ///
    /// Returns `None` at end of stream **or while paused** (check
    /// [`PacedPump::is_paused`] to tell them apart). Inject the sleeper:
    /// production passes `std::thread::sleep`, tests advance a fake clock.
    pub fn next_paced(&mut self, sleep: &mut dyn FnMut(Duration)) -> Option<RawFrame> {
        loop {
            match self.poll() {
                Poll::Frame(frame) => return Some(frame),
                Poll::Wait(wait) => sleep(wait),
                Poll::Paused | Poll::End => return None,
            }
        }
    }

    /// Freeze playback; the source is not read while paused.
    pub fn pause(&mut self) {
        self.clock.pause();
    }

    /// Continue from the paused position.
    pub fn resume(&mut self) {
        self.clock.resume();
    }

    /// Whether playback is paused.
    pub fn is_paused(&self) -> bool {
        self.clock.is_paused()
    }

    /// Restart at `seconds`: respawn the source, jump the clock, restart
    /// the frame index. The pause state is kept.
    pub fn seek(&mut self, seconds: f64) -> Result<(), PumpError> {
        let seconds = if seconds.is_finite() {
            seconds.max(0.0)
        } else {
            0.0
        };
        self.source.seek(seconds)?;
        let to = Duration::from_secs_f64(seconds);
        self.clock.seek(to);
        self.base = to;
        self.next_index = 0;
        self.pending = None;
        self.ended = false;
        Ok(())
    }

    /// Current media position according to the clock.
    pub fn position(&self) -> Duration {
        self.clock.position()
    }

    /// Frames dropped so far to keep up with the clock.
    pub fn dropped_frames(&self) -> u64 {
        self.dropped
    }

    /// The frame rate frames are paced at.
    pub fn fps(&self) -> f64 {
        self.fps
    }

    /// Which clock drives playback.
    pub fn clock_kind(&self) -> ClockKind {
        self.kind
    }

    /// Why the preferred (audio) clock was not used, when it was not.
    pub fn fallback_reason(&self) -> Option<&str> {
        self.fallback_reason.as_deref()
    }

    /// The wrapped source.
    pub fn source(&self) -> &S {
        &self.source
    }
}

impl PacedPump<Pump> {
    /// Open `src` at the normalized Lane A extent, paced at its own frame
    /// rate against the wall clock.
    ///
    /// Runs `ffmpeg -hide_banner -i <src>` once (the existing
    /// [`probe_args`] sidecar call, no extra binary) and reads the frame
    /// rate from its stream report; if the report names none the pump
    /// falls back to [`DEFAULT_FPS`].
    pub fn open_lane_a(src: &str) -> Result<Self, PumpError> {
        let fps = probe_fps(src)?;
        let pump = Pump::open_lane_a(src)?;
        Self::new(pump, fps, Box::new(WallClock::new(MonotonicTime::new())))
    }
}

#[cfg(feature = "audio")]
impl PacedPump<Pump> {
    /// Like [`PacedPump::open_lane_a`], but with audio as the master clock.
    ///
    /// When the tape has an audio stream and a default output device
    /// exists, the audio session ([`crate::audio::AudioPlayback`]) becomes
    /// the pump's clock: pause, resume and seek then move audio and video
    /// together. If audio cannot start (no device, unsupported format, no
    /// audio stream, sidecar failure) playback falls back to the wall
    /// clock, silently but honestly: [`PacedPump::fallback_reason`] says
    /// why.
    pub fn open_lane_a_with_audio(src: &str) -> Result<Self, PumpError> {
        use crate::audio::AudioPlayback;

        let report = parse_probe_report(&run_probe(src)?);
        let fps = report.fps.unwrap_or(DEFAULT_FPS);
        let pump = Pump::open_lane_a(src)?;
        let wall = || -> Box<dyn MediaClock> { Box::new(WallClock::new(MonotonicTime::new())) };
        if !report.has_audio {
            return Self::new(pump, fps, wall())
                .map(|p| p.with_fallback_reason(Some("tape has no audio stream".into())));
        }
        match AudioPlayback::open(src, Duration::ZERO) {
            Ok(audio) => Self::with_kind(pump, fps, Box::new(audio), ClockKind::Audio),
            Err(err) => Self::new(pump, fps, wall())
                .map(|p| p.with_fallback_reason(Some(format!("audio unavailable: {err}")))),
        }
    }
}

/// Probe `src` for its video frame rate via the ffmpeg stream report.
pub(crate) fn probe_fps(src: &str) -> Result<f64, PumpError> {
    let report = run_probe(src)?;
    Ok(parse_probe_report(&report).fps.unwrap_or(DEFAULT_FPS))
}

/// Run `ffmpeg -hide_banner -i <src>` and return its stderr report.
///
/// ffmpeg exits non-zero here (no output file) by design; only a failure
/// to spawn is an error.
pub(crate) fn run_probe(src: &str) -> Result<String, PumpError> {
    let output = std::process::Command::new("ffmpeg")
        .args(probe_args(src))
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(PumpError::Spawn)?;
    Ok(String::from_utf8_lossy(&output.stderr).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::ManualTime;
    use std::cell::RefCell;
    use std::rc::Rc;

    const FPS: f64 = 10.0; // 100 ms per frame keeps the arithmetic exact
    const PERIOD: Duration = Duration::from_millis(100);

    /// Yields `total` 1x1 frames whose first pixel byte is their index.
    struct FakeSource {
        total: u8,
        next: u8,
        seeks: Rc<RefCell<Vec<f64>>>,
    }

    impl FakeSource {
        fn new(total: u8) -> (Self, Rc<RefCell<Vec<f64>>>) {
            let seeks = Rc::new(RefCell::new(Vec::new()));
            (
                FakeSource {
                    total,
                    next: 0,
                    seeks: Rc::clone(&seeks),
                },
                seeks,
            )
        }
    }

    impl FrameSource for FakeSource {
        fn next_frame(&mut self) -> Option<RawFrame> {
            if self.next >= self.total {
                return None;
            }
            let frame = RawFrame {
                width: 1,
                height: 1,
                rgba: vec![self.next, 0, 0, 255],
            };
            self.next += 1;
            Some(frame)
        }

        fn seek(&mut self, seconds: f64) -> Result<(), PumpError> {
            self.seeks.borrow_mut().push(seconds);
            self.next = 0;
            Ok(())
        }
    }

    fn paced(total: u8) -> (PacedPump<FakeSource>, ManualTime, Rc<RefCell<Vec<f64>>>) {
        let time = ManualTime::new();
        let (source, seeks) = FakeSource::new(total);
        let clock = WallClock::new(time.clone());
        let pump = PacedPump::new(source, FPS, Box::new(clock)).expect("valid fps");
        (pump, time, seeks)
    }

    fn index_of(frame: &RawFrame) -> u8 {
        frame.rgba[0]
    }

    #[test]
    fn first_frame_is_due_immediately_then_the_next_waits_one_period() {
        let (mut pump, _time, _) = paced(5);
        match pump.poll() {
            Poll::Frame(f) => assert_eq!(index_of(&f), 0),
            other => panic!("expected frame 0, got {other:?}"),
        }
        assert_eq!(pump.poll(), Poll::Wait(PERIOD));
    }

    #[test]
    fn a_waiting_frame_is_held_not_lost() {
        let (mut pump, time, _) = paced(5);
        let _ = pump.poll(); // frame 0
        assert_eq!(pump.poll(), Poll::Wait(PERIOD));
        time.advance(Duration::from_millis(40));
        assert_eq!(pump.poll(), Poll::Wait(Duration::from_millis(60)));
        time.advance(Duration::from_millis(60));
        match pump.poll() {
            Poll::Frame(f) => assert_eq!(index_of(&f), 1, "held frame must be released"),
            other => panic!("expected frame 1, got {other:?}"),
        }
    }

    #[test]
    fn next_paced_releases_frames_at_the_source_rate_without_sleeping() {
        let (mut pump, time, _) = paced(5);
        let mut slept = Vec::new();
        let mut release_times = Vec::new();
        {
            let time = time.clone();
            let mut sleep = |d: Duration| {
                slept.push(d);
                time.advance(d);
            };
            while let Some(frame) = pump.next_paced(&mut sleep) {
                use crate::clock::TimeSource;
                release_times.push((index_of(&frame), time.now()));
            }
        }
        let expected: Vec<(u8, Duration)> = (0..5).map(|i| (i, PERIOD * u32::from(i))).collect();
        assert_eq!(release_times, expected);
        assert_eq!(slept, vec![PERIOD; 4], "one period slept between frames");
        assert_eq!(pump.dropped_frames(), 0);
        assert_eq!(pump.poll(), Poll::End);
    }

    #[test]
    fn late_frames_are_dropped_to_catch_up() {
        let (mut pump, time, _) = paced(20);
        let _ = pump.poll(); // frame 0 at t=0
        time.advance(Duration::from_millis(1000)); // clock jumps to frame 10's slot
        match pump.poll() {
            Poll::Frame(f) => {
                // Frames 1..=7 are >200 ms late and dropped; frame 8 is
                // exactly 200 ms late (not beyond the threshold) and shown.
                assert_eq!(index_of(&f), 8);
            }
            other => panic!("expected a caught-up frame, got {other:?}"),
        }
        assert_eq!(pump.dropped_frames(), 7);
    }

    #[test]
    fn drops_per_poll_are_bounded() {
        let (mut pump, time, _) = paced(200);
        let _ = pump.poll();
        time.advance(Duration::from_secs(15)); // hopelessly behind
        match pump.poll() {
            Poll::Frame(f) => assert_eq!(u32::from(index_of(&f)), MAX_DROPS_PER_POLL + 1),
            other => panic!("expected a frame after bounded drops, got {other:?}"),
        }
        assert_eq!(pump.dropped_frames(), u64::from(MAX_DROPS_PER_POLL));
    }

    #[test]
    fn pause_consumes_nothing_and_resume_continues_in_step() {
        let (mut pump, time, _) = paced(5);
        let _ = pump.poll(); // frame 0
        pump.pause();
        assert!(pump.is_paused());
        time.advance(Duration::from_secs(60));
        assert_eq!(pump.poll(), Poll::Paused);
        assert_eq!(pump.position(), Duration::ZERO);
        pump.resume();
        assert_eq!(
            pump.poll(),
            Poll::Wait(PERIOD),
            "no catch-up burst after pause"
        );
        time.advance(PERIOD);
        match pump.poll() {
            Poll::Frame(f) => assert_eq!(index_of(&f), 1),
            other => panic!("expected frame 1, got {other:?}"),
        }
        assert_eq!(pump.dropped_frames(), 0);
    }

    #[test]
    fn seek_respawns_source_jumps_clock_and_restarts_indexing() {
        let (mut pump, time, seeks) = paced(5);
        let _ = pump.poll();
        time.advance(PERIOD);
        let _ = pump.poll();
        pump.seek(30.0).expect("fake seek");
        assert_eq!(*seeks.borrow(), vec![30.0]);
        assert_eq!(pump.position(), Duration::from_secs(30));
        // The first frame after the seek is due at once (index 0 at base).
        match pump.poll() {
            Poll::Frame(f) => assert_eq!(index_of(&f), 0),
            other => panic!("expected the post-seek frame 0, got {other:?}"),
        }
        assert_eq!(pump.poll(), Poll::Wait(PERIOD));
    }

    #[test]
    fn seek_clears_the_end_of_stream_latch() {
        let (mut pump, time, _) = paced(1);
        let _ = pump.poll();
        time.advance(PERIOD);
        assert_eq!(pump.poll(), Poll::End);
        pump.seek(0.0).expect("fake seek");
        assert!(matches!(pump.poll(), Poll::Frame(_)));
    }

    #[test]
    fn seek_sanitizes_negative_and_non_finite_offsets() {
        let (mut pump, _time, seeks) = paced(3);
        pump.seek(-5.0).expect("clamped");
        pump.seek(f64::NAN).expect("clamped");
        assert_eq!(*seeks.borrow(), vec![0.0, 0.0]);
    }

    #[test]
    fn bad_frame_rates_are_rejected() {
        for fps in [0.0, -24.0, f64::NAN, f64::INFINITY] {
            let (source, _) = FakeSource::new(1);
            let clock = WallClock::new(ManualTime::new());
            assert!(
                matches!(
                    PacedPump::new(source, fps, Box::new(clock)),
                    Err(PumpError::BadFrameRate)
                ),
                "fps {fps} must be rejected"
            );
        }
    }

    #[test]
    fn ntsc_rate_keeps_exact_index_to_time_mapping() {
        // 30000/1001 fps: frame 30 lands at 1.001 s, not 1.000 s.
        let (source, _) = FakeSource::new(1);
        let clock = WallClock::new(ManualTime::new());
        let pump = PacedPump::new(source, 30000.0 / 1001.0, Box::new(clock)).expect("fps");
        let pts = Duration::from_secs_f64(30.0 / pump.fps());
        assert!((pts.as_secs_f64() - 1.001).abs() < 1e-9);
    }

    #[test]
    fn clock_kind_defaults_to_wall_and_records_fallback() {
        let (source, _) = FakeSource::new(1);
        let clock = WallClock::new(ManualTime::new());
        let pump = PacedPump::new(source, FPS, Box::new(clock))
            .expect("fps")
            .with_fallback_reason(Some("no output device".into()));
        assert_eq!(pump.clock_kind(), ClockKind::Wall);
        assert_eq!(pump.fallback_reason(), Some("no output device"));
    }

    fn ffmpeg_present() -> bool {
        std::process::Command::new("ffmpeg")
            .arg("-version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    /// Real sidecar end to end, still without sleeping: the probe reads the
    /// fixture's true 5 fps, a real [`Pump`] feeds a [`PacedPump`], and a
    /// fake clock is stepped by hand.
    #[test]
    fn probed_fps_paces_a_real_pump_on_a_fake_clock() {
        if !ffmpeg_present() {
            println!("SKIP probed_fps_paces_a_real_pump_on_a_fake_clock: ffmpeg not installed");
            return;
        }
        let dir = std::env::temp_dir().join(format!("nbatv-paced-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let mp4 = dir.join("synth.mp4");
        let status = std::process::Command::new("ffmpeg")
            .args(["-y", "-v", "error", "-f", "lavfi", "-i"])
            .arg("testsrc=duration=1:size=32x32:rate=5")
            .args(["-f", "lavfi", "-i", "sine=duration=1"])
            .args([
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "aac",
                "-shortest",
            ])
            .arg(&mp4)
            .status()
            .expect("spawn ffmpeg");
        assert!(status.success());
        let src = mp4.to_string_lossy().into_owned();

        let report = parse_probe_report(&run_probe(&src).expect("probe"));
        assert_eq!(report.fps, Some(5.0));
        assert!(report.has_audio);

        let time = ManualTime::new();
        let pump = Pump::open_scaled(&src, 32, 32).expect("open pump");
        let mut paced = PacedPump::new(
            pump,
            report.fps.expect("fps"),
            Box::new(WallClock::new(time.clone())),
        )
        .expect("paced");
        let mut shown = 0;
        loop {
            match paced.poll() {
                Poll::Frame(f) => {
                    assert_eq!(f.rgba.len(), 32 * 32 * 4);
                    shown += 1;
                }
                Poll::Wait(wait) => {
                    assert_eq!(wait, Duration::from_millis(200), "5 fps = 200 ms/frame");
                    time.advance(wait);
                }
                Poll::Paused => panic!("never paused"),
                Poll::End => break,
            }
        }
        assert_eq!(shown, 5);
        assert_eq!(paced.dropped_frames(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Audio-master end to end on a real device (silent tape, so nothing
    /// audible): the pump reports the audio clock and real-time pacing
    /// releases 5 frames in about 0.8 s. `cargo test --features audio --
    /// --ignored`.
    #[cfg(feature = "audio")]
    #[test]
    #[ignore = "needs a real audio output device and ffmpeg"]
    fn audio_master_clock_paces_a_real_pump() {
        let dir = std::env::temp_dir().join(format!("nbatv-paced-audio-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let mp4 = dir.join("silent.mp4");
        let status = std::process::Command::new("ffmpeg")
            .args(["-y", "-v", "error", "-f", "lavfi", "-i"])
            .arg("testsrc=duration=1:size=32x32:rate=5")
            .args(["-f", "lavfi", "-i", "anullsrc=r=44100:cl=mono"])
            .args([
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "aac",
                "-shortest",
            ])
            .arg(&mp4)
            .status()
            .expect("spawn ffmpeg");
        assert!(status.success());

        let mut paced =
            PacedPump::open_lane_a_with_audio(&mp4.to_string_lossy()).expect("open with audio");
        assert_eq!(
            paced.clock_kind(),
            ClockKind::Audio,
            "{:?}",
            paced.fallback_reason()
        );
        let started = std::time::Instant::now();
        let mut shown = 0;
        while paced.next_paced(&mut std::thread::sleep).is_some() {
            shown += 1;
        }
        let elapsed = started.elapsed();
        assert!(shown >= 4, "shown {shown}");
        assert!(
            elapsed >= Duration::from_millis(600),
            "released too fast: {elapsed:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
