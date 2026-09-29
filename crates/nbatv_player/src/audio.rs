//! Lane A audio: a second ffmpeg sidecar decodes the tape's audio to raw
//! `f32le` PCM ([`crate::lane_a::audio_pipe_args`]) and a `cpal` output
//! stream plays it. Audio is **the master clock** when active; see
//! [`crate::clock`] for the rule and [`crate::paced`] for how video follows.
//!
//! ## Feature gate
//!
//! Everything device-free (format math, the PCM queue, the audio clock) is
//! always compiled and unit-tested. The `cpal`-backed [`AudioPlayback`] only
//! exists with the cargo feature `audio` (default **off**: on Linux `cpal`
//! links `libasound`, which CI's ubuntu image does not provide).
//!
//! ## `cpal` directly, not `rodio`
//!
//! Evidence (crates.io / vendored sources, `cargo info`):
//!
//! - `rodio 0.22` is a thin layer over `cpal 0.17`; its `Player` API is
//!   built around self-decoding `Source`s, gapless queues and effects. We
//!   already have a decoder (the ffmpeg sidecar), so we would only feed
//!   rodio a custom `Source` over a channel: rodio then adds symphonia
//!   (default `mp4`/`flac`/... features, or `default-features = false` and
//!   hand-picked ones) and `rand`, for nothing we use.
//! - The master clock needs the **number of frames the device consumed**.
//!   With `cpal` that is the output callback itself (each callback counts
//!   what it took from the queue). rodio's `Player::get_pos` reports source
//!   position, not device consumption, and an underrun there is invisible.
//! - Pause must not consume samples and an underrun must freeze the clock;
//!   both are two lines in a `cpal` callback and awkward through rodio's
//!   `Source` pull model.
//! - Dependency cost: `cpal` alone (`dasp_sample` + platform audio crates)
//!   vs. `cpal` plus rodio's tree; ADR 0001 keeps `nbatv_player` a
//!   near-leaf, so the smaller option wins. ADR 0003 is unaffected: the
//!   decoder stays an unlinked sidecar process; `cpal` is Rust code over
//!   the OS audio API (CoreAudio/WASAPI/ALSA), no C compiled in this
//!   workspace.
//!
//! ## Data flow
//!
//! ```text
//! ffmpeg -vn -f f32le -ar R -ac C -   ->  reader thread  ->  PcmBuffer  ->  cpal callback
//!        (respawned on seek)               (bounded, gen-tagged)  (counts frames played)
//!                                                                    |
//!                                              AudioClock.position() = base + played / R
//! ```
//!
//! - **Pause:** the callback emits silence and consumes nothing, so the
//!   clock freezes.
//! - **Seek:** kill the sidecar, clear the queue, bump the *generation*
//!   (stale reader pushes are refused), reset `played`, set the new `base`,
//!   respawn at the offset.
//! - **Stop:** dropping [`AudioPlayback`] kills the sidecar and the stream.
//! - **Audio ends before video:** the clock hands over to the monotonic
//!   clock from the last audio position, so video is never stranded.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use crate::clock::{MediaClock, TimeSource};

/// Bytes per `f32` sample on the PCM pipe.
pub const BYTES_PER_SAMPLE: usize = 4;

/// Default queue depth between the sidecar and the device.
pub const DEFAULT_BUFFER: Duration = Duration::from_millis(500);

/// Sample layout of the PCM pipe (interleaved `f32`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioFormat {
    /// Frames per second per channel.
    pub sample_rate: u32,
    /// Interleaved channel count.
    pub channels: u16,
}

impl AudioFormat {
    /// `None` when either field is zero (a format that cannot play).
    pub fn new(sample_rate: u32, channels: u16) -> Option<Self> {
        (sample_rate > 0 && channels > 0).then_some(AudioFormat {
            sample_rate,
            channels,
        })
    }

    /// Playback time of `frames` PCM frames.
    pub fn frames_to_duration(&self, frames: u64) -> Duration {
        let rate = u64::from(self.sample_rate);
        let secs = frames / rate;
        let rem = frames % rate;
        // rem < rate <= u32::MAX, so rem * 1e9 fits in u64.
        let nanos = rem * 1_000_000_000 / rate;
        Duration::new(secs, nanos as u32)
    }

