# 08 — Fan preservation networks (nicolas-found42/nba-tv#8)

Resolves the #8 question: where fan preservation networks keep NBA full-game Game Tape, and how
an app could find it per Game. Sweep covers Reddit, Discord, classic-sports forums, collector
wikis, VHS/tape-trading circles, public-tracker listings (pointers only), sports-gaming mod
scenes, and dead forums via Wayback. Lane note: streaming *platforms* (YouTube/Dailymotion/IA
etc.) belong to #7 and are not re-covered; add-on/code leads belong to #9. Method: web search
plus reading the community pages and listings themselves; nothing was downloaded. Some community
sites block automated reads (noted per lead); those claims cite the pages' own indexed text.

**Verdict in one paragraph.** The fan-preservation world is not a set of streaming sites but a
small trading economy, and it is richer — and better structured — than the #4 landscape pass
assumed. Its core is a handful of **structured collector catalogs**: USA Sports on DVD alone
lists **20,901 NBA games**, each record tagged with the **Basketball-Reference boxscore URL**,
broadcast network, duration, and completeness notes — i.e. a ready-made identity crosswalk to
the #3 schedule/Box Score backbone. Gregg's Sports Archive adds 23,500+ events (all sports) with
fine-grained condition grading back to the 1962 Finals; Pontel GmbH commercially sells "over
20,000 game DVDs" including "over 7000 NBA games never produced on DVD" digitized to order.
Around them sit **request/trade communities** (Interbasket's video forum, long-lived forum tape
threads, r/VintageNBA as the Reddit hub) that identify games by description when no list
exists. **Invite-walled private trackers** (MySpleen, TV-Vault) hold VHS-ripped material but are
grey and closed; the **Lost Media Wiki** maps the negative space (which games are known
missing); and the only **lawful-acquisition** lanes here are licensed/commercial physical media
(Pontel-style labels, eBay commercial releases). No Discord-based tape network is publicly
indexable; the mod scene recreates eras virtually and bundles no real Game Tape. For the app:
these networks are **existence metadata and human request channels, not stream URLs** — every
digital-copy channel in this sweep is grey (unlicensed trade or sale) and pointer-only under the
map's acquisition-method gate.

---

## 1. Structured collector catalogs (the find)

### 1.1 USA Sports on DVD — the strongest lead

