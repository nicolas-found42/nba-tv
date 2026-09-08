# 13 — Basketball-Reference scrape rate limits

**Question.** What request rate to Basketball-Reference risks a ban, and what is the
documented/allowed ceiling? Resolves the open rate-limit question behind the repo's
current polite-fetch posture (`FETCH_MIN_INTERVAL` = 3.5 s in
`crates/nbatv_ingest/src/lib.rs`, identifying UA). Pure research — no code or policy
changes.

**Claim convention.** `[DOC]` = quoted from a primary source listed in §6 (retrieved
2026-09-08). `[OBSERVED]` = measured by this note's own probes on 2026-09-08 (§4).
`[INFERENCE]` = reasoned from the above; verify before acting on it.

**Verdict in one paragraph.** There **is** a published numeric ceiling, so this note
does not need to infer one: Sports Reference's bot-traffic policy caps
Basketball-Reference (an "other site," i.e. not FBref/Stathead) at **20 requests per
minute**, with violators' sessions jailed **up to a day** [DOC:
https://www.sports-reference.com/bot-traffic.html]. robots.txt independently sets
**`Crawl-delay: 3`** [DOC: https://www.basketball-reference.com/robots.txt] — a 3 s
floor between hits is exactly 20/min, so the two documented signals agree with each
other. The repo's current ≥3.5 s spacing (≈17/min max sustained) already sits ~12%
under both signals; **keep it, and never exceed 20 requests in any rolling 60 s
window.** The ban tripwire itself was deliberately left unprobed (§4).

---

## 1. robots.txt — verbatim and analysis

Fetched 2026-09-08 from <https://www.basketball-reference.com/robots.txt> [DOC].
Quoted verbatim:

```text
User-agent: AhrefsBot
Disallow: /

User-agent: GPTBot
Disallow: /

User-agent: Twitterbot
Disallow:

User-agent: *
Disallow: /basketball/
Disallow: /blazers/
Disallow: /dump/
Disallow: /fc/
Disallow: /my/
Disallow: /7103
Disallow: /play-index/*.cgi?*
Disallow: /play-index/plus/*.cgi?*
Disallow: */gamelog/
Disallow: */splits/
Disallow: */on-off/
Disallow: */lineups/
Disallow: */shooting/

Disallow: /req/
Disallow: /short/
Disallow: /nocdn/

Crawl-delay: 3

# Disallow the plagiarism.org robot, www.slysearch.com
User-agent: SlySearch
User-agent: GroundControl
User-agent: Ground-Control
User-agent: Carmine
User-agent: Skynet
User-agent: The-Matrix
User-agent: Matrix
User-agent: HAL9000
Disallow: /            #Will disallow or robot from all urls on your site
```

What this says, point by point:

- **Crawlers named.** `AhrefsBot` and `GPTBot` are banned site-wide (`Disallow: /`);
  `Twitterbot` is fully allowed (empty `Disallow:`); eight legacy/joke agents
  (`SlySearch`, `GroundControl`, `Ground-Control`, `Carmine`, `Skynet`,
  `The-Matrix`, `Matrix`, `HAL9000`) are banned site-wide; everything else falls
  under `User-agent: *` [DOC].
- **`/boxscores/` and `/leagues/` are NOT disallowed** for `User-agent: *`. Neither
  path nor any prefix covering them appears in the `*` block, so the season schedule
  pages (`/leagues/NBA_2026_games.html`) and every boxscore page the archive needs
  are robot-permitted paths [DOC + minimal reading of the text above].
- **Disallowed for `*`:** site sections `/basketball/`, `/blazers/`, `/dump/`, `/fc/`,
  `/my/`, `/7103`, the play-index CGI endpoints, player sub-pages (`*/gamelog/`,
  `*/splits/`, `*/on-off/`, `*/lineups/`, `*/shooting/`), and `/req/`, `/short/`,
  `/nocdn/` [DOC]. None of these is on the archive's fetch path (schedules, boxes,
  season totals), but a crawler MUST NOT follow links into them.
