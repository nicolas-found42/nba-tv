# 09 — Open-source code sweep for Tape Source leads

Ticket: [#9](https://github.com/nicolas-found42/nba-tv/issues/9) · map: [#1](https://github.com/nicolas-found42/nba-tv/issues/1) · researched 2026-09-07.
Question: what does open-source code already know about NBA Game Tape sources that earlier sweeps missed?
Baseline for "new": the ticket-#4 Source Ladder rungs — official NBA free tier (YouTube/NBA App), Internet Archive, fan archival YouTube channels. Anything else is a new Tape Source lead.
Non-goals honored: platform/community deep-dives belong to #7/#8; map untouched. Legality gate (map Notes): acquisition method must be lawful; grey paths are reported as grey and never endorsed.

Method: GitHub repo + code search (`search/repositories`, `search/code`), gh-grep literal-code search over 1M+ indexed repos, the awesome-lists curated corpus, the yt-dlp/streamlink source trees, and web search to corroborate each source. Every claim below cites the repo, file, or page that owns it.

## Verdict

- **Strongest lead: [`choucisan/nba_games`](https://github.com/choucisan/nba_games)** — an MIT catalog that crosswalks **189 verified full-length NBA games on YouTube** to official NBA.com game IDs (`0021500874`-style), named `YYYY-MM-DD-away-vs-home`, with box scores and play-by-play. It is a ready-made per-game identity crosswalk for the YouTube rung of the Source Ladder, and proof that the big full-games playlist it was built from is machine-minable.
- **New Tape Sources surfaced by code** (not YouTube/NBA App/IA): **VK Video**, **Bilibili**, **Dailymotion** — each has a dedicated NBA full-game/replay presence and a maintained yt-dlp extractor. Reported grey where uploads are unlicensed; acquisition shape (public page → stream URL, no DRM circumvention) is the same posture as the existing YouTube fan-channel rung.
- **Replay-site family** (`basketballreplays.net`, `basketball-video.com`, `nbareplayhd.com`, `nbabite.com`) is confirmed scrapeable by open-source code (CloudStream provider), but these are platform-lane (#7) targets and unlicensed rehosts — grey.
- The rest of the code landscape is **dead, paid, or live-only**: all six yt-dlp NBA extractors are disabled as broken; every Kodi NBA add-on requires a paid League Pass; Stremio/CloudStream sports providers are live-stream-only; no archive.org NBA tooling, no torrent cataloging, no streamlink plugin exists.

## Lead inventory

### 1. `choucisan/nba_games` — full-game catalog + identity crosswalk (strongest)

- Source: [repo README](https://github.com/choucisan/nba_games) · [Hugging Face `choucsan/NBA_Games`](https://huggingface.co/datasets/choucsan/NBA_Games) (live, license MIT, DOI minted).
- Built from YouTube playlist `PLNbBj4TorBWerlzB1A5iM3XjwigqFG8sY` ("one of the most widely referenced NBA full-game collections"): 595 raw entries → 217 valid full-game candidates → **189 games verified** against NBA.com date pages, ~347 hours total.
- Each record: YouTube video ID + URL, cleaned matchup, verified game date, duration; folder name `YYYY-MM-DD-away-vs-home` uses **official NBA.com home/away order**; box-score rows carry `game_id` (official NBA game ID) and `nba_game_url`.
- Pipeline includes historical-franchise normalization (`PHL/PHI`, `SAN/SAS`, `NJN/BKN`, `NOH/NOP/CHA`, `SEA/OKC`) — the same identity problem our ladder must solve.
- Reuse: MIT (GitHub + HF both state MIT). Distributes no video; recommends yt-dlp for retrieval and asks users to comply with YouTube ToS.
- Verdict: not a new *source* (YouTube rung), but a new **catalog artifact** — drop-in bootstrap for per-game YouTube identity matching; era coverage is classic-era skew (playlist-driven), so it complements rather than replaces the official free tier.

### 2. `yt-dlp/yt-dlp` `extractor/nba.py` — the NBA.com endpoint map (documented, currently broken)

- Source: [yt_dlp/extractor/nba.py](https://github.com/yt-dlp/yt-dlp/blob/master/yt_dlp/extractor/nba.py) (read in full, 2026-09-07). All extractor classes carry `_WORKING = False`: `NBAWatchIE`, `NBAWatchEmbedIE`, `NBAWatchCollectionIE`, `NBAIE`, `NBAEmbedIE`, `NBAChannelIE`.
- Endpoints it documents (the reusable part):
  - Solr video catalog: `neulionscnbav2-a.akamaihd.net/solr/nbad_program/usersearch` — queryable by `seoName`/`pid`, returns name/description/releaseDate/runtime/tags.
  - Stream resolution: `watch.nba.com/service/publishpoint?type=video&format=json&id=<pid>` → HLS/MP4 path.
  - Collection listing: `content-api-prod.nba.com/public/1/endeavor/video-list/collection/<id>` — paginated public catalog of nba.com/watch collections (the classics/Finals collections the ladder targets).
  - Team-site video API: `api.nba.net/2/{team}/video,imported_video,wsc/` with a hard-coded `accessToken: internal|bb88df6b…`, returning `mp4`/`m3u8` URLs.
  - Direct file CDNs via Turner CVP: progressive MP4 at `nba.cdn.turner.com/nba/big`, HLS VOD at `nbavod-f.akamaihd.net`.
- Access shape: NBA.com free video tier (recaps/collections), no auth for free items; League Pass games gated. ToS posture: NBA.com ToS applies; these are official free surfaces (External Surface candidates).
- License: **Unlicense** (public domain) — the endpoint map is freely reusable.
- Verdict: not new as a source (official NBA tier), but the only public documentation of how to enumerate it programmatically. Extractors disabled ⇒ any build-side use means re-deriving against today's site; treat as a starting map, not working code.

### 3. New-platform extractors: VK Video, Bilibili, Dailymotion (new Tape Sources)

- **VK Video** — dedicated NBA full-game presence: channel [@all_about_nba](https://m.vkvideo.ru/@all_about_nba) ("AANBA | NBA broadcasts | NBA in Russia", **28,587 videos** of game records with Russian commentary) and playlist [vkvideo.ru/playlist/-233508484_3](https://vkvideo.ru/playlist/-233508484_3) ("NBA FULL GAMES"). Direct page fetch is bot-hostile (redirect loop in our read), consistent with VK's anti-scraping; yt-dlp's [`vk.py`](https://github.com/yt-dlp/yt-dlp/blob/master/yt_dlp/extractor/vk.py) `VKIE` resolves VK video pages to streams (Unlicense). ToS posture: user-uploaded unlicensed footage → **grey** (public streaming pages; same family as YouTube fan uploads, never endorsed).
- **Bilibili** — Chinese full-game replay community: search surfaces dedicated NBA full-game/replay listings ([`NBA全场高清中文录像回放`](https://search.bilibili.com/all?keyword=NBA%E5%85%A8%E5%9C%BA%E9%AB%98%E6%B8%85%E4%B8%AD%E6%96%87%E5%BD%95%E5%83%8F%E5%9B%9E%E6%94%BE)) and official-account classic replays (哔哩哔哩篮球赛事, e.g. 【NBA经典回放】2016-02-28 GSW vs OKC); a community video also discusses Bilibili holding rights to past-game replays ([BV1vL3o6UEpF](https://www.bilibili.com/video/BV1vL3o6UEpF/)) — so the surface mixes licensed/geo-CN official content (cleaner) with unlicensed uploads (**grey**). Extractor: [`bilibili.py`](https://github.com/yt-dlp/yt-dlp/blob/master/yt_dlp/extractor/bilibili.py) (Unlicense).
- **Dailymotion** — dedicated replay channels, e.g. [user `sporthdlive`](https://www.dailymotion.com/user/sporthdlive/) ("Watch NBA Full Game Replays Free Online"); extractors [`dailymotion.py`](https://github.com/yt-dlp/yt-dlp/blob/master/yt_dlp/extractor/dailymotion.py) include `DailymotionIE` + `DailymotionSearchIE`/playlist classes, i.e. per-user enumeration is built in. Unlicensed reuploads → **grey**.
- Verdict: three new Tape Source candidates for the map — fan-upload platforms outside the #4 ladder. All are External Surfaces (stream, never bundle); none cover pre-1980 scarcity better than the existing ladder; none have NBA-specific open-source scrapers (the generic extractors are the only code).

### 4. CloudStream `BasketballReplays` provider — proof the replay-site family is scrapeable

- Source: [Kraptor123/Cs-Karma](https://github.com/Kraptor123/Cs-Karma) (no license file; active 2026-09) — `BasketballReplays/src/main/kotlin/com/byayzen/BasketballReplays.kt`: `mainUrl = "https://basketballreplays.net"` with paged "All Matches" listing, uCoz-style search (`/search/?q=…&m=site&m=publ`), and a bundled `Filemoon.kt` embed resolver (Filemoon = the video host grey replay sites use).
- Access shape: HTML listing → embed page → hosted stream; ToS posture: unlicensed rehost site → **grey**; platform detail belongs to #7 (`basketballreplays.net`, `basketball-video.com` appear in [fmhy/edit `docs/video.md`](https://github.com/fmhy/edit/blob/main/docs/video.md) Sports Replays and [champagne-wiki live-sports.mdx](https://github.com/champagnewiki/champagne-wiki/blob/main/movies-shows-sports/live-sports.mdx)).
- Corroborating breadth: adblock/host lists widely reference the family (`nbabite.com`, `givemenbastreams.com` in [bebasid domainlist](https://github.com/bebasid/bebasid/blob/master/dev/domainlist); `nbareplayhd.com` anti-adblock filters in [ghostery/adblocker](https://github.com/ghostery/adblocker/blob/master/packages/adblocker/assets/ublock-origin/filters.txt) — MIT/MPL lists, evidence not scrapers).
- Verdict: no license ⇒ code is reference-only. Confirms the sites are technically minable, but the legality gate and #7's ownership make this a cross-reference, not a ladder rung we endorse.

### 5. `axelmierczuk/sportyfin` — Jellyfin integration pattern (archived, live-oriented)

- Source: [repo](https://github.com/axelmierczuk/sportyfin) (MIT, **archived**, last push 2022-04; README marked "NO LONGER MAINTAINED").
- What it does: scrapes configurable live-stream sites (`stream_link` env; paths `…/api/nba-tournaments?date=…`, `/streams-table/…`, origins like `reddit.rnbastreams.com` in [`util/scraping.py`](https://github.com/axelmierczuk/sportyfin/blob/main/sportyfin/util/scraping.py)), discovers m3u8 by sniffing page HTML and Selenium network logs, bypasses bitly gateways, then emits M3U tuned into **Jellyfin Live TV**.
- Verdict: dead end for archive purposes (live-first, upstream sites unmaintained, grey streams), but the **M3U→Jellyfin tuner pattern and m3u8-discovery techniques are reusable** if the Player Backend ever needs generic stream discovery.

### 6. Kodi NBA add-ons — paid League Pass only (dead end)

- [maxgalbu/xbmc.plugin.video.nba](https://github.com/maxgalbu/xbmc.plugin.video.nba) (GPL-3.0): "watch NBA games with nba league pass"; [chamchenko/plugin.video.nbainternational](https://github.com/chamchenko/plugin.video.nbainternational) (GPL-2.0): "Requires an Active NBA International League Pass"; [russholio/kodi-nba-league-pass](https://github.com/russholio/kodi-nba-league-pass) (GPL-3.0, 2016); stevmert's [NBA International League Pass repo](https://stevmert.github.io/nba.leaguepass.repo/repo/) ([forum thread](https://forum.kodi.tv/showthread.php?pid=3000768)).
- Verdict: all four consume the paid League Pass service; their "archive/condensed games" are the NBA App baseline already on the ladder. No free-archive Kodi add-on exists. Paid — excluded by map constraints.

### 7. Stremio / CloudStream sports providers — live-only, grey (dead end)

- [hetp4401/nbastreams-Stremio-Addon](https://github.com/hetp4401/nbastreams-Stremio-Addon) (no license, 2021, targets the dead `nbastreams.xyz` era); [jpants36/stremio-addon-ppvstreams](https://github.com/jpants36/stremio-addon-ppvstreams) (MIT, PPV/live); `WebStaticCS/Addon-Sport-` now 404s; [Null9960/streamsports99-stremio](https://github.com/Null9960/streamsports99-stremio) is live-events; [821938089/cloudstream-extensions](https://github.com/821938089/cloudstream-extensions) `Stream1Provider` aggregates `/nbastreams`-style live pages (license unknown).
- Verdict: every hit is live-stream plumbing over grey aggregators; none catalogs Game Tape. Dead end.

## Dead ends and why

| Lane | Result | Why / evidence |
|---|---|---|
| streamlink | No NBA/basketball plugin | Plugin dir contains only `sportal.py`, `sportschau.py` ([tree](https://github.com/streamlink/streamlink/tree/master/src/streamlink/plugins)) |
| yt-dlp replay-site support | Absent | `supportedsites.md` has zero hits for `nbabite`/`basketball-video`/`nbareplay`/`streamed` (checked 2026-09-07) |
| archive.org NBA tooling | No uploader/enumerator code found | Code + repo searches for archive.org + NBA full-game enumeration returned nothing NBA-relevant; IA rung stays as surveyed in #4 |
| Torrent lane | No NBA-specific cataloging code | Repo searches ("nba games download", stremio torrent add-ons) surface only generic torrent infra (Torrentio/Jackett-class); grey, no per-game NBA catalog ⇒ dead end |
| r/nbastreams-era scrapers | Dead | Domains (`nbastreams.xyz`, `givemenbastreams.com`, `rnbastreams.com`) survive only in adblock/host lists ([bebasid](https://github.com/bebasid/bebasid/blob/master/dev/domainlist), [Adblock4limbo](https://github.com/limbopro/Adblock4limbo)); [awesome-piracy](https://github.com/Igglybuff/awesome-piracy) (CC0) still lists the dead subreddit links |
| Curated awesome-lists corpus | Nothing NBA-tape-relevant | Corpus searches ("NBA", "basketball streaming", "sports streaming replays") return only tech-streaming infra (Spark/NATS-class) and video-game noise; the piracy/fmhy lists above surfaced via code search, not the corpus |
| NBA stats libraries (`nba_api` etc.) | Video-free | Stats/box-score only (e.g. `gmf05/nba` scrapes NBA.com data, no video; license none) — data lane is #3's territory |

## Ladder implications (for the map, not edits here)

1. **Catalog rung**: adopt `choucisan/nba_games` (MIT) as the bootstrap identity crosswalk for the YouTube rung; its NBA.com-verified `YYYY-MM-DD-away-vs-home` naming matches the ladder's (season,date,away,home) key.
2. **Possible new rungs**: VK Video / Bilibili / Dailymotion as fan-upload External Surfaces, searched only after the ladder's existing rungs fail — same acquisition posture as YouTube fan channels (public pages, no DRM circumvention), flagged grey, never endorsed.
3. **Official-tier enumeration**: keep the `nba.py` endpoint map (Solr index, `publishpoint`, `content-api-prod` collections) as re-derivation notes; the extractors are disabled upstream, so treat the map as archaeology, not working code.
4. **Replay-site family**: leave to #7; the code only proves scrapeability, and licensing posture (unlicensed rehosts) keeps it grey.

## Sources

- https://github.com/choucisan/nba_games · https://huggingface.co/datasets/choucsan/NBA_Games
- https://github.com/yt-dlp/yt-dlp/blob/master/yt_dlp/extractor/nba.py (+ LICENSE: Unlicense; `vk.py`, `bilibili.py`, `dailymotion.py`, `supportedsites.md`)
- https://m.vkvideo.ru/@all_about_nba · https://vkvideo.ru/playlist/-233508484_3
- https://search.bilibili.com/all?keyword=NBA全场高清中文录像回放 · https://www.bilibili.com/video/BV1vL3o6UEpF/
- https://www.dailymotion.com/user/sporthdlive/
- https://github.com/Kraptor123/Cs-Karma (BasketballReplays provider) · https://basketballreplays.net (via provider source)
- https://github.com/axelmierczuk/sportyfin (README, util/scraping.py)
- https://github.com/maxgalbu/xbmc.plugin.video.nba · https://github.com/chamchenko/plugin.video.nbainternational · https://github.com/russholio/kodi-nba-league-pass · https://forum.kodi.tv/showthread.php?pid=3000768
- https://github.com/hetp4401/nbastreams-Stremio-Addon · https://github.com/jpants36/stremio-addon-ppvstreams · https://github.com/821938089/cloudstream-extensions
- https://github.com/fmhy/edit/blob/main/docs/video.md · https://github.com/champagnewiki/champagne-wiki/blob/main/movies-shows-sports/live-sports.mdx · https://github.com/Igglybuff/awesome-piracy
- https://github.com/bebasid/bebasid/blob/master/dev/domainlist · https://github.com/limbopro/Adblock4limbo · https://github.com/ghostery/adblocker/blob/master/packages/adblocker/assets/ublock-origin/filters.txt
- https://github.com/streamlink/streamlink (plugins tree) · https://github.com/gmf05/nba
