//! Repro for normalized Lane A decode: every game's tape has a different
//! native size, but the rawvideo pipe carries no header, so the shell must
//! decode at one fixed extent. Red before the scaled builders land.

use nbatv_player::{play_scaled_args, seek_scaled_args};
#[test]
fn scaled_args_force_the_declared_extent() {
    let args = play_scaled_args("tape.mp4", 640, 360);
    assert!(
        args.windows(2).any(|w| w == ["-vf", "scale=640:360"]),
        "scaled play args must force the extent, got {args:?}"
    );
    assert_eq!(args.last().map(String::as_str), Some("-"));
}

#[test]
fn scaled_seek_keeps_input_seek_and_extent() {
    let args = seek_scaled_args("tape.mp4", 10.0, 640, 360);
    let ss = args.iter().position(|a| a == "-ss").expect("-ss present");
    let input = args.iter().position(|a| a == "-i").expect("-i present");
    assert!(ss + 2 == input, "input seek shape must hold, got {args:?}");
    assert!(
        args.windows(2).any(|w| w == ["-vf", "scale=640:360"]),
        "scaled seek args must force the extent, got {args:?}"
    );
}
