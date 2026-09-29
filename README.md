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

- Code (pure-Rust workspace) plus an empty database schema.
- **NO media, NO keys, NO bulk data.** Grey sources are recorded as pointers
  only; the repo contains zero downloaded tape and zero credentials.

You provision:

- An `ffmpeg` binary (decode sidecar for the Player Backend).
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
| `crates/nbatv_ingest` | Fetch → normalize → store pipeline for Schedules and Box Scores |
| `crates/nbatv_shell` | App window and all browse screens (Home → Season → Team → Game) |
| `crates/nbatv_player` | Player Backend: the two playback lanes (ffmpeg-sidecar, embed) |
| `crates/nbatv_ladder` | Source Ladder: per-Game Tape Source search order + pointer catalog |

Decisions behind this shape: `docs/adr/0001-workspace-layout.md`,
`docs/adr/0002-no-media-in-repo.md`. Domain language: `CONTEXT.md`.
