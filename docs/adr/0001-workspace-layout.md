# ADR 0001 — One workspace, five crates, pure Rust

Date: 2026-09-07 · Map nicolas-found42/nba-tv#1 (decisions #2/#3/#5)

## Context

The app has five separable jobs (store, ingest, browse, play, find tape) that
share vocabulary (Season, Team, Game, Schedule, Box Score, Game Tape) and
contract types (`game_id` as the Basketball-Reference box-score slug;
`tape_sources(game_id, rank, source_class, url_or_pointer, match_confidence,
verified_at)`). Research verdicts fix the hard boundaries: streaming-first
with a bounded Cache Tier (#2), Basketball-Reference backbone + NBA.com
enrichment for Schedule and Box Score (#3), two playback lanes (#5/#10).

## Decision

One Cargo workspace, five crates: `nbatv_db`, `nbatv_ingest`, `nbatv_shell`,
`nbatv_player`, `nbatv_ladder` (ownership per README layout table). One
`resolver = "2"` workspace, shared `[workspace.lints]`, no `default-run`
(no flagship binary yet — the Shell binary arrives with its crate).

Workspace code is **pure Rust**. Exactly two exceptions, both at the process
boundary, never as linked C in workspace code:

- **Lane A:** the `ffmpeg` *sidecar binary* (user-provisioned) behind a
  sidecar binding — decode frames become textures; no C codec compiled in.
- **Lane B:** the `wry` webview hosting a vendor's sanctioned embed player
  (YouTube-class External Surfaces) — pixels live in the webview.

## Consequences

- Contract types are duplicated locally per crate for now; integration dedups
  onto `nbatv_db` later. Slices must not touch each other's paths.
- Adding a C decode dependency (e.g. `openh264`-style bindings) or an HLS
  stack would need a new ADR: #2 proved no $0 source serves HLS, so the
  dependency surface stays small by construction.
