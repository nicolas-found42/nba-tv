# 15 — Full game sources: free inventory + retrieval strategy

Personal archival pipeline question: where do **full, free-to-access** NBA games actually
exist on the internet (beyond YouTube), so that a per-game `yt-dlp` pull plus organized
Google Drive folders can archive them? The user completes any sign-ins themselves.
Hard filters: **FULL games only** (not highlights), **completely free to access**
(no paid tiers, no login-walled paid services). Personal-use only throughout;
per [ADR-0002](../adr/0002-no-media-in-repo.md) no media bytes ever enter the repo —
only registry rows (URLs, metadata). Nothing was downloaded for this note;
all probes are read-only HTTP GETs (Internet Archive APIs, help pages, upstream
source files), run 2026-09-08.

## 0. What 04 / 08 / 11 already establish — and what this note adds

One section, then new material only:

- **04 (tape source landscape)** measured the terrain: the official NBA free tier is the
  anchor (hundreds of Classic Games + all Finals series since 1990 free with NBA ID);
  Internet Archive holds 16,133 NBA-tagged items but only ~6 pre-1970 (All-Star games,
  not full games); YouTube fan/team archives fill the 1980s–90s long tail but churn;
  pre-1970 full games are effectively absent (Wilt's 100-point game: no video known).
  It set hit-rate bands (~0% pre-1970, 2–5% 1970s, 10–20% 1980s, 30–50% 1990s–2000s,
  Finals ~100% back to 1990) and the per-game source-exhaustion pipeline with the
  `tape_sources(game_id, rank, source_class, url_or_pointer, match_confidence,
  verified_at, query_used, notes)` registry contract.
- **08 (fan preservation networks)** found the trading economy behind the streams:
  USA Sports on DVD (20,901 NBA games, each with a Basketball-Reference boxscore URL →
  `game_id` crosswalk), Gregg's Sports Archive (score-grammar lines back to the 1962
  Finals), Pontel, Interbasket/BigFooty/r/VintageNBA request channels, invite-walled
  private trackers (pointer-only, no automation), Lost Media Wiki as the negative-space
  map. All pointer-only, never stream URLs.
- **11 (source ladder v2)** folded everything into rungs 0–7 with playback classes
  (**file** = Cache Tier-eligible bytes: rungs 1, 4; **External Surface** = embed-only:
  rungs 0, 2, 3; **pointer**: rungs 5–7), the non-Anglo rehost cluster (rung 3),
  the standing empty-corpus sweep (rung 4), the FAST exclusion (linear, no per-game
  addressing), and the yt-dlp `nba.py` endpoint map as archaeology.

**This note adds:** (a) per-class full-game inventory *verified reachable today* with
working `archive.org` queries verbatim, (b) factual yt-dlp capability answers
(what it can and cannot fetch, verified against upstream master), (c) the YouTube
search/retrieval syntax for the pipeline, (d) a Drive folder + filename scheme keyed
on the repo's `game_id` with an `rclone --dry-run` / `--files-from` manifest pattern,
(e) an honest era × class density table and practical yield estimate. No ladder changes.

## 1. Internet Archive — the only free file-level corpus (rung 1)

Primary source: `https://archive.org/advancedsearch.php?q=...&output=json`
(free, no key, no quota). Identity fields: `identifier`, `title`, `date`, `year`,
`description`, `subject`. File enumeration: `https://archive.org/metadata/<identifier>`
(no key). All queries below were run 2026-09-08.

### 1.1 Queries that work, verbatim

**Best inventory query — identifier wildcard (game-shaped slugs):**

```
identifier:(*nba*finals*game*) AND mediatype:(movies)
```

Result 2026-09-08: **numFound = 105**. First page mixes noise (a 2024 vlog) with
genuine full games, all visible in the returned rows:

| identifier | title | full game? |
|---|---|---|
| `detroit-pistons-vs-portland-trailblazers-1990-nba-finals-game-5` | Detroit Pistons Vs Portland Trailblazers 1990 NBA Finals Game 5 | yes-class |
| `2010-nba-finals-lakers-vs-celtics-game-3` | 2010 NBA Finals Lakers Vs Celtics Game 3 | yes-class |
| `2004nbafinalsgame4` | Full D-VHS Tape #3 - ABC (2004) | yes (off-air ABC tape) |
| `1997-nba-finals-game-6` | 1997 NBA Finals | yes-class |

