//! Lane A executable pump: spawn the sidecar `ffmpeg` decode pipe and read
//! RGBA frames off stdout.
//!
//! [`Pump`] owns the child process built from the [`crate::lane_a`]
//! argument vectors ([`crate::lane_a::play_pipe_args`] for play,
//! [`crate::lane_a::seek_play_args`] for a seek restart). Frames arrive as
//! [`crate::lane_a::RawFrame`]s of a caller-declared extent; the pump reads
//! exactly `width * height * 4` bytes per frame. Seeking drops the current
//! child and respawns with the seek vector, so the shell never owns process
//! plumbing — it just calls [`Pump::next_frame`] and [`Pump::seek`].
//!
//! No media is downloaded here and no window or GPU context is touched.
//! Tests synthesize tiny fixtures with
//! `ffmpeg -f lavfi -i testsrc=duration=1:size=32x32:rate=5` into a unique
//! [`std::env::temp_dir`] subdirectory (removed on drop) and skip gracefully
//! when no `ffmpeg` binary is installed.

use std::io::Read;
use std::process::{Child, ChildStdout, Command, Stdio};

use crate::lane_a::{
    play_pipe_args, play_scaled_args, seek_play_args, seek_scaled_args, RawFrame, LANE_A_HEIGHT,
    LANE_A_WIDTH,
};

/// Why a [`Pump`] could not be opened or re-seeked.
#[derive(Debug)]
pub enum PumpError {
    /// The declared extent has a zero side; a frame would hold no pixels.
    ZeroExtent,
    /// `width * height * 4` overflows `usize` on this platform.
    FrameTooLarge,
    /// The sidecar `ffmpeg` process could not be spawned.
    Spawn(std::io::Error),
}

impl std::fmt::Display for PumpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PumpError::ZeroExtent => write!(f, "pump extent must be non-zero"),
            PumpError::FrameTooLarge => write!(f, "pump frame size overflows usize"),
            PumpError::Spawn(err) => write!(f, "could not spawn ffmpeg sidecar: {err}"),
        }
    }
}

impl std::error::Error for PumpError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            PumpError::Spawn(err) => Some(err),
            _ => None,
        }
    }
}

/// Executable Lane A pump: owns the sidecar decode child and yields frames.
///
/// [`frames_yielded`](Pump::frames_yielded) counts frames read from the
/// current child only; [`seek`](Pump::seek) resets it alongside the respawn.
pub struct Pump {
    child: Child,
    stdout: ChildStdout,
    src: String,
    width: u32,
    height: u32,
    frame_len: usize,
    frames_yielded: u64,
    /// Whether the extent filter is applied: `false` for the unscaled
    /// builders, `true` for the scaled ones. A seek must emit the same
    /// frame size as the original spawn, or the reader mis-frames every
    /// subsequent frame.
    scaled: bool,
}

impl Pump {
    /// Start decoding `src` from the beginning at the declared extent.
    ///
    /// Spawns `ffmpeg` with [`play_pipe_args`](crate::lane_a::play_pipe_args).
    /// Returns [`PumpError::ZeroExtent`] or [`PumpError::FrameTooLarge`]
    /// without spawning when the extent is unusable.
    pub fn open(src: &str, width: u32, height: u32) -> Result<Self, PumpError> {
        let frame_len = checked_frame_len(width, height)?;
        let (child, stdout) = spawn_child(&play_pipe_args(src))?;
        Ok(Pump {
            child,
            stdout,
            src: src.to_string(),
            width,
            height,
            frame_len,
            frames_yielded: 0,
            scaled: false,
        })
    }

    /// Start decoding `src` at the normalized Lane A extent
    /// ([`LANE_A_WIDTH`] x [`LANE_A_HEIGHT`]).
    ///
    /// The tape's native size is unknown up front (every ladder tape
    /// differs), so this forces the extent through the scale filter: the
    /// reader is correct by construction for every game. This is the
    /// constructor the shell's Play path uses.
    pub fn open_lane_a(src: &str) -> Result<Self, PumpError> {
        Self::open_scaled(src, LANE_A_WIDTH, LANE_A_HEIGHT)
    }

    /// Start decoding `src` at a forced `width` x `height` extent.
    ///
    /// Same contract as [`Pump::open`], but the sidecar scales every input
    /// frame to the declared extent first, so callers that do not know the
    /// native size (the shell, for every game) still frame the pipe
    /// correctly.
    pub fn open_scaled(src: &str, width: u32, height: u32) -> Result<Self, PumpError> {
        let frame_len = checked_frame_len(width, height)?;
        let (child, stdout) = spawn_child(&play_scaled_args(src, width, height))?;
        Ok(Pump {
            child,
            stdout,
            src: src.to_string(),
            width,
            height,
            frame_len,
            frames_yielded: 0,
            scaled: true,
        })
    }

