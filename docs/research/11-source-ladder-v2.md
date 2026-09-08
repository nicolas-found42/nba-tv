# 11 — Source Ladder v2 (nicolas-found42/nba-tv#11)

Resolves #11: fold the closed sweeps into one updated **Source Ladder** contract. Pure
synthesis — no new probes, no new hunting; every claim is owned by an input doc cited by
section. Inputs: [04-tape-source-landscape.md](04-tape-source-landscape.md) (ladder v1,
rungs S0–S5, `tape_sources` contract), [07-free-platforms.md](07-free-platforms.md)
(non-Anglo rehost cluster, Odysee/PeerTube standing rungs, FAST note),
[08-fan-networks.md](08-fan-networks.md) (collector catalogs as existence-pointer rung, BR
crosswalk), [09-code-leads.md](09-code-leads.md) (choucisan crosswalk, VK/Bilibili/Dailymotion
candidates). Language per [CONTEXT.md](../../CONTEXT.md): **Tape Source**, **Source Ladder**,
**External Surface**, **Cache Tier**. All probe evidence dates from the input docs
(2026-09-07); this file makes no new measurements.

**Verdict in one paragraph.** The v1 ladder (04 §3.2: official tier → Internet Archive →
YouTube fan corpus → other platforms → purchase → institutions) keeps its skeleton and its
ordering logic — likely yield and acquisition legality, high rungs first — and gains four
folds. Rung 3 ("other platforms") becomes a measured **non-Anglo rehost cluster** (VK, OK.ru,
CDA.pl, Bilibili, enumerated by search-engine `site:` queries because every platform API is
gated) plus Dailymotion's keyless API as a cheap extra pass — all embed-only **External
Surfaces**. A cheap standing rung holds the two **bytes-reachable but empty** platforms
(Odysee, PeerTube — keyless, progressive MP4, Player-Backend-native, yield ≈ 0). A new
existence-pointer rung between platforms and purchase holds the **collector catalogs and fan
networks** (#8): structured catalogs with Basketball-Reference crosswalks (USA Sports on DVD,
20,901 records), score-grammar archives (Gregg's), request communities — never stream URLs.
The YouTube rung gains the **choucisan/nba_games** MIT identity crosswalk (#9), and the
official rung inherits the yt-dlp `nba.py` endpoint map as archaeology. The league's FAST
channel ("The NBA Channel", Pluto TV/Tubi) stays **outside** the ladder: lawful and free but
linear, with no per-game addressing. Exhaustion now means the free-stream rungs 0–4 consumed
with recorded queries; rungs 5–7 only ever produce pointers.

---

## 1. Rung order at a glance

Playback classes per CONTEXT.md + #2's Player Backend decision: **file** = progressive
MP4-over-HTTP reachable by the app; **External Surface** = plays only in a browser/vendor
player; **pointer** = not playable, existence metadata only.

| Rank | Rung | Sources | Playback | Legality | Status vs v1 |
|---|---|---|---|---|---|
| 0 | Official NBA free tier | NBA ID catalog; Classic Games YouTube playlist as machine proxy | External Surface | Clean (licensed) | kept; + `nba.py` endpoint archaeology (#9) |
| 1 | Internet Archive | advancedsearch metadata sweeps | file | Grey-note (public retrieval; enforcement out of scope per map) | unchanged |
| 2 | YouTube fan/team corpus | curated channels + residual search | External Surface | Grey (unlicensed rehost) | kept; + choucisan/nba_games crosswalk (#9) |
| 3 | Non-Anglo rehost cluster | VK, OK.ru, CDA.pl, Bilibili via `site:`; Dailymotion keyless API pass | External Surface (embed-only) | Grey (unlicensed rehost) | **upgraded from vague list** (#7, #9) |
| 4 | Standing empty-corpus sweep | Odysee, PeerTube | file (bytes-reachable, keyless) | Method clean; corpus ≈ 0 | **new standing rung** (#7) |
| 5 | Collector catalogs & fan networks | USASD, Gregg's, Pontel catalog, personal catalogs, Interbasket/BigFooty/r/VintageNBA, MySpleen/TV-Vault, Lost Media Wiki | pointer | Grey pointer-only (LMW clean) | **new rung** (#8) |
| 6 | Purchase-only | official DVD/used media, eBay commercial, Pontel purchase lane; League Pass documented-excluded | pointer | Clean (licensed purchase); LP excluded by $0 | kept; absorbs #8 physical lane |
| 7 | Institutional & newsreel archives | Paley, UCLA FTVA, British Pathé, Getty | pointer | Clean (licensed to archive; no stream) | kept; + newsreel fragments (#7) |

A Game moves down the ladder only after the higher rung returned no CONFIRMED/LIKELY match
for its identity (04 §3.2, unchanged).

---

## 2. Per-rung contract

### Rung 0 — Official NBA free tier (the anchor)

- **Identity/enumeration.** No published public JSON API for the NBA App's classic catalog
  itself; the machine-readable proxy is the official **NBA Classic Games YouTube playlist**
  (04 §S0), enumerated with `playlistItems.list` (1 unit/call) into a local catalog table,
  matched to `game_id`, re-enumerated monthly (catalog grows — "more Finals and popular games
  being released throughout the season"). Scope: hundreds of Classic Games + every Finals
  game since 2000 / all Finals series since 1990.
  From #9: the yt-dlp `extractor/nba.py` endpoint map (Unlicense) documents the official-tier
  plumbing — Solr catalog (`neulionscnbav2-a.akamaihd.net/solr/nbad_program/usersearch` by
  `seoName`/`pid`), stream resolution (`watch.nba.com/service/publishpoint?type=video&format=json&id=<pid>`),
  collection listings (`content-api-prod.nba.com/public/1/endeavor/video-list/collection/<id>`),
  team video API (`api.nba.net/2/{team}/video,imported_video,wsc/` with a hard-coded access
  token), and Turner CVP file CDNs — **all extractor classes are `_WORKING = False`**;
  treat the map as re-derivation notes, not working code (09 §2, §Ladder 3).
- **Playback.** External Surface: NBA App as playback origin; playlist items as YouTube
  embed. (The archaeology suggests some official endpoints historically served file URLs —
  progressive MP4 via Turner CVP — but that is unverified today; re-derive before assuming.)
- **Legality.** **Clean** — league-owned, licensed, free with registration (04 §3.7).

### Rung 1 — Internet Archive

- **Identity/enumeration.** `archive.org/advancedsearch.php?q=...&output=json` — free, no
  key, no quota. Strategy is **metadata search over subject/title/date**, not collection
  browsing (no curated NBA full-game collection exists). Identity fields: `identifier`,
  `title`, `date`, `year`, `description`, `subject`; sweep whole decades in batches (04 §S1).
- **Playback.** **File** — direct file URLs, MP4-over-Range per #2; the Cache Tier-eligible
  free rung alongside rung 4.
- **Legality.** **Grey-note** — third-party uploads, public free access; per the map's
  acquisition-method gate, retrieving a publicly shared file differs in kind from the
  licensed channels; enforcement posture explicitly out of scope (04 §S1, §3.7).

### Rung 2 — YouTube fan/team corpus

- **Identity/enumeration.** Channel-scoped `search.list` over the curated channel list
  (Channel 23, Lakers Basketball Classics, FreeDawkins; hoopsencyclopedia = condensed edits,
  identity-confirmation only), then residual global search by decade, newest first. This rung
  stays the **quota bottleneck** (100 searches/day default) — enumerate-before-search stands
  (04 §S2, §3.5).
  **New crosswalk (#9): `choucisan/nba_games`** (MIT, GitHub + Hugging Face): 189 verified
  full-length NBA games on YouTube crosswalked to official NBA.com game IDs
  (`0021500874`-style), records named `YYYY-MM-DD-away-vs-home` in official home/away order,
  with box scores, play-by-play, and historical-franchise normalization (PHL/PHI, SAN/SAS,
  NJN/BKN, NOH/NOP/CHA, SEA/OKC). Built from playlist `PLNbBj4TorBWerlzB1A5iM3XjwigqFG8sY`
  (595 raw → 217 valid → 189 verified, ~347 h); distributes no video; recommends yt-dlp with
  YouTube ToS compliance. Drop-in **bootstrap for per-game YouTube identity matching**;
  classic-era skew means it complements, not replaces, rung 0 (09 §1, §Ladder 1).
- **Playback.** External Surface (YouTube embed).
- **Legality.** **Grey** — unlicensed rehost; acquisition = streaming a public URL; same note
  as rung 1, reported never endorsed (04 §S2).

### Rung 3 — Non-Anglo rehost cluster

- **Identity/enumeration.** One search-engine **`site:` query per game per platform** over
  **VK** (`site:vk.com`), **OK.ru** (`site:ok.ru`), **CDA.pl** (`site:cda.pl`), **Bilibili**
  (`site:bilibili.com`) — the four platforms with **measured** classic-NBA corpora (07 §2.2–2.5).
  Platform APIs are **not** the enumeration path: all gated (VK `video.search` error 15;
  OK.ru error 101; CDA 401; Bilibili 412 risk control) — search-engine indexing of public
  watch pages is the working method, and titles + durations in the listings feed 04's
  scoring unchanged (07 §2, §4.1, §4.3).
  From #9 (corroboration): dedicated NBA presences — VK channel
  [@all_about_nba](https://m.vkvideo.ru/@all_about_nba) (28,587 videos, "NBA broadcasts | NBA
  in Russia") and the "NBA FULL GAMES" playlist; Bilibili search surface
  【NBA全场高清中文录像回放】 and official-account classic replays (哔哩哔哩篮球赛事); Dailymotion
  replay channels (user `sporthdlive`). Maintained yt-dlp extractors exist for all three
  (`vk.py`, `bilibili.py`, `dailymotion.py`, Unlicense).
  **Dailymotion** is the rung's cheap extra pass: Data API v2 search works **keyless**, but
  the measured classic corpus is ≈ empty — Audible Magic + INA fingerprinting auto-removes
  matched broadcasts, so expect churn even for whatever exists (07 §2.1).
  **Non-English tokens join the alias template set** (04 §3.3): NBA经典回放 / 全场回放 /
  全场高清中文录像回放, Финал НБА, plus era-true round labels.
- **Measured era value:** VK = 1990s (1998 Finals full games; decade-plus stable uploads
  2012–2024); OK.ru = 1980s non-star-team playoff tape (1986 ECF G2, 1981 ECF); CDA.pl =
  1980s–90s complete Finals broadcasts (1998 F G6 02:10:55, 1991 F G5, 1983-84 F G7) — the
  strongest new lead; Bilibili = 1990s→present Chinese-commentary replays (1997 F G5, 2016 F
  G7, 2018 ECF G2, 4K 1998 "Last Shot") (07 §2.2–2.5).
- **Playback.** **External Surface, embed-only on every platform in the rung** — no public
  file URLs anywhere in the cluster; therefore **no legitimate app-side ingestion path into
  the Cache Tier**: fetching bytes from them would require third-party download tooling
  outside the Player Backend contract (07 §4.2, §4.4).
- **Legality.** **Grey** — unlicensed rehost class, same acquisition posture as rung 2
  (public page → stream, no DRM circumvention per #9); Bilibili nuance from #9: the surface
  mixes licensed/geo-CN official content with unlicensed uploads — still classified grey
  overall with that caveat. Reported, never endorsed (07 §5, 09 §3).

### Rung 4 — Standing empty-corpus sweep (Odysee, PeerTube)

- **Identity/enumeration.** Odysee: JSON-RPC `claim_search`, keyless (probed working).
  PeerTube: Sepia Search REST, keyless; same shape as the per-instance API. Both **perfect
  mechanism, empty corpus**: measured classic-NBA yield ≈ 0 (noise: 2K gameplay, vlogs,
  TikTok rips) (07 §2.6–2.7). Keep as a standing rung because enumeration is free/keyless and
  re-scans are cheap.
- **Playback.** **File** — the only platforms in the whole sweep besides IA whose bytes are
  app-reachable: Odysee `get` → `streaming_url` progressive MP4 (`player.odycdn.com/...`);
  PeerTube `files[].fileUrl` progressive MP4 per resolution + HLS + `fileDownloadUrl`.
  Player-Backend-native and Cache Tier-eligible if content ever appears (07 §2.6–2.7).
- **Legality.** **Method clean** (keyless public API / free stream); any content found would
  be the uploader's licensing problem (grey if unlicensed rehost); corpus ≈ 0 today
  (07 §5).

### Rung 5 — Collector catalogs & fan networks (existence pointers)

Position per 08 §9: a new rung between the platform rungs and purchase-only. These networks
are **existence metadata and human request channels, not stream URLs**; the registry stores
`EXISTS_NOT_STREAMABLE` / `KNOWN_MISSING` / `REVIEW` rows with source + grade + query used.

- **Identity/enumeration, per lane:**
  - **USA Sports on DVD** — strongest lead: 20,901 NBA games, public filterable HTML, each
    record carrying a **Basketball-Reference boxscore URL** (→ `game_id` via the BR slug),
    network, duration, format, and completeness notes; one-time crawl + periodic re-crawl.
  - **The Sports Archive (greggsportsvideos.com)** — 23,500+ events, verified from the 1962
    Finals onward; line grammar `{season} {round} Game {n} {away} {score} @ {home} {score}
    ({grade})(defects)` — match via score+round+teams → BR playoff index; alias table needed
    for era-true round labels ("1966 NBA ECSF", "1975 NBA EC1R").
  - **Pontel GmbH** — catalog pages crawlable per season/round; existence pointer
    `EXISTS_NOT_STREAMABLE` (purchase lane sits in rung 6).
  - **Personal collector catalogs** (Brad's Weebly pattern) — cheap one-time crawls of
    catalogs discovered via community mentions; owner's grade as metadata.
  - **Request/trade communities** — Interbasket video forum (quarterly keyword passes,
    thread titles → REVIEW), BigFooty-class long-lived tape threads (curated list, 90-day
    re-check), r/VintageNBA (keyword RSS + archivist-account watchlist), team subreddits
    (occasional finds). Identity-by-description ("describe the game and I'll tell you")
    confirms the human-review lane 04 already designed (08 §1–§4).
  - **Private trackers (MySpleen, TV-Vault)** — invite-only, no public listings; record as
    human-pointer only ("exists in private preservation circles"); **no automation** (08 §5).
  - **Lost Media Wiki** — the negative-space map: MediaWiki `categorymembers` on "Lost
    recordings of sports events" (427 pages); title grammar `{away} {score}-{home} {score}
    ({state} footage of {event}; {year})` → `KNOWN_MISSING`/partial notes; prunes hopeless
    pre-1980 re-scans and catches resurrections first (08 §6).
  - **Churn posture:** community lists die even with living authors; snapshot what you
    ingest into the local registry at ingest time, never hot-link; re-scans re-ingest
    (08 §9, §3.2).
- **Playback.** **Pointer** — never a stream URL; never automated download.
- **Legality.** **Grey, pointer-only** for the catalogs and trade channels (unlicensed
  copies; sale/trade/download of unlicensed digital copies); **clean** for Lost Media Wiki
  (existence metadata); forum/Reddit surfaces are lawful to read while their outcomes usually
  land on grey links — per-game human judgment (08 §1.1, §9 legality table).

### Rung 6 — Purchase-only

- **Identity/enumeration.** Known DVD/media titles (official NBA Entertainment/Warner
  classics; the IA "DVD Transfer" pattern shows they circulate), used VHS/DVD marketplaces,
  eBay commercial releases (saved keyword feeds `{team} {year} VHS/DVD`), Pontel purchase
  lane (custom-burned DVD; on-site license claim **unverified** today — grey for the sale
  itself, pointer recorded either way). NBA League Pass is the official paid archive —
  documented here as the official fallback that exists, **excluded by the $0 standing
  preference** (04 §S4; 08 §7, §9).
- **Playback.** **Pointer** — record "tape exists, not streamable" + pointer (e.g. the IA
  item for the official transfer); streaming a purchased copy's rip instead of buying is a
  per-user choice under the acquisition-method gate, not a ladder output.
- **Legality.** **Clean** when the acquisition is purchasing a licensed copy — this is the
  **Cache Tier's acquisition story** (buying a licensed copy, not downloading a rip);
  League Pass excluded by $0 preference; eBay home-recorded tapes grey (sellers' own
  ambiguity documented) (04 §S4; 08 §7).

### Rung 7 — Institutional & newsreel archives

- **Identity/enumeration.** **Paley Archive** (160,000+ programs incl. sports; demonstrably
  holds classic NBA games) and **UCLA Film & TV Archive** (kinescopes, 2" tape) — catalog
  lookups for pre-1980 gaps, last-resort pointer for pre-1970 Games. New from #7:
  **British Pathé** (85,000-film newsreel library) and Getty licensing stock — pre-1970
  1–2-minute **fragments**, never full games, licensing-gated playback; same S5 role:
  existence check, not a streaming source (07 §2.13; 04 §S5).
- **Playback.** **Pointer** — on-site research access / licensing desk; never stream URLs.
- **Legality.** **Clean** — licensed to the archive; no stream involved (04 §3.7).

---

## 3. Cross-rung machinery (carried over from v1, with deltas)

- **Identity key (unchanged, #3 contract).** `game_id = (season, game_date_utc,
  away_team_id, home_team_id)`; every hit from every rung reduces to this key. Alias set
  gains: non-English tokens (rung 3), era-true round labels (rung 5), star-name hooks
  (unchanged). New crosswalks: choucisan/nba_games → rung 2; USASD BR slug → rung 5; Gregg
  score/round grammar → rung 5; `nba.py` endpoints → rung 0 (archaeology).
- **Scoring (unchanged).** CONFIRMED / LIKELY / REVIEW / Reject over title + description +
  duration; 07 §4.3 confirms it applies verbatim to cluster listings surfaced with titles +
  durations.
- **Exhaustion definition (updated).** A Game is marked **unavailable** only when **rungs
  0–4** have each been queried with the Game's query set, each query recorded with timestamp
  and query text, and no candidate reached LIKELY+ — or when only rung 5–7 pointers exist
  (status `EXISTS_NOT_STREAMABLE`). For pre-1980 thin eras the rung 5 catalog sweep +
  Lost-Media-Wiki check are part of "trying very hard". Nothing is marked absent on a single
  failed search (04 §3.4, extended).
- **Quota (unchanged in shape).** The only quota'd surface remains YouTube `search.list`
  (rung 2, 100 calls/day); rungs 0–1 are quota-free, rungs 3–5 are search-engine queries,
  keyless APIs, and static-HTML crawls. Sweep priority: Finals/playoffs first, decades
  newest→oldest; rung 3 adds one `site:` query per platform per Game — cheap, no YouTube
  quota.
- **Registry (`tape_sources`, updated).** Schema unchanged
  (`game_id, rank, source_class, url_or_pointer, match_confidence, verified_at,
  query_used, notes`); `rank` now spans 0–7. UI rule: tape button when a **rung 0–4** entry
  is verified; otherwise `exists, not streamable (rung 5–7 pointer)` or
  `unavailable (ladder consumed <date>)`. Playback resolves the URL at play time — no
  rehost. Cache Tier: byte-eligible rungs are **1 and 4** (plus licensed purchase via
  rung 6); embed-only rungs **0, 2, 3 never feed it**.
- **Re-scan cadence (extended).** ABSENT Games re-checked after 90 days; rung 0
  re-enumerated monthly; rung 4 cheap standing re-scan; rung 5 re-crawls on NEW tags /
  quarterly keyword passes; community lists re-ingested, never trusted stale.

---

## 4. Not in the ladder (explicit non-drops)

Everything below surfaced in #7–#9 and is deliberately **not** a rung:

| Item | Source | Why not a rung |
|---|---|---|
| "The NBA Channel" FAST feed (Pluto TV / Tubi) | #7 §2.12 | Lawful, league-owned, free — but a live linear stream with **no per-game addressing, no search, no API**; cannot resolve `(season, date, away, home)`. Keep as an **ambient External Surface the Shell can link to**, not a ladder rung. |
| Replay-site family (`basketballreplays.net`, `basketball-video.com`, `nbareplayhd.com`, `nbabite.com`) | #9 §4 | Scrapeability proven by a CloudStream provider, but the proving repo has **no license**, the sites are unlicensed rehosts, and #9 explicitly declines endorsement; platform detail belongs to #7, which did not adopt them. Left out as a rung; kept as a grey cross-reference only. |
| Vimeo, Twitch, Facebook Watch, Rumble | #7 §2.8–2.11 | Auth-gated / ephemeral / search-less / bot-walled, and no classic corpus measured. Dead ends; out of the ladder. |
| Wikimedia Commons | #7 §3 | Free-licenses-only policy — proprietary broadcasts cannot exist there by construction. |
| RuTube, Niconico, Sibnet | #7 §3 | Geo/bot-gated from the probe vantage (Sibnet: anti-bot wall + browser-only embed despite being a known niche host). Re-check from another vantage if the ladder ever needs them; not rungs today. |
| BitChute, DTube/3Speak | #7 §3 | Corpus ≈ empty / subsumed by the Odysee measurement. |
| Peacock "NBA Classic Games" | #7 §2.12 | Subscription required — excluded by the $0 standing preference. |
| Invidious / Piped front-ends | #7 §3 | Access layers over the YouTube catalog, not sources. |
| Discord tape networks, NLSC mod scene, dead community directories, basketballforum.com, Facebook collector groups | #8 §4, §8, §10 | No publicly indexable tape network / virtual recreations only / lists rot with their authors / no tape subforum / login-walled. Lesson retained in rung 5: snapshot what you ingest. |
| `sportyfin`, Kodi NBA add-ons, Stremio/CloudStream sports providers, streamlink, torrent lane | #9 | Live-only, archived, or paid-League-Pass; none catalog Game Tape. sportyfin's M3U→Jellyfin tuner and m3u8-discovery techniques noted for the Player Backend if ever needed. |
| NBA stats libraries (`nba_api` etc.) | #9 | Data lane, video-free — #3's territory. |
| yt-dlp NBA extractors (six classes) | #9 §2 | All `_WORKING = False`; kept as rung-0 archaeology, not as working code. |

Nothing else from #7–#9 was left out: every platform, catalog, network, and code lead that
measured as live with classic-NBA content is placed in a rung above.

---

## 5. What changed vs v1 (04 §3.2)

1. **Rung 3 made concrete and clustered** (was: "Dailymotion, Vimeo, OK.ru, Bilibili and
   similar… no stable public APIs in general"). Now: `site:`-search pass over the measured
   non-Anglo cluster **VK, OK.ru, CDA.pl, Bilibili**, plus Dailymotion's keyless API as a
   cheap extra pass; platform APIs explicitly **not** the enumeration path (all gated —
   probes in #7).
2. **New rung 4: standing empty-corpus sweep.** Odysee + PeerTube — the only platforms whose
   bytes are reachable (progressive MP4, keyless), with measured yield ≈ 0; cheap standing
   re-scan.
3. **New rung 5: collector catalogs & fan networks** (#8), placed between platforms and
   purchase-only exactly as #8's handoff specifies: existence pointers and human request
   channels, never stream URLs; USASD's BR links + Gregg's score grammar make it the
   cheapest per-game existence sweep in the landscape.
4. **Rung 2 gains the choucisan/nba_games crosswalk** (#9): MIT-licensed, 189 verified games,
   NBA.com game IDs, `YYYY-MM-DD-away-vs-home` naming — bootstrap identity matching for the
   YouTube rung.
5. **Rung 0 gains the yt-dlp `nba.py` endpoint map** (#9) as re-derivation archaeology
   (extractors disabled upstream).
6. **Rung 7 absorbs the newsreel lane** (#7): British Pathé / Getty fragments join Paley and
   UCLA as pre-1970 existence pointers.
7. **FAST feed explicitly excluded** from the ladder (#7): lawful ambient surface linked
   from the Shell; no per-game addressing.
8. **Playback classes made explicit per rung** (CONTEXT.md terms): file (rungs 1, 4),
   External Surface (rungs 0, 2, 3), pointer (rungs 5–7) — and with them the Cache Tier
   boundary: bytes only ever enter from rungs 1/4 or a licensed purchase (rung 6); embed-only
   platforms have no app-side ingestion path.
9. **Exhaustion widened to rungs 0–4** (was 0–3), with rung 5 catalog sweeps + Lost Media
   Wiki checks as part of "trying very hard" for pre-1980 Games; the registry's `rank` field
   and UI rule renumbered accordingly.
10. **Bilibili classification refined** (#9): mixes licensed/geo-CN official content with
    unlicensed uploads; still rung 3, still grey overall, caveat recorded.
