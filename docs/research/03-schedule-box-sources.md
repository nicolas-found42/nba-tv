# Research: Free schedule + box-score sources for every NBA season (1946–present)

Ticket: nicolas-found42/nba-tv#3 · Part of map #1 · Resolved 2026-09-07

## Question

Which free sources can supply **schedule + team/player box scores** (no PBP) for **every NBA season ever, 1946–present, including defunct teams**? Evaluate: unofficial NBA Stats API (nba_api), data.nba.net, Basketball-Reference (scrape policy), balldontlie free tier, sportsdataverse, Kaggle/static dumps — on era coverage and gaps (esp. early-season player totals), license/ToS, static-vs-API shape, and refresh cadence for an archive (not live). Recommend the source set and ingest shape.

## Verdict (TL;DR)

**Use Basketball-Reference as the complete backbone (verified 1946→2026 today), enrich modern seasons from the unofficial stats.nba.com JSON API, bootstrap + crosswalk with the FiveThirtyEight `nbaallelo.csv` dump (CC BY 4.0, `game_id` = BR box-score slug).** Reject balldontlie (box scores/stats are paid tiers), reject data.nba.net (dead), reject nathanlauga Kaggle dump (stale 2004–2020). Everything is $0. One build-time smoke test remains for stats.nba.com (network-blocked in this build environment; documented fallback: BR alone already covers the full requirement).