- **`Crawl-delay: 3`** — at most one request per 3 seconds per crawler on
  robot-convention-following clients [DOC]. Note this is a convention signal, not an
  enforced server limit; the enforced limit is §2's 20/min rule. Arithmetically they
  coincide (60 / 3 = 20), which is presumably intentional [INFERENCE].

## 2. Terms of Use / data-use policy — exact language

Two governing documents, both primary sources owned by the site operator (Sports
Reference LLC):

- Site Terms of Use: <https://www.sports-reference.com/termsofuse.html> (page states
  "Effective Date: October 1, 2004 … Last Updated: May 19, 2023") [DOC].
- Data-use / licensing guidelines: <https://www.sports-reference.com/data_use.html>,
  which excerpts the Terms and points to "clause 5 for permitted uses of our data"
  [DOC].

### 2.1 Automated access (Terms §5, item 9)

> "without our express written permission, use any automated means to access or use
> the Site, including scripts, bots, scrapers, data miners, or similar software, in a
> manner that adversely impacts site performance or access; or" [DOC:
> termsofuse.html §5]

Key precision: the prohibition is qualified — automated access **"in a manner that
adversely impacts site performance or access"** without written permission. Polite,
rate-limited fetching is the conduct that keeps a scraper on the right side of this
clause; aggressive spidering is what it forbids [INFERENCE from the clause text].
The data-use page reinforces this: "we need to block any aggressive spidering of the
site to maintain site performance for non-bot traffic (i.e. actual people using the
site)" [DOC: data_use.html].

### 2.2 Competing-database and AI-training bans (Terms §5, items 10–11)

> "use any material or Content from the Site, including without limitation any
> statistics or data, (i) to create any database, archive, or other data store that
> competes with or constitutes a material substitute for the services or data stores
> offered on the Site or by the Site's Data Providers or (ii) to provide any service
> that competes with or constitutes a material substitute for the services or data
> stores offered on the Site or by the Site's Data Providers; or" [DOC:
> termsofuse.html §5]

> "copy or use any material or Content from the Site, including without limitation
> any statistics, data, text, graphics, or images, for purposes of training,
> fine-tuning, prompting, or instructing artificial intelligence models or
> technologies in any manner…" [DOC: termsofuse.html §5]

The data-use page paraphrases the practical effect: "you should not create websites
or tools based on data you scrape from Sports Reference or any of our sites or use
our data to train generative artificial intelligence models without our permission"
[DOC: data_use.html]. A local, private, never-republished archive that credits SR as
its source sits under the §5 preamble's welcomed use — "sharing, using, modifying,
repackaging, or publishing data found on individual SRL webpages is welcomed,
whether for commercial or non-commercial purposes," provided the use credits SRL
"to the maximum extent possible" and does not violate the express restrictions
(including the two quoted above) [DOC: termsofuse.html §5 preamble]. Custom bulk
extracts from SR itself "start at a minimum of $5,000" per the data-use page [DOC:
data_use.html] — the paid alternative to scraping.

### 2.3 Search engines must obey robots.txt (Terms §6)

> "Search engines are expected to follow the guidelines set forth in the robots.txt
> file that we provide." [DOC: termsofuse.html §6]

This is the clause that elevates the §1 `Crawl-delay: 3` from convention to a
site-stated expectation for automated clients [INFERENCE].

### 2.4 The numeric ceiling: bot-traffic policy (the ban rule)

<https://www.sports-reference.com/bot-traffic.html> (dated October 26, 2022; "Update:
May 29, 2024") [DOC]. Quoted in full — it is short, and it is the entire documented
ceiling:

> "Sports Reference is primarily dependent on ad revenue, so we must ensure that
> actual people using web browsers have the best possible experience when using this
> site. Unfortunately, non-human traffic, ie bots, crawlers, scrapers, can overwhelm
> our servers with the number of requests they send us in a short amount of time.
> Therefore we are implementing rate limiting on the site. We will attempt to keep
> this page up to date with our current settings.
>
> Currently we will block users sending requests to:
>
> - FBref and Stathead sites more often than ten requests in a minute.
> - our other sites more often than twenty requests in a minute.
> - This is regardless of bot type and construction and pages accessed.
> - If you violate this rule your session will be in jail for up to a day." [DOC:
> bot-traffic.html]