- Scale and shape: "NBA GAME DATABASE (20901)" — a filterable web database of NBA games
  ("A huge collection of NBA, NCAA, NFL, NHL and MLB games to trade, from the earliest days of
  pro sports until today"). [Site front page](https://www.usasportsondvd.com/) /
  [NBA section](https://www.usasportsondvd.com/nba).
- **Era coverage**: site claims earliest-days→today; records verified in this sweep from the
  1962-63 NBA Finals Game 6 ("112 · 1962-1963 NBA Finals (Game 6) April 24, 1963") through
  2001–2003 regular-season regional broadcasts (Fox Sports Net Cavs TV, Sunshine Network Magic
  TV) — exactly the deep regular-season hole official archives don't fill.
- **Game identity scheme**: each record carries teams, scores, season label, date, **and a
  Basketball-Reference boxscore link** (e.g.
  `basketball-reference.com/boxscores/200304120CLE.html`) plus an internal numeric Game ID,
  NETWORK, DURATION, FORMAT, RESOLUTION, QUALITY, SIZE, and free-text completeness notes
  ("Full broadcast with no halftime, commercials included"; "INCOMPLETE GAME — broadcast starts
  with 11:20 left in 2nd quarter"). The BR link reduces every record to the #3 identity key
  `(season, game_date, away, home)` via the same BR slug crosswalk #4 already uses.
- **Access shape**: browse/filter (team, date range, keyword) → add to Wishlist → email the
  trader → "download it or get it on a DVD". Trade exchanges, selling, and buying are all
  offered ("I always consider all trade offers, whether they involve exchanges, selling, or
  buying"); "even if you have nothing to trade... we can work something out".
- **Churn risk**: low-moderate. One motivated operator, updated daily ("it will still be updated
  daily with new games"), but it is a single person's collection; the site has no public API.
- **Automated per-game lookup**: the database and its filters are public HTML; a catalog crawl
  (team/date-filtered listing pages) yields existence records keyed by the embedded BR URL.
  Keep it to **existence metadata** (`tape_sources` pointer, match_confidence from the record's
  own completeness notes); acquisition is the operator's grey channel — see §9.
- **Legality: GREY.** Sale/trade of unlicensed digital copies. Cite-the-source note: the site's
  own framing is "trading", with downloads sold; report as grey, never endorse.

### 1.2 The Sports Archive (greggsportsvideos.com)

- Scale and shape: "Now Featuring Over 23,500 Events!! ... for over 42 years I have been
  recording and trading games/events and listing them on this website", last updated 06/16/26.
  Subjects include an "NBA, ABA, & WNBA BASKETBALL" page.
  [Home](http://greggsportsvideos.com/) · [Subjects](http://greggsportsvideos.com/subjects.htm).
- **Era coverage**: NBA playoffs listings verified from **1962 NBA Finals Game 7** onward
  ("1962 NBA Finals Game 7 L.A. Lakers 107 @ Boston 110 (VG)(Edited possessions, B&W with
  natural arena sounds 0:38)" — B&W, partial, edited: the reality of pre-1970 tape), through
  1970s series with per-game completeness ("1972 NBA Finals Game 5 ... (Missing last 8 min)";
  "1978 NBA Finals Game 4 ... missing last 1:38 of regulation, and missing first 3 minutes of
  OT"). The NBA page runs 8,632 lines; sampled portion is playoff-heavy.
  [NBA page](http://greggsportsvideos.com/nba_basketball.htm).
- **Game identity scheme**: line format `{season} {round label} Game {n} {away} {score} @
  {home} {score} ({grade})(defects)` — identity by season + round + game number + teams +
  final score, *no date*. Matching: score+round+teams → BR playoff index → `game_id`. Round
  labels include era-true forms ("1966 NBA ECSF", "1975 NBA EC1R") — alias table needed
  (matches #4 §3.1).
- **Access shape**: static hand-built HTML pages; "email me with your list of available
  material and hopefully we can work out a trade"; blank-media trades accepted; explicit
  grading scale (EX→PO) and "I collect ORIGINAL broadcasts" rule. Trading rules/disclaimers are
  the classic collector-to-collector boilerplate (incl. the bogus "INTERNET PRIVACY ACT of
  1995" clause — flag it as folklore, not law).
- **Churn risk**: moderate — 42-year hobbyist, single point of failure, Word-generated HTML;
  content valuable exactly while the owner keeps the site up.
- **Automated per-game lookup**: one-time full crawl of the static pages + periodic re-crawl
  (NEW tags carry month/year); parse the bolded entry grammar above into candidate records
  feeding #4's scoring queue.

### 1.3 Pontel GmbH — the commercial arm of the same economy

- Scale and shape: "Currently over 20,000 Game DVDs in our Shops!"; banner: "Over 7000 NBA
  Games never produced on DVD have been digitized and are now available to order, custom made
  just for you!" ([pontel.com](https://www.pontel.com/), incl. an
  [NBA Archive](https://www.pontel.com/subscriptions/contents/en-us/d98_NBA_Archive.html)
  subscription shop).
- **Era coverage**: not stated in aggregate; shops are organized season → round → game
  (product pages like "Final Round", "Conference Semi-Finals"), with 2026 All-Star Weekend
  offered — modern seasons actively added.
- **Game identity scheme**: shop navigation by season/round; product pages per game.
- **Access shape**: purchase (custom-burned DVD, shipped). This is the label a 2004 collector
  described as producing "officially licenced NBA game tapes" on
  [BigFooty](https://www.bigfooty.com/forum/threads/nba-games-on-vhs-dvd.145122/) — but the
  current site states no licensing; treat licensing as **unverified**.
- **Legality: GREY** (commercial sale of broadcast copies; license unverified on-site today).
  Pointer-only; the lawful twin of this lane is the licensed-physical market in §7.
- **Automated per-game lookup**: static catalog pages crawlable per season/round; existence
  pointer `EXISTS_NOT_STREAMABLE` per #4's rung-4 pattern.

### 1.4 Personal collector catalogs (the Weebly pattern)

- Example: [Brad's Sports DVD Collection](http://bradssportgames.weebly.com) — "I collect
  mostly Basketball and some NFL DVD's, here is a list of all my games", trade by email, want
  list, region notes ("collected games from all around the world"), and an explicit quality
  grammar (HD / A perfect → E unwatchable). Self-described "over 1000 games on DVD... trade
  with other NBA game collectors all over the world" per its owner's
  [BigFooty post](https://www.bigfooty.com/forum/threads/nba-games-on-vhs-dvd.145122/).
- **Era coverage**: owner-specific (Brad's includes 1988 Olympics Boomers, NBL, Magic's
  Australia tour — non-NBA breadth typical of these catalogs).
- **Identity**: personal list pages (static HTML), often team- or star-organized.
- **Access/churn**: email trades; sites live as long as their owner's hobby does; discovery is
  word-of-mouth through forums/Reddit.
- **Automated per-game lookup**: cheap one-time crawls of catalogs discovered via §2/§3
  mentions; treat each as an existence source with the owner's grade as metadata. Grey (same
  class as §1.2).

## 2. Forum request/trading communities

### 2.1 Interbasket — Basketball Videos/Downloads

- Live dedicated video forum: "In this basketball video forum - you may watch, link, download,
  and request basketball videos from all-over the world"
  ([section](https://www.interbasket.net/forum/forums/basketball-videos-downloads.74/)).
  Thread evidence: ["NBA Classic games"](https://www.interbasket.net/forum/threads/nba-classic-games.16125/)
  (a collector gathering "all finales of 1980 to our days, as well as everything all star game,
  dunk slam, matches of legends") and the long-running
  ["NBA video thread" series](https://www.interbasket.net/forum/threads/nba-video-thread-vol-xxvi.2560907/)
  (vol. XXVI and counting).
- **Access shape**: request-and-link queue — post what you seek, others reply with links.
  **Identity**: freeform thread titles. **Churn**: high for old links (the forum's FIBA thread
  on "Torrentz / rapidshare" links documents the dead-link era); the site blocks automated
  reads (HTTP 403 to bots; verified in this sweep) — automation must be human-browser passes or
  very-low-rate crawls, or rely on search indexes.
- **Era coverage**: mix; the classic-games thread skews Finals/All-Star 1980s→.
- **Lookup sketch**: quarterly keyword passes (game tokens from #4 §3.1) through the section's
  thread titles; ingest links into the candidate queue with REVIEW confidence.

### 2.2 BigFooty and the long-lived tape threads

- [NBA games on VHS / DVD...](https://www.bigfooty.com/forum/threads/nba-games-on-vhs-dvd.145122/)
  (started Dec 3, 2004; forum alive today): collectors describe holdings of 500–600 tapes
  ("from about 1989 (some earlier) through to 1998"), full Finals series ("entire 1984, 1985 &
  1987 Finals Series"), and trade by email/PM. Two structural findings:
  - **Identity-by-description**: "I do not have a 'list' as such... if you want to know if I
    have a certain game, just describe it... and I will tell you if I have it or not" — several
    networks hold tape with **no machine-identifiable catalog at all**; the app's request flow
    must support human-described matching.
  - **Networked discovery**: posts cross-link to other collectors' catalogs (→ §1.4) and to
    commercial labels (→ §1.3).
- Same pattern elsewhere: ["Michael Jordan ... games on vhs and dvd"](https://forum.chicitysports.com/threads/michael-jordan-chicago-bulls-games-on-vhs-and-dvd.11649/)
  (chicitysports), [PonTel NBA Games (VHS) for sale](https://www.ozcardtrader.com.au/threads/pontel-nba-games-vhs-for-sale.3014/)
  (ozcardtrader card forum, physical market).
- **Era**: skew 1984–1998 (VHS boom), regional/Australian broadcasts included.
- **Lookup sketch**: no automation beyond site search; keep a curated list of long-lived
  threads and re-check on the 90-day cadence from #4.

### 2.3 basketballforum.com — probed, alive, no tape (dead end)

- Wayback shows captures back to 2000
  ([CDX](https://web.archive.org/cdx/search/cdx?url=basketballforum.com&limit=20)), but the
  site is **not dead**: a 2026-05-28 capture shows a live 8M-post/40K-member forum with
  February 2026 threads. No tape/video subforum surfaced — general discussion only.
  ([2026 capture](https://web.archive.org/web/20260528133926/https://www.basketballforum.com/)).
- Dead-end reason: wrong kind of community; nothing per-game to mine.

## 3. Reddit

### 3.1 r/VintageNBA — the Reddit hub

- Scope statement on the sub: "Discussions about vintage basketball (defined as Dec 1891 to
  Jun 2008). Learn, share, debate." ([r/VintageNBA](https://www.reddit.com/r/VintageNBA/)).
- Maintains its own reference layer: "Reference Posts (including decade overviews)" linked from
  its curated video list
  ([2023 Wayback capture](https://web.archive.org/web/20230611220430/https://www.reddit.com/r/VintageNBA/comments/c2x3s8/links_to_oldschool_videos_games_highlights_etc/)).
- The same capture is the **churn cautionary tale**, in the community's own words:
  - "Torontos (*>1000 games from 60's, 70's, 80's*) — UPDATE (Oct 2022): Did this get
    permanently deleted? I swear, the NBA absolutely hates fans having easy access..."
  - "[List of links to ~500 old games] — **Uh oh, this amazing list appears to be lost
    forever. I spoke with the guy who made it, and even he doesn't have a back-up.**"
  - Its members also chase DMCA sweeps ("Did the internet get scrubbed of basically all 60s and
    70s games..." — [thread](https://www.reddit.com/r/VintageNBA/comments/16i4prf/did_the_internet_get_scrubbed_of_basically_all/)).
- **Access shape**: public subreddit + search; no catalog, freeform titles; **churn: high**
  (channel deletions, link rot, list loss).
- **Lookup sketch**: subscribe via public RSS per flair/keyword; maintain a watchlist of
  archivist accounts; ingest new post titles into #4's candidate queue. Community channel-list
  posts (like the captured one) are themselves seeds for the platform-lane sweep (#7).

### 3.2 Dead community directories — the unmet need, twice over

- 2010: "The Full Game Encyclopedia - Links to Full NBA Games" on r/nba — "Update: 154 games
  available right now. 1961-62 Date Teams Box ..." — a hand-built index that did not survive
  ([thread](https://www.reddit.com/r/nba/comments/lboi7/the_full_game_encyclopedia_links_to_full_nba_games/)).
- 2022: "Is there a place to find a directory of NBA Game Footage?" — the community explicitly
  still lacks one ("...if not if the great people here would want to set a part of this...")
  ([thread](https://www.reddit.com/r/VintageNBA/comments/s790ds/is_there_a_place_to_find_a_directory_of_nba_game/)).
- Dead-end reason (as a *source*): community directories rot with their authors. Lesson for the
  map: the app's local registry (with snapshots of external lists it ingests) is the durable
  version of what these attempts lacked — and re-scans must re-ingest, not trust old lists.

### 3.3 Team subreddits — opportunistic tape finds, not catalogs

- Members surface personal VHS holdings ("Was going through the old VHS tapes pile and found
  this" — [r/lakers](https://www.reddit.com/r/lakers/comments/12v54fb/was_going_through_the_old_vhs_tapes_pile_and/)).
  Treat as an occasional discovery stream feeding the candidate queue; no per-game lookup is
  automatable beyond keyword RSS.

## 4. Discord — dead end (no publicly indexable tape network)

- Directory sweeps (DISBOARD NBA/basketball tags, top.gg, Hive Index "7 Best Basketball Discord
  Servers") surface fan-chat/2K-league/card servers only — e.g.
  [DISBOARD nba tag](https://disboard.org/servers/tag/nba),
  [Hive Index basketball](https://thehiveindex.com/topics/basketball/platform/discord/)
  ("NBA Chat, NBA Top Shot, Hoop Haus, and Nefty Ballers" — none tape-focused). r/nba's own
  Discord is a moderation-run chat hub ([2018 mod post](https://www.reddit.com/r/nba/comments/8qupcu/mod_post_looking_for_discord_moderators_to_help/)).
- Dead-end reason: tape sharing on Discord is invite-walled by design (no crawlable catalog,
  no public invite graph, server deletion risk). If such servers exist, they are discoverable
  only through §2/§3 relationships — a human request channel, not an automatable one. Grey
  would apply where footage is shared; nothing public to evaluate.

## 5. Private trackers (grey, pointers only — never download)

- **MySpleen** — private tracker for "TV / COMEDY / ANIMATION / 80-90'S VHS NOSTALGIA"
  ([Opentrackers profile](https://opentrackers.org/myspleen/)); community description: "MySpleen
  is a tracker for old and out of print media... MySpleen is the VHS tracker"
  ([r/trackers comparison](https://www.reddit.com/r/trackers/comments/qsas5w/obscure_niche_trackers_comparison_myspleen_ms/)),
  populated with "old and obscure TV shows and films... ripped from personal VHS tapes"
  ([freeleech thread](https://www.reddit.com/r/trackers/comments/5j4pd5/myspleen_currently_has_sitewide_freeleech_for_the/)).
  **Signup closed** (Opentrackers banner; invite-hunting persists through
  [Sep 2023](https://www.reddit.com/r/trackers/comments/16bue3p/myspleen_recruitmentinvites/)
  and 2024–2025 comments on the profile page). Classic sports VHS rips are a known part of its
  catalog; no public catalog is visible without membership.
- **TV-Vault** — Gazelle-based tracker for TV shows ("TV Vault is Gazelle-based site for TV
  shows", [r/trackers](https://www.reddit.com/r/trackers/comments/qsas5w/obscure_niche_trackers_comparison_myspleen_ms/))
  with an off-air waiting rule ("shows have to be off the air for 4 years before uploading" —
  [reminder thread](https://www.reddit.com/r/trackers/comments/18uq4oh/tvvault_reminder_shows_that_ended_in_2019_are/)).
  Sports broadcasts historically circulate there; per-game NBA depth unknown from outside.
- **Access shape**: invite-only; no public listings. **Churn**: account/ratio churn, tracker
  mortality. **Lookup sketch: none** — record as human-pointer only ("exists in private
  preservation circles"); do not automate. **Legality: GREY** (unauthorized distribution);
  pointer-only per map gate.

## 6. Collector wikis — Lost Media Wiki as the negative-space map

- Category "Lost recordings of sports events" holds **427 pages**
  ([category](https://lostmediawiki.com/Category:Lost_recordings_of_sports_events); the wiki
  blocks automated reads — HTTP 403 — so this sweep cites its indexed text).
  NBA-adjacent entries verified via search: "1974 NBA All-Star Game (Partially Lost NBA
  All-Star Game played in Seattle, 1974)" ("the game's broadcast appears to have been either
  lost, destroyed, or taped over") — [entry](https://lostmediawiki.com/1974_NBA_All-Star_Game_(Partially_Lost_NBA_All-Star_Game_played_in_Seattle,_1974));
  "UCLA Bruins 69-71 Houston Cougars (partially lost footage of 'Game of the Century' NCAA
  game; 1968)"; "Maryland Terrapins 48-80 Immaculata Mighty Macs (partially found footage...
  1975)" — its title scheme is literally `Teams result (lost/partially found footage of
  event; year)`.
- **Role for the app**: an existence-check source in reverse — documents which Games are
  *known missing or partial*, prunes hopeless re-scans of pre-1980 regular-season Games, and
  catches resurrections ("partially found" entries surface new transfers first).
- **Identity**: title grammar + article body; **access**: public MediaWiki (API exists; the
  bot-wall applies to page reads, not necessarily the API). **Churn**: low (Fandom-style wiki
  with active editors).
- **Lookup sketch**: MediaWiki `categorymembers` on the sports category; title-parse
  `{away} {score}-{home} {score} ({state} footage of {event}; {year})` into the registry as
  `EXISTS_NOT_STREAMABLE`/`KNOWN_MISSING` notes.

## 7. Physical market — the lawful lane inside fan networks

- **eBay / used-media commerce**: commercial releases circulate lawfully as physical goods
  (e.g. [Basketball VHS Tapes category](https://www.ebay.com/b/Basketball-VHS-Tapes/309/bn_2899956),
  incl. "NBA Dynasty Series" boxed sets); home-recorded game tapes are a grey zone sellers
  themselves flag ("One of the tapes has some old NBA games from 1987. Can I list those..." —
  [r/Flipping thread](https://www.reddit.com/r/Flipping/comments/1myj5p4/selling_old_recorded_vhs_tapes_with_nba_games/)).
- **Licensed labels discovered through fan networks**: PonTel (§1.3; collectors described its
  tapes as officially licensed in 2004) and the official NBA Entertainment/Warner classics
  already noted in #4 §S4. Fan forums are where these labels' catalogs circulate second-hand.
- **Access shape**: storefront search; listings ephemeral. **Lookup sketch**: saved keyword
  feeds (`{team} {year} VHS/DVD`); record as lawful-purchase pointers (rung-4 per #4), never
  as streams. This is the Cache Tier's acquisition story — buying a licensed copy, not
  downloading a rip.

## 8. Sports-gaming mod scenes — dead end for real tape

- The scene's center is [NLSC](https://www.nba-live.com/) ("Our database of mods and modding
  utilities for NBA Live, NBA 2K, and other basketball video games" —
  [forum index](https://forums.nba-live.com/)). Its classic-era work is **virtual recreation**:
  "1991-1992 Season Mod — the best classic total conversion mod ever made for any nba 2K. The
  TV presentation, replays..."
  ([thread](https://forums.nba-live.com/viewtopic.php?f=136&t=81685&start=275)),
  "CLASSIC SEASONS MOD 86-97"
  ([thread](https://www.forums.nba-live.com/viewtopic.php?f=241&t=106204&start=775)).
- Its video threads are **recorded gameplay**, not broadcast tape ("Classic Basketball Game
  Videos - NBA Live 97 intro and gameplay... All of my videos are recorded in 1080P, and the
  games are either on the PC/Xbox One or Xbox 360" —
  [thread](https://forums.nba-live.com/viewtopic.php?f=72&t=104498)).
- Dead-end reason: no real Game Tape is bundled — recreating eras in-engine avoids the
  copyright problem bundling footage would create; nothing here feeds a per-game lookup.

## 9. Integration into the map (handoff notes)

- **Where these sit in the #4 Source Ladder**: a new rung between "other platforms" and
  "purchase-only" — **collector catalogs + request networks = existence pointers and human
  request channels**. They never produce stream URLs; the registry stores
  `EXISTS_NOT_STREAMABLE`/`REVIEW` rows with source + grade + query used. USA Sports on DVD's
  BR links and Gregg's score/round grammar make these two catalogs the cheapest per-game
  existence sweep in the whole landscape (static HTML, no quota, no search API).
- **Identity story confirmed**: every live lead reduces to the #3 key — USASD hands over the
  BR slug directly; Gregg matches via (season, round, game#, teams, score); forums/Reddit need
  the #4 alias/query template set; BigFooty-style "describe the game" traders remind us the
  matching queue needs a human-review lane, which #4 already designed.
- **Churn economics**: community link-lists die even with living authors (the lost ~500-game
  Google Doc), channels get deleted (Torontos, Oct 2022), and DMCA sweeps shape what Reddit
  considers still-existing ("scrubbed of all 60s and 70s games"). The map's
  registry + 90-day re-scan + snapshot-what-you-ingest posture is the correct response; the
  app should copy external community lists into its own registry at ingest time, never
  hot-link them.
- **Legality table (acquisition-method gate)**:

| Network | Acquisition | Status |
|---|---|---|
| USA Sports on DVD | paid download / DVD or trade | GREY — unlicensed copies; pointer-only |
| The Sports Archive; personal catalogs (Brad's) | collector-to-collector media trade | GREY — same class; pointer-only |
| Pontel GmbH | purchase (custom DVD) | GREY — commercial sale, on-site license unverified; the 2004 "officially licensed" collector claim is unconfirmed today |
| eBay (commercial releases) | purchase of physical goods | Lawful resale lane (S4 twin from #4) |
| eBay (home recordings) | purchase | GREY — sellers' own ambiguity documented |
| Interbasket / forums / Reddit requests | links & requests | Surface is lawful to read; outcomes usually land on grey links — human-in-the-loop, per-game judgment |
| MySpleen / TV-Vault | invite-only torrents | GREY — pointer-only, no automation |
| Lost Media Wiki | wiki reading | Clean — existence metadata |
| Discord | none public | n/a (dead end) |
| NLSC mod scene | mods/gameplay videos | Clean but no Game Tape (dead end) |

- **Strongest lead**: USA Sports on DVD — 20,901 BR-linked records make a per-game existence
  sweep nearly free. Runner-up: Gregg's 1962→ playoffs archive with completeness grammar.

## 10. Dead-end summary

| Lead | Reason |
|---|---|
| Discord tape networks | No publicly indexable servers; invite-walled by design; nothing to automate |
| NLSC / sports-gaming mod scene | Virtual recreations and gameplay videos only; no broadcast footage bundled |
| Community-built game directories (r/nba 2010 Encyclopedia, ~500-game Google Doc) | Rot with their authors; both documented dead or lost |
| basketballforum.com | Alive but general-discussion only; no tape subforum |
| Facebook collector groups | Login-walled; nothing evaluable from outside (surfaced only via search: Indiana VHS collector groups) |
