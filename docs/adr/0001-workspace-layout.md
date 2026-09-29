# ADR 0001 — One workspace, six crates, Rust-first

Date: 2026-09-07 · Map nicolas-found42/nba-tv#1 (decisions #2/#3/#5)

Amended 2026-09-08: `nbatv_catalog` joins as the sixth crate (issue #18's
sweep pipeline: SourceProbe port, TapeCatalog orchestration) — same purity
rule, same ownership pattern; the shell consumes it for sweep verdicts and
the review list.

Amended 2026-09-29: the "pure Rust, exactly two exceptions" rule was stale.
Bundled SQLite and several user-provisioned sidecar processes are also in
use; they are recorded in [ADR 0003](0003-c-dependencies-and-sidecars.md),
which supersedes the purity wording below. The crate-dependency and
binary statements are corrected in place.

## Context

The app has six separable jobs (store, ingest, browse, play, find tape,
sweep sources for tape) that
share vocabulary (Season, Team, Game, Schedule, Box Score, Game Tape) and
contract types (`game_id` as the Basketball-Reference box-score slug;
`tape_sources(game_id, rank, source_class, url_or_pointer, match_confidence,
verified_at)`). Research verdicts fix the hard boundaries: streaming-first
with a bounded Cache Tier (#2), Basketball-Reference backbone + NBA.com
enrichment for Schedule and Box Score (#3), two playback lanes (#5/#10).

## Decision

One Cargo workspace, six crates: `nbatv_db`, `nbatv_ingest`, `nbatv_shell`,
`nbatv_player`, `nbatv_ladder`, `nbatv_catalog` (ownership per the README
"Workspace layout" table). One `resolver = "2"` workspace, shared
`[workspace.lints]`, no `default-run` (the root is a virtual manifest, so
each binary is run explicitly with `-p`/`--bin`).

Layering intent: `nbatv_db` owns the schema and the contract types and
depends on no other workspace crate. `nbatv_ladder` and `nbatv_player` are
dependency-free leaves (no workspace or third-party dependencies).
`nbatv_catalog` builds on `nbatv_db` and `nbatv_ladder`; `nbatv_ingest`
writes the archive through `nbatv_db`; `nbatv_shell` composes the rest.
Nothing depends on `nbatv_shell`.

Binaries live with the crate that owns the job: the Shell binary in
`nbatv_shell`, the `nbatv-crawl` and `nbatv-ingest` drivers in
`nbatv_ingest`, and `nbatv-catalog-runner` in `nbatv_catalog`. There is no
designated flagship binary.

Workspace code is Rust. Non-Rust code enters only at process boundaries or
as the single bundled-SQLite exception, and every such case is listed in
[ADR 0003](0003-c-dependencies-and-sidecars.md). Two exceptions were
recorded here originally and remain in force:

- **Lane A:** the `ffmpeg` *sidecar binary* (user-provisioned) behind a
  sidecar binding — decode frames become textures; no C codec compiled in.
- **Lane B:** the `wry` webview hosting a vendor's sanctioned embed player
  (YouTube-class External Surfaces) — pixels live in the webview.

## Consequences

- Contract types are owned by `nbatv_db`; crates that read or write the
  archive (`nbatv_catalog`, `nbatv_ingest`, `nbatv_shell`) depend on it
  instead of duplicating them. `nbatv_ladder` and `nbatv_player` keep their
  own small types and stay free of dependencies. New shared contract types
  go in `nbatv_db`, not into a second copy.
- Adding a C decode dependency (e.g. `openh264`-style bindings) or an HLS
  stack would need a new ADR: #2 proved no $0 source serves HLS, so the
  dependency surface stays small by construction.