    /// Restart decoding at `seconds`, dropping the current child.
    ///
    /// Respawns with the same builder family as the original spawn
    /// (input seek: `-ss` before `-i`) and resets
    /// [`frames_yielded`](Pump::frames_yielded) to zero. Previously yielded
    /// frames are unaffected.
    pub fn seek(&mut self, seconds: f64) -> Result<(), PumpError> {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let args = if self.scaled {
            seek_scaled_args(&self.src, seconds, self.width, self.height)
        } else {
            seek_play_args(&self.src, seconds)
        };
        let (child, stdout) = spawn_child(&args)?;
        self.child = child;
        self.stdout = stdout;
        self.frames_yielded = 0;
        Ok(())
    }

    /// Read the next decoded frame, or `None` at end of stream.
    ///
    /// Reads exactly `width * height * 4` bytes from the sidecar pipe; a
    /// short read (clean EOF or a truncated tail frame) ends iteration and
    /// the partial bytes are discarded, never surfaced as a frame.
    pub fn next_frame(&mut self) -> Option<RawFrame> {
        let mut rgba = vec![0u8; self.frame_len];
        self.stdout.read_exact(&mut rgba).ok()?;
        self.frames_yielded += 1;
        Some(RawFrame {
            width: self.width,
            height: self.height,
            rgba,
        })
    }

    /// Declared decode extent, in pixels.
    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Frames yielded from the current child (reset by [`seek`](Pump::seek)).
    pub fn frames_yielded(&self) -> u64 {
        self.frames_yielded
    }
}

impl Iterator for Pump {
    type Item = RawFrame;

    fn next(&mut self) -> Option<RawFrame> {
        self.next_frame()
    }
}

