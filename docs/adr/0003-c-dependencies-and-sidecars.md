# ADR 0003 — C dependencies and sidecar binaries

Date: 2026-09-29 · Map nicolas-found42/nba-tv#1 (amends [ADR 0001](0001-workspace-layout.md); decisions #2/#5/#10)

## Context

ADR 0001 declared the workspace pure Rust with exactly two exceptions (the
`ffmpeg` sidecar and the `wry` webview). The code has since grown two things
that rule did not name:

- `rusqlite` with `features = ["bundled"]`, which compiles the C SQLite
  amalgamation. Declared in `nbatv_db`, `nbatv_catalog`, `nbatv_ingest` and
  `nbatv_shell`.
- More user-provisioned sidecar processes than `ffmpeg`: `curl`, `rclone`,
  `yt-dlp`, and `ffprobe`.

Leaving them undocumented made ADR 0001 false and hid a real decision: what
counts as acceptable non-Rust surface in a $0, offline-buildable repo.

## Decision

Non-Rust surface is allowed only in the forms below. Anything else needs a
new ADR (unchanged from ADR 0001's rule for C decode or HLS stacks).

### Exception 3: SQLite via `rusqlite` `bundled`

The archive is a SQLite file. There is no $0, pure-Rust SQLite that reads and
writes the same file format, and a hand-rolled store would fork the format
the driver already has on disk. `bundled` also lets a fresh clone build
offline with no system libraries (ADR 0002's "buildable and testable
offline"). This is the only C code compiled into the workspace.

Target shape: `rusqlite` is declared once, in `[workspace.dependencies]`,
and inherited by member crates; `nbatv_db` is the only crate that opens
connections and owns SQL. Today several crates still declare `rusqlite`
directly and open connections themselves; consolidating that is a follow-up
refactor tracked by the code PRs, not part of this docs change.

### Sidecar binaries

Sidecars sit on the same footing as `ffmpeg` (ADR 0001, Lane A): the user
provisions them, workspace code spawns them as processes, and nothing is
linked. Each is a process boundary, faked in tests so the suite stays
offline.

| Binary | Used by | For |
|---|---|---|
| `ffmpeg` | `nbatv_player` (Lane A) | decode frames for in-window playback |
| `ffprobe` | `nbatv_catalog` (`fetch.rs`) | duration check on fetched Cache Tier files |
| `curl` | `nbatv_catalog` (`fetch.rs`, `ia_probe.rs`), `nbatv_ingest` (`nbatv-crawl`) | HTTP transport with an identifying user agent |
| `rclone` | `nbatv_catalog` (`drive.rs`) | the optional Drive mirror stage: `rclone copy` only, never `sync`/`move`/delete |
| `yt-dlp` | `nbatv_catalog` (`ytdlp_probe.rs`) | metadata-only search: `--flat-playlist --dump-json` |

`yt-dlp` is a search probe, never a stream extractor. It returns candidate
metadata that becomes pointer/embed rows; it does not fetch media bytes. The
ban from #10 on extracting streams from vendor players is unaffected.

No sidecar is required to build or test. Only the features that call one
need it at run time, and the app degrades honestly without it (ADR 0002).

## Consequences

- "Pure Rust" is no longer claimed. The accurate statement is: Rust
  workspace, one bundled C library (SQLite), and user-provisioned sidecar
  processes that are never linked.
- README's "you provision" list names the sidecars. A new sidecar needs a row
  in the table above.
- Adding another compiled C dependency, or a sidecar that extracts streams,
  needs a new ADR.
- The single-owner SQLite target gives the C surface one place to audit and
  one place to change the schema.