Full request form (copy-paste):

```
https://archive.org/advancedsearch.php?q=identifier%3A%28%2Anba%2Afinals%2Agame%2A%29+AND+mediatype%3A%28movies%29&fl%5B%5D=identifier&fl%5B%5D=title&fl%5B%5D=date&rows=50&output=json
```

**Second query — fielded title (broader, noisier):**

```
title:(NBA Finals) AND mediatype:(movies)
```

Result 2026-09-08: **numFound = 253**. Surfaces genuine items the identifier query
misses — `1975-nba-finals-game-1` ("1975 NBA Finals"),
`1977-nba-finals-game-1_202508` ("1977 NBA Finals"),
`93-nbafinals-game-6` ("1993 NBA Finals - Game 6 - Chicago Bulls vs Phoenix Suns
[VHS]", dated 1993-06-20) — but the top rows are podcast/prediction noise, so this
query feeds the scoring queue (04 §3.3 duration/title tokens), not direct inventory.

**Anti-pattern — unfielded free text:**

```
(NBA Finals) AND mediatype:(movies)
```

Result 2026-09-08: **numFound = 832**, top rows dominated by TV-news clips, sports
talk, and 2K predictions. Do not use for inventory; fielded queries only.

**Baseline (from 04, not re-run):** `subject:(nba) AND mediatype:(movies)` decade
sweeps total 16,133 items; per-decade floors and the pre-1970 All-Star-only finding
stand. There is still no curated NBA full-game *collection* — strategy remains
metadata search over identifier/title/subject/date, not collection browsing.

### 1.2 A full game is really there, as files — proof

`https://archive.org/metadata/1996-nba-finals-game-3` (read 2026-09-08) lists **six
original MPEG4 files**, one per game of the 1996 Finals:

| file | bytes | duration |
|---|---|---|
| `1996 NBA Finals Game 1.mp4` | ~608 MB | 8453 s (~2 h 21 m) |
| `1996 NBA Finals Game 2.mp4` | ~634 MB | 8494 s |
| `1996 NBA Finals Game 3.mp4` | ~714 MB | 8286 s |
| `1996 NBA Finals Game 4.mp4` | ~573 MB | 7701 s |
| `1996 NBA Finals Game 5.mp4` | ~600 MB | 7912 s |
| `1996 NBA Finals Game 6.mp4` | ~681 MB | 8373 s |

Plus IA-generated derivatives (thumbnails; the standard `_files.xml` / transcode
set rides alongside). That is the rung-1 pattern in full: ~2–2.5 h, ~600 MB,
360p-era MPEG4 originals, directly fetchable over plain HTTP at
`https://archive.org/download/<identifier>/<filename>` (HTTP range-request
friendly — the Player Backend's Lane A shape). Retrieval for the pipeline is either
`yt-dlp https://archive.org/details/<identifier>` (archive.org is in yt-dlp's
supported-sites list — [yt-dlp README](https://github.com/yt-dlp/yt-dlp),
retrieved 2026-09-08: "support for thousands of sites") or direct HTTP from the
metadata file list. No login, no key.

### 1.3 TV News Archive — checked, not a game source

Two rows=0/count probes, 2026-09-08:

- `(nba) AND collection:(tv)` → **numFound = 0**.
- `(nba) AND collection:(tvnews)` → **numFound = 12,281** — but the identifiers are
  timestamped news programs (`KTVU_20260320_..._West_Coast_News_Wrap`,
  `CNBC_20191008_..._Closing_Bell`, ...) whose *closed captions* mention the NBA.
  These are news broadcasts with NBA segments/mentions, not game broadcasts.

Verdict: the TV News Archive carries no full games (density zero as inventory).
It is at most a curiosity source for a news segment *about* a game — out of scope
for the pipeline.

### 1.4 Rung-1 card

- **Access:** free, no login, no key, no quota.
- **Full-game density:** high for Finals 1975→ (strongest 1990s–2000s: NBC-era VHS
  rips and DVD transfers); medium for playoffs of the same span; low for regular
  season; rare pre-1975 (All-Star/highlights per 04); ~zero pre-1970.
- **Retrieval:** advancedsearch inventory → metadata file list → `yt-dlp` or direct
  HTTP. Uploader patterns to expect: home VHS rips of national broadcasts
  (the 1993 G6 item is a WNBC New York rip) and NBA Entertainment DVD transfers.
- **Rights:** third-party uploads; public free retrieval. Grey-note per the map's
  acquisition-method gate (04 §3.7) — personal-use-only framing, never rehost.
- **Ladder:** rung 1, **file** playback → Cache Tier-eligible.

## 2. NBA's own free offerings (rung 0)

### 2.1 The free tier, re-verified today

[NBA Help Center, "Classic Games and Original NBA Content"](https://support.watch.nba.com/hc/en-us/articles/28006218859415-Classic-Games-and-Original-NBA-Content)
(read 2026-09-08; page stamp "Updated February 03, 2026"):

> "Fans can access **hundreds of full Classic Games** and other original NBA content
> **for free with an NBA ID**. Available Classic Games include a selection of full
> games for each team as well as **all NBA Finals series since 1990**."

Access path: sign in with NBA ID → NBA App (Live → Featured) or NBA.com
(Watch → Featured) → pick the game/series. **Access status: free + registration**
(the user completes sign-in themselves); not a paid tier, not provider-gated.
**Density:** Finals 1990→ ≈ 100% by construction; plus a per-team selection of
hundreds. This is the single largest lawful full-game corpus and the pipeline's
first stop per game. Machine proxy for enumeration stays the official
[NBA Classic Games YouTube playlist](https://www.youtube.com/playlist?list=PLlVlyGVtvuVniS7jx4DyESaKRUANIzyhE)
("More than 500 Classic Games now available in the Watch tab of the NBA App" —
per 04 §S0); playlist URLs are ordinary yt-dlp targets.

### 2.2 yt-dlp × nba.com — factual capability answer

Read against upstream master 2026-09-08
([yt_dlp/extractor/nba.py](https://raw.githubusercontent.com/yt-dlp/yt-dlp/master/yt_dlp/extractor/nba.py)):

- `NBAWatchEmbedIE` (`nba:watch:embed`, `watch.nba.com/embed?id=…`) — **`_WORKING = False`**.
- `NBAWatchIE` (`nba:watch`, `nba.com/.../video/...` and `watch.nba.com/video/...`) —
  **`_WORKING = False`**.
- `NBAWatchCollectionIE` (`nba:watch:collection`, `.../watch/list/collection/…`) —
  **`_WORKING = False`**.
- The plumbing underneath targets retired endpoints (Neulion Solr catalog
  `neulionscnbav2-a.akamaihd.net`, `watch.nba.com/service/publishpoint`, Turner CVP
  CDNs) — consistent with 09/11's "endpoint map as archaeology" verdict.

So: **yt-dlp does not fetch nba.com/watch video URLs today.** That is a statement
about extractor state, not circumvention — the extractors are disabled upstream,
and rung-0 playback stays where the league puts it (NBA App, YouTube embeds:
External Surface). The pipeline pulls rung 0 via the YouTube playlist mirror
with yt-dlp, or the user watches in-app; it does not scrape nba.com video
endpoints.

### 2.3 League Pass free windows — excluded, honestly

Full-game replays require an active subscription and single-game purchase covers
the current season only (per 04 §S4, NBA Help Center). Occasional free-preview
weekends are ephemeral marketing windows, not addressable inventory — they cannot
be inventoried per game and are out of scope for the pipeline. **Access: paid →
fails the free filter.**

### 2.4 Rung-0 card

- **Access:** free with NBA ID registration (login, not paid).
- **Full-game density:** Finals 1990→ ~100%; per-team classics selection (hundreds).
- **Retrieval:** manual/in-app (user signed in); playlist-proxy enumeration + yt-dlp
  for the YouTube mirror.
- **Rights:** clean — league-owned, licensed. The rights-best source in the landscape.
- **Ladder:** rung 0, External Surface.

## 3. YouTube full-game channel landscape (rung 2)

### 3.1 What exists — categories, not a directory

Full-game reupload channels are real and numerous, but they are an untrusted,
churning search corpus, not a catalog (04 §S2; 08 §3.1 documents the churn:
the Torontos archive deletion, the lost ~500-game community list, DMCA sweeps
of 60s/70s material). No new channel names are listed here — 04 §S2's curated
seed list (team archives, era/star archives, series posters) stands as the
directory, and anything added to it gets re-verified at ingest, never trusted
stale. The durable categories:

| category | shape | full-game density |
|---|---|---|
| Team archives | one franchise across eras (e.g. 1970 Finals G7 → 1980s → modern replays) | med–high for that team |
| Star/era archives | full games of one player's teams, often remastered, series playlists | high for covered seasons |
| Series posters | Finals/playoff series uploads, member-supported full-game posters | high for Finals/playoffs |
| Condensed/edit channels | 10–15 min edits, tributes, mixtapes | ~zero (identity confirmation only) |
| Talk/clip channels | predictions, debates, 2K content | zero — reject by duration |

### 3.2 Search + retrieval syntax for the pipeline

Query templates per game (04 §3.3), issued as yt-dlp search prefixes against the
seed-channel list first (channel-scoped), then global residual — the quota
bottleneck (YouTube `search.list` ≈ 100 calls/day) is unchanged from 04 §3.5:

```
yt-dlp "ytsearch20:<Away> vs <Home> Full Game <Month> <Day> <Year>" \
  --flat-playlist --print "%(id)s | %(title)s | %(duration)s"
yt-dlp "ytsearch20:<Year> NBA Finals Game <N> Full Game Replay" \
  --flat-playlist --print "%(id)s | %(title)s | %(duration)s"
```

Typical title conventions to match (and to generate): `<Team A> vs <Team B>
Full Game <date>`; `<Year> NBA Finals | Game <N> | Full Game Replay`;
`<Away> @ <Home> <Month> <Day>, <Year>`; era-true round labels for old games
("Western Championship" for what is now the WCF — per 04 §3.1 alias table).
Channel surfaces: `https://www.youtube.com/<handle>/videos`,
`.../playlists` (era-labeled series playlists are the highest-signal object on
team/star archives).

Full-game gate at pull time (factual yt-dlp capabilities per the
[README](https://github.com/yt-dlp/yt-dlp), retrieved 2026-09-08 — Video
Selection / Download Options / Verbosity sections):

```
yt-dlp --match-filter "duration > 4200" "<url>"     # ≥70 min: full-game floor
yt-dlp --download-archive archive.log "<url>"        # idempotent per-game pulls
```

CONFIRMED needs what 04 §3.3 already requires (exact date + both teams +
duration ≥ ~70 min); reject `< ~45 min` and `highlights/condensed/mixtape`
tokens. Bootstrap: the `choucisan/nba_games` crosswalk (189 verified YouTube IDs
→ official NBA.com game IDs, per 09/11 rung 2) seeds identity matching so the
pipeline starts from known-good `(game_id, youtube_id)` pairs.

### 3.3 Rung-2 card

- **Access:** free, no login (occasional age-gates).
- **Full-game density:** high for Finals/playoffs and star teams 1984→; low for
  non-star regular season; pre-1984 thin and actively scrubbed.
- **Retrieval:** channel-scoped `ytsearch` → global residual; pull via yt-dlp
  (YouTube is yt-dlp's canonical target); `--download-archive` ledger per game.
- **Rights:** user-uploaded, unlicensed for nearly all NBA games. Personal-use-only
  framing: stream pointers in the registry, never rehost, never redistribute.
  Stated plainly so the pipeline stays honest.
- **Ladder:** rung 2, External Surface (embed-only — never Cache Tier bytes).

## 4. Other free classes (no new probes; placement confirmed)

- **National-broadcaster free windows (ABC/ESPN/TNT).** No per-game free replay
  inventory exists on the open web: national replays sit behind provider-auth apps,
  and the league's own FAST feed ("The NBA Channel") is linear with no per-game
  addressing — explicitly outside the ladder (11 §4). **Density as inventory: ~zero;
  manual-check class only.** Not re-probed here; the structural reason (no
  addressing, auth walls) is documented in 11.
- **International free relays (rung 3).** The measured corpus stands per 11:
  Bilibili Chinese-commentary full replays (1997 Finals G5, 2016 Finals G7, 2018
  ECF G2, 4K 1998 "Last Shot") including official-account classics; VK
  (`@all_about_nba`, "NBA FULL GAMES" playlist, 1998 Finals); OK.ru 1980s
  non-star-team playoff tape; CDA.pl complete 1980s–90s Finals broadcasts.
  Retrieval: one `site:` query per platform per game + maintained yt-dlp
  extractors (`vk.py`, `bilibili.py`, `dailymotion.py` — per 11 §rung 3).
  **Access: free. Rights: grey (unlicensed rehost class; Bilibili mixes in
  licensed/geo-CN official content — caveat recorded in 11). Playback:
  embed-only, never Cache Tier bytes.**
- **Reddit / forum pointer classes (rung 5).** r/VintageNBA archivist watchlists
  and keyword RSS, team-sub VHS finds, Interbasket request threads, BigFooty-style
  "describe the game" traders (08 §§1–3) — these feed the candidate queue at
  REVIEW confidence or record `EXISTS_NOT_STREAMABLE`; they are never pull
  targets. Discord remains a dead end (08 §4: no publicly indexable tape network).
  No new probing — 08's sketches are the procedure.
- **University / newsreel archives (rung 7).** Paley (on-site viewing), UCLA FTVA
  (kinescopes, 2″ tape), British Pathé / Getty (1–2-minute fragments,
  licensing-gated) — existence pointers for pre-1980 gaps, never streams
  (04 §S5, 11 rung 7). Pre-1970 full games stay at ≈ 0 by the physics of what was
  televised and preserved, not by lack of searching.
- **Cheap standing sweeps (rung 4).** Odysee `claim_search` + PeerTube Sepia
  Search, both keyless, both measured ≈ empty (11 rung 4) — keep as free re-scans;
  the only other file-reachable rung if content ever appears. Dailymotion's
  keyless Data API v2 is the rung-3 extra pass (corpus ≈ empty, fingerprint
  churn — 11 rung 3).

Consolidated per-class card table (04/08/11 verdicts + today's probes):

| class | access | full-game density | retrieval | rights | rung |
|---|---|---|---|---|---|
| NBA free tier + Classics playlist | free + NBA ID | Finals 1990→ ~100%; team selection (100s) | app (manual) / yt-dlp on playlist mirror | clean | 0 |
| Internet Archive | free, no key | Finals/playoffs 1975→ high; RS low; pre-75 rare | advancedsearch → metadata → yt-dlp/HTTP | grey-note | 1 |
| YouTube fan corpus | free | playoffs/star teams 1984→ high; else low | ytsearch → yt-dlp | grey | 2 |
| Non-Anglo cluster (VK/OK/CDA/Bili) | free | 1980s–90s Finals/playoffs med | `site:` queries → yt-dlp/embed | grey | 3 |
| Odysee / PeerTube | free, keyless | ≈ 0 | keyless API re-scan | method clean | 4 |
| Collector catalogs / forums / Reddit | free to read | existence only (20k+ pointers) | HTML crawl / RSS / manual | grey pointer / LMW clean | 5 |
| Purchase (DVD / used media) | paid | high where it exists | storefront → buy | clean (licensed copy) | 6 |
| Institutional / newsreel | on-site / licensed | fragments; pre-70 ≈ 0 | catalog lookup | clean | 7 |
| TV News Archive | free | **0 — not games** (checked 2026-09-08) | n/a | n/a | — |
| League Pass / broadcaster auth | paid / provider-gated | high but gated | excluded | clean | — |

## 5. Drive organization — folder scheme + rclone manifest pattern

### 5.1 Scale and the identity key

The pool is ~75–77k games over 80 league-years (03: 63.2k FTE rows 1946–2015 +
~1.2k/season after). The practical free yield is far smaller (§6), but the scheme
must not assume smallness: **shard by season**, one directory per season, flat
files inside. The repo's `game_id` is the Basketball-Reference box-score slug,
`^\d{9}[A-Z]{3}$` (contract: `nbatv_db::is_valid_game_id`, mirrored in
`nbatv_ingest::validate_game_id`): `YYYYMMDD` + day-game index + home-team BR
abbrev — e.g. `194611010TRH` = 1946-11-01, game 0, home TRH (Toronto Huskies);
`199806140CHI` = 1998-06-14, home CHI. The away team and the season slug are
**not** in the slug (October games straddle the season boundary), so they come
from the `games` row — the manifest carries them.

### 5.2 Layout and filename template

```
gdrive:nba-archive/
  manifest/
    manifest.csv        # one row per retrieved FILE (a game may have several)
    files-from.txt      # generated from manifest.csv: relative paths, one per line
  tape/
    {season}/           # e.g. 1946-47, 1997-98 — from the games row, never parsed
      {game_id}__{AWAY}-at-{HOME}__{src}.{ext}
```

Filename template:

```
{game_id}__{AWAY}-at-{HOME}__{src-tag}.{ext}
```

`src-tag` = `{rank-letter}{class}-{source-slug}`, e.g.
`r1-ia-1996-nba-finals-game-3`, `r2-yt-<youtube-id>`, `r0-nba-classics`.
Examples:

```
tape/1997-98/199806140CHI__UTA-at-CHI__r1-ia-1997-nba-finals-game-6.mp4
tape/1997-98/199806140CHI__UTA-at-CHI__r0-nba-classics.mp4
tape/1946-47/194611010TRH__NYK-at-TRH__r5-usasd-pointer.txt
```

Why this shape: `game_id` sorts chronologically and joins back to the registry;
`AWAY-at-HOME` is human-browsable and matches the Shell's `NYK @ TRH` labels;
the source tag keeps the registry's ranked multi-source list (04 §3.3: keep a
ranked list, not one URL) representable on disk; pointer-only rungs get a `.txt`
stub (source + grade + query used) so the folder tree mirrors the registry
exactly, including `EXISTS_NOT_STREAMABLE` / `KNOWN_MISSING` states. Sidecars
(`.info.json` from yt-dlp `--write-info-json`) sit beside the file under the
same stem.

`manifest.csv` columns (superset of the `tape_sources` registry row):

```
game_id,season,date,away,home,game_type,rank,source_class,source_url,
filename,bytes,sha1,verified_at
```

`filename` is the path relative to `tape/` (e.g.
`1997-98/199806140CHI__UTA-at-CHI__r1-ia-1997-nba-finals-game-6.mp4`).

### 5.3 Applying a manifest with rclone `--dry-run` + `--files-from`

Flags per [rclone filtering docs](https://rclone.org/filtering/) (retrieved
2026-09-08): "`--files-from`, `--files-from-raw` and `--files-from0` flags
over-ride and cannot be combined with other filter options" — the manifest file
*is* the file list, one relative path per line. And: "To test filters without
risk of damage to data, apply them to `rclone ls`, or with the `--dry-run` and
`-vv` flags."

Pipeline pattern (staging dir holds yt-dlp output named per §5.2; the remote is
the Drive folder):

```
# 1. generate the file list from the registry manifest
cut -d, -f10 manifest/manifest.csv > manifest/files-from.txt
# 2. rehearse — shows what would transfer, touches nothing
rclone copy --dry-run -vv ./staging gdrive:nba-archive/tape \
  --files-from manifest/files-from.txt
# 3. apply — same command minus --dry-run
rclone copy ./staging gdrive:nba-archive/tape \
  --files-from manifest/files-from.txt
# 4. audit later pulls against what is already archived
rclone check ./staging gdrive:nba-archive/tape \
  --files-from manifest/files-from.txt
```

`--files-from` paths are relative to the source root (`./staging`), which is why
the season-sharded layout maps 1:1 between staging, manifest, and remote.
`--download-archive` on the yt-dlp side plus `rclone check` on the Drive side
give the two idempotence ledgers (pulled / archived); `tape_sources.verified_at`
records when each row was confirmed, feeding the 90-day re-scan cadence (11 §3).

## 6. Honest limits — what does not exist free, and practical yield

### 6.1 What does NOT exist free (stated plainly)

1. **Most regular-season full games before ~2015 on IA.** The archive's density is
   Finals/playoffs; regular-season tape of non-star teams is the systematic hole
   (04's IA decade table + §1.1 queries above).
2. **Pre-1970 full games, anywhere.** ~zero by preservation physics: scattered
   telecasts, taped-over broadcasts; surviving fragments are All-Star games,
   newsreels, radio (Wilt 100-pt game: no video known — 04 §2). Rung 5/7 pointers
   are the correct output here, not pulls.
3. **2010s–present regular-season full games, free.** Everything was recorded, but
   it sits behind League Pass / provider auth, and open uploads churn under
   takedowns (04 §era table; 08 §3.1 DMCA-sweep thread).
4. **A curated, browsable IA collection of NBA full games.** Does not exist
   (04 §S1, still true 2026-09-08) — inventory is query-built every time.
5. **Broadcaster per-game free archives / FAST per-game addressing.** Linear only
   (11 §4). League Pass free-preview weekends are unaddressable ephemera.

### 6.2 Density table — era × source class (planing numbers)

Anchors: 04's measured IA floors + documented official scope + today's probes.
Cells are share of *that era's games* with ≥1 CONFIRMED free full-game source;
ranges are estimates except where marked measured.

| era | R0 official | R1 IA | R2 YouTube | R3 non-Anglo | R5 pointers (existence) |
|---|---|---|---|---|---|
| 1946–59 | — | ≈ 0 | ≈ 0 | — | rare (LMW known-missing) |
| 1960s | — | ≈ 0 (All-Star only, measured) | ≈ 0–1% | — | sparse (Gregg's from '62 Finals) |
| 1970s | — | low (28 items, measured floor) | 1–5% RS; 20–50% Finals/playoffs | — | med (Gregg's, USASD) |
| 1980s | team selection | med (35 items + DVD transfers) | 5–15% RS; 40–70% playoffs | med (OK.ru, CDA.pl) | high (USASD, Gregg's) |
| 1990s | **Finals 100%** (documented) | high (233 items, measured) | 15–40% RS; 50–80% playoffs | med–high (VK, CDA.pl, Bili) | high |
| 2000s | **Finals 100%** (documented) | high (269 items, measured) | same shape as 90s | med (Bili) | high |
| 2010s–now | **Finals 100** (documented) | noisy/low (tag flood) | gated/churned RS | low–med (Bili recent) | med |

### 6.3 Practical yield estimate for the pipeline

- **Near-certain:** every Finals game 1990→ (~200+ games incl. the full series
  slates) + the hundreds-strong per-team Classics selection — via NBA ID
  (manual) and the Classics playlist mirror (yt-dlp).
- **High-confidence bulk:** low hundreds of Finals/playoff full games 1975–2010
  from IA identifier/title sweeps (§1.1), as ~600 MB direct downloads.
- **Long tail:** low single-digit thousands of games, concentrated 1984–2010
  playoffs and star-team regular seasons, via rung 2/3 search — each at REVIEW→
  CONFIRMED human cost and takedown risk; expect list rot (snapshot at ingest).
- **Existence map, not pulls:** ~20k+ rung-5 pointers (USASD alone) that tell the
  UI "tape exists, not streamable" — the honest badge 04 §3.6 designed.
- **Near-zero, do not plan pulls:** pre-1970 everything; pre-1984 non-Finals;
  modern regular season outside brief free windows.

Bottom line: a personal pipeline realistically archives on the order of
**1–3k full games**, Finals-complete from 1990, playoff-strong back to the
mid-70s, star-skewed throughout — plus a ~20k-row existence map that makes the
*unavailable* badges truthful. That is the archive the ladder was built to
produce; this note is the per-class inventory that starts filling it.

## Sources (primary, with retrieval dates)

- Internet Archive advancedsearch API — `identifier:(*nba*finals*game*) AND
  mediatype:(movies)` (numFound 105), `title:(NBA Finals) AND mediatype:(movies)`
  (253), `(NBA Finals) AND mediatype:(movies)` (832), `(nba) AND collection:(tv)`
  (0), `(nba) AND collection:(tvnews)` (12,281 caption hits) — all
  `https://archive.org/advancedsearch.php`, retrieved 2026-09-08.
- IA metadata API — `https://archive.org/metadata/1996-nba-finals-game-3`
  (six original MPEG4 files, ~570–714 MB, ~7700–8500 s each), retrieved 2026-09-08.
- NBA Help Center — [Classic Games and Original NBA Content](https://support.watch.nba.com/hc/en-us/articles/28006218859415-Classic-Games-and-Original-NBA-Content)
  (page stamp 2026-02-03), retrieved 2026-09-08.
- yt-dlp upstream master — [yt_dlp/extractor/nba.py](https://raw.githubusercontent.com/yt-dlp/yt-dlp/master/yt_dlp/extractor/nba.py)
  (`NBAWatchIE`, `NBAWatchEmbedIE`, `NBAWatchCollectionIE` all `_WORKING = False`),
  retrieved 2026-09-08.
- yt-dlp — [README](https://github.com/yt-dlp/yt-dlp) (supported-sites claim;
  usage sections: Video Selection, Download Options, Verbosity/Simulation),
  retrieved 2026-09-08.
- rclone — [Filtering docs](https://rclone.org/filtering/) (`--files-from`
  override rule; `--dry-run` + `-vv` rehearsal rule), retrieved 2026-09-08.
- Repo priors — 04-tape-source-landscape.md, 08-fan-networks.md,
  11-source-ladder-v2.md (all 2026-09-07); game_id contract
  (`nbatv_db::is_valid_game_id`, `^\d{9}[A-Z]{3}$`); ADR-0002 (no media in repo).
