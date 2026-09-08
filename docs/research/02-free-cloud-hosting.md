# Research 02 — Free-cloud home for the full-game tape archive

**Ticket:** nicolas-found42/nba-tv#2 · Part of map #1 · Researched 2026-09-07 against primary sources (provider docs, ToS, pricing pages) plus two live endpoint probes.
**Driver input folded in:** the driver holds **5 TB of Google Drive storage** and offers it as the primary host candidate; this file evaluates Drive as the primary and the full landscape behind it. **Ticket #4 input folded in (closed):** its verdict is **resolve-public-URLs-at-play-time, no rehost** — the exhaustion pipeline writes `tape_sources(game_id, rank, source_class, url_or_pointer, match_confidence, verified_at, …)` consumed by the player, and most sources are **stream-only** (official NBA free tier, YouTube fan channels — [docs/research/04-tape-source-landscape.md](04-tape-source-landscape.md)).

## Constraints (from ticket #2 and map #1)

- Personal use; **$0** (no payment); open-source-compatible; pure-Rust lean.
- **Stream, never bundle:** no bulk video stored locally; the cloud host *is* the storage.
- Archive, not live. Schedule + box score always shown; tape best-effort, marked unavailable only after exhausting sources (ticket #4's "exhaust all sources").
- Per map #1 standing preference, legality gate = acquisition method; platform ToS/takedown are analyzed here as **practical availability risk**, not as a legality verdict.
- Scale: map #1 estimates **80 seasons / ~60k+ games** ([map #1 "Not yet specified"]). At ~1–2 GB per full game (480p–720p H.264 re-encode for ~2–2.5 h of video) that is **~60–120+ TB** at full-history scale.

## TL;DR verdict

1. **Full-history rehost on any $0 cloud: not feasible.** Every paid-capacity cloud gives ≤ ~20–100 GB free (R2 10 GB, B2 10 GB, OCI 20 GB, Pinata 1 GB, Drive free 15 GB, Dropbox 2 GB, MEGA 20 GB, Storj trial only). The two services that accept arbitrary volume free — Internet Archive and YouTube — are both notice/enforcement-fragile for pro sports footage, and YouTube's bytes are only playable through YouTube's own player.
2. **Decision: streaming-first. No bulk rehost.** Playback resolves public URLs at play time from the `tape_sources` table (ticket #4's contract); the app never rehosts the corpus. Every stream-only source (S0 official NBA, S2/S3 YouTube fan channels) is played in place and must *never* be pulled down into Drive — converting those to owned bytes is prohibited (NBA App is a registered streaming service; YouTube stream extraction violates YouTube ToS restrictions 1 & 3).
3. **What the 5 TB Drive is actually for: a bounded personal cache tier, not the archive.** Drive is the only $0 host that is (a) explicitly ToS-compatible for personal video streaming ("Google Drive allows you to store, share, and stream video content"), (b) proven to serve direct HTTP byte-range progressive playback (verified live, `206 Partial Content`), and (c) large enough to matter (5 TB ≈ **1,400–6,000 games** as cache capacity). Its job: (i) **downloadable sources that don't stream well in place** — IA-style VHS rips/DVD transfers and download-only hosts — normalized to faststart MP4 and streamed back over Range; (ii) **favorites/repeated-viewing cache**; (iii) optional availability insurance for re-findable games. The app must be fully functional with zero Drive usage (pure link-out is the baseline mode).
4. **What it forces on the player:** progressive HTTP MP4 over `Range` requests (no HLS/DASH exists anywhere in the $0 stack), files must be H.264/AAC MP4 with the moov atom at the front, Drive URLs need redirect-following + `confirm` token handling, and YouTube links are **external/embed-only** (in-app stream extraction violates YouTube ToS §restrictions 1 and 3).

---

## 1. Primary candidate: Google Drive (driver's 5 TB)

### 1.1 Capacity vs. the archive

- Driver quota: **5 TB** (driver-provided fact; e.g. Google One 2 TB/5 TB-class plan storage; this file treats the 5 TB as given).
- Free-tier baseline for comparison: Google accounts include "up to **15 GB** of cloud storage at no charge", shared across Drive/Gmail/Photos — [Google storage help](https://support.google.com/drive/answer/2375123).
- Fit at full history: 5 TB ÷ (1–2 GB/game) = **2,500–5,000 games**, i.e. **~4–8% of the map's ~60k+ games** (up to ~10% with ~800 MB aggressive 480p re-encodes). Verdict: **bounded subset, never the full archive.** [arithmetic on map #1's game count]
- Upload pacing: Drive's documented upload cap is **750 GB/day** ("Google Workspace users can only upload 750 GB per day between My Drive and all shared drives… users who reach the 750 GB limit… can't upload or copy additional files until 24 hours have passed") and max **5 TB per file** — [Drive API usage limits](https://developers.google.com/workspace/drive/api/guides/limits). 5 TB therefore needs **≥ 7 calendar days** of API-visible uploads regardless of bandwidth, and weeks over residential upstream (e.g. ~16 days at 30 Mbps). [arithmetic]
- Per-file 5 TB cap means a game never needs splitting (vs. GitHub Releases' 2 GiB/file, §2.10).

### 1.2 Serving behavior — what Drive actually delivers

- **Direct progressive streaming: verified.** Live probe 2026-09-07 of a public file on Google's download endpoint:
  `https://drive.usercontent.google.com/download?id=…&export=download&confirm=t` returns **HTTP 206 Partial Content, `accept-ranges: bytes`, `content-range: bytes 0-1023/<full size>`** for `Range: bytes=0-1023`. A player can seek progressively through the *original uploaded file* — no Google transcoding involved on this path. (Endpoint pattern corroborated by real-world code, e.g. [yt-dlp's googledrive extractor](https://github.com/yt-dlp/yt-dlp/blob/master/yt_dlp/extractor/googledrive.py), which assembles exactly this URL form.)
- **Confirm-token flow:** large/public files first serve a virus-scan interstitial page; the client must follow redirects and re-request with `confirm=t` (and sometimes a `uuid`). Documented pattern in production downloaders ([yt-dlp extractor](https://github.com/yt-dlp/yt-dlp/blob/master/yt_dlp/extractor/googledrive.py); `curl`-based shell flows resolving `confirm=` from the interstitial). The Rust client needs a redirect-following HTTP client plus interstitial parsing — no more.
- **API route:** `files.get`/`files.download` via Drive API (200 quota units per download call) — [Drive API usage limits](https://developers.google.com/workspace/drive/api/guides/limits). API egress quota: **1 TB/day per project** before charges apply; Google plans to start charging *above* this later in 2026 with ≥ 90 days' notice (same page). Single-user playback (~1–2 GB per game session) is orders of magnitude below that ceiling. [arithmetic]
- **Embed route:** Drive's web player (`drive.google.com/file/d/<id>/preview` iframe) streams with Google's own adaptive player; it also plays formats the direct path can't (MKV etc.). Use only if a webview is acceptable; the pure-Rust path is the direct byte-range route.
- **What it does NOT serve:** no HLS/DASH manifest is exposed to third-party clients on either direct route. **No $0 option in this study serves HLS to a custom player** (Cloudflare Stream does, but is paid and forbids third-party CDNs — §2.3).

### 1.3 ToS for lawfully-acquired sports footage

- **Personal video streaming is explicitly allowed:** "Google Drive allows you to store, share, and stream video content, but should not be used as a replacement for a content distribution network… Google Drive will restrict usage when it appears that it's being used for **large-scale public streaming**. Repeated violations may result in additional action, including terminating your account or ability to use Google Drive." — [Abuse Program Policies & Enforcement, "Content Distribution"](https://support.google.com/docs/answer/148505). A single-user personal archive is the allowed case; a shared public mirror is not.
- **Copyright enforcement is notice-driven, not scan-driven:** Drive has no Content ID (Content ID is a YouTube system — [YouTube's docs](https://support.google.com/youtube/answer/2797370)). Drive's policy: "Do not share copyrighted content without authorization… It is our policy to respond to clear notices of alleged copyright infringement. **Repeated infringement… will result in account termination.**" — [same program-policies page, "Copyright Infringement"](https://support.google.com/docs/answer/148505). Enforcement is triggered by reports through [Google's legal troubleshooter](https://support.google.com/legal/troubleshooter/1114905).
- **File-level restriction:** "We may review content to determine whether it is illegal or violates our Program Policies, and we may remove or refuse to display content that we reasonably believe violates our policies or the law" — [Drive Additional Terms of Service, §2](https://www.google.com/drive/terms-of-service/), effective 2025-10-22. Google has also announced files violating ToS get flagged/restricted from sharing (announced 2021; the flagging behavior is described in the same enforcement paragraph of [the abuse-policy page](https://support.google.com/docs/answer/148505): "access restriction of content, removal of content, and limitation or termination of a user's access to Google products").
- **Quota-abuse enforcement:** "Google may take action on accounts that go above storage quota limits… we may reject new uploads, compress content that exists, or delete content" — [abuse policies](https://support.google.com/docs/answer/148505). Staying at ≤ 5 TB avoids this.
- **Inactivity:** Drive action at least once every 2 years required ([abuse policies, "Account Inactivity"](https://support.google.com/docs/answer/148505)) — a streaming app clears this trivially.

### 1.4 Takedown/deletion risk — the honest assessment

- **Probability:** low while the sharing surface stays personal. No automated fingerprinting scans Drive; a DMCA notice must name a specific file. Keeping files at "anyone with the link" (never listed publicly) plus a non-public app minimizes discovery. [INFERENCE built on the notice-driven policy cited above]
- **Blast radius:** the worst case is **whole-Google-account termination** for repeat copyright notices ([abuse policies](https://support.google.com/docs/answer/148505); [Drive Additional Terms §3](https://www.google.com/drive/terms-of-service/)) — i.e., not just the archive but Gmail/Photos on that account. Mitigations: keep the archive on a dedicated account if possible, and never make a Drive copy the *only* copy of a game (see §3.4 rebuild policy).
- **Single point of failure:** with no local bulk storage allowed, Drive copies are the only copies of the subset. Account loss = subset loss. Hence: only upload games that ticket #4's sources can re-find elsewhere; Drive is a *cache of first choice*, not an archive of last resort.
- **User-reported (not officially documented) cap:** heavily-downloaded *public* files can hit a per-file "download quota exceeded" throttle until the quota window resets; Google has never published the number ([community reports](https://support.google.com/drive/thread/2035857)). Single-user playback (a few hundred GB/day of egress at most) should never trigger it. [user-reported; treat as unquantified]

**Drive verdict: feasible and recommended — as the bounded cache tier of a streaming-first topology, not as the corpus host.** Best cost/ToS profile of every option studied for holding *downloadable* sources; useless for stream-only ones (those are played in place).

---

## 2. Full option matrix

All figures below are from the provider's own pages (accessed 2026-09-07 unless noted).

| Option | Free storage | Egress/bandwidth | Serves to a custom player | ToS / takedown risk for lawfully-acquired NBA footage | Verdict |
|---|---|---|---|---|---|
| **Google Drive (driver's 5 TB)** | 5 TB (driver-provided; free tier 15 GB) | No published per-user cap; API project egress 1 TB/day pre-charges; per-file public-share throttle user-reported | **Direct progressive MP4 over Range — verified (206)**; or Google's embed player | Personal streaming explicitly allowed; copyright = notice-driven; repeat notices → **account termination** | **Bounded cache tier for downloadable sources** (§1, §3); not the corpus host |
| **Internet Archive** | Unlimited ("no fees"; IA states ~$2/GB permanent storage cost, donation-funded; backs up all files; perpetual intent) | "Free storage, and free bandwidth, forever" for library-type cultural materials; no published download cap; upload queue throttles with 503 SlowDown | **Direct progressive MP4 over Range — verified (206)** on IA-derived h.264 files; serves original + derived files over plain HTTP | Notice-and-takedown via DMCA agent; **repeat-infringer account termination**; mirroring material that exists online → "removal from archive.org and your account being locked"; item caps: 1 TB/item hard, 1000 files/500 GB/item recommended, 5,000 files/day | **Best link-out target** for full-history coverage (other people's IA items). Rehost of the driver's own archive: not durable — §2.2 |
| **YouTube (unlisted/private)** | Unlimited uploads, free | N/A — bytes only via YouTube's player | **Player-only.** Data API exposes metadata + `player.embedHtml`, **no stream URLs** ([videos.list resource](https://developers.google.com/youtube/v3/docs/videos)); API ToS §16.3 grants no right to make audiovisual content available "other than through the use of the YouTube API Services" ([API ToS](https://developers.google.com/youtube/terms/api-services-terms-of-service)); downloading/scraping/stream-extraction prohibited by ToS restrictions 1 & 3 ([YouTube ToS](https://www.youtube.com/t/terms)) | **Highest risk.** Every upload is **automatically scanned by Content ID** ([docs](https://support.google.com/youtube/answer/2797370)); rights-holders can block/monetize/track; **even private and unlisted videos are reviewed for copyright** ([privacy settings doc](https://support.google.com/youtube/answer/157177)); strikes → channel removal; repeat-infringer termination ([ToS](https://www.youtube.com/t/terms)). API-uploaded videos from unverified projects are **private-by-default** pending API audit ([API docs](https://developers.google.com/youtube/v3/docs/videos)) | **Link-out/external only.** Mass-rehost = mass Content ID claims and channel loss. Embed/launch-in-browser for games that live on YouTube |
| **Cloudflare R2 free tier** | **10 GB**/month ([pricing](https://developers.cloudflare.com/r2/pricing/)); beyond that $0.015/GB-month (10 TB ≈ $150/mo — violates $0) | Egress free; **`r2.dev` public URLs are rate-limited and "intended for non-production use"** ([public buckets doc](https://developers.cloudflare.com/r2/buckets/public-buckets/)); custom domain required for real serving (needs a domain) | Direct HTTP (S3/custom domain), Range-capable | IP-infringing content "may be blocked or removed"; account termination "upon receiving any number of DMCA notifications" ([self-serve agreement §8](https://www.cloudflare.com/terms/); [developer-platform terms §8](https://www.cloudflare.com/service-specific-terms-developer-platform/)) | **Rejected.** 10 GB < one season; free tier can't hold even highlight reels |
| **Backblaze B2 free tier** | **10 GB** ("First 10GB storage is always free") ([pricing](https://www.backblaze.com/cloud-storage/pricing)); $6.95/TB-mo beyond | Free egress up to 3× stored, then $0.01/GB | Direct HTTP/S3, Range | DMCA-driven; repeat-infringer termination ([B2 ToS](https://www.backblaze.com/company/terms.html)) | **Rejected.** Same 10-GB wall |
| **Oracle Cloud Always Free** | **20 GB** object storage (Standard+IA+Archive combined, post-trial), 50k API calls/mo, 200 GB block volume, small ARM VMs ([docs](https://docs.oracle.com/en-us/iaas/Content/FreeTier/freetier_topic-Always_Free_Resources.htm)) | 10 Mbps load balancer class; VM egress unmetered but tiny storage bounds it | Direct HTTP; could host the *catalog*, not video | Idle VMs reclaimed (95th-pct CPU/network/memory < 20% over 7 days); idle accounts deemed abandoned after 30 days ([docs](https://docs.oracle.com/en-us/iaas/Content/FreeTier/freetier_topic-Always_Free_Resources.htm); [FAQ](https://www.oracle.com/cloud/free/)) | **Rejected for video; useful only as catalog/API host** if ever needed |
| **Storj (IPFS/distributed)** | **No standing free tier**: 25 GB / 30-day trial, then $5/month minimum floor ([pricing](https://storj.io/pricing), [FAQ](https://www.storj.io/pricing)) | $7/TB egress on Standard | S3-compatible | Paid for any real use | **Rejected.** $0 impossible past trial |
| **IPFS pinning (Pinata; web3.storage)** | Pinata free: **1 GB**, 500 files, 10 GB bandwidth ([pricing](https://pinata.cloud/pricing)); web3.storage's own pricing page now redirects to a different product (fil.one, observed 2026-09-07) — its free tier is gone | Pinning-gateway-bound; HLS video streaming is a paid feature on Pinata | Gateway HTTP (Range varies by gateway) | Pinning services honor DMCA (Pinata terms) | **Rejected.** Three orders of magnitude short |
| **Google Drive free tier / Dropbox / MEGA** | 15 GB / 2 GB / 20 GB ([Google](https://support.google.com/drive/answer/2375123); [Dropbox plans](https://www.dropbox.com/plans) "Basic: 2 GB"; [MEGA](https://mega.io/pricing) "free plan includes 20 GB") | MEGA free: dynamic IP-based 6-hour transfer limits ([help](https://help.mega.io/plans-storage/space-storage/transfer-quota)); Dropbox/Drive: per-file public-share throttles (user-reported) | Direct HTTP (Range on Drive verified; MEGA is E2E-encrypted, gateway-muxed) | Dropbox: don't share content you don't have the right to; repeat-infringer termination ([ToS](https://www.dropbox.com/terms)); MEGA: no copyright-infringing storage, takedown/removal without notice, suspension on repeated notices, termination for repeat infringers, free accounts inactive > 3 months suspended ([terms §20.8.2, 24, 25–27, 38.4](https://mega.io/terms)) | **Rejected as primary.** 2–20 GB caps are ~0.03% of the archive. (Driver's separate 5 TB plan is the exception that matters) |
| **GitHub Releases** | Free; **2 GiB per release file**, "no limit on the total size of a release, nor bandwidth usage" ([docs](https://docs.github.com/en/repositories/releasing-projects-on-github/about-releases)) | Unmetered per docs, but AUP: throttle/suspend for "significantly excessive" bandwidth, repo deletion for "undue strain" ([AUP §9](https://docs.github.com/en/site-policy/acceptable-use-policies/github-acceptable-use-policies)) | Direct HTTP, Range-capable | IP-infringing content prohibited ([AUP §3](https://docs.github.com/en/site-policy/acceptable-use-policies/github-acceptable-use-policies)); repeat-infringer account termination ([ToS §F](https://docs.github.com/en/site-policy/github-terms/github-terms-of-service)) | **Rejected.** A media library on Releases is a textbook AUP misuse; 2 GiB/file forces splitting; repo loss = archive loss |

### 2.2 Internet Archive as a *rehost* target — why it loses to Drive

IA is the only provider with effectively **unlimited free storage** ("At this time we have no fees for uploading and preserving materials… permanent storage costs us approximately $2.00US per gigabyte… we duplicate/backup all files… our intention is to store… in perpetuity" — [IA info page](https://help.archive.org/help/archive-org-information/); "free storage, and free bandwidth, forever" for library-type materials — [Rights page](https://help.archive.org/help/rights/)). But as a place to rehost *the driver's own NBA archive*:

- **Notice-and-takedown with termination:** IA removes on copyright complaint ("we will remove it per our Copyright Policy… terminating the accounts of users who… are 'repeat infringers'") — [Rights page](https://help.archive.org/help/rights/); DMCA notice elements at [IA Copyright Policy](https://archive.org/about/terms.php). Counter-notice restores in 10–14 days only if no suit is filed.
- **Bulk-mirroring rule bites exactly our use case:** "please keep a copy locally on your own drives… Uploading them prior to that may result in their removal from archive.org and your account being locked" (for mirroring material that exists online) — [What is ok to upload](https://help.archive.org/help/uploading-what-is-not-ok-or-not-ok-to-upload/). A 60k-game bulk upload of material that mostly already exists on IA/YouTube is the pattern this policy targets.
- **Throughput ceilings:** 1 TB/item hard limit, 1000 files/500 GB/item recommended, 5,000 files/day, upload-queue `503 SlowDown` throttling ([upload tips](https://help.archive.org/help/uploading-tips/); [ias3 use limits](https://archive.org/developers/ias3.html)). 60 TB ≈ ≥120 days at the daily file cap, longer with SlowDowns. [arithmetic]
- **Still the best *link-out* target:** IA items uploaded by the wider community already contain enormous amounts of NBA content, serve plain progressive MP4 (verified `206` on Range, 2026-09-07), have stable resolvable URLs (`archive.org/download/<item>/<file>` 302→storage node), and IA's 2001-era ToS disclaims any uptime guarantee — the catalog must treat every IA link as fallible and fall through the chain.

### 2.3 Notable non-feasible-for-us non-option

Cloudflare Stream serves HLS and handles transcode/CDN, but it is paid, and its terms *forbid* serving Stream-hosted video through third-party delivery: "Videos hosted or encoded by Cloudflare Stream must be served using Cloudflare Stream" ([service-specific terms](https://www.cloudflare.com/service-specific-terms-developer-platform/)). Listed only to close the "why not a real video CDN" question.

---

## 3. Recommended topology: streaming-first with a bounded Drive cache

### 3.1 The decision (this is the resolution of the ticket)

**Streaming-first. The app resolves public URLs at play time and rehosts nothing.** This adopts ticket #4's contract: the exhaustion pipeline writes `tape_sources(game_id, rank, source_class, url_or_pointer, match_confidence, verified_at, query_used, notes)` and the player consumes it at play time ([docs/research/04-tape-source-landscape.md §3.6](04-tape-source-landscape.md)). Bulk self-hosting is rejected — not because it is technically impossible on IA, but because (a) it is takedown-fragile (§2.2), (b) it is redundant: the measurable corpus #4 found (official NBA classics + fan archives + IA's 16k items) is already **streamable where it lives**, and (c) rehosting the whole thing is ~60 TB against a 5 TB Drive and ~never-against-DMCA IA. [INFERENCE from the cited policy + #4's measured landscape]

The driver's 5 TB Drive is **consciously repurposed as a bounded personal cache tier**, not the archive. The driver should make this pivot explicitly: the Drive does *not* increase what is watchable (that is decided by #4's per-game hit rates); it stabilizes and normalizes a slice of it.

### 3.2 What the 5 TB Drive is for — and what it must never be for

**For (cache jobs):**
1. **Downloadable sources that don't stream well in place.** IA's VHS rips and DVD transfers (ticket #4's S1 pattern, e.g. the 1993 Finals G6 WNBC rip) exist as downloadable files; some are oversized, oddly encoded, or served by hosts where progressive streaming is poor (download-only hosts, E2E-encrypted MEGA links). Those get pulled once, normalized to faststart MP4, cached into Drive, and streamed back over Range forever after.
2. **Favorites / repeated-viewing cache.** Any game the user replays can be cached so it never depends on a link-out source that may vanish.
3. **Optional availability insurance.** For games where the only link-out source is a churning fan upload, a cached copy keeps it watchable after the source dies — but only for games the chain can re-find elsewhere (§3.5 rule 1).

**Never for (hard boundary):**
- **Stream-only sources.** S0 (official NBA App/NBA ID classics — a registered streaming service) and S2/S3 (YouTube fan channels) are played in place and are **never downloaded into Drive**. Pulling NBA App streams violates its service terms; pulling YouTube streams violates YouTube ToS restrictions 1 and 3 ([YouTube ToS](https://www.youtube.com/t/terms)). The Drive must not become a mechanism for converting stream-only content into owned bytes — that boundary is a driver decision made consciously here.

**Drive-absent mode:** the app must be fully functional with zero Drive usage — pure link-out is the baseline mode; the cache is an accelerator, never a dependency.

### 3.3 Play-time resolution order

```
per game_id, player iterates:
  0. Drive cache entry (if any)     → progressive MP4 (Range)   [user-owned, most stable]
  1. tape_sources by rank:
     S0 official NBA                → play in place (NBA App / official web player)
     S1 IA item                     → progressive MP4 (Range), in place
     S2 YouTube                     → external player / embed (never in-app extraction)
     S3 other platforms             → external
     S4/S5 pointers                 → "exists, not streamable" badge
  2. nothing verified               → "unavailable (ladder consumed <date>)" badge
```

Schedule + box score always render regardless (map rule; ticket #3's data).

### 3.4 What 5 TB buys as a cache (capacity math)

Arithmetic on map #1's ~60k+ games; file sizes are our codec assumptions:

| Encoding target | Size/game | Games the cache could hold | Share of full history |
|---|---|---|---|
| 720p H.264 ~3 Mbps | ~3.5 GB | ~1,400 | ~2% |
| 480p/540p re-encode ~1 Mbps | ~1 GB | ~5,000 | ~8% |
| aggressive 480p ~0.8 Mbps | ~0.8 GB | ~6,250 | ~10% |

Read this as a **stabilizer budget, not coverage**: #4's realistic free-streamable hit rate peaks at 30–50% of games (era-dependent), so a filled cache can hold a large share of everything *findable* (Finals back to 1990, star-team eras, favorites) — but it changes availability, not discovery. Leave 10–15% headroom under the quota (§3.5 rule 3).

### 3.5 Ops rules the risk analysis forces

1. **Cache only re-findable, downloadable games.** A Drive copy requires ≥ 1 viable remote source in `tape_sources` (so account loss costs a cache, not the game); and the source must be a downloadable artifact (S1-style files), never a stream-only surface (§3.2 boundary).
2. **Dedicated Google account** for the cache (blast-radius isolation; Google terminates accounts, not files, on repeat notices — §1.3).
3. **Stay ≤ 5 TB**, never over quota (Google may delete over-quota content — §1.3).
4. **Catalog records file IDs, not raw URLs.** Drive IDs → URLs are recomputed at play time (URL forms evolve; IDs are stable). IA records: item + file name. YouTube records: video ID. The cache directory's own index is part of the app's local DB (small, not bulk video).
5. **No payment, no API-billing exposure:** stay under the Drive API 1 TB/day/project threshold so the planned 2026 API billing never touches us (single-user streaming is ~100× under it).

### 3.6 Fallback if $0 cloud fails (feeds map #1's open question)

"Cloud fails" has two cases under this topology, both bounded:
- **Cache loss** (Drive account terminated): the play-time chain falls through to `tape_sources` — the game remains watchable wherever it was re-findable, and the cache can be rebuilt from those same sources. Cost: favorites must re-stream until re-cached.
- **Link-out rot** (IA items / YouTube uploads vanish): #4's re-scan cadence (90-day re-checks, monthly official-catalog re-enumeration) plus the human-review queue recovers what reappears; what never reappears is honestly marked unavailable — which is exactly map #1's "try hard, then mark unavailable" requirement.

No separate disaster plan is needed: the chain *is* the fallback mechanism.

---

## 4. What this forces on the player (input for tickets #5/#6)

1. **Protocol: progressive HTTP MP4 with `Range` requests. Verified on both hosts that matter** (Drive `drive.usercontent.google.com` → 206; IA `archive.org/download` → 302 → storage node → 206; probes 2026-09-07). **No HLS/DASH anywhere in the $0 stack** — the player can skip HLS entirely, which shrinks the Rust dependency surface.
2. **Container discipline:** cache copies must be **H.264/AAC in MP4 with the moov atom at the front** (`faststart`). Progressive seeking over Range only works when the index precedes the media. MKV/AVI downloadable sources (ticket #4's S1 pattern) must be re-encoded/remuxed *before* caching into Drive (transcode decisions happen at cache-fill time, never at play time). Streamed-in-place sources skip this entirely — the player takes what the source serves.
3. **HTTP client requirements:** follow redirect chains (IA: `archive.org` → `ia8xx.us.archive.org`; Drive: `drive.google.com/uc` → `drive.usercontent.google.com`), parse Drive's `confirm`-token interstitial for large files, handle `503 SlowDown` backoff on IA (upload side only — irrelevant to playback), and resume on `206`.
4. **Decode reality check (Rust):** streaming transport in pure Rust is trivial (reqwest/ureq + Range). **Full H.264 decode in pure Rust is the open gap** — `openh264` bindings are C; pure-Rust H.264 decoders are immature. The realistic pure-Rust-lean options are (a) bundled `ffmpeg`/`mpv` sidecar (not pure Rust), (b) `openh264`-based decode (C dependency), or (c) webview-embed for everything (loses the "lean player" goal). This is a decision for ticket #5/#6; this research fixes only the *protocol*, not the codec stack.
5. **YouTube = external surface:** embed iframe (`player.embedHtml` is what the API hands back) or OS browser/player launch. Any in-app stream extraction requires circumventing YouTube's player and downloading streams — both prohibited by YouTube ToS restrictions 1 and 3 ([ToS](https://www.youtube.com/t/terms)) — and is additionally fragile (signature-rotation). Decision for the UX ticket: probably launch-external for YouTube, in-app for MP4 chain.

---

## 5. Interactions with sibling tickets

- **#3 (schedule+box):** unaffected; this ticket decides tape transport only.
- **#4 (tape landscape): CLOSED — its contract is adopted as-is.** The exhaustion pipeline writes `tape_sources(game_id, rank, source_class, url_or_pointer, match_confidence, verified_at, …)` consumed at play time; playback resolves URLs, never rehosts. This ticket adds to that contract: (a) the resolution order puts a Drive cache entry ahead of remote rows when one exists (§3.3), (b) `source_class` must distinguish **downloadable** artifacts (cacheable, S1-style) from **stream-only** surfaces (never cached, S0/S2/S3) so the §3.2 boundary is enforceable in code.
- **#5 (player choice):** must support progressive Range MP4 (hard requirement), resolve `tape_sources` at play time with the cache-first order (§3.3), decide the decode stack (open), and treat YouTube + official NBA App as external surfaces (hard constraint).
- **#6 (prototype):** the flow sketch should show browse → game → play-time resolution (cache → chain) → progressive playback, with the unavailable-marking path visible.

## 6. Sources

Primary documents (accessed 2026-09-07):

- Google Drive API usage limits — https://developers.google.com/workspace/drive/api/guides/limits
- Google Drive Additional Terms of Service (eff. 2025-10-22) — https://www.google.com/drive/terms-of-service/
- Google Abuse Program Policies & Enforcement (Drive) — https://support.google.com/docs/answer/148505
- Google storage tiers (15 GB free) — https://support.google.com/drive/answer/2375123
- Google legal troubleshooter (copyright reports) — https://support.google.com/legal/troubleshooter/1114905
- YouTube Terms of Service (2023-12-15) — https://www.youtube.com/t/terms
- YouTube privacy settings (unlisted/private copyright review) — https://support.google.com/youtube/answer/157177
- YouTube Content ID — https://support.google.com/youtube/answer/2797370
- YouTube API Services Terms of Service — https://developers.google.com/youtube/terms/api-services-terms-of-service
- YouTube Data API `videos` resource — https://developers.google.com/youtube/v3/docs/videos
- Internet Archive Terms of Use / Privacy / Copyright Policy (2014-12-31 / 2001-03-10) — https://archive.org/about/terms.php
- IA Help: Uploading tips — https://help.archive.org/help/uploading-tips/
- IA Help: What is ok or not ok to upload — https://help.archive.org/help/uploading-what-is-not-ok-or-not-ok-to-upload/
- IA Help: Rights — https://help.archive.org/help/rights/
- IA Help: Archive.org Information — https://help.archive.org/help/archive-org-information/
- IA S3-like API — https://archive.org/developers/ias3.html
- Cloudflare R2 pricing — https://developers.cloudflare.com/r2/pricing/
- Cloudflare R2 public buckets — https://developers.cloudflare.com/r2/buckets/public-buckets/
- Cloudflare Self-Serve Subscription Agreement — https://www.cloudflare.com/terms/
- Cloudflare Service-Specific Terms (Developer Platform; Stream) — https://www.cloudflare.com/service-specific-terms-developer-platform/
- Backblaze B2 pricing — https://www.backblaze.com/cloud-storage/pricing
- Storj pricing — https://storj.io/pricing
- Pinata pricing — https://pinata.cloud/pricing
- Oracle Cloud Free Tier + Always Free docs — https://www.oracle.com/cloud/free/ ; https://docs.oracle.com/en-us/iaas/Content/FreeTier/freetier_topic-Always_Free_Resources.htm
- Dropbox Terms of Service + plans — https://www.dropbox.com/terms ; https://www.dropbox.com/plans
- MEGA Terms of Service + pricing + transfer quota — https://mega.io/terms ; https://mega.io/pricing ; https://help.mega.io/plans-storage/space-storage/transfer-quota
- GitHub Releases doc + AUP + ToS — https://docs.github.com/en/repositories/releasing-projects-on-github/about-releases ; https://docs.github.com/en/site-policy/acceptable-use-policies/github-acceptable-use-policies ; https://docs.github.com/en/site-policy/github-terms/github-terms-of-service

Empirical probes (2026-09-07, both return `206 Partial Content` with `accept-ranges: bytes`):

- IA: `https://archive.org/download/203325_Marathon_Trims_R1/203325_Marathon_Trims_R1_master.intros.mp4` with `Range: bytes=0-1023`
- Google Drive: `https://drive.usercontent.google.com/download?id=<public-file-id>&export=download&confirm=t` with `Range: bytes=0-1023`

User-reported (flagged as such in text): Drive per-file "download quota exceeded" on over-shared public files — https://support.google.com/drive/thread/2035857
