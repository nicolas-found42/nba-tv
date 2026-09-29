//! Cache Tier normalize round-trip, run through a real `ffmpeg` (#14 T2:
//! "normalize round-trip verified moov-first").
//!
//! 1. Synthesize a small MP4 whose `moov` atom is at the END (ffmpeg's
//!    default, no `+faststart`) and prove that input is *not* moov-first,
//!    so the assertion below can actually fail.
//! 2. Run exactly [`normalize_args`] through ffmpeg.
//! 3. Read the output bytes, locate `moov`/`mdat`, and assert moov-first.
//!
//! Skips (prints `SKIP ...`, passes) when `ffmpeg` is not installed, so CI
//! images without it stay green. Media is synthetic and lives in a scratch
//! dir removed on drop; nothing is downloaded.

use std::path::{Path, PathBuf};
use std::process::Command;

use nbatv_player::{
    is_moov_first, mp4_top_level_offsets, normalize_args, supports_range, Mp4RangeReadiness,
};

fn ffmpeg_present() -> bool {
    Command::new("ffmpeg")
        .arg("-version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Unique scratch dir, removed on drop (panics clean up too).
struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("nbatv-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        Scratch(dir)
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn ffmpeg(args: &[String]) {
    let out = Command::new("ffmpeg")
        .args(["-y", "-v", "error"])
        .args(args)
        .output()
        .expect("spawn ffmpeg");
    assert!(
        out.status.success(),
        "ffmpeg {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn offsets(path: &Path) -> (Option<u64>, Option<u64>) {
    mp4_top_level_offsets(&std::fs::read(path).expect("read mp4"))
}

#[test]
fn normalize_args_turn_a_moov_last_mp4_into_a_moov_first_one() {
    if !ffmpeg_present() {
        println!(
            "SKIP normalize_args_turn_a_moov_last_mp4_into_a_moov_first_one: ffmpeg not installed"
        );
        return;
    }
    let dir = Scratch::new("norm-rt");

    // Non-faststart source: ffmpeg's default MP4 muxing writes moov last.
    let src = dir.path("src.mp4");
    ffmpeg(&[
        "-f".into(),
        "lavfi".into(),
        "-i".into(),
        "testsrc=duration=2:size=64x64:rate=10".into(),
        "-c:v".into(),
        "libx264".into(),
        "-pix_fmt".into(),
        "yuv420p".into(),
        src.to_string_lossy().into_owned(),
    ]);
    let (src_moov, src_mdat) = offsets(&src);
    assert!(
        src_moov.is_some() && src_mdat.is_some(),
        "fixture must hold moov + mdat, got {src_moov:?}/{src_mdat:?}"
    );
    assert!(
        !is_moov_first(src_moov, src_mdat),
        "fixture must NOT be moov-first or the test proves nothing \
         (moov {src_moov:?}, mdat {src_mdat:?})"
    );

    // The exact Cache Tier contract vector, through a real ffmpeg.
    let out = dir.path("normalized.mp4");
    ffmpeg(&normalize_args(
        &src.to_string_lossy(),
        &out.to_string_lossy(),
    ));
    let (moov, mdat) = offsets(&out);
    assert!(
        moov.is_some() && mdat.is_some(),
        "normalized MP4 must hold moov + mdat, got {moov:?}/{mdat:?}"
    );
    assert!(
        is_moov_first(moov, mdat),
        "+faststart must put moov before mdat (moov {moov:?}, mdat {mdat:?})"
    );

    let readiness = Mp4RangeReadiness {
        moov_first: is_moov_first(moov, mdat),
        accepts_range: supports_range(Some("bytes")),
    };
    assert!(readiness.is_progressive_ready());
    println!(
        "normalize round-trip: input moov@{src_moov:?} mdat@{src_mdat:?} (not moov-first) \
         -> output moov@{moov:?} mdat@{mdat:?} (moov-first)"
    );
}
