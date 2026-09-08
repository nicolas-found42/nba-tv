# 07 — Free tape platforms beyond YouTube (nicolas-found42/nba-tv#7)

Resolves the #7 question: which free video platforms beyond YouTube hold NBA full-game
**Game Tape**, and what each one actually allows. Upgrades §S3 of
[04-tape-source-landscape.md](04-tape-source-landscape.md) ("Dailymotion, Vimeo, OK.ru,
Bilibili and similar… no stable public APIs in general") from a hand-wave to a measured
per-platform verdict: era coverage, search/enumeration API, playback protocol, ToS posture on
sports footage, and whether **tape bytes or only embedded players** are reachable from an app.
Fan communities/forums → sibling #8; open-source code leads → sibling #9; map edits → none.
All probes dated **2026-09-07**, metadata-only (no media downloaded). Legality gate per map
Notes: this doc classifies the **acquisition method**; grey paths are reported as grey.

**Verdict in one paragraph.** The sweep found **no new lawful, app-integrable Tape Source**:
every platform with real classic-NBA full-game tape (VK, OK.ru, CDA.pl, Bilibili — the
non-Anglo rehost cluster) is **embed-only or key-gated**, i.e. an **External Surface** by the
CONTEXT.md definition, in the same grey unlicensed-rehost class as YouTube fan channels; and
every platform whose bytes are genuinely reachable (Odysee, PeerTube — direct progressive MP4
over HTTP, no key, matching the #2 Player Backend decision) holds **~zero classic NBA tape**
(measured). The strongest lead is the rehost cluster itself — OK.ru and CDA.pl demonstrably
hold 1980s–90s full games the current Source Ladder would miss (1986 ECF G2 at 1:56:56; 1998
Finals G6 at 02:10:55; 1983-84 Finals G7) — enumerable for free via search-engine `site:`
queries, playable in a browser, never as app-resolved MP4. Secondary new lead: the league's
own free linear FAST channel ("The NBA Channel" on Pluto TV / Tubi) is a lawful ambient surface
with no per-game addressing. Dailymotion's thin corpus is explained by its ToS: Audible Magic +
INA fingerprinting auto-removes matched content. Dead ends (measured): Wikimedia Commons (by
policy), Twitch (14/60-day VOD expiry + no content search), Rumble (bot-walled, no public API,
current-era rehosts only), RuTube/Niconico (geo-gated from a US vantage), BitChute (empty),
Facebook Watch (no public search API). Verdict for the ladder: extend rung 3 of 04's design
with a `site:`-search pass over the rehost cluster; expect External-Surface playback only.

---

## 1. Scoreboard

Playback classes: **bytes** = direct file URL reachable from an app (MP4-over-Range per #2);
**embed** = playback inside the platform's own player/iframe only (External Surface);
**gated** = platform API/search requires keys/cookies/geo the app can't assume.

| Platform | Enumeration | Playback protocol | Bytes? | ToS on sports footage | Classic-NBA corpus (measured 2026-09-07) | Verdict |
|---|---|---|---|---|---|---|
| **VK Video** | REST `video.search` needs user token (probed: error 15); web watch pages public, search-engine indexed | Meta-owned player; embed `video_ext.php`; no public file URLs | no | UGC rehost — grey | **Yes — 1990s**: multiple 1998 Finals full games (1h53m–1h54m; uploads 2012–2024) | Live lead (grey) |
| **OK.ru** | REST API needs app key (probed: error 101); search-engine indexed | in-site player; embed widget | no | UGC rehost — grey | **Yes — 1980s**: 1986 ECF G2, related 1981 ECF (1:56:56) | Live lead (grey) |
| **CDA.pl** | `api.cda.pl` 401 (probe); search-engine indexed; topic pages | in-site player + `ebd.cda.pl` embed; direct URLs not public | no | hosting service, notice-and-takedown — grey | **Yes — 1980s–90s**: 1998 F G6 (02:10:55), 1991 F G5, 1983-84 F G7 | Live lead (grey, strongest) |
| **Bilibili** | search API → HTTP 412 risk control (probe); unofficial API needs cookies + WBI signing; search-engine indexed | DASH via unofficial API; auth/headers required | gated/unofficial | uploaders must hold copyright (English ToS) | **Yes — 1990s–2010s**: NBA经典回放 series (1997 F G5, 2016 F G7, 2018 ECF G2), 4K 1998 "Last Shot", full-replay series | Live lead (grey) |
| **Dailymotion** | Data API v2 search works **keyless** (probe) | embed only; stream URLs = paid Enterprise plan feature (probe: 403 `can-read-video-streams`) | no | prohibits infringing uploads; **Audible Magic + INA fingerprinting auto-removal** | **~None**: 1/100 top hits ≥70 min (a video-game longplay); era queries return intros/previews | Weak (clean API, empty corpus) |
| **Odysee (LBRY)** | `claim_search` JSON-RPC, keyless (probe) | direct **progressive MP4** via `get` → `streaming_url` (probe) | **yes** | uploaders warrant IP rights ($/tos §3) | **None**: era queries return noise (2K gameplay, vlogs, TikTok rips) | Weak (perfect API, empty corpus) |
| **PeerTube** | Sepia Search API keyless (probe); instance REST API open | **progressive MP4** `files[].fileUrl` + HLS (probe) | **yes** | per-instance moderation (federated) | **None found**: 58 hits, all noise | Weak (perfect API, empty corpus) |
| **Vimeo** | API requires OAuth (probed 401/8003) | embed for third-party videos; file access only as authorized | no | upload-only-with-rights; anti-scraping except via API | **None found**: `site:vimeo.com` finals queries empty | Dead end for tape |
| **Rumble** | search page → **403 bot wall** (probe); `/api/v0` → login redirect; embed probe → 410 | embed player | no | explicit prohibition + warrant + zero-tolerance (ToS); anti-scraping clause | current-era rehosts only (WNBA full game found); **no classics found** | Dead end for classics |
| **Twitch** | Helix Get Videos by id/user/game only (docs); no keyword search | Twitch player embed (HLS) | no | VODs auto-delete **14/60 days**; API exposes `muted_segments` (DMCA muting) | ephemeral present-era only | Dead end |
| **Facebook Watch** | no public video search (Graph API /search allowlist, deprecated types per v8.0 changelog) | Meta player; `plugins/video.php` embed | no | Meta content rules; enforcement opaque | unverifiable programmatically | Dead end |
| **Pluto/Tubi "NBA Channel" (FAST)** | none — linear channel, no per-game addressing | live linear feed (HLS) | no | league-owned, lawful, free | ambient classics, non-enumerable | Lawful ambient surface |
| **British Pathé** | site search (85k films) | site player; newsreel licensing | licensing, not free | archive-licensed | pre-1970 **fragments** (1–2 min newsreels), not full games | Existence pointer (S5 class) |
| Wikimedia Commons | MediaWiki API (probe) | n/a | n/a | **only free-licensed/PD, no fair use** | 75 hits, all photos | Dead end by construction |
| RuTube | search API returns empty for everything from US IP (probe) | unknown | — | — | unverifiable from this vantage | Dead end (geo) |
| Sibnet | search → `robot.php` anti-bot wall (probe) | embed (browser-only) | unverified | — | known niche host; not reachable from app | Dead end for app |
| Niconico | snapshot API → 403 CloudFront (probe) | — | — | — | unreachable from US vantage | Dead end (geo) |
| BitChute | HTML search works (probe) | embed | no | — | **~empty** (1 video link in results) | Dead end (content) |
| Peacock "NBA Classic Games" | subscription required (Roku listing) | — | — | licensed | exists, **paid** | Out of $0 scope |

---

## 2. Live leads

### 2.1 Dailymotion — clean keyless API, empty classic corpus, embed-only playback

- **Enumeration.** Data API v2 public search works with **no API key**: `GET
  https://api.dailymotion.com/videos?search=…&fields=id,title,duration` returned results
  unauthenticated in probes ([PublicAPIs listing confirms public reads are keyless](https://publicapis.io/dailymotion-api)).
  The `total` field was capped at 1000 with `has_more: true` in probes.
- **Measured corpus.** Top-100 results for "NBA full game": **1 of 100 ≥70 min** — and it is
  "NBA Bounce FULL GAME Longplay (PS5, PS4, Switch)" (12,868 s), i.e. a video game. A Polish
  "cały mecz NBA" pass: 0/100 ≥70 min. Era queries ("Bulls Jazz 1998 Game 6", "NBA Finals 1993
  Game 5 full", "Celtics Lakers 1987 finals") return intros, interviews, betting previews and
  NBA 2K gameplay — not Game Tape. (All: probes 2026-09-07.)
- **Playback protocol.** For content you don't own: **embed only**. Requesting
  `stream_hls_url` on a public video returned **HTTP 403** — *"Insufficient rights for the
  `fields` parameter… Required roles: can-read-video-streams, can-read-my-video-streams"*
  (probe). Official docs: stream URLs come from `POST /v2/videos/{id}/streams`, a **paid plan
  feature** ("Pro Enterprise plan with add-on Stream URL feature"), **time-limited and
  IP-locked**, serving **HLS/DASH/HbbTV — no progressive MP4 at all**
  ([Generate stream URLs](https://developers.dailymotion.com/docs/generate-stream-urls)).
- **ToS on sports footage.** Uploads "must not infringe intellectual property rights of any
  third-party" and uploaders warrant all necessary rights/clearances (Terms of Use
  [§6.1(iii)](https://legal.dailymotion.com/en/terms-of-use/), §6.2); the Prohibited Content
  Policy prohibits "any copyright-infringing content" (§J). §2.6 prohibits automated access
  "without a prior written approval" — programmatic access must go through the official API,
  not site scraping.
- **Why the corpus is thin — fingerprinting.** Dailymotion runs **Audible Magic** (audio) and
  **INA** (video) fingerprinting: "Whenever a piece of video Content uploaded… matches the
  digital fingerprint databases… the video Content will be automatically removed" (Prohibited
  Content Policy, §2). Classic NBA broadcasts are exactly the content such databases match →
  structural auto-takedown. Expect churn even for whatever exists.
- **Role in the ladder:** a cheap keyless metadata rung (like IA in cost), expected yield ≈0
  for classic full games; playback = External Surface.

### 2.2 VK Video (vk.com) — real 1990s corpus, gated API, player-only playback

- **Measured corpus.** Search-engine indexed VK watch pages show multiple full 1998 Finals
  games with durations in the listing itself: "1998 NBA Finals - Chicago Bulls vs Utah Jazz -
  Game 6… 1 ч 54 мин 29 с" ([vk.com/video-80014383_456240961](https://vk.com/video-80014383_456240961)),
  "NBA Finals 1998 _ Utah Jazz vs Chicago Bulls - Game 6 [FULL]… 1 ч 53 мин 39 с"
  ([vk.com/video-77298064_456239434](https://vk.com/video-77298064_456239434)), "1998 NBA
  Finals / Utah Jazz @ Chicago Bulls / Game 5 / HD… 1 ч 33 мин 16 с"
  ([m.vk.com/video-23345605_163212645](https://m.vk.com/video-23345605_163212645)), plus a
  VHS-digitization channel ("NBA Retro Basket Кассетное Видео",
  [vk.com/video-157249074_456239150](https://vk.com/video-157249074_456239150)). Watch pages
  state public playback "без регистрации" (without registration). Upload dates 2012–2024
  indicate a decade-plus stable corpus, unlike YouTube's takedown churn.
- **Enumeration.** REST API `video.search` **requires a user access token** — probed:
  `{"error_code":15,"error_msg":"Access denied: token required"}`. Practical enumeration =
  search-engine `site:vk.com` queries per game (works: the evidence above came from an
  unauthenticated search engine).
- **Playback protocol.** In-site player on watch pages; no public direct-file URL was
  observable from probes. Embed exists (`video_ext.php`) for sites. Bytes: not reachable →
  External Surface.
- **ToS.** VK's terms pages are login-walled to automated fetches (probe returned no text);
  classify acquisition as the same **grey unlicensed-rehost class** as YouTube fan channels
  (map §S2 note applies). Not endorsed; reported as grey.
- **Role:** rung-3 site-search pass; era value = 1990s (and likely 2000s) deep cuts YouTube
  lost to takedowns.

### 2.3 OK.ru (Odnoklassniki) — the 1980s hole-filler

- **Measured corpus.** Indexed OK.ru video pages include "NBA East Finals86 Boston Celtics -
  Atlanta Hawks (Game 2)" ([m.ok.ru/video/9784333634248](https://m.ok.ru/video/9784333634248))
  with related items "Celtics - Sixers (1981 Playoffs - Eastern Conference Finals)… 1:56:56" —
  i.e. **1980s playoff full games with durations consistent with complete broadcasts** (probe
  via search engine, 2026-09-07). This is the era where 04's estimated hit rate is 10–20% and
  YouTube fan channels skew to superstar teams — OK.ru holds non-star-team playoff tape.
- **Enumeration.** REST API is **app-key-gated**: probed
  `{"error_code":101,"error_msg":"PARAM_API_KEY : No application key"}`. The developer portal
  documents a search method family ([apiok.ru/dev/methods/rest/search](https://apiok.ru/en/dev/methods/rest/search/))
  but only for registered apps. Practical enumeration = `site:ok.ru` queries.
- **Playback.** In-site player; embed widget exists for content owners
  ([apiok.ru/ext](https://apiok.ru/en/ext/)). Bytes not reachable → External Surface.
- **Legality:** same grey rehost class. Not endorsed.
- **Role:** rung-3 site-search pass; era value = 1980s.

### 2.4 CDA.pl — Polish fan archive with complete Finals broadcasts

- **Measured corpus.** Indexed pages: "NBA Finals 1998 Game 6 Chicago Bulls @ Utah Jazz —
  **02:10:55**" ([cda.pl/video/2447762740](https://www.cda.pl/video/2447762740)); a topic page
  listing "NBA Finals 1991 - Game 5 - Chicago Bulls vs. Los Angeles Lakers"
  ([cda.pl/info/nba_finals](https://www.cda.pl/info/nba_finals)); user folders carrying "NBA
  1983-84 Finals Game 7 - Boston Celtics vs LA Lakers - [EN]"
  ([cda.pl/fajconsky_77](https://www.cda.pl/fajconsky_77/folder-glowny),
  [m.cda.pl/Grzegorz_Cholewa1](https://m.cda.pl/Grzegorz_Cholewa1/40)). Durations (2h10m55s)
  are complete-broadcast length, not condensed edits.
- **Enumeration.** Developer API is key-gated: `api.cda.pl/video/<id>` → **401 unauthorized**
  (probe); `www.cda.pl/api` → 404. Site search redirects to topic pages for scripts. Practical
  enumeration = search-engine `site:cda.pl` queries per game.
- **Playback.** In-site player plus an embed player (`ebd.cda.pl/620x368/{id}` observed in the
  indexed embed variant). Direct file URLs not public. External Surface.
- **ToS.** CDA positions itself as a **hosting service**: users store "Materiały", the
  administrator "does not interfere with the content of stored data" (Regulamin §3.1, §6.2)
  and runs notice-and-takedown for "praw autorskich" (copyright) (§10.1,
  [cda.pl/regulamin](https://www.cda.pl/regulamin)). Grey rehost class; not endorsed.
- **Role:** rung-3 site-search pass; era value = 1980s–90s Finals; the single strongest new
  lead for complete Finals broadcasts outside the official corpus.

### 2.5 Bilibili — the Chinese-broadcast mirror era

- **Measured corpus.** Indexed Bilibili search results show a standing 【NBA经典回放】 (NBA
  Classic Replay) upload pattern: 1997 Finals G5 Bulls–Jazz, 2016 Finals G7 Cavaliers–Warriors,
  2018 ECF G2, a **4K HDR 1998 Finals "Last Shot" G6**, and a (全场回放系列) full-game-replay
  series (Lakers vs Cavaliers, 01:09:03)
  ([search.bilibili.com results](https://search.bilibili.com/all?keyword=n%2520ba/),
  [BV1ecgh6uEZG](https://www.bilibili.com/video/BV1ecgh6uEZG/)). Content spans 1990s–2010s,
  much of it Chinese-commentary broadcasts (CCTV5-era for 2000s) — a distinct commentary
  variant, useful as a fallback source when no English-language copy survives.
- **Enumeration.** The public search endpoint is behind aggressive risk control: probed
  `api.bilibili.com/x/web-interface/search/type` → **HTTP 412** captcha/risk-control page.
  The community API documentation ([SocialSisterYi/bilibili-API-collect](https://github.com/SocialSisterYi/bilibili-API-collect))
  documents the required cookie/WBI-signing machinery for official endpoints. Practical
  enumeration = search-engine `site:bilibili.com` queries (works: evidence above).
- **Playback.** DASH via unofficial endpoints with mandatory headers (Referer/cookies) —
  gated/unofficial; not app-integrable as progressive MP4. External Surface.
- **ToS.** English Terms of User Service: for uploaded content "you guarantee that you have
  legitimate copyright or corresponding authorization towards it", and Bilibili may delete or
  block content after third-party claims
  ([international_en](https://www.bilibili.com/blackboard/protocal/international_en.html),
  [user agreement](https://www.bilibili.com/blackboard/era/44ZJAVYNG9lkl24M.html)). Grey
  rehost class.
- **Role:** rung-3 site-search pass; era value = 1990s→present, Chinese commentary.

### 2.6 Odysee (LBRY) — the only keyless bytes-reachable platform; corpus ≈ 0

- **Enumeration.** Odysee's JSON-RPC proxy accepts unauthenticated `claim_search`
  (`POST https://api.na-backend.odysee.com/api/v1/proxy?m=claim_search`) — probed working.
  Text search is weak and capped (`total_items` = 10000 in probes).
- **Measured corpus.** Queries "NBA Finals full game" (+video filter) → **0 items**;
  "basketball", "NBA full game", "1998 NBA Finals Bulls Jazz", "Michael Jordan full game" →
  noise: NBA 2K captures, TikTok rips, travel vlogs, music (probes 2026-09-07). No classic NBA
  Game Tape exists there in measurable quantity.
- **Playback protocol.** The one probe that matters for #2's player decision: `get` with
  `save_file:false` returns **`streaming_url: https://player.odycdn.com/v6/streams/<claim>/<hash>.mp4`**
  — a **direct progressive MP4**, exactly the bytes-over-Range shape the Player Backend wants.
  (Probe 2026-09-07.)
- **ToS.** "By posting any Content… you represent and warrant that you have the lawful right,
  including all necessary intellectual property rights, to distribute and reproduce such
  Content" ([Odysee ToS §3](https://odysee.com/$/tos)). Content would still be grey rehost
  uploads if it existed.
- **Verdict:** mechanism champion, content ghost. Keep as a ladder rung **only** because
  enumeration + playback are free, keyless, and MP4-native; expected yield ≈0. Re-scan cheaply.

### 2.7 PeerTube + Sepia Search — federated, bytes-reachable, empty

- **Enumeration.** [Sepia Search](https://sepiasearch.org) aggregates public instances and
  exposes the standard REST search keylessly (`GET
  https://sepiasearch.org/api/v1/search/videos?search=…`) — same shape as the per-instance
  API ([REST reference, operation searchVideos](https://docs.joinpeertube.org/api-rest-reference.html#tag/Video/operation/searchVideos)).
  Probed working.
- **Measured corpus.** "nba finals full game" → 58 results, all noise (a German politics
  video, an "Inside The NBA" studio show at 46 min, NBA-2K-adjacent content). No classic NBA
  full game found (probes 2026-09-07).
- **Playback protocol.** Fully open: the per-video REST call returns `files[].fileUrl` —
  **progressive MP4 per resolution** — plus `streamingPlaylists[].playlistUrl` (HLS) and
  `fileDownloadUrl` (probe on a live instance, 2026-09-07). This is the second
  bytes-reachable, keyless platform in the sweep, and it matches the #2 progressive-MP4
  player decision natively.
- **Moderation.** PeerTube is federated software; moderation policy is **per instance**
  ([joinpeertube.org](https://joinpeertube.org/)). A hit on some instance must be checked
  against that instance's rules; in practice any NBA upload would be a grey rehost.
- **Verdict:** same as Odysee — perfect mechanism, empty corpus. Cheap standing rung.

### 2.8 Vimeo — auth-gated API, embed-only, no classic corpus found

- **Enumeration.** Unauthenticated `GET https://api.vimeo.com/videos?query=…` → **401,
  error 8003 "The app didn't receive the user's credentials"** (probe); `/search` likewise 401.
  The API does expose a "Federated Search Items" response type
  ([reference](https://developer.vimeo.com/api/reference/response/federated-search-items)) — a
  search surface exists, behind OAuth.
- **ToS / scraping.** Permitted use includes "**Stream videos that you have the right to
  view**"; the Acceptable Use section prohibits accessing/downloading content "except as
  expressly authorized", and automated access is allowed only "using our APIs, in accordance
  with our API License Addendum" ([Vimeo Terms of Service](https://vimeo.com/terms), §3, §5).
  Uploaders "may only upload content that you have the right to upload", with repeat-infringer
  termination (§5.1).
- **Playback protocol.** For third-party videos the API returns embed/play URLs, not files;
  file access is limited to content you are authorized to access (ToS §5; corroborated by
  developer reports that responses for others' videos "will not contain the files or download
  keys" — [Stack Overflow](https://stackoverflow.com/questions/15658543/vimeo-video-downloading-through-api),
  secondary source). Bytes not reachable → External Surface at best.
- **Measured corpus.** `site:vimeo.com` "NBA Finals" full/complete-game queries returned no
  classic full games (web search 2026-09-07). Vimeo's paid-tier creator base never hosted a
  classic-NBA rehost corpus of any size.
- **Verdict:** dead end for tape; keep out of the ladder.

### 2.9 Rumble — bot-walled, partner-only API, current-era rehosts

- **Reachability probes.** Search page → **HTTP 403** from scripts (even with a browser UA);
  `rumble.com/api/v0/` → 301 (login redirect); embed endpoint → 410 for a fabricated id
  (probes 2026-09-07). No public content-search API; the community Platform API references
  that circulate are not an official public offering (only second/third-party mentions, e.g.
  [Reddit](https://www.reddit.com/r/RumbleForum/comments/1p4v9sp/new_to_rumble/) — anecdotal).
- **ToS.** Rumble's Terms explicitly prohibit content "subject to copyright by another person
  unless… fair use or… expressly authorized", require uploaders to warrant that submitted
  content "does not contain third party copyrighted material", declare **zero tolerance** for
  copyright infringement, and prohibit "**Systematic retrieval of data or Content…** to create
  or compile" ([rumble.com/s/terms](https://rumble.com/s/terms)) — i.e. scraping the catalog is
  contractually out.
- **Measured corpus.** Web-indexed Rumble shows **current-era** full basketball games via
  automated "US Sports" re-upload channels (e.g. a WNBA Connecticut Sun vs Indiana Fever "FULL
  GAME" upload, [rumble.com/v7euk0c](https://rumble.com/v7euk0c-us-sports-basketball-feat.-connecticut-sun-vs.-indiana-fever-full-game-high.html));
  no classic NBA full games surfaced. Era value ≈ present season only, and that overlaps what
  official channels already cover.
- **Verdict:** dead end for classics; embed-only; not in the ladder.

### 2.10 Twitch — ephemeral VODs, no content search

- **Enumeration.** The Helix video surface enumerates **by video id, by broadcaster, or by
  game** (game_id capped ~500 results) — there is no keyword content search in this surface
  ([Twitch API docs, Videos](https://dev.twitch.tv/docs/api/videos/)).
- **Retention.** "Twitch automatically deletes VODs after **14 days** for normal broadcasters
  and **60 days** for all others like partners" (same docs). Even a hit is short-lived.
- **Playback.** Via Twitch's own player/embed ([Embedding Video and
  Clips](https://dev.twitch.tv/docs/embed/video-and-clips/)) — HLS, no progressive MP4;
  bytes not reachable.
- **Rights signal in the API itself.** Video objects carry a `muted_segments` field —
  Twitch's DMCA audio-muting enforcement is visible in the data model (docs above). Full
  game-broadcast VODs are precisely what gets muted/taken down.
- **Verdict:** dead end. Archive ≠ Twitch's model.

### 2.11 Facebook Watch — no public search, Meta-player only

- **Enumeration.** Meta's Graph API `/search` surface has been progressively stripped: the
  v8.0 changelog deprecates `type=place`, `type=audience_interest`, `type=adzipcode` and
  friends, leaving a narrow allowlist that never included video content search
  ([Graph API changelog v8.0](https://developers.facebook.com/docs/graph-api/changelog/version8.0/)).
  There is no public way to enumerate NBA watch-page video programmatically.
- **Playback.** Inside Meta's player; external sites embed via
  `facebook.com/plugins/video.php?href=…` ([Embedded Video Player docs](https://developers.facebook.com/docs/plugins/embedded-video-player)).
  No public stream URLs. External Surface at best.
- **Measured corpus.** NBA full-game reuploads on Facebook are real but sit behind
  login/personalized surfaces and cannot be verified or enumerated from an app context
  (probes + search-engine checks 2026-09-07 found no stable public index).
- **Verdict:** dead end for the app; not in the ladder.

### 2.12 FAST/AVOD: "The NBA Channel" on Pluto TV and Tubi — lawful, free, non-enumerable

- The NBA's official channel is offered **free** on Pluto TV
  ([pluto.tv/us/watch/live-tv/18228](https://pluto.tv/us/watch/live-tv/18228/)) and Tubi
  ([tubitv.com/live/400000116/the-nba-channel](https://tubitv.com/live/400000116/the-nba-channel)) —
  both pages state "The NBA's official channel, now available on Pluto TV/Tubi". It mixes
  news, features and classic games as a **linear** feed (fan reports of "a random NBA game" on
  the channel — [r/nba](https://www.reddit.com/r/nba/comments/1lt8wop/is_there_a_way_to_access_completely_random_nba/),
  anecdotal).
- **Why it matters and why it isn't a Tape Source:** it is league-owned (cleanest legality of
  anything in this sweep) and free — but it is a **live linear stream with no per-game
  addressing, no search, no API**. It cannot resolve `(season, date, away, home)` to a game.
  Classify as an ambient External Surface the Shell can link to, not a ladder rung.
- Related, out of scope by the $0 preference: "NBA Classic Games" via **Peacock** is listed
  with "Subscription required" ([Roku listing](https://www.roku.com/whats-on/tv-shows/nba-classic-games?id=162a54a205f996b5df21e1915bccfae7)).

### 2.13 Newsreel archives (British Pathé) — pre-1970 fragments, existence pointer

- [British Pathé](https://www.britishpathe.com/search) licenses an 85,000-film newsreel
  library; basketball newsreels of the era exist (e.g. a 1967 Kentucky college-final newsreel
  on its YouTube mirror). Newsreels are 1–2-minute **fragments**, never full games, and
  playback is licensing-gated, not free. Same role as S5 institutions in 04: a pre-1970
  **existence check**, not a streaming source. (Getty similarly stocks "NBA 60s" clips for
  licensing — [gettyimages.com/videos/nba-60s](https://www.gettyimages.com/videos/nba-60s).)

---

## 3. Dead ends (with reasons)

| Platform | Reason it dies |
|---|---|
| **Wikimedia Commons** | Policy: "Wikimedia Commons only accepts free content… [or] public domain… does not accept fair use" ([Commons:Licensing](https://commons.wikimedia.org/wiki/Commons:Licensing)). Proprietary NBA broadcasts cannot exist there by construction; API file-namespace search returned 75 hits, **all photographs** (watch-party JPGs etc.) (probe 2026-09-07). |
| **Twitch** | VODs auto-delete 14/60 days; no keyword content search; embed/HLS only (§2.10). |
| **Rumble** | Bot wall (403), login-gated API, contractual anti-scraping; corpus = current-era rehosts only (§2.9). |
| **Facebook Watch** | No public video search API; Meta-player embed only; corpus unverifiable (§2.11). |
| **Vimeo** | OAuth-gated API; embed-only for third-party content; no classic corpus found (§2.8). |
| **RuTube** | Search API returns empty for **every** query including generic "баскетбол" from a US IP — geo/IP-gated; content unverifiable from this vantage (probe 2026-09-07). Re-check from an EU/RU vantage if the ladder ever needs it. |
| **Sibnet (video.sibnet.ru)** | Search redirects script traffic to an anti-bot page (`robot.php`) (probe); browser-only embed; no API. A known niche host of classic tape, but not reachable from an app without a browser session. |
| **Niconico** | Snapshot search API blocked with 403 (CloudFront) from a US vantage (probe); Japanese geo/bot gating; NBA corpus would be negligible anyway. |
| **BitChute** | HTML search works but the NBA corpus is ~empty (one video link in results) (probe 2026-09-07). |
| **Blockchain video (DTube/3Speak-class)** | Same network family as Odysee with a far smaller corpus; subsumed by the Odysee measurement (§2.6). |
| **Peacock "NBA Classic Games"** | "Subscription required" (Roku listing) — excluded by the $0 standing preference. |
| **Invidious/Piped-class YouTube front-ends** | Not sources: they are access layers over the YouTube catalog the Source Ladder already queries; any engineering treatment belongs to the code-leads sweep (#9). |

---

## 4. What this changes in the Source Ladder (handoff to 04's §3)

1. **Rung 3 ("Other platforms") becomes concrete and clustered.** Replace the vague
   "per-platform site search" with: one search-engine `site:` query per game per platform over
   **VK, OK.ru, CDA.pl, Bilibili** (the measured-content platforms), then Dailymotion's
   keyless API as a cheap extra pass. Platform APIs are **not** the enumeration path (all
   gated: VK error 15, OK.ru error 101, CDA 401, Bilibili 412 — probes above).
2. **Everything on rung 3 is an External Surface.** None of these platforms exposes bytes the
   Player Backend can resolve (#2's progressive-MP4 decision stands unchallenged); playback =
   browser/embed. The only bytes-reachable platforms (Odysee, PeerTube) are the empty ones —
   keep them as a cheap standing rung (keyless JSON/REST, MP4-native) with expected yield ≈0.
3. **Match-confidence note for the cluster.** These surfaces surface titles + durations in
   their listings (the evidence above was gathered from public listing metadata); 04's
   CONFIRMED/LIKELY scoring (date + both teams + duration ≥ ~70 min) applies unchanged.
   Non-English titles (Bilibili: NBA经典回放 / 全场回放; VK: Финал НБА) belong in the alias
   template set from 04 §3.3.
4. **Grey-path handling.** The cluster is the same acquisition class as YouTube fan channels
   (§S2 note in 04): streaming a public URL is the acquisition method; per the map gate it is
   reported as grey, never endorsed. Because playback is embed-only, there is **no** legitimate
   app-side ingestion path from these platforms into the Cache Tier — fetching bytes from them
   would require third-party download tooling, which is outside the Player Backend contract.
5. **Two lawful free surfaces to keep visible in the Shell:** the league's linear NBA Channel
   on Pluto/Tubi (ambient, no per-game addressing) and the S0 NBA ID catalog (already the map's
   anchor). Neither changes per-game resolution.

## 5. Legality summary (acquisition-method gate)

| Platform class | Acquisition | Classification |
|---|---|---|
| Odysee, PeerTube, Dailymotion API, FAST channels | keyless public API / free stream | lawful method; content, where it exists, is the uploader's licensing problem (grey if unlicensed rehost) |
| VK, OK.ru, CDA.pl, Bilibili, Sibnet, Rumble | stream public URL / embed | **grey** — unlicensed rehost class, same note as 04 §S2; reported, not endorsed |
| Vimeo, Twitch, Facebook Watch | embed per platform terms | lawful embed, but no tape corpus for classics |
| Peacock | payment | excluded ($0 preference) |
| British Pathé | licensing | existence pointer only (S5) |

## Method

- **Live probes (2026-09-07, no media downloaded):** Dailymotion Data API v2 (search + stream
  fields); Odysee JSON-RPC `claim_search` ×6 queries + `get`; Sepia Search API; PeerTube
  instance REST (framatube.org / tube.tchncs.de); Vimeo API (unauth 401 probes); VK
  `video.search` (error 15); OK.ru REST (error 101); CDA API (401); Bilibili search (412);
  RuTube search (empty ×4 queries); Niconico snapshot (403); Sibnet search (anti-bot
  redirect); Rumble (403/301/410); BitChute HTML search; Wikimedia Commons API search;
  Dailymotion/VK/CDA/Vimeo/Rumble/Odysee ToS and policy pages; Dailymotion stream-URL docs;
  Twitch videos/embed docs; Facebook Graph API changelog.
- **Content-evidence searches:** search-engine `site:`-scoped queries for VK, OK.ru, CDA.pl,
  Bilibili, Vimeo, Rumble (titles + durations as listed on the platforms' own public pages).
- **Awesome-lists corpus:** section discovery for "video platforms" returned no relevant
  platform leads (GDPR service lists, text-to-video platforms, media libraries) — the
  platform set above stands as the sweep.
- **Known limits:** probes ran from a single US vantage; RuTube/Niconico/Sibnet verdicts are
  vantage-dependent and flagged as such. All content counts are floors, not exhaustive
  catalogs.