| Source | Era coverage | Box scores (team+player) | Cost / terms | Shape | Verdict |
|---|---|---|---|---|---|
| Basketball-Reference (scrape) | **1946-47 → current season** [VERIFIED] | Every game, all eras, defunct teams, era-appropriate columns | Free; ToS restricts automated use without permission; robots.txt allows league/team pages @ Crawl-delay 3 | Static HTML per season/game | **Primary backbone** |
| NBA Stats API (`stats.nba.com`, used via raw JSON; nba_api documents it) | Documented 1946-47 → present (nbadb contract); not verifiable from this build env (blocked) | boxscoretraditionalv2: player+team box; leaguegamelog: schedule+GAME_ID | Free, no key; unofficial; NBA.com ToS §9 allows private non-commercial use w/ attribution | JSON API, rate-limited, needs browser-like headers + residential IP | **Modern enrichment** (after smoke test) |
| FiveThirtyEight `nba-elo/nbaallelo.csv` | 1946-11-01 → 2015-06-16, BAA+NBA+ABA [VERIFIED by download] | Scores only (no box) | **CC BY 4.0**; frozen (sports updates ended Jun 2023) | One 18 MB CSV, 126,314 rows; `game_id` = BR slug | **Bootstrap + crosswalk** |
| nbadb (`wyattowalsh/nbadb`) + Kaggle mirror | 1946-47 → present for `nba_api` contracts (project's own contract, CI-tested) | Full box via NBA Stats API pipeline | **MIT** code; Kaggle dataset license unlisted (content inherits NBA terms) | Python builder → DuckDB/SQLite/Parquet | **Optional bulk path / sanity check** |
| balldontlie free tier | Claims 1946-current | **Games = free; Game Player Stats / Box Scores / Standings = paid** (GOAT $39.99/mo) | Free = 5 req/min, API key, 1 sport | JSON API, cursor pagination | Reject (box is paid) |
| sportsdataverse (hoopR, nbastatR) | Wrappers: NBA API / ESPN / BR; prebuilt loaders 2002-present | Inherits upstream | MIT (both) | R packages + GitHub-release data | Reference implementations only |
| data.nba.net | DEAD | — | TLS cert invalid; HTTP 400 on all paths probed | Static JSON (was) | Reject |
| cdn.nba.com/static/json (liveData…) | Modern seasons (sample 2021); archive depth unverified | liveData boxscore | Free; Akamai-WAF (403 from datacenter/build env) | Static JSON | Optional late-era extra; live-oriented |
| nathanlauga/nba-games (Kaggle) | 2004 → Dec 2020 [dataset meta] | game details CSV | License unlisted on Kaggle | CSVs | Reject (stale) |

## Method

Primary sources read/probed on 2026-09-07: live HTTP probes of Basketball-Reference pages (1946-47 season schedule, first-ever game box, 1947 totals, 1974/1980 totals pages, 2025-26 schedule), Sports Reference policy pages (data_use, robots.txt), NBA.com Terms of Use, nba_api endpoint documentation, hoopR/nbastatR/hoopR-data/nbadb repos (READMEs + DESCRIPTION licenses), balldontlie docs + OpenAPI spec, FiveThirtyEight data repo, and a **direct download + analysis of `nbaallelo.csv`** (18,173,265 bytes). stats.nba.com and cdn.nba.com could not be probed from this build environment (see §2.2). Every claim below cites its source; `[VERIFIED]` = probed live on 2026-09-07, `[DOC]` = read from the cited primary doc, `[INFERENCE]` = reasoned, verify at build.

## 1. Source-by-source

### 1.1 Basketball-Reference (basketball-reference.com) — VERIFIED backbone

**Era coverage.** League-year schedule pages run unbroken from `/leagues/BAA_1947_games.html` (1946-47) through `/leagues/NBA_2026_games.html` (current 2025-26) — both fetched [VERIFIED]. The 1946-47 page carries the complete BAA schedule: date, visitor/home, scores, OT flag, attendance, arena, and a box-score link per game (first link: `/boxscores/194611010TRH.html`). Defunct teams are first-class: Toronto Huskies (TRH), Chicago Stags (CHS), Pittsburgh Ironmen (PIT), St. Louis Bombers (STB), Washington Capitols (WSC), Detroit Falcons (DTF), Cleveland Rebels (CLR), Providence Steamrollers (PRO) all appear with team-season pages (`/teams/TRH/1947.html`). The 2025-26 page shows the same structure (NBA Cup games flagged in a Notes column) [VERIFIED].

**Box scores.** `/boxscores/194611010TRH.html` [VERIFIED] renders a full per-player box for both teams of the first NBA game ever (NYK 68 @ TRH 66): FG, FGA, FT, FTA, PF, PTS per player + team totals (MP=240). Era-appropriate columns only — that game's MP/rebound/assist cells are blank because they were not recorded then. **Early-season player totals gap, stated precisely:** the 1946-47 league totals page (`/leagues/BAA_1947_totals.html`) [VERIFIED] lists all 161 players with G, FG, FGA, FT, FTA, AST, PF, PTS — including multi-team seasons as `2TM`/`3TM` split rows (e.g., Ed Sadowski TRH 10g + CLR 43g) — and has **no** TRB/STL/BLK/3P columns at all. 1973-74 and 1979-80 totals pages exist with the modern table family [VERIFIED pages exist]; steals/blocks enter the box in 1973-74 and three-pointers in 1979-80 `[INFERENCE — pin exact headers at build]`. Consequence for the app: box columns must be nullable and the UI renders "—" for unrecorded-era stats; the box is never "unavailable" — it is always present at the era's resolution.

**Terms / scrape policy.** Sports Reference's data-use page [DOC: sports-reference.com/data_use.html] says: (a) their Site Terms prohibit — as quoted by SR themselves — use of "any automated means to access or use the Site … **in a manner that adversely impacts site performance or access**" without express written permission; (b) you may not build a database that "competes with or constitutes a material substitute" for their services, nor use their content to train AI models; (c) custom data extracts start at $5,000; (d) "copyright law is clear that facts cannot be copyrighted." Their robots.txt [VERIFIED] sets `Crawl-delay: 3` and disallows only `/dump/`, `/my/`, play-index CGIs, and player sub-pages (`*/gamelog/`, `*/splits/`, `*/on-off/`, `*/lineups/`, `*/shooting/`) — **league/team/season/boxscore pages are not disallowed**. Map legality gate is acquisition method: a personal, single-user, non-commercial, non-redistributed, non-AI-training fetch at ≥3 s/request on robot-permitted paths is the least-invasive acquisition consistent with what SR documents; it is not something they "bless," and it must never become a published dataset.

**Shape & cadence.** Static HTML, one page per season (schedule incl. playoffs), one page per game (box), one page per league-year stats table. Page freshness is published per page (`meta-revised`, e.g. "16:31:52 03-Sep-2026") [VERIFIED] — usable as an ETag-ish re-crawl hint. Archive cadence: fetch once per completed season; annual gentle re-crawl picks up BR corrections. No rate-limit beyond robots.txt Crawl-delay 3.

### 1.2 Unofficial NBA Stats API (stats.nba.com), as used by nba_api — modern enrichment

**Endpoints (contract [DOC: nba_api endpoint docs]):**
- `GET /stats/leaguegamelog?LeagueID=00&PlayerOrTeam=T&Season={YYYY-YY}&SeasonType=…&Sorter=DATE` → per-season **schedule + result + team stat line**: `SEASON_ID, TEAM_ID, TEAM_ABBREVIATION, TEAM_NAME, GAME_ID, GAME_DATE, MATCHUP, WL, MIN, FGM…PTS, PLUS_MINUS, VIDEO_AVAILABLE` (nba_api `leaguegamelog.md`).
- `GET /stats/boxscoretraditionalv2?GameID={10-digit}` → **player box + team box** (`PlayerStats`, `TeamStats`, `TeamStarterBenchStats` incl. `PLUS_MINUS`) (nba_api `boxscoretraditionalv2.md`; GameID pattern `^\d{10}$`).
- `GET /stats/commonteamyears?LeagueID=00` → all franchises incl. defunct with NBA team IDs and active year ranges [DOC: endpoint in nba_api docs; response not probe-able here].
- `GET /stats/playercareerstats?PlayerID=…` → per-player season splits; `GET /stats/scoreboardv2?GameDate=…` → per-day schedule.

**Era coverage.** The actively maintained, MIT-licensed bulk extractor **nbadb** [DOC: github.com/wyattowalsh/nbadb README] states as its own CI-tested contract: "nbadb covers the **1946-47 season to present** for executable `nba_api` contracts," with a "trust floor: preserve and improve full historical `nba_api` coverage for every year available per endpoint." That is strong documented evidence that schedule/box endpoints reach 1946-47. **Caveat (honest):** stats.nba.com could not be probed from this build environment — DNS resolves to Akamai (`e8017.dsci.akamaiedge.net`), TCP:443 connects, but the TLS handshake stalls (tarpit); plain HTTP:80 returns 301; the `read` fetcher and a reader-proxy both time out [VERIFIED blocked here]. nbadb's CI runs extraction through a NordVPN GitHub action (`.github/actions/nordvpn-connect` in its tree) — corroborating that NBA endpoints block datacenter/CI IPs. **Build-time smoke test (2 requests, residential network):** `leaguegamelog Season=1946-47` and `boxscoretraditionalv2 GameID=0024600001`. If they fail, BR alone still satisfies the requirement end-to-end.

**Terms.** nba_api is MIT and points at NBA.com's Terms of Use [DOC: nba_api README]. NBA.com ToS [DOC: nba.com/termsofuse]: §1 permits downloading displayed material "to any single computer … for your personal, noncommercial use"; **§9 "NBA Statistics"** requires prominent NBA.com attribution, restricts statistics use to "legitimate news reporting or private, non-commercial purposes," and — the one live tripwire — prohibits use "in connection with any … **database (in any medium or format) of comprehensive, regularly updated statistics** … without the Operator's express prior consent." A frozen personal archive (refreshed once per finished season, never redistributed, never published) sits in the permitted private/non-commercial space and outside "regularly updated"; a continuously-refreshed public product would not be. §9(vi) bans real-time/archived PBP products — this app shows no PBP at all, so that clause is moot. Note the API is officially undocumented; nba_api itself says "NBA.com does not provide information regarding new, changed, or removed endpoints" [DOC].

**Shape & cadence.** JSON API, free, keyless, but flaky: needs browser-like headers (`Referer: https://www.nba.com/` etc.), modest concurrency, and typically a residential IP; occasional 4xx/blank responses require retries. For an archive: fetch seasons once after they end (leaguegamelog = 1 request/season for the schedule; ~1.2k boxscore requests/season), then freeze. ~30-80 seasons × ~1.2k = 35-100k requests total if enriching everything ≥1996; volume is small but must be throttled.

### 1.3 FiveThirtyEight `nba-elo/nbaallelo.csv` — verified bootstrap + crosswalk

Downloaded and parsed [VERIFIED]: 18,173,265 bytes, **126,314 rows** (2 rows per game ⇒ ~63.2k games), spanning **1946-11-01 → 2015-06-16**, leagues **BAA + NBA + ABA** (ABA rows 1968-1976), **104 distinct team slugs** incl. all the defunct ones (TRH, CHS, PIT, STB, DTF, CLR, PRO, AND, SHE, WSB…). Columns: `gameorder, game_id, lg_id, _iscopy, year_id, date_game, seasongame, is_playoffs, team_id, fran_id, pts, elo_i, elo_n, win_equiv, opp_id, opp_fran, opp_pts, …` [VERIFIED header]. Decisive property: **`game_id` is exactly the Basketball-Reference box-score slug** — row 1 is `194611010TRH`, matching `/boxscores/194611010TRH.html` [VERIFIED]; last row `201506170CLE`. So this one CC-licensed file is simultaneously (a) a ready schedule/scores skeleton for 1946-2015, (b) the defunct-team roster of slugs, and (c) a crosswalk from every historical game to its BR box page. License: "our data sets are available under the Creative Commons Attribution 4.0 International License" [DOC: fivethirtyeight/data README]; the same README states "As of June 13, 2023, sports predictions and forecasts are no longer being updated" — it is a frozen snapshot, ideal for an archive, useless for seasons >2015.

### 1.4 balldontlie free tier — reject as primary

[DOC: docs.balldontlie.io] "The API contains data from **1946-current**. An API key is required." Tier table [DOC, quoted verbatim]: **Free** gets Teams ✓, Players ✓, Games ✓ (schedule, `?seasons[]=…&season_type=…`, cursor pagination, per_page ≤ 100) but **Game Player Stats ✗, Season Averages ✗, Box Scores ✗, Team Standings ✗** (paid tiers only: ALL-STAR $9.99 / GOAT $39.99 per sport). Free rate limit **5 req/min**. Box scores — the core need — are behind the paid wall, and 5 req/min makes even a schedule pull (600+ paginated pages for 80 seasons) slow. Verdict: not part of the $0 source set; at most a paid-tier-free cross-check for current-season schedules.

### 1.5 sportsdataverse (hoopR, nbastatR) — reference implementations, not a source

**hoopR** [DOC: DESCRIPTION] MIT, v3.1.0, "a full NBA Stats API wrapper" + ESPN + Basketball-Reference + RealGM wrappers; bulk loaders are backed by the `sportsdataverse-data` release repos, and **hoopR-data** [DOC: repo README] ships "hoopR data **2002-Present**" (ESPN-sourced PBP/team box/player box; a sibling `hoopR-nba-stats-data` repo covers the NBA-stats-sourced pipeline). **nbastatR** [DOC: DESCRIPTION] MIT, v0.1.153 (dated 2026-01-31 — still maintained), wraps NBA Stats API + Basketball-Reference. Neither is a data source of its own for 1946-2001: coverage inherits ESPN (2002+) or the NBA API/BR. Use them as executable reference semantics for stats.nba.com endpoints; do not add an R/Python runtime to a pure-Rust app.

### 1.6 data.nba.net and cdn.nba.com static JSON

**data.nba.net is dead.** `https://data.nba.net/data/10s/prod/v1/2014/schedule.json` (and the site root) return **HTTP 400** even with `-k` after the read fetcher reported `ERR_TLS_CERT_ALTNAME_INVALID` on the host [VERIFIED]. Do not build on it.

**cdn.nba.com/static/json/liveData/…** is the current official static-JSON family for live/final game data (base URL per nba_api's live `boxscore.md` [DOC], sample payload from Jan 2021); all four probed game IDs — including 2023-24 `0022300001` — returned **403** from this environment with and without browser UA (Akamai WAF) [VERIFIED blocked here]. It is a live-oriented source with shallow archive depth; treat as an optional late-era convenience from a normal connection, never as the archive.

### 1.7 Kaggle / static dumps

- **`wyattowalsh/nbadb`** [DOC: GitHub, MIT, active]: Python pipeline that bulk-builds the "NBA Database" from the nba_api surface; CLI `nbadb init` does a "full local build from scratch (1946-present)"; outputs **DuckDB / SQLite / Parquet / CSV**; its Kaggle dataset (`wyattowalsh/basketball`, meta: "Daily Updated SQLite Database — 64,000+ Games, 4800+ Players, and 30 Teams") is the published mirror. The "30 Teams" note means the teams table is current-team-shaped; defunct teams live in game/franchise rows, not as 30+ entities. Best use here: a **bulk, one-shot modern+historical enrichment dump** generated once by `nbadb init` (or downloaded from Kaggle), consumed by the Rust app as files — keeps the app itself pure-Rust while acknowledging stats.nba.com's datacenter blocking. Kaggle dataset license is unlisted; treat its content as NBA-API data under NBA terms (private use), not as freely redistributable.
- **`nathanlauga/nba-games`** [DOC: Kaggle meta]: "all NBA games from 2004 season to dec 2020" — stale, shallow; reject.

## 2. Era coverage & gaps master view

| Era | Schedule+scores | Team box | Player box detail | Notes |
|---|---|---|---|---|
| 1946-47 → 1949-50 (BAA→NBA) | BR complete [VERIFIED 1947] | FG/FT/PF/PTS + MP=240 totals [VERIFIED] | FG, FT, PF, PTS; **MP, REB, AST, STL, BLK not recorded** [VERIFIED columns] | AST exists in season totals [VERIFIED 1947 totals] but not consistently per-game |
| 1950-51 → 1972-73 | BR complete | + REB (1951+), AST in boxes | MP present | STL/BLK still absent |
| 1973-74 → 1978-79 | BR complete | + STL, BLK | full classic set | |
| 1979-80 → 1995-96 | BR complete | + 3P | full classic set | |
| 1996-97 → present | BR + stats.nba.com | + PLUS_MINUS (API), full modern set | API box adds +/-, starter/position | NBA GAME_ID available → tape crosswalk key |
| Defunct teams (1946-2026) | BR: dedicated team-season pages [VERIFIED TRH/1947] | yes | yes (with 2TM/3TM split rows in totals [VERIFIED]) | stats.nba.com covers defunct franchises via commonteamyears/team IDs [DOC] |
| ABA (1967-76) | out of NBA scope; FTE CSV carries ABA rows 1968-76 anyway [VERIFIED] | — | — | keep `league` column; decide inclusion later |

Volume: ~63.2k games through 2015 (FTE row count [VERIFIED]) + ~1.2k/season for 2016→2026 ≈ **75-77k games total**, matching map #1's "60k+" expectation.

## 3. Recommended source set

1. **Basketball-Reference** — complete, verified 1946→current backbone for schedule, team+player box, and per-season player totals (incl. defunct teams). Fetched as static HTML at ≥3 s/request on robot-permitted paths, personal use only, never republished.
2. **stats.nba.com raw JSON** (contract documented by nba_api, MIT) — modern-era enrichment: official `GAME_ID` + `VIDEO_AVAILABLE` (feeds ticket #4's tape hunt), `PLUS_MINUS`, NBA team/person IDs for the ID crosswalk, `commonteamyears` for defunct-franchise IDs. Gated by the two-request smoke test; fall back to BR-only if blocked.
3. **FiveThirtyEight `nbaallelo.csv`** (CC BY 4.0) — day-one bootstrap of every game 1946-2015 with BR-slug game IDs + the defunct-team slug list; also supplies ABA rows if ever wanted.
4. **Optional bulk path:** `nbadb init` (MIT) run once off-app to produce a Parquet/SQLite enrichment dump when stats.nba.com direct access is unreliable; the Rust app consumes the files.

Rejected: balldontlie (box/stats paid), data.nba.net (dead), cdn.nba.com static JSON (WAF, live-oriented), nathanlauga Kaggle (stale), hoopr/nbastatR as data (wrappers only).

## 4. Ingest shape (fetch → normalize → store)

**Store.** Single local SQLite (rusqlite) + a raw-snapshot blob dir (`data/raw/{source}/{season}/*.html.gz|json.gz`) so re-normalization never re-fetches. Tables:

- `seasons(league TEXT, year INT, label TEXT)` — BAA_1947 … NBA_2026.
- `teams(br_slug TEXT PK, nba_team_id INT NULL, franchise_id TEXT NULL, city, name, abbrev, active_from, active_to)` — defunct teams included; sourced from BR team index + FTE fran_id + commonteamyears.
- `players(br_slug TEXT PK, nba_person_id INT NULL, name, first_season, last_season)`.
- `games(game_id TEXT PK, nba_game_id TEXT NULL, league, season, date, game_type REGULAR|PLAYOFFS|NBA_CUP, home_team, away_team, home_pts, away_pts, ot TEXT NULL, arena NULL, attendance NULL, br_url, sources TEXT)` — PK is the BR slug (verified format `194611010TRH`, identical in FTE).
- `box_team(game_id, team_br, mp NULL, fg, fga, ft, fta, oreb NULL, dreb NULL, reb NULL, ast NULL, stl NULL, blk NULL, pf, pts, plus_minus NULL)`.
- `box_player(game_id, team_br, player_br, starter NULL, position NULL, mp NULL, fg…pts, plus_minus NULL, dnp_reason NULL)` — every post-1950 stat nullable; NULL means "era did not record" and the UI shows "—" (map rule: schedule+box always shown; only tape can be "unavailable").
- `player_season_totals(player_br, season, team_br, g, fg…pts)` — from BR league-year totals pages, one row per (player, season, team) so 2TM/3TM seasons keep their splits.

**Fetch order.** (0) FTE CSV → games skeleton 1946-2015 + slug crosswalk (1 request, CC-BY, instant). (1) BR league-year `_games.html` chain BAA_1947→NBA_2026 (~80 requests) → game index incl. playoff section + box URLs. (2) BR boxscore pages (~75k requests at ≥3.5 s ≈ 3 days, resumable, per-season batches) → team+player boxes; store raw HTML gz. (3) BR league-year `_totals.html` (80 requests) → player_season_totals. (4) Optional modern enrichment: stats.nba.com `leaguegamelog` per season (80 requests) → `nba_game_id` + `VIDEO_AVAILABLE`; `boxscoretraditionalv2` per game ≥1996 (~36k requests, throttled); `commonteamyears` once; `playercareerstats` only for players with modern-era rows.

**Normalize.** Parse with a versioned parser; keep raw snapshot; map era columns to nullable fields; resolve team/player slugs through the crosswalk tables; do not invent values for unrecorded stats.

**Refresh cadence (archive, not live).** Seasons are frozen once complete: one fetch pass after the Finals, then done. Re-crawl trigger: BR `meta-revised` timestamp per page [VERIFIED present] checked at most annually for corrections. No continuous polling (NBA ToS §9(vii)); no republishing of the dataset (SR ToS; NBA ToS §1); attribution lines for NBA.com and Sports-Reference in the app's UI footer.

## 5. Open items / build-time checks

1. **stats.nba.com smoke test** from a residential connection: `leaguegamelog Season=1946-47`, `boxscoretraditionalv2 GameID=0024600001` (this build env is Akamai-tarpitted; nbadb's NordVPN CI action corroborates the blocking).
2. Pin exact BR column boundaries (STL/BLK 1973-74, 3P 1979-80) from league-year totals headers during parser bring-up `[INFERENCE]`.
3. Decide ABA inclusion (rows already in hand via FTE).
4. If stats.nba.com is unusable: accept BR-only (fully sufficient for schedule+box) and rely on ticket #4 sources for modern tape keys instead of `VIDEO_AVAILABLE`.

## Sources

- Basketball-Reference: [/leagues/BAA_1947_games.html](https://www.basketball-reference.com/leagues/BAA_1947_games.html) · [/boxscores/194611010TRH.html](https://www.basketball-reference.com/boxscores/194611010TRH.html) · [/leagues/BAA_1947_totals.html](https://www.basketball-reference.com/leagues/BAA_1947_totals.html) · [/leagues/NBA_2026_games.html](https://www.basketball-reference.com/leagues/NBA_2026_games.html) · [robots.txt](https://www.basketball-reference.com/robots.txt) (all probed 2026-09-07)
- Sports Reference data-use policy: https://www.sports-reference.com/data_use.html
- NBA.com Terms of Use: https://www.nba.com/termsofuse (§1, §9)
- nba_api (MIT): https://github.com/swar/nba_api — [leaguegamelog.md](https://github.com/swar/nba_api/blob/master/docs/nba_api/stats/endpoints/leaguegamelog.md), [boxscoretraditionalv2.md](https://github.com/swar/nba_api/blob/master/docs/nba_api/stats/endpoints/boxscoretraditionalv2.md), [live boxscore.md](https://github.com/swar/nba_api/blob/master/docs/nba_api/live/endpoints/boxscore.md)
- FiveThirtyEight data (CC BY 4.0): https://github.com/fivethirtyeight/data — `nba-elo/nbaallelo.csv` (downloaded & analyzed 2026-09-07)
- nbadb (MIT): https://github.com/wyattowalsh/nbadb · Kaggle mirror: https://www.kaggle.com/datasets/wyattowalsh/basketball
- balldontlie: https://docs.balldontlie.io/ · https://www.balldontlie.io/openapi/nba.yml · https://www.balldontlie.io/ (pricing)
- hoopR (MIT): https://github.com/sportsdataverse/hoopR · hoopR-data: https://github.com/sportsdataverse/hoopR-data
- nbastatR (MIT): https://github.com/abresler/nbastatR
- Kaggle nathanlauga/nba-games: https://www.kaggle.com/datasets/nathanlauga/nba-games
- data.nba.net probe: `data.nba.net/data/10s/prod/v1/2014/schedule.json` → TLS cert invalid / HTTP 400 (2026-09-07)