Reading for Basketball-Reference: BR is one of "our other sites" (the FBref and
Stathead carve-outs are named separately), so **the documented ceiling for BR is 20
requests/minute, enforced per session regardless of bot type or pages accessed,
with violations jailed up to 24 hours** [DOC + minimal inference on which bucket BR
falls in]. Corroborating IP-level language on the data-use page: "If we notice
excessive activity from a particular IP address we will be forced to take
appropriate measures, which will include, but not be limited to, blocking that IP
address" [DOC: data_use.html].

## 3. Demand curve: page weight vs server cost

Why the ceiling is where it is, from this note's own measurements [OBSERVED, §4]:

- Sampled compressed page weights: season schedule pages ~238 KB (1946-47) and ~267
  KB (2025-26); boxscore page ~166 KB (1946-11-01 first-ever game). All within the
  previously reported ~150–500 KB band.
- Full-archive arithmetic: ~75k boxscore pages × ~150–270 KB ≈ **12–20 GB total
  transfer**, plus ~160 season/totals index pages (negligible). That is a trivial
  amount of bytes in absolute terms — a single user streaming one game uses more.
- So the binding cost is **request rate, not bytes**: SR's stated motive is ad
  revenue and human browsing experience, and unthrottled bots "overwhelm our servers
  with the number of requests they send us in a short amount of time" [DOC:
  bot-traffic.html]. Each page is a dynamic app-server render behind Cloudflare
  (all probe responses carried `server: cloudflare` [OBSERVED]), not a static CDN
  file — concurrent bot barrages compete directly with human page loads.
- Throughput math at compliant rates: 75k pages at 3.5 s spacing ≈ 73 h (~3 days);
  at the absolute 20/min ceiling ≈ 62.5 h (~2.6 days). Pushing from the current
  posture to the ceiling saves **under half a day on a multi-day one-shot build** —
  no material gain for measurable ban risk. The demand curve therefore argues for
  staying well under the ceiling, not exploring it [INFERENCE].

## 4. Observed enforcement (probe log)

**Design.** Baseline-only probing: confirm 200s and inspect rate-limit signaling at
a rate both documented signals permit. No burst test, no ceiling exploration — the
ceiling is documented (§2.4), so hammering to find the tripwire would add nothing
but risk. The ban threshold is therefore **documented but unprobed**; stated plainly
per the research brief.

**Request log** (3 requests, sequential, ~6.0 s spacing, all from one session on
2026-09-08 ~15:12 UTC; UA on every request:
`nba-tv-research/0.1 (personal archive research; polite crawl ~1 req/6s)`):

| # | URL | Gap | Status | Bytes (body as received) | Notable headers |
|---|---|---|---|---|---|
| 1 | `/leagues/BAA_1947_games.html` | — | 200 | 237,671 | `server: cloudflare`, `content-type: text/html`, `content-encoding: gzip`; **no** `Retry-After`, **no** `RateLimit-*`/`X-RateLimit-*` headers |
| 2 | `/boxscores/194611010TRH.html` | 6.0 s | 200 | 166,073 | same as above |
| 3 | `/leagues/NBA_2026_games.html` | 6.0 s | 200 | 267,303 | same as above |

(Document fetches for this note — robots.txt, BR homepage, data_use.html,
termsofuse.html, bot-traffic.html — were separately retrieved via the research
reader on 2026-09-08; all returned normally with no blocking, captcha, or rate
headers encountered.)

**Findings.**

- Baseline polite fetching works exactly as documented: 200s, full pages, no
  degradation signals at ~10 req/min [OBSERVED].
- BR sends **no proactive rate-limit headers** (`RateLimit-*`, `X-RateLimit-*`) and
  no `Retry-After` on normal responses — a client cannot read its quota position
  from headers and MUST track its own request rate [OBSERVED + INFERENCE].
