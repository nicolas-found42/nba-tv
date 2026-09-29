//! Media clocks: the time base Lane A paces frames against.
//!
//! ## Master-clock design
//!
//! Decoded video frames carry no timestamps on the rawvideo pipe, so the
//! pump releases frame `n` when the **media clock** reaches
//! `base + n / fps` (see [`crate::paced`]). Which clock that is depends on
//! whether audio is playing:
//!
//! - **No audio (default build, silent tape, or no output device):**
//!   [`WallClock`], a monotonic clock that advances in real time.
//! - **Audio active (feature `audio`):** the audio clock
//!   ([`crate::audio::AudioClock`]) is the master. Its position is the
//!   number of PCM frames the output device has consumed divided by the
//!   sample rate. Audio cannot skip or stretch without an audible glitch,
//!   so video follows audio: a late video frame is dropped, an early one
//!   waits. An audio underrun freezes the clock, so video waits for audio
//!   instead of running ahead of it.
//!
//! Both implement [`MediaClock`], so the pacer never knows which one it
//! drives. Time itself is injected through [`TimeSource`], which lets tests
//! substitute [`ManualTime`] and advance it by hand: no test sleeps.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// A monotonic time reading, as a duration since an arbitrary fixed epoch.
///
/// Only differences between two readings are meaningful.
pub trait TimeSource {
    /// Time elapsed since this source's epoch.
    fn now(&self) -> Duration;
}

impl<T: TimeSource + ?Sized> TimeSource for Arc<T> {
    fn now(&self) -> Duration {
        (**self).now()
    }
}

/// Real monotonic time ([`Instant`]-backed).
#[derive(Debug, Clone, Copy)]
pub struct MonotonicTime {
    epoch: Instant,
}

impl MonotonicTime {
    /// Start a monotonic time source whose epoch is "now".
    pub fn new() -> Self {
        MonotonicTime {
            epoch: Instant::now(),
        }
    }
}

impl Default for MonotonicTime {
    fn default() -> Self {
        Self::new()
    }
}

impl TimeSource for MonotonicTime {
    fn now(&self) -> Duration {
        self.epoch.elapsed()
    }
}

/// Hand-driven time for headless tests: only moves when told to.
///
/// Clones share one reading, so a test keeps a handle and advances the
/// clock a pump owns.
#[derive(Debug, Clone, Default)]
pub struct ManualTime(Arc<AtomicU64>);

impl ManualTime {
    /// A manual time source reading zero.
    pub fn new() -> Self {
        Self::default()
    }

    /// Move the reading forward by `by`.
    pub fn advance(&self, by: Duration) {
        let micros = u64::try_from(by.as_micros()).unwrap_or(u64::MAX);
        self.0.fetch_add(micros, Ordering::SeqCst);
    }
}

impl TimeSource for ManualTime {
    fn now(&self) -> Duration {
        Duration::from_micros(self.0.load(Ordering::SeqCst))
    }
}

/// The position of playback within the tape, and the transport controls
/// that move it.
pub trait MediaClock {
    /// Current media position (seconds into the tape, as a duration).
    fn position(&self) -> Duration;
    /// Freeze the position.
    fn pause(&mut self);
    /// Let the position advance again from where it froze.
    fn resume(&mut self);
    /// Jump to `to` (used with a sidecar respawn at the same offset).
    fn seek(&mut self, to: Duration);
    /// Whether the clock is frozen.
    fn is_paused(&self) -> bool;
}

/// Wall-clock [`MediaClock`]: the position advances one second per second
/// of [`TimeSource`] time while running.
///
/// Starts running at position zero.
#[derive(Debug)]
pub struct WallClock<T: TimeSource> {
    time: T,
    /// Media position at the last (re)start, or the frozen position.
    base: Duration,
    /// Time reading when the clock last started running; `None` = paused.
    running_since: Option<Duration>,
}

impl<T: TimeSource> WallClock<T> {
    /// A running clock at position zero.
    pub fn new(time: T) -> Self {
        Self::starting_at(time, Duration::ZERO)
    }

    /// A running clock at position `start`.
    pub fn starting_at(time: T, start: Duration) -> Self {
        let now = time.now();
        WallClock {
            time,
            base: start,
            running_since: Some(now),
        }
    }
}

impl<T: TimeSource> MediaClock for WallClock<T> {
    fn position(&self) -> Duration {
        match self.running_since {
            Some(since) => self.base + self.time.now().saturating_sub(since),
            None => self.base,
        }
    }

    fn pause(&mut self) {
        if self.running_since.is_some() {
            self.base = self.position();
            self.running_since = None;
        }
    }

    fn resume(&mut self) {
        if self.running_since.is_none() {
            self.running_since = Some(self.time.now());
        }
    }

    fn seek(&mut self, to: Duration) {
        self.base = to;
        if self.running_since.is_some() {
            self.running_since = Some(self.time.now());
        }
    }

    fn is_paused(&self) -> bool {
        self.running_since.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEC: Duration = Duration::from_secs(1);

    #[test]
    fn wall_clock_tracks_the_time_source() {
        let time = ManualTime::new();
        let clock = WallClock::new(time.clone());
        assert_eq!(clock.position(), Duration::ZERO);
        time.advance(SEC * 3);
        assert_eq!(clock.position(), SEC * 3);
    }

    #[test]
    fn pause_freezes_and_resume_continues_from_the_frozen_position() {
        let time = ManualTime::new();
        let mut clock = WallClock::new(time.clone());
        time.advance(SEC * 2);
        clock.pause();
        assert!(clock.is_paused());
        time.advance(SEC * 10);
        assert_eq!(clock.position(), SEC * 2, "paused clock must not advance");
        clock.resume();
        assert!(!clock.is_paused());
        time.advance(SEC);
        assert_eq!(clock.position(), SEC * 3);
    }

    #[test]
    fn pause_and_resume_are_idempotent() {
        let time = ManualTime::new();
        let mut clock = WallClock::new(time.clone());
        time.advance(SEC);
        clock.pause();
        clock.pause();
        time.advance(SEC);
        clock.resume();
        clock.resume();
        time.advance(SEC);
        assert_eq!(clock.position(), SEC * 2);
    }

    #[test]
    fn seek_moves_the_position_and_keeps_the_running_state() {
        let time = ManualTime::new();
        let mut clock = WallClock::new(time.clone());
        time.advance(SEC * 5);
        clock.seek(SEC * 100);
        assert_eq!(clock.position(), SEC * 100);
        time.advance(SEC);
        assert_eq!(clock.position(), SEC * 101);

        clock.pause();
        clock.seek(SEC * 7);
        time.advance(SEC * 4);
        assert_eq!(clock.position(), SEC * 7, "seek while paused stays paused");
        assert!(clock.is_paused());
    }

    #[test]
    fn manual_time_clones_share_one_reading() {
        let a = ManualTime::new();
        let b = a.clone();
        a.advance(Duration::from_millis(250));
        assert_eq!(b.now(), Duration::from_millis(250));
    }

    #[test]
    fn monotonic_time_never_goes_backwards() {
        let t = MonotonicTime::new();
        let first = t.now();
        assert!(t.now() >= first);
    }
}