    /// PCM frames spanning `d` (rounded down).
    pub fn duration_to_frames(&self, d: Duration) -> u64 {
        let whole = d.as_secs().saturating_mul(u64::from(self.sample_rate));
        let part = u64::from(d.subsec_nanos()) * u64::from(self.sample_rate) / 1_000_000_000;
        whole.saturating_add(part)
    }

    /// Interleaved samples that hold `d` of audio, whole frames only.
    pub fn samples_for(&self, d: Duration) -> usize {
        let frames = self.duration_to_frames(d);
        usize::try_from(frames)
            .unwrap_or(usize::MAX / usize::from(self.channels))
            .saturating_mul(usize::from(self.channels))
    }
}

/// Turn little-endian `f32` bytes into samples.
///
/// `carry` holds bytes left over from the previous chunk (a read can end
/// mid-sample); the new leftover (< 4 bytes) is stored back into it.
/// Non-finite samples become silence so a corrupt stream cannot blast the
/// device with NaN.
pub fn decode_f32le(carry: &mut Vec<u8>, chunk: &[u8]) -> Vec<f32> {
    carry.extend_from_slice(chunk);
    let whole = carry.len() - carry.len() % BYTES_PER_SAMPLE;
    let samples = (0..whole)
        .step_by(BYTES_PER_SAMPLE)
        .map(|i| {
            let s = f32::from_le_bytes([carry[i], carry[i + 1], carry[i + 2], carry[i + 3]]);
            if s.is_finite() {
                s.clamp(-1.0, 1.0)
            } else {
                0.0
            }
        })
        .collect();
    carry.drain(..whole);
    samples
}

#[derive(Debug)]
struct State {
    samples: VecDeque<f32>,
    capacity: usize,
    /// PCM frames the device consumed since `base`.
    played_frames: u64,
    /// Media position corresponding to `played_frames == 0`.
    base: Duration,
    paused: bool,
    /// Bumped by every reset; stale producers are refused.
    generation: u64,
    /// The producer reached end of stream.
    eof: bool,
}

/// The bounded PCM queue shared by the reader thread (producer) and the
/// device callback (consumer). Cheap to clone (shared state).
#[derive(Debug, Clone)]
pub struct PcmBuffer {
    state: Arc<Mutex<State>>,
    format: AudioFormat,
}

