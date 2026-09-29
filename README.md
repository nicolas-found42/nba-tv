# nba-tv — personal NBA archive

A personal archive of NBA games: the **Schedule** and result of each **Game**,
with **Game Tape** where tape exists. The **Box Score** is always shown; only
tape can be unavailable.

- **Schedule + Box Score: always.** Every Season from 1946–47 to the present,
  every Team (active and defunct), every Game.
- **Game Tape: best-effort.** Each Game resolves its Tape Sources through the
  fixed Source Ladder at play time. When every rung is exhausted, the Game is
  honestly marked unavailable (or pointer-only) — the Box Score still renders.

The whole project is **$0**: no paid tiers, no paid services, no purchased data.

## What ships vs what you provision

The repo ships:

- Code (a Rust workspace; SQLite is compiled in via `rusqlite`'s `bundled`
  feature, see [ADR 0003](docs/adr/0003-c-dependencies-and-sidecars.md)) plus
  an empty database schema.
- **NO media, NO keys, NO bulk data.** Grey sources are recorded as pointers
  only; the repo contains zero downloaded tape and zero credentials.

You provision:

- An `ffmpeg` binary (decode sidecar for the Player Backend), and `ffprobe`
  (duration check on fetched tape). Both ship with ffmpeg.
- `curl` (HTTP transport for the crawl and the source probes).
- Optionally `yt-dlp` (metadata-only YouTube search probe) and `rclone` (the
  optional Drive mirror stage). Sidecars are user-provisioned processes,
  never linked; see ADR 0003.
- A network connection (sources resolve at play time; ingest fetches run rarely).
- Optionally, a personal file-store account for the bounded Cache Tier
  (favorites / repeated-viewing cache). The app is fully functional with zero
  cache usage — the cache is an accelerator, never a dependency.

## Personal-use-only data terms

- **Basketball-Reference:** fetch gently (≥ 3.5 s between requests, crawlable
  paths only), personal use only, **never republished** — neither the fetched
  pages nor any database derived from them.
- **NBA.com stats content:** frozen personal archive only (fetched once per
  finished Season, never redistributed). The Shell footer carries attribution
  lines for both sources:
  - Schedule and Box Score data courtesy of Sports-Reference.
  - NBA statistics courtesy of NBA.com, used for private non-commercial purposes.

## Workspace layout

| Crate | Owns |
|---|---|
| `crates/nbatv_db` | SQLite schema + storage (Seasons, Teams, Games, Box Scores, Tape Sources) |
| `crates/nbatv_ingest` | Crawl → snapshot → normalize → store pipeline for Schedules and Box Scores (`nbatv-crawl`, `nbatv-ingest`) |
| `crates/nbatv_shell` | App window and all browse screens (Home → Season → Team → Game) |
| `crates/nbatv_player` | Player Backend: the two playback lanes (ffmpeg-sidecar, embed) |
| `crates/nbatv_ladder` | Source Ladder: per-Game Tape Source search order + pointer catalog |
| `crates/nbatv_catalog` | TapeCatalog sweep: per-rung source probes, scoring, Cache Tier fetch, optional Drive mirror (`nbatv-catalog-runner`) |

Decisions behind this shape: `docs/adr/0001-workspace-layout.md`,
`docs/adr/0002-no-media-in-repo.md`,
`docs/adr/0003-c-dependencies-and-sidecars.md`. Domain language: `CONTEXT.md`.

## Running it

The root `Cargo.toml` is a virtual manifest, so name the package or binary.
Local data lives under `data/` (git-ignored, see ADR 0002):

| Path | Holds |
|---|---|
| `data/raw/br/<season>/` | Basketball-Reference snapshots written by `nbatv-crawl` |
| `data/archive.db` | SQLite archive written by `nbatv-ingest`, read by the Shell |
| `data/cache/` | Cache Tier local tape copies |
| `data/webview-profile/` | Lane B webview profile (vendor sign-in state) |

```sh
# 1. Snapshot Basketball-Reference (resume-safe; --dry-run lists requests, no network)
cargo run -q -p nbatv_ingest --bin nbatv-crawl -- --dry-run
cargo run -q -p nbatv_ingest --bin nbatv-crawl -- [--from ENDING] [--to ENDING] [--workers N] [--interval SECS]

# 2. Normalize the snapshots into the archive db (idempotent)
cargo run -q -p nbatv_ingest --bin nbatv-ingest -- --raw data/raw/br --db data/archive.db

# 3. Open the Shell on data/archive.db
cargo run -p nbatv_shell

# Optional: headless sweep -> fetch -> mirror over a season range (dry-run by default)
cargo run -q -p nbatv_catalog --bin nbatv-catalog-runner -- 1946-47 1950-51
```

Tests need no network or media: `cargo test --workspace`.

## Status

Delivered without a ticket of their own: the Basketball-Reference crawl
driver `nbatv-crawl` (including `--workers`), the snapshot ingest
`nbatv-ingest`, and the archive schema, Source Ladder and player lanes.

Known deferrals (see the open issues on
[#1](https://github.com/nicolas-found42/nba-tv/issues/1) for current state):

- **stats.nba.com enrichment** is deferred per #15: `games.nba_game_id` is
  NULL for every ingested Game today.
- **Lane A audio** is not implemented; Lane A decodes video frames only.
- **Lane B** resize/DPI tracking and the transport chrome are pending
  verification on the driver machine.
- **Source Ladder probes**: which rungs have a live probe is still moving;
  see the issues rather than this file.