impl Drop for Pump {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Validate the declared extent and return the per-frame byte count.
fn checked_frame_len(width: u32, height: u32) -> Result<usize, PumpError> {
    if width == 0 || height == 0 {
        return Err(PumpError::ZeroExtent);
    }
    (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or(PumpError::FrameTooLarge)
}

/// Spawn `ffmpeg` with a prebuilt [`crate::lane_a`] argument vector.
///
/// Stdout is piped for frame reads; stderr is piped and drained on a detached
/// thread so sidecar progress logs can never fill the pipe and stall the
/// decode. Stdin is null so the child never blocks on terminal input.
fn spawn_child(args: &[String]) -> Result<(Child, ChildStdout), PumpError> {
    let mut child = Command::new("ffmpeg")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(PumpError::Spawn)?;
    let stdout = child
        .stdout
        .take()
        .expect("ffmpeg stdout was requested piped");
    let mut stderr = child
        .stderr
        .take()
        .expect("ffmpeg stderr was requested piped");
    std::thread::spawn(move || {
        let mut sink = [0u8; 8192];
        loop {
            match stderr.read(&mut sink) {
                Ok(0) => break,
                Ok(_) => {}
                Err(_) => break,
            }
        }
    });
    Ok((child, stdout))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Unique scratch dir under the system temp dir, removed on drop.
    struct TempDir(std::path::PathBuf);

    impl TempDir {
        fn create(tag: &str) -> std::io::Result<Self> {
            let dir = std::env::temp_dir().join(format!("nbatv-pump-{}-{tag}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir)?;
            Ok(TempDir(dir))
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn ffmpeg_present() -> bool {
        Command::new("ffmpeg")
            .arg("-version")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    /// Synthesize the shared fixture: 1s of `testsrc` at 32x32/5fps, i.e.
    /// exactly 5 frames of 32x32 RGBA on the decode pipe. Panics when ffmpeg
    /// exists but synthesis fails; callers skip beforehand when absent.
    fn make_synth_mp4(dir: &TempDir) -> std::path::PathBuf {
        let mp4 = dir.0.join("synth.mp4");
        let status = Command::new("ffmpeg")
            .args([
                "-y",
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc=duration=1:size=32x32:rate=5",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
            ])
            .arg(&mp4)
            .status()
            .expect("spawn ffmpeg to synthesize fixture");
        assert!(status.success(), "fixture synthesis must succeed");
        assert!(mp4.is_file(), "synthesized fixture must exist");
        mp4
    }

    #[test]
    fn rejects_zero_extent_without_spawning() {
        assert!(matches!(
            Pump::open("tape.mp4", 0, 32),
            Err(PumpError::ZeroExtent)
        ));
        assert!(matches!(
            Pump::open("tape.mp4", 32, 0),
            Err(PumpError::ZeroExtent)
        ));
    }

    #[test]
    fn rejects_overflowing_frame_bytes() {
        assert!(matches!(
            Pump::open("tape.mp4", u32::MAX, u32::MAX),
            Err(PumpError::FrameTooLarge)
        ));
    }

    #[test]
    fn pump_yields_declared_extent_frames() {
        if !ffmpeg_present() {
            println!("SKIP pump_yields_declared_extent_frames: ffmpeg not installed");
            return;
        }
        let dir = TempDir::create("full").expect("scratch temp dir");
        let mp4 = make_synth_mp4(&dir);
        let src = mp4.to_string_lossy().into_owned();

        let mut pump = Pump::open(&src, 32, 32).expect("spawn lane A pump");
        assert_eq!(pump.dimensions(), (32, 32));
        let mut count = 0u64;
        while let Some(frame) = pump.next_frame() {
            assert_eq!(frame.width, 32);
            assert_eq!(frame.height, 32);
            assert_eq!(frame.rgba.len(), 32 * 32 * 4);
            count += 1;
        }
        assert_eq!(
            count, 5,
            "testsrc duration=1 rate=5 must decode to exactly 5 frames"
        );
        assert_eq!(pump.frames_yielded(), 5);
    }

    #[test]
    fn seek_restarts_at_offset() {
        if !ffmpeg_present() {
            println!("SKIP seek_restarts_at_offset: ffmpeg not installed");
            return;
        }
        let dir = TempDir::create("seek").expect("scratch temp dir");
        let mp4 = make_synth_mp4(&dir);
        let src = mp4.to_string_lossy().into_owned();

        let mut pump = Pump::open(&src, 32, 32).expect("spawn lane A pump");
        let mut full_first: Option<Vec<u8>> = None;
        let mut full_count = 0u64;
        while let Some(frame) = pump.next_frame() {
            if full_first.is_none() {
                full_first = Some(frame.rgba.clone());
            }
            full_count += 1;
        }
        assert_eq!(full_count, 5);

        pump.seek(0.5).expect("seek respawns the sidecar");
        assert_eq!(
            pump.frames_yielded(),
            0,
            "seek resets the per-position frame counter"
        );
        let mut seek_first: Option<Vec<u8>> = None;
        let mut seek_count = 0u64;
        while let Some(frame) = pump.next_frame() {
            if seek_first.is_none() {
                seek_first = Some(frame.rgba.clone());
            }
            seek_count += 1;
        }
        assert!(
            seek_count >= 1,
            "seeked pump must yield frames again, got {seek_count}"
        );
        assert!(
            seek_count <= full_count,
            "offset restart must not replay more than the full stream: {seek_count} <= {full_count}"
        );
        assert_ne!(
            seek_first.expect("seeked stream has a first frame"),
            full_first.expect("full stream has a first frame"),
            "offset restart must not replay the t=0 frame"
        );
    }
    #[test]
    fn scaled_pump_normalizes_any_native_size() {
        if !ffmpeg_present() {
            println!("SKIP scaled_pump_normalizes_any_native_size: ffmpeg not installed");
            return;
        }
        // The synth fixture is 32x32 native; a scaled open at 64x48 must
        // still frame the pipe correctly — this is the every-game path
        // (unknown native size in, fixed extent out).
        let dir = TempDir::create("scaled").expect("scratch temp dir");
        let mp4 = make_synth_mp4(&dir);
        let src = mp4.to_string_lossy().into_owned();

        let mut pump = Pump::open_scaled(&src, 64, 48).expect("spawn scaled pump");
        assert_eq!(pump.dimensions(), (64, 48));
        let mut count = 0u64;
        while let Some(frame) = pump.next_frame() {
            assert_eq!((frame.width, frame.height), (64, 48));
            assert_eq!(frame.rgba.len(), 64 * 48 * 4);
            count += 1;
        }
        assert_eq!(count, 5, "scaled decode must yield all 5 synth frames");

        // A seek on a scaled pump must keep the forced extent.
        pump.seek(0.5).expect("scaled seek respawns the sidecar");
        let frame = pump.next_frame().expect("scaled seek yields frames");
        assert_eq!((frame.width, frame.height), (64, 48));
        assert_eq!(frame.rgba.len(), 64 * 48 * 4);
    }
}