impl PcmBuffer {
    /// An empty queue holding at most `depth` of audio, positioned at
    /// media time zero and not paused.
    pub fn new(format: AudioFormat, depth: Duration) -> Self {
        let capacity = format.samples_for(depth).max(usize::from(format.channels));
        PcmBuffer {
            state: Arc::new(Mutex::new(State {
                samples: VecDeque::new(),
                capacity,
                played_frames: 0,
                base: Duration::ZERO,
                paused: false,
                generation: 0,
                eof: false,
            })),
            format,
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        // A poisoned lock means a callback panicked; the queue is plain
        // data, so keep serving it rather than cascade the panic.
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The pipe layout.
    pub fn format(&self) -> AudioFormat {
        self.format
    }

    /// Current generation; producers tag their pushes with it.
    pub fn generation(&self) -> u64 {
        self.lock().generation
    }

    /// Queue as many of `samples` as fit and return how many were taken,
    /// or `None` when `generation` is stale (the producer must stop).
    pub fn push(&self, generation: u64, samples: &[f32]) -> Option<usize> {
        let mut st = self.lock();
        if st.generation != generation {
            return None;
        }
        let room = st.capacity.saturating_sub(st.samples.len());
        let take = room.min(samples.len());
        st.samples.extend(&samples[..take]);
        Some(take)
    }

    /// The producer of `generation` hit end of stream.
    pub fn mark_eof(&self, generation: u64) {
        let mut st = self.lock();
        if st.generation == generation {
            st.eof = true;
        }
    }

    /// Fill a device buffer. The output-callback body.
    ///
    /// Takes whole frames from the queue and zero-fills the rest. Only
    /// frames actually taken advance the clock, so an underrun freezes it;
    /// a paused buffer emits silence and consumes nothing. Returns the
    /// number of PCM frames consumed.
    pub fn fill(&self, out: &mut [f32]) -> u64 {
        let mut st = self.lock();
        let channels = usize::from(self.format.channels);
        let taken = if st.paused {
            0
        } else {
            let want = out.len().min(st.samples.len());
            want - want % channels
        };
        for (slot, sample) in out.iter_mut().zip(st.samples.drain(..taken)) {
            *slot = sample;
        }
        out[taken..].fill(0.0);
        let frames = (taken / channels) as u64;
        st.played_frames += frames;
        frames
    }

    /// Hold or release consumption.
    pub fn set_paused(&self, paused: bool) {
        self.lock().paused = paused;
    }

    /// Whether consumption is held.
    pub fn is_paused(&self) -> bool {
        self.lock().paused
    }

    /// Drop queued audio, restart the counter at media position `base`,
    /// invalidate old producers, and return the new generation.
    pub fn reset(&self, base: Duration) -> u64 {
        let mut st = self.lock();
        st.samples.clear();
        st.played_frames = 0;
        st.base = base;
        st.eof = false;
        st.generation += 1;
        st.generation
    }

    /// Media position the device has reached.
    pub fn played_position(&self) -> Duration {
        let st = self.lock();
        st.base + self.format.frames_to_duration(st.played_frames)
    }

    /// Whether the producer finished and the device drained everything.
    pub fn is_drained(&self) -> bool {
        let st = self.lock();
        st.eof && st.samples.is_empty()
    }

    /// Samples currently queued.
    pub fn queued_samples(&self) -> usize {
        self.lock().samples.len()
    }
}

/// Remembered moment the audio ran out, for the wall-clock handover.
#[derive(Debug, Clone, Copy)]
struct Handover {
    /// Media position when audio ended (or when last re-based).
    position: Duration,
    /// Time reading matching `position`.
    at: Duration,
}

/// The audio master clock: position is what the device has played.
///
/// If the audio stream ends before the video does, the position keeps
/// advancing in real time from where audio stopped ([`TimeSource`]) so the
/// remaining video still plays out.
pub struct AudioClock<T: TimeSource> {
    buffer: PcmBuffer,
    time: T,
    handover: std::cell::Cell<Option<Handover>>,
}

impl<T: TimeSource> AudioClock<T> {
    /// A clock over `buffer`, using `time` only for the end-of-audio
    /// handover.
    pub fn new(buffer: PcmBuffer, time: T) -> Self {
        AudioClock {
            buffer,
            time,
            handover: std::cell::Cell::new(None),
        }
    }

    /// The shared PCM queue.
    pub fn buffer(&self) -> &PcmBuffer {
        &self.buffer
    }
}

impl<T: TimeSource> MediaClock for AudioClock<T> {
    fn position(&self) -> Duration {
        if let Some(h) = self.handover.get() {
            return if self.buffer.is_paused() {
                h.position
            } else {
                h.position + self.time.now().saturating_sub(h.at)
            };
        }
        if self.buffer.is_drained() {
            self.handover.set(Some(Handover {
                position: self.buffer.played_position(),
                at: self.time.now(),
            }));
            return self.buffer.played_position();
        }
        self.buffer.played_position()
    }

    fn pause(&mut self) {
        if self.handover.get().is_some() {
            let position = self.position();
            self.handover.set(Some(Handover {
                position,
                at: self.time.now(),
            }));
        }
        self.buffer.set_paused(true);
    }

    fn resume(&mut self) {
        if let Some(h) = self.handover.get() {
            self.handover.set(Some(Handover {
                position: h.position,
                at: self.time.now(),
            }));
        }
        self.buffer.set_paused(false);
    }

    fn seek(&mut self, to: Duration) {
        self.handover.set(None);
        self.buffer.reset(to);
    }

    fn is_paused(&self) -> bool {
        self.buffer.is_paused()
    }
}

#[cfg(feature = "audio")]
pub use device::{AudioError, AudioPlayback};

#[cfg(feature = "audio")]
mod device {
    use std::io::Read;
    use std::process::{Child, Command, Stdio};
    use std::time::Duration;

    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use cpal::{SampleFormat, Stream};

    use super::{decode_f32le, AudioClock, AudioFormat, PcmBuffer, DEFAULT_BUFFER};
    use crate::clock::{MediaClock, MonotonicTime};
    use crate::lane_a::audio_pipe_args;

    /// Why audio could not start. Callers fall back to the wall clock.
    #[derive(Debug)]
    pub enum AudioError {
        /// No default output device.
        NoDevice,
        /// The device's default format is not one this player can feed.
        Unsupported(String),
        /// The output stream could not be built or started.
        Stream(String),
        /// The audio sidecar could not be spawned.
        Spawn(std::io::Error),
        /// The audio sidecar has no piped stdout.
        MissingPipe,
    }

    impl std::fmt::Display for AudioError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                AudioError::NoDevice => write!(f, "no default audio output device"),
                AudioError::Unsupported(why) => write!(f, "unsupported audio output: {why}"),
                AudioError::Stream(why) => write!(f, "audio output stream failed: {why}"),
                AudioError::Spawn(err) => write!(f, "could not spawn ffmpeg audio sidecar: {err}"),
                AudioError::MissingPipe => write!(f, "ffmpeg audio sidecar has no piped stdout"),
            }
        }
    }

    impl std::error::Error for AudioError {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            match self {
                AudioError::Spawn(err) => Some(err),
                _ => None,
            }
        }
    }

    /// Live audio session: sidecar + reader thread + `cpal` stream, and the
    /// master [`MediaClock`] for the video that goes with it.
    ///
    /// Pause, resume and seek arrive through the [`MediaClock`] methods (the
    /// [`crate::paced::PacedPump`] calls them), so audio follows the video
    /// session's transport by construction. Dropping it stops everything.
    pub struct AudioPlayback {
        clock: AudioClock<MonotonicTime>,
        // Held to keep the device stream alive; dropping stops playback.
        _stream: Stream,
        child: Option<Child>,
        src: String,
        format: AudioFormat,
    }

    impl AudioPlayback {
        /// Open the default output device and start decoding `src` from
        /// `start` into it.
        pub fn open(src: &str, start: Duration) -> Result<Self, AudioError> {
            let host = cpal::default_host();
            let device = host.default_output_device().ok_or(AudioError::NoDevice)?;
            let supported = device
                .default_output_config()
                .map_err(|e| AudioError::Unsupported(e.to_string()))?;
            if supported.sample_format() != SampleFormat::F32 {
                return Err(AudioError::Unsupported(format!(
                    "default output is {:?}, need F32",
                    supported.sample_format()
                )));
            }
            let config = supported.config();
            let format = AudioFormat::new(config.sample_rate, config.channels)
                .ok_or_else(|| AudioError::Unsupported("zero rate or channels".into()))?;
            let buffer = PcmBuffer::new(format, DEFAULT_BUFFER);
            let generation = buffer.reset(start);
            buffer.set_paused(false);

            let callback_buffer = buffer.clone();
            let stream = device
                .build_output_stream(
                    &config,
                    move |out: &mut [f32], _| {
                        callback_buffer.fill(out);
                    },
                    |err| eprintln!("nbatv_player audio stream error: {err}"),
                    None,
                )
                .map_err(|e| AudioError::Stream(e.to_string()))?;
            stream
                .play()
                .map_err(|e| AudioError::Stream(e.to_string()))?;

            let child = spawn_sidecar(src, start, format, &buffer, generation)?;
            Ok(AudioPlayback {
                clock: AudioClock::new(buffer, MonotonicTime::new()),
                _stream: stream,
                child: Some(child),
                src: src.to_string(),
                format,
            })
        }

        /// Negotiated device format.
        pub fn format(&self) -> AudioFormat {
            self.format
        }

        fn kill_sidecar(&mut self) {
            if let Some(mut child) = self.child.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }

    impl MediaClock for AudioPlayback {
        fn position(&self) -> Duration {
            self.clock.position()
        }

        fn pause(&mut self) {
            self.clock.pause();
        }

        fn resume(&mut self) {
            self.clock.resume();
        }

        fn seek(&mut self, to: Duration) {
            self.kill_sidecar();
            // Clock reset bumps the generation; then respawn at `to`.
            self.clock.seek(to);
            let generation = self.clock.buffer().generation();
            if let Ok(child) =
                spawn_sidecar(&self.src, to, self.format, self.clock.buffer(), generation)
            {
                self.child = Some(child);
            } else {
                // No sidecar: the queue drains, is never refilled (eof is
                // set by the missing producer), and the clock hands over
                // to real time.
                self.clock.buffer().mark_eof(generation);
            }
        }

        fn is_paused(&self) -> bool {
            self.clock.is_paused()
        }
    }

    impl Drop for AudioPlayback {
        fn drop(&mut self) {
            self.clock.buffer().reset(Duration::ZERO);
            self.kill_sidecar();
        }
    }

    fn spawn_sidecar(
        src: &str,
        start: Duration,
        format: AudioFormat,
        buffer: &PcmBuffer,
        generation: u64,
    ) -> Result<Child, AudioError> {
        let mut child = Command::new("ffmpeg")
            .args(audio_pipe_args(
                src,
                start.as_secs_f64(),
                format.sample_rate,
                format.channels,
            ))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(AudioError::Spawn)?;
        let Some(mut stdout) = child.stdout.take() else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(AudioError::MissingPipe);
        };
        let buffer = buffer.clone();
        std::thread::spawn(move || {
            let mut carry = Vec::new();
            let mut chunk = [0u8; 16 * 1024];
            'read: loop {
                let n = match stdout.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => n,
                };
                let samples = decode_f32le(&mut carry, &chunk[..n]);
                let mut rest = &samples[..];
                while !rest.is_empty() {
                    match buffer.push(generation, rest) {
                        None => return, // stale: a seek or stop replaced us
                        Some(0) => std::thread::sleep(Duration::from_millis(5)),
                        Some(taken) => rest = &rest[taken..],
                    }
                }
                if buffer.generation() != generation {
                    break 'read;
                }
            }
            buffer.mark_eof(generation);
        });
        Ok(child)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::ManualTime;

    const STEREO_48K: AudioFormat = AudioFormat {
        sample_rate: 48_000,
        channels: 2,
    };

    fn buffer() -> PcmBuffer {
        PcmBuffer::new(STEREO_48K, Duration::from_secs(1))
    }

    #[test]
    fn format_rejects_zero_fields() {
        assert!(AudioFormat::new(0, 2).is_none());
        assert!(AudioFormat::new(48_000, 0).is_none());
        assert_eq!(AudioFormat::new(48_000, 2), Some(STEREO_48K));
    }

    #[test]
    fn frame_time_math_is_exact_and_round_trips() {
        let f = STEREO_48K;
        assert_eq!(f.frames_to_duration(48_000), Duration::from_secs(1));
        assert_eq!(f.frames_to_duration(24_000), Duration::from_millis(500));
        assert_eq!(f.frames_to_duration(0), Duration::ZERO);
        assert_eq!(f.duration_to_frames(Duration::from_millis(250)), 12_000);
        assert_eq!(f.samples_for(Duration::from_millis(250)), 24_000);
        // 44.1 kHz does not divide a second evenly; still monotonic/close.
        let cd = AudioFormat::new(44_100, 1).expect("format");
        let d = cd.frames_to_duration(44_101);
        assert!(d > Duration::from_secs(1) && d < Duration::from_millis(1001));
        assert_eq!(
            cd.duration_to_frames(Duration::from_secs(3600)),
            158_760_000
        );
    }

    #[test]
    fn decode_handles_split_samples_and_sanitizes() {
        let mut carry = Vec::new();
        let bytes: Vec<u8> = [0.5f32, -0.25, f32::NAN, 2.0]
            .iter()
            .flat_map(|s| s.to_le_bytes())
            .collect();
        // Split mid-sample: 6 bytes, then the remaining 10.
        let first = decode_f32le(&mut carry, &bytes[..6]);
        assert_eq!(first, vec![0.5]);
        assert_eq!(carry.len(), 2);
        let second = decode_f32le(&mut carry, &bytes[6..]);
        assert_eq!(
            second,
            vec![-0.25, 0.0, 1.0],
            "NaN -> 0, 2.0 clamped to 1.0"
        );
        assert!(carry.is_empty());
    }

    #[test]
    fn fill_counts_only_consumed_frames_and_zero_fills_underrun() {
        let buf = buffer();
        let gen = buf.generation();
        assert_eq!(buf.push(gen, &[0.1, 0.2, 0.3, 0.4]), Some(4));
        let mut out = [9.0f32; 8];
        let frames = buf.fill(&mut out);
        assert_eq!(frames, 2, "4 samples / 2 channels");
        assert_eq!(out, [0.1, 0.2, 0.3, 0.4, 0.0, 0.0, 0.0, 0.0]);
        assert_eq!(
            buf.played_position(),
            STEREO_48K.frames_to_duration(2),
            "underrun silence must not advance the clock"
        );
        let mut again = [1.0f32; 4];
        assert_eq!(buf.fill(&mut again), 0);
        assert_eq!(again, [0.0; 4]);
    }

    #[test]
    fn fill_never_splits_a_frame() {
        let buf = buffer();
        let gen = buf.generation();
        buf.push(gen, &[1.0, 2.0, 3.0]); // 1.5 frames queued
        let mut out = [0.0f32; 6];
        assert_eq!(buf.fill(&mut out), 1);
        assert_eq!(out[..2], [1.0, 2.0]);
        assert_eq!(buf.queued_samples(), 1, "odd sample waits for its pair");
    }

    #[test]
    fn push_is_bounded() {
        let buf = PcmBuffer::new(STEREO_48K, Duration::from_millis(10)); // 480 frames = 960 samples
        let gen = buf.generation();
        let big = vec![0.0f32; 2000];
        assert_eq!(buf.push(gen, &big), Some(960));
        assert_eq!(buf.push(gen, &big), Some(0), "full queue takes nothing");
    }

    #[test]
    fn paused_buffer_outputs_silence_and_consumes_nothing() {
        let buf = buffer();
        let gen = buf.generation();
        buf.push(gen, &[0.5; 8]);
        buf.set_paused(true);
        let mut out = [7.0f32; 8];
        assert_eq!(buf.fill(&mut out), 0);
        assert_eq!(out, [0.0; 8]);
        assert_eq!(buf.queued_samples(), 8);
        buf.set_paused(false);
        assert_eq!(buf.fill(&mut out), 4);
    }

    #[test]
    fn reset_invalidates_old_producers_and_rebases_the_clock() {
        let buf = buffer();
        let old = buf.generation();
        buf.push(old, &[0.5; 8]);
        let mut out = [0.0f32; 8];
        buf.fill(&mut out);
        let new = buf.reset(Duration::from_secs(90));
        assert_ne!(old, new);
        assert_eq!(buf.push(old, &[0.5; 8]), None, "stale producer refused");
        assert_eq!(buf.queued_samples(), 0);
        assert_eq!(buf.played_position(), Duration::from_secs(90));
        assert_eq!(buf.push(new, &[0.5; 4]), Some(4));
    }

    #[test]
    fn stale_eof_is_ignored() {
        let buf = buffer();
        let old = buf.generation();
        let new = buf.reset(Duration::ZERO);
        buf.mark_eof(old);
        assert!(!buf.is_drained());
        buf.mark_eof(new);
        assert!(buf.is_drained());
    }

    fn clock() -> (AudioClock<ManualTime>, ManualTime) {
        let time = ManualTime::new();
        (AudioClock::new(buffer(), time.clone()), time)
    }

    #[test]
    fn clock_position_is_device_consumption_not_wall_time() {
        let (clock, time) = clock();
        time.advance(Duration::from_secs(30)); // wall time is irrelevant
        assert_eq!(clock.position(), Duration::ZERO);
        let gen = clock.buffer().generation();
        clock.buffer().push(gen, &vec![0.0; 96_000]); // 1 s stereo
        let mut out = vec![0.0f32; 48_000]; // 0.5 s
        clock.buffer().fill(&mut out);
        assert_eq!(clock.position(), Duration::from_millis(500));
    }

    #[test]
    fn clock_pause_resume_seek_drive_the_buffer() {
        let (mut clock, _time) = clock();
        clock.pause();
        assert!(clock.is_paused() && clock.buffer().is_paused());
        clock.resume();
        assert!(!clock.is_paused());
        let before = clock.buffer().generation();
        clock.seek(Duration::from_secs(120));
        assert_eq!(clock.position(), Duration::from_secs(120));
        assert_eq!(clock.buffer().generation(), before + 1);
    }

    #[test]
    fn clock_hands_over_to_real_time_when_audio_ends() {
        let (mut clock, time) = clock();
        let gen = clock.buffer().generation();
        clock.buffer().push(gen, &vec![0.0; 96_000]);
        let mut out = vec![0.0f32; 96_000];
        clock.buffer().fill(&mut out); // 1 s played
        clock.buffer().mark_eof(gen);
        assert_eq!(clock.position(), Duration::from_secs(1));
        time.advance(Duration::from_secs(2));
        assert_eq!(
            clock.position(),
            Duration::from_secs(3),
            "video keeps playing after the audio track ends"
        );
        clock.pause();
        time.advance(Duration::from_secs(10));
        assert_eq!(
            clock.position(),
            Duration::from_secs(3),
            "pause freezes the handover"
        );
        clock.resume();
        time.advance(Duration::from_secs(1));
        assert_eq!(clock.position(), Duration::from_secs(4));
    }

    #[test]
    fn seek_after_handover_returns_to_audio_time() {
        let (mut clock, time) = clock();
        let gen = clock.buffer().generation();
        clock.buffer().mark_eof(gen);
        let _ = clock.position(); // latch handover
        time.advance(Duration::from_secs(5));
        clock.seek(Duration::from_secs(40));
        assert_eq!(clock.position(), Duration::from_secs(40));
        time.advance(Duration::from_secs(5));
        assert_eq!(
            clock.position(),
            Duration::from_secs(40),
            "audio clock ignores wall time once fed again"
        );
    }

    /// Real audio sidecar through the argument builder: 1 s of a 440 Hz
    /// sine resampled to 8 kHz mono must arrive as 8000 `f32` samples.
    /// Needs only ffmpeg, no audio device. Skips when ffmpeg is absent.
    #[test]
    fn audio_sidecar_emits_the_requested_pcm_format() {
        use crate::lane_a::audio_pipe_args;
        use std::process::Command;
        if !Command::new("ffmpeg")
            .arg("-version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            println!("SKIP audio_sidecar_emits_the_requested_pcm_format: ffmpeg not installed");
            return;
        }
        let dir = std::env::temp_dir().join(format!("nbatv-audio-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let wav = dir.join("tone.wav");
        let made = Command::new("ffmpeg")
            .args([
                "-y",
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=1",
            ])
            .arg(&wav)
            .status()
            .expect("spawn ffmpeg");
        assert!(made.success());

        let out = Command::new("ffmpeg")
            .args(audio_pipe_args(&wav.to_string_lossy(), 0.0, 8_000, 1))
            .output()
            .expect("run audio sidecar");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let mut carry = Vec::new();
        let samples = decode_f32le(&mut carry, &out.stdout);
        assert!(carry.is_empty(), "pipe must hold whole f32 samples");
        assert_eq!(samples.len(), 8_000, "1 s at 8 kHz mono");
        assert!(
            samples.iter().any(|s| s.abs() > 0.1),
            "tone must be audible, not silence"
        );

        // Input seek: starting 0.5 s in leaves about half the samples.
        let half = Command::new("ffmpeg")
            .args(audio_pipe_args(&wav.to_string_lossy(), 0.5, 8_000, 1))
            .output()
            .expect("run seeked audio sidecar");
        let n = half.stdout.len() / BYTES_PER_SAMPLE;
        assert!((3_900..=4_100).contains(&n), "seeked audio has {n} samples");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Real-device smoke test (needs an output device, so `#[ignore]`d):
    /// plays 1 s of *silence* through the default device and checks the
    /// audio clock advances, freezes on pause, and follows a seek.
    /// Run: `cargo test -p nbatv_player --features audio -- --ignored`.
    #[cfg(feature = "audio")]
    #[test]
    #[ignore = "needs a real audio output device and ffmpeg"]
    fn audio_playback_clock_follows_transport_on_a_real_device() {
        use crate::clock::MediaClock;
        use std::process::Command;
        let dir = std::env::temp_dir().join(format!("nbatv-audio-dev-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let wav = dir.join("silence.wav");
        assert!(Command::new("ffmpeg")
            .args([
                "-y",
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "anullsrc=r=44100:cl=mono"
            ])
            .args(["-t", "30"])
            .arg(&wav)
            .status()
            .expect("spawn ffmpeg")
            .success());

        let mut audio = AudioPlayback::open(&wav.to_string_lossy(), Duration::ZERO)
            .expect("open default output device");
        std::thread::sleep(Duration::from_millis(700));
        let playing = audio.position();
        assert!(
            playing > Duration::from_millis(200),
            "clock advanced: {playing:?}"
        );

        audio.pause();
        std::thread::sleep(Duration::from_millis(100)); // let the callback see it
        let frozen = audio.position();
        std::thread::sleep(Duration::from_millis(400));
        assert_eq!(audio.position(), frozen, "paused clock must not move");

        audio.seek(Duration::from_secs(10));
        assert_eq!(audio.position(), Duration::from_secs(10));
        audio.resume();
        std::thread::sleep(Duration::from_millis(700));
        let after = audio.position();
        assert!(
            after > Duration::from_secs(10),
            "resumed after seek: {after:?}"
        );
        assert!(after < Duration::from_secs(12), "no runaway: {after:?}");
        drop(audio);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
