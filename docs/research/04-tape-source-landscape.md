# 04 — Tape source landscape, all eras (nicolas-found42/nba-tv#4)

Resolves the #4 question: where full-game NBA tape actually exists across all eras, what
"try very hard, then mark unavailable" concretely means, per-era hit-rate expectations, and the
per-game source-exhaustion pipeline design. Landscape + pipeline only — no downloading, no
hosting, no code. Sibling tickets: #3 supplies schedule/box (the identity key below), #2
supplies the remote-hosting target this pipeline writes URLs into, #5/#6 consume the verdict.

**Verdict in one paragraph.** Usable full-game tape is era-gated by what was televised and
preserved, not by what any single source holds. The official NBA free tier (NBA ID) is now the
single largest lawful, streamable corpus — a selection of full games per team plus **all NBA
Finals series since 1990** (Help Center, updated 2026-02-03) and every Finals game since 2000
(2022 launch announcement) — mirrored on the official "NBA Classic Games" YouTube playlist
("More than 500 Classic Games now available in the Watch tab of the NBA App"). Internet Archive
adds a measurable but small archive (16,133 items tagged NBA; only ~6 items pre-1970, and those
are All-Star games, not full games). YouTube fan/team archives (Channel 23, Lakers Basketball
Classics, FreeDawkins, hoopsencyclopedia) fill team- and star-shaped holes, especially 1980s–90s
regular season and playoffs, but churn under takedowns and have no stable catalog. Pre-1970 full
games are effectively absent online (Wilt's 100-point game: no video known to exist — radio
quarter only); institutional broadcast archives (Paley Center, UCLA FTVA) are the last-resort
existence check, not a streaming source. Expected free-streamable hit rate: ~0% before 1970,
2–5% for the 1970s, 10–20% for the 1980s, 30–50% for the 1990s–2000s (estimates; Finals ~100%
back to 1990 by construction).

---

## 1. Source landscape

Six source classes, ordered by acquisition-method legality and likely yield. Classes S0–S2 are
the free streaming core; S3 is best-effort; S4–S5 record existence but are not $0-streamable.

### S0 — Official NBA free tier (NBA ID): the anchor source

- Scope: "Fans can access hundreds of full Classic Games and other original NBA content for
  free with an NBA ID. Available Classic Games include a selection of full games for each team
  as well as all NBA Finals series since 1990." — [NBA Help Center, "Classic Games and Original
  NBA Content"](https://support.watch.nba.com/hc/en-us/articles/28006218859415-Classic-Games-and-Original-NBA-Content),
  updated Feb 3, 2026.
