//! Repro for the NYK @ TRH 1946-11-01 report: pressing Play on a Playable
//! game must start a Lane A session in the Player Backend. Red before the
//! Lane A effect slice lands: `press_play` records `PlayProgressive` but
//! spawns nothing and the Game view shows nothing.

use nbatv_shell::{PlayDispatch, Route, ShellApp};

#[test]
fn play_on_progressive_game_starts_lane_a_session() {
    let mut app = ShellApp::new();
    app.navigate(Route::Game {
        game_id: "194611010TRH".into(),
    });
    app.press_play("194611010TRH");
    assert!(
        matches!(
            app.last_dispatch(),
            Some(PlayDispatch::PlayProgressive { .. })
        ),
        "fixture must still resolve progressive"
    );
    assert!(
        app.lane_a_status().is_some(),
        "BUG: Play recorded PlayProgressive but no Lane A session started"
    );
}

#[test]
fn lane_a_session_retires_on_navigate() {
    let mut app = ShellApp::new();
    app.press_play("194611010TRH");
    app.navigate(Route::Game {
        game_id: "194704160BOS".into(),
    });
    assert!(
        app.lane_a_status().is_none(),
        "leaving the game must retire the Lane A session"
    );
}

#[test]
fn non_progressive_play_retires_lane_a() {
    let mut app = ShellApp::new();
    app.press_play("194611010TRH");
    // Pointer game: no decodable bytes, so no Lane A session may survive.
    app.press_play("194711150BOS");
    assert!(app.lane_a_status().is_none());
}

fn ffmpeg_present() -> bool {
    std::process::Command::new("ffmpeg")
        .arg("-version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

struct TempDir(std::path::PathBuf);

impl TempDir {
    fn create(tag: &str) -> std::io::Result<Self> {
        let dir =
            std::env::temp_dir().join(format!("nbatv-shell-lane-a-{}-{tag}", std::process::id()));
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

/// Synthesize 1s of 32x32/5fps testsrc: native size differs from the Lane A
/// extent on purpose, proving every game's size normalizes.
fn make_synth_mp4(dir: &TempDir) -> std::path::PathBuf {
    let mp4 = dir.0.join("synth.mp4");
    let status = std::process::Command::new("ffmpeg")
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
    mp4
}

#[test]
fn lane_a_decodes_any_native_size_to_the_end() {
    if !ffmpeg_present() {
        println!("SKIP lane_a_decodes_any_native_size_to_the_end: ffmpeg not installed");
        return;
    }
    let dir = TempDir::create("decode").expect("scratch temp dir");
    let mp4 = make_synth_mp4(&dir);
    let src = mp4.to_string_lossy().into_owned();

    let mut app = ShellApp::new();
    app.begin_lane_a(src);
    assert!(matches!(
        app.lane_a_status(),
        Some(nbatv_shell::LaneAStatus::Playing)
    ));
    while matches!(app.lane_a_status(), Some(nbatv_shell::LaneAStatus::Playing)) {
        let _ = app.advance_lane_a();
    }
    assert_eq!(app.lane_a_status(), Some(&nbatv_shell::LaneAStatus::Ended));
    assert_eq!(app.lane_a_frames_converted(), 5);
}

#[test]
fn lane_a_reports_undecodable_src_honestly() {
    if !ffmpeg_present() {
        println!("SKIP lane_a_reports_undecodable_src_honestly: ffmpeg not installed");
        return;
    }
    let mut app = ShellApp::new();
    app.begin_lane_a("/nonexistent/nbatv-no-such-tape.mp4".to_string());
    // Spawn succeeds; the failure surfaces on the first pull, fast, with
    // no network and no panic.
    assert_eq!(app.advance_lane_a(), None);
    assert!(matches!(
        app.lane_a_status(),
        Some(nbatv_shell::LaneAStatus::Error(_))
    ));
    assert_eq!(app.lane_a_frames_converted(), 0);
}

#[test]
fn lane_a_pause_holds_and_resume_continues() {
    let mut app = ShellApp::new();
    app.begin_lane_a("/nonexistent/nbatv-pause-probe.mp4".to_string());
    app.set_lane_a_paused(true);
    assert_eq!(app.lane_a_status(), Some(&nbatv_shell::LaneAStatus::Paused));
    assert!(app.lane_a_paused());
    app.set_lane_a_paused(false);
    assert_eq!(
        app.lane_a_status(),
        Some(&nbatv_shell::LaneAStatus::Playing)
    );
}

#[test]
fn lane_a_restart_replays_from_zero() {
    if !ffmpeg_present() {
        println!("SKIP lane_a_restart_replays_from_zero: ffmpeg not installed");
        return;
    }
    let dir = TempDir::create("restart").expect("scratch temp dir");
    let mp4 = make_synth_mp4(&dir);
    let src = mp4.to_string_lossy().into_owned();

    let mut app = ShellApp::new();
    app.begin_lane_a(src);
    let _ = app.advance_lane_a();
    assert!(app.lane_a_frames_converted() >= 1);
    app.restart_lane_a();
    assert_eq!(app.lane_a_frames_converted(), 0);
    assert!(matches!(
        app.lane_a_status(),
        Some(nbatv_shell::LaneAStatus::Playing)
    ));
}