- Backend fingerprint is Cloudflare (`server: cloudflare` on all three) with gzip
  transfer encoding [OBSERVED]. Enforcement (session jail up to a day [DOC]) is
  therefore plausibly a Cloudflare/session-layer block, not an HTTP-429-with-headers
  API scheme — expect a block page or connection refusal rather than a polite 429
  [INFERENCE].
- **Not tested:** the 20/min tripwire, `Retry-After` on 429, jail duration, jail
  scope (session vs IP), and captcha/block-page text. Any claim about what the block
  looks like beyond "jailed up to a day" would be community anecdote, and none is
  relied on here.

## 5. Verdict and recommendation

**Documented ceiling (no inference needed).** 20 requests/minute on
Basketball-Reference; >20/min risks a session jail of up to 24 h
[DOC: bot-traffic.html]. robots.txt adds `Crawl-delay: 3` on robot-permitted paths
(which include `/leagues/` and `/boxscores/`) [DOC: robots.txt]. There is no
published daily quota, no API key tier, and no header-advertised quota position.

**Recommended operating range: sustained 10–17 requests/minute** (one request every
3.5–6 s), single-threaded, strictly sequential, on robot-permitted paths only:

- The floor (3.5 s / ~17/min) is the repo's existing posture and keeps ~12% margin
  under **both** documented signals simultaneously (3 s Crawl-delay, 20/min jail
  rule) — a single constant satisfies both [INFERENCE].
- The 10–17/min band (rather than exactly 17) leaves headroom for retries and for
  the reader-style document fetches that share the session/IP [INFERENCE].
- Never exceed 20 requests in any rolling 60 s window — the jail rule counts burst
  rate, not average rate ("more often than twenty requests in a minute … regardless
  of … pages accessed") [DOC + INFERENCE].
- Keep the identifying UA, keep per-season resumability (a killed run must resume,
  never restart — restarts multiply load for zero gain), skip files already on disk,
  and never follow links into §1's disallowed paths [INFERENCE].
- Honor attribution: credit Sports Reference as the data source to the maximum
  extent possible (Terms §5 preamble [DOC]), never republish the dataset, never use
  scraped content for AI training (Terms §5 items 10–11 [DOC]).

**Ban signals to watch for** (escalate immediately; do not retry into them):

1. HTTP **429** (especially with a `Retry-After` header — absent on baseline
   responses [OBSERVED], so its appearance is itself a signal), or a sudden run of
   **403s** from Cloudflare [INFERENCE — header behavior unprobed, §4].
2. Captcha interstitials, "access denied" / block-page HTML, or a session that stops
   succeeding while a fresh session works — consistent with the documented session
   jail of up to a day [DOC on the jail; page-text specifics UNPROBED].
3. IP-wide failure across sessions/UAs — consistent with the documented IP-block
   escalation [DOC: data_use.html].

**Backoff policy (recommended).** On any signal above: **stop the run immediately**
(page-weight math in §3 shows there is never schedule pressure worth risking a
24 h jail for). Wait at least 10 minutes, then send one probe; if it fails, stand
down for up to 24 h (the documented maximum jail [DOC]) before a single re-probe.
On recovery, resume at half the previous rate (≥7 s spacing) for the rest of that
batch. Log every backoff event with timestamp, signal, and rate — repeated
backoffs mean the standing rate must drop, not that retries must rise [INFERENCE].

## 6. Sources (primary only)

- <https://www.basketball-reference.com/robots.txt> — full text quoted in §1.
- <https://www.sports-reference.com/termsofuse.html> — Terms of Use, §5 (items 9–11,
  preamble), §6; "Last Updated: May 19, 2023."
- <https://www.sports-reference.com/data_use.html> — data-use/licensing guidelines;
  automated-use excerpt, $5,000 custom-extract minimum, IP-blocking statement.
- <https://www.sports-reference.com/bot-traffic.html> — rate-limit/jail policy;
  "October 26, 2022 / Update: May 29, 2024."
- This note's own probes (§4 table): 3 requests, ~6 s spacing, identifying UA,
  2026-09-08.

No community reports (forums, GitHub issues, scraper-library lore) were used for any
limit claim above; per the brief they would be labeled anecdote, and none was
needed — the operator publishes the number.