- Launch scope (Sep 27, 2022): "archival footage from the NBA vault, including 500 of the best
  classic games in NBA history. Every NBA Finals game since 2000 will also be available, with
  more Finals and popular games being released throughout the season"; plus archived NBA
  Entertainment series. — [NBA.com, reimagined NBA App
  release](https://www.nba.com/news/nba-launches-reimagined-app-a-destination-for-nba-fans-of-every-team).
- Machine-readable proxy: the official **[NBA Classic Games YouTube
  playlist](https://www.youtube.com/playlist?list=PLlVlyGVtvuVniS7jx4DyESaKRUANIzyhE)** describes
  itself as "More than 500 Classic Games now available in the Watch tab of the NBA App" — i.e. a
  mirror of the app catalog that the YouTube Data API can enumerate cheaply (playlistItems.list
  = 1 unit/call). There is no published public JSON API for the NBA App's classic catalog
  itself; treat the playlist as the crawlable surface and the NBA App as the playback origin.
- Integration: ingest the playlist into a local catalog table; match items to game IDs; watch
  for the Finals expansion the release promised ("more Finals and popular games being released
  throughout the season").
- Legality: league-owned, licensed, free with registration. Cleanest possible acquisition.

### S1 — Internet Archive: measurable, small, pre-1990 thin

Measured baseline (advancedsearch `subject:(nba) AND mediatype:(movies)`, rows=0, per year
range; queries dated 2026-09-07):

| Year range | numFound | What the pre-1970 items actually are |
|---|---|---|
| 1946–1959 | 2 | noise (a compilation, a promo) |
| 1960–1969 | 4 | All-Star games/highlights only (1962, 1965, 1968, 1969) |
| 1970–1979 | 28 | mix of All-Star, Finals, playoff broadcasts |
| 1980–1989 | 35 | Finals, All-Star weekend, official DVD transfers |
| 1990–1999 | 233 | Finals on NBC (e.g. [1996 Finals G3](https://archive.org/details/1996-nba-finals-game-3)), VHS rips ([1993 Finals G6 off WNBC](https://archive.org/details/93-nbafinals-game-6)) |
| 2000–2009 | 269 | Finals, Inside Stuff-era recordings |
| 2010–2026 | 14,594 | dominated by uploads/other NBA-tagged video; year = often upload or game year |
| total | **16,133** | |

- No dedicated curated NBA full-game collection exists: a `mediatype:collection` search for
  nba/basketball returns only user *favorites* collections (`fav-*`) and unrelated guides — the
  same query returned 0 items in any `collection:basketball`. IA strategy must be **metadata
  search over subject/title/date**, not collection browsing.
- Two uploader patterns matter: (a) VHS-era home recordings of national broadcasts (the 1993
  Finals G6 item is a WNBC New York VHS rip), (b) rips of official NBA Entertainment/TBS DVD
  releases (title pattern "DVD Transfer", 12 items; e.g. 1988 All-Star Saturday "(c)1988 NBA
  Entertainment/TBS" transfer) — i.e. even IA partially reflects the purchase channel (S4).
- API: `archive.org/advancedsearch.php?q=...&output=json` — free, no key, no quota. Identity
  fields available: `identifier`, `title`, `date`, `year`, `description`, `subject`.
- Legality: third-party uploads; public free access. Per the map's gate (legality of the
  *acquisition method*), retrieving a publicly shared file differs in kind from the licensed
  S0/S4 channels; enforcement posture beyond that is explicitly out of scope.

### S2 — YouTube fan/team archives: the long tail's real home

- **Channel 23: The Complete Michael Jordan Archive** (@MJsChannel23): self-described
  "definitive source for Michael Jordan's full games, restored for the modern era... digitally
  remastered" — full games with era-labeled series playlists ([about
  page](https://www.youtube.com/@MJsChannel23/about); sample: "Full Game: Showtime Hits Chicago
  Stadium | Lakers vs Bulls (February 19, 1985)").
- **Lakers Basketball Classics**: full Lakers games spanning eras (1970 Finals G7, 1972
  streak games with Chick Hearn calls, 1980s, Finals replays) — team-shaped archive pattern.
- **hoopsencyclopedia**: "The Original and Longest Running Basketball Tribute Channel"
  (27.8K subscribers, 126 videos) — edited/condensed game footage rather than full games;
  useful as highlights/identity confirmation, not as a full-game source for most games.
- **FreeDawkins**: full-game uploads with member support; star/series-centric.
- Structural properties: no stable catalog, uploads skew to Finals/playoffs and superstar
  teams, channels disappear and reappear under new names (takedown churn), titles are freeform
  ("1991 NBA Finals | Game 5 | Full Game Replay"). ⇒ treat as an untrusted but high-yield
  search corpus, not a browseable catalog; re-scan regularly.
- Legality: unlicensed rehostings. Acquisition = streaming a public URL; same note as S1.

### S3 — Other free video platforms (best-effort sweep)

Dailymotion, Vimeo, OK.ru, Bilibili and similar host classic full games, discoverable via
site search; no stable public APIs in general. Worth a per-game one-query pass at low priority;
results feed the same matching queue. Legality: same class as S2.

### S4 — Purchase-only channels (lawful acquisition, not $0 streaming)

- Official NBA Entertainment/Warner classics on physical media (the "DVD Transfer" pattern on
  IA shows these exist and circulate), plus used VHS/DVD marketplaces. Acquisition = purchase
  of a licensed copy — the cleanest alternative when streaming fails; the app records "tape
  exists, not streamable" and a pointer (e.g. IA item for the official transfer) rather than a
  stream URL. Streaming these from S1's rips instead is a choice the user makes per the
  acquisition-method gate.
- NBA League Pass is the official paid archive: full replays require an active subscription
  ([Help Center](https://support.watch.nba.com/hc/en-us/articles/360011802673-Full-Game-Replays-and-Condensed-Game-Availability)),
  single-game purchase covers the current season only ([Help
  Center](https://support.watch.nba.com/hc/en-us/articles/115000581693-NBA-Single-Game)), and
  carrier/bundle feature lists include "Classic games & Pop-up classic games" and "Archive"
  ([Verizon +play](https://support.watch.nba.com/hc/en-us/articles/10902305038999-League-Pass-with-Verizon-play),
  [Xumo](https://support.watch.nba.com/hc/en-us/articles/26670359382167-League-Pass-with-Xumo)) —
  but no public page states the historical depth of the LP archive, and team DTC services even
  expire prior-season archives ([BlazerVision](https://support.watch.nba.com/hc/en-us/articles/26513701946263-BlazerVision)).
  Excluded by the $0 standing preference; documented here as the official fallback that
  exists.

### S5 — Institutional broadcast archives (existence check, on-site access)

- **Paley Archive**: "over 160,000 television and radio programs and advertisements...
  including... sports" ([collection](https://www.paleycenter.org/collection-2)); the collection
  demonstrably holds classic NBA games (a Paley free-admission weekend screened "classic NBA
  games from the Paley Archive" — [PRNewswire,
  2023](https://www.prnewswire.com/news-releases/the-paley-center-for-media-announces-free-admission-weekend-june-10--11-301829106.html)).
  Access is on-site viewing, not streaming.
- **UCLA Film & Television Archive**: 350,000 motion pictures and 170,000 television programs
  plus newsreels ([collections](https://www.cinema.ucla.edu/collections/)); holds 16mm
  kinescopes and 2" videotape ([Archive Television
  Treasures](https://www.cinema.ucla.edu/series/archive-television-treasures/)). Whether any
  specific NBA broadcast survives there is findable only via catalog search; use as
  last-resort pointer for pre-1970 games.
- Role in the pipeline: record "exists in institution X" with a pointer; these never become
  stream URLs.

---

## 2. Per-era availability and hit-rate expectations

Why the curve is what it is (context, secondary sources clearly labeled): NBA Finals games were
not all nationally televised until 1970 (ABC) ([sportsbroadcastjournal,
Finals-on-TV history](https://www.sportsbroadcastjournal.com/a-storied-history-remembering-nba-finals-through-a-broadcast-lens/));
NBC's first NBA run was 1954-55 through 1961-62 ([NBA on television in the 1960s,
Wikipedia](https://en.wikipedia.org/wiki/NBA_on_television_in_the_1960s) — secondary context);
CBS carried the league from 1973-74 ([NBA on CBS](https://cbs.fandom.com/wiki/NBA_on_CBS) —
secondary context). The extreme case is documented first-hand: Wilt's 100-point game (Mar 2,
1962) was not televised and "no video of the night's action is known to exist" — only the
fourth-quarter radio call survives ([ESPN](https://www.espn.com/nba/story/_/id/11666359/wilt-chamberlain-audio-fourth-quarter-100-point-game);
[NBA.com hosts the audio](https://www.nba.com/watch/video/fourth-quarter-of-wilts-100-pt-game-k1e86z)).

Planning numbers (per-season denominator: the 82-game schedule modernly; the map's ~60k+
games/80 seasons is the pool). IA columns are **measured** (§S1 table); hit rates are
**estimates** — anchors are the IA floors, the official catalog's documented scope, and the
channel examples above.

| Era | What existed on TV | What survives online (free) | Realistic hit rate (est.) | Notes |
|---|---|---|---|---|
| 1946–1959 | Rare, scattered telecasts; Finals not fully televised | ~nothing full-game; All-Star/highlight fragments | **<1%, plan ≈0** | S5/S4 checks only; expect "unavailable" for the entire era |
| 1960s | Occasional national/regional telecasts (NBC run ended '62) | IA: 4 items, all All-Star; occasional kinescope | **≈0–1%** | Manual review queue; celebrate exceptions |
| 1970s | All Finals on TV from 1970; CBS from '73-74; locals patchy | IA: 28; fan uploads of Finals/playoffs exist (1970 Finals G7, 1972 streak, 1976 3OT) | Finals/playoffs **20–50%**; regular season **1–5%**; aggregate **2–5%** | Sweep playoffs first; All-Star games over-index |
| 1980s | Near-universal local TV + superstations; Finals on CBS | Vault 500 includes many 80s classics; strong fan channels (Channel 23 from 1984-85; Lakers Basketball Classics) | Finals **~90–100%**; playoffs **40–70%**; regular season **5–15%**; aggregate **10–20%** | Star-shaped coverage: Bulls/Lakers/Celtics better than average |
| 1990s | National (NBC/TBS/TNT) + locals; VHS era at peak | **Official: all Finals since 1990**; IA: 233; heavy fan uploads | Finals **100% (official)**; playoffs **50–80%**; regular season **15–40%**; aggregate **30–50%** | The era where S0 alone carries Finals; sweep cheap |
| 2000s | Digital-era broadcasts; League Pass replays exist | **Official: every Finals game since 2000**; IA: 269; fan re-uploads | Finals **100% (official)**; aggregate **~30–50%** | Same shape as 90s |
| 2010s–now | Everything recorded (League Pass replays) | Finals official; regular season gated/DMCA'd | Finals **100% (official)**; aggregate **~25–45%** free | Takedown churn is the binding constraint, not preservation |

Estimate method: hit rate = fraction of games with ≥1 CONFIRMED source across S0–S3. Anchors:
Finals-back-to-1990 is documented, not estimated; IA counts are floors (subject tagging is
incomplete); regular-season regularity of fan uploads is the softest input — treat decade bands
as planning ranges, and let the pipeline's own sweep replace them with measured rates over time.

---

## 3. Per-game source-exhaustion pipeline (design)

### 3.1 Game identity (contract with #3)

- Key: `game_id = (season, game_date_utc, away_team_id, home_team_id)` from the schedule/box
  ingest. Every source hit must reduce to this key.
- Alias table per team: current name, era names and spellings ("SuperSonics"/"Sonics",
  "Supersonics", "Seattle"), city-only forms, and star-name hooks ("Jordan", "Bird", "Magic")
  since fan titles often lead with the star, not the teams.
- Playoff metadata from #3's schedule: round label, series game number → tokens the fan corpus
  actually uses: "1996 NBA Finals Game 3", "ECF GM 4", "Conference Finals Game 4", "Western
  Championship" (the 1976 Suns item's phrasing).

### 3.2 Ordered source ladder (the exhaustion order)

| Rank | Source | Query mechanics | Cost/quota | Notes |
|---|---|---|---|---|
| 0 | Official NBA catalog | Enumerate NBA Classic Games playlist (playlistItems.list); keep matched subset | 1 unit/item | One-time + incremental; also exposes Finals-since-1990 |
| 1 | Internet Archive | advancedsearch JSON: title/subject/date queries per game | free, no key | Cheap; sweep whole decades in batches |
| 2 | YouTube fan corpus | channel-scoped search for curated channel list; then global search residual | search.list **100 calls/day** (default), 1 unit each; videos.list 1 unit | The quota bottleneck — see 3.5 |
| 3 | Other platforms | per-platform site search (Dailymotion/Vimeo/OK.ru/Bilibili) | free/best-effort | Mark "manual-ok" |
| 4 | Purchase-only | Known DVD/media titles; record existence + pointer | n/a | "Tape exists, not streamable" |
| 5 | Institutions | Paley/UCLA catalog lookups for pre-1980 gaps | n/a | Existence pointer only |

A game moves down the ladder only after the higher rung returned no CONFIRMED/LIKELY match for
its identity.

### 3.3 Query generation and matching

- Template set per game: `{Away} vs {Home} {year}`; `{Away} @ {Home} {month day, year}`;
  `"NBA Finals Game {n}" {year} {team}` / round variants; `"{star} {date}"` last resort.
  Cap variants at ~4 per game to respect quota.
- Candidate scoring over title+description+duration:
  - CONFIRMED: exact date parsed AND both teams matched AND full-game duration signal
    (duration ≥ ~70 min or title tokens "Full Game Replay/Complete").
  - LIKELY: date OR (round+game#) exact, other side partial → human review queue.
  - REVIEW: star-name-only or era-mismatch candidates → human review queue.
  - Reject: duration < ~45 min, "highlights/condensed/mixtape" tokens (this is where
    hoopsencyclopedia-style edits and the 10-15-min "condensed games" get filtered).
- Cross-source dedupe: `(game_id, source, upload_id)` unique; keep a ranked list of verified
  sources per game (official first, then quality/completeness), not just one.

### 3.4 Exhaustion definition (what "try very hard" means, concretely)

A game is **marked unavailable** only when: rungs 0–3 have each been queried with the game's
query set, each query recorded with a timestamp and the query text used, and no candidate
reached LIKELY+; OR only rung 4/5 pointers exist (status `EXISTS_NOT_STREAMABLE`). Nothing is
marked absent on a single failed search — the record must show the full ladder was consumed.

### 3.5 Quota and sweep strategy

- YouTube search.list = 100 calls/day (default allocation; videos.list 1 unit, playlistItems
  1 unit — [Google quota doc, updated 2026-09-04](https://developers.google.com/youtube/v3/determine_quota_cost)).
  IA advancedsearch and playlist enumeration are effectively unbounded for this use.
- Therefore: enumerate-before-search. The official playlist (rung 0) and IA (rung 1) cost no
  search quota and cover Finals 1990→ and a per-decade floor; spend the 100 searches/day on
  (a) channel-scoped sweeps of the curated fan list, then (b) residual global search by
  decade chunks, newest first (highest hit rate first). Priority order: Finals/playoffs of all
  eras → 1990s regular season → 2000s → 1980s → pre-1980 manual-only.
- Re-scan cadence: ABSENT games re-checked after 90 days (fan-corpus churn); official catalog
  re-enumerated monthly (it is growing — "more Finals and popular games being released
  throughout the season").

### 3.6 Output registry (what this pipeline writes)

Local table `tape_sources(game_id, rank, source_class, url_or_pointer, match_confidence,
verified_at, query_used, notes)`; UI rule from the map: schedule + box always render; tape
button appears when `rank=0..3` entry is verified, else shows `unavailable (ladder consumed
<date>)` or `exists, not streamable (S4/S5 pointer)`. Playback resolves the URL at play time
(no rehost) — the contract with #2/#6.

### 3.7 Legality summary by source class (acquisition-method gate)

| Source | Acquisition | License status |
|---|---|---|
| S0 NBA App/official YouTube | free stream w/ NBA ID | licensed, clean |
| S1 Internet Archive | free public download/stream | third-party uploads; differs in kind from licensed channels — enforcement posture out of scope per map |
| S2/S3 fan re-uploads | free public stream | unlicensed rehost; same note as S1 |
| S4 purchase (DVD/used media; League Pass) | payment | licensed copy; LP excluded by $0 preference |
| S5 institutions | on-site research access | licensed to archive; no stream |

---

## 4. Handoff notes for the map

- The measurable, lawful core is bigger than assumed: **every Finals game since 1990 free**,
  plus a 500-game official classics set — that alone is a browsable archive the app can
  enumerate via one YouTube playlist, with no scraping.
- The pipeline's scarce resource is YouTube search quota, not IA; sweep order should be
  Finals/playoffs first, and decade-by-decade newest→oldest.
- Pre-1980 regular-season full games are a realistic near-zero; "try very hard" there means
  S1 sweeps + a human-review exceptions queue, then an honest `unavailable` badge — which the
  map already wants next to a always-present schedule + box (from #3).
- Decade hit-rate bands above are estimates by design; the registry (3.6) will produce the
  measured numbers that replace them.
