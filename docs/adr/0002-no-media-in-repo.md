# ADR 0002 — No media, keys, or bulk data in the repo

Date: 2026-09-07 · Map nicolas-found42/nba-tv#1 (decisions #2/#3/#4)

## Context

Full-history tape is ~60–120+ TB; no $0 host holds it, and rehosting it is
takedown-fragile (#2). Grey Tape Sources (fan rehosts, file libraries) are
unavoidable for coverage but must never become owned bytes in our hands beyond
the personal cache. Ingest inputs (Basketball-Reference pages, NBA.com JSON)
are fetched under personal-use-only terms (#3).

## Decision

1. **No video in the repo, ever.** No `.mp4`/`.mkv` (git-ignored), no bulk
   tape anywhere in git. Tests use tiny inline fixtures, never network.
2. **No keys or secrets.** No API keys, no account credentials, no `.pem`/key
   files (git-ignored), no `.env`. CI runs with no secrets.
3. **No bulk data.** Raw ingest snapshots live under `data/raw/` (git-ignored,
   local only) so re-normalization never re-fetches; the repo ships code plus
   an empty schema.
4. **Grey sources are pointers only.** The Source Ladder records
   `url_or_pointer` + `match_confidence` per `(game_id, rank)`; rungs 5–7
   (collector catalogs, purchase-only, institutional) only ever produce
   pointers. Nothing grey is downloaded into the repo.
5. **The Cache Tier is bounded and outside the repo.** It holds only
   downloadable, re-findable Games normalized to faststart MP4 — never
   stream-only surfaces (official NBA free tier, YouTube), which play in place
   as External Surfaces. Cache loss is a cache rebuild, never data loss: the
   Ladder chain is the fallback.

## Consequences

- A fresh clone is fully buildable and testable offline with zero media.
- Playback of any Game degrades gracefully: Cache Tier entry → Ladder rungs by
  rank → unavailable/pointer badge. The Box Score always renders regardless.
