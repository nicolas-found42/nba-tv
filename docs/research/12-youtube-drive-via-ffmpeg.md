# 12 — YouTube → ffmpeg → Google Drive via CLI pipeline

Date: 2026-09-08. Scope: how one *could* move specific YouTube-hosted NBA
game footage to a 5 TB Google Drive using `yt-dlp` as URL resolver,
`ffmpeg` for download/remux, and `rclone` (or the Drive API) for upload.
Method: primary sources only — project READMEs/docs, first-party API docs,
the platform Terms themselves, and the statute. Every factual claim below
carries its source inline. Nothing was downloaded in the course of this
research; §4 (legality) is REQUIRED reading before §5 and constrains what
may lawfully flow through this pipeline.

**Verdict in one paragraph.** Technically the pipeline is four solved
problems: `yt-dlp` resolves a watch URL to direct media/manifest URLs
(`-g`/`--print`, `-f`) and already handles YouTube's signature-throttling
countermeasures; `ffmpeg` natively ingests the resulting HLS/DASH manifests
and remuxes with `-c copy` (no re-encode); `rclone copy/move` to a `drive:`
remote with a self-made OAuth client ID handles multi-TB uploads subject to
an *undocumented, observed* ~750 GiB/day ceiling (≈7 days minimum per
5 TiB); and the Drive API's resumable-upload type is the documented
build-it-yourself alternative. Legally, the pipeline is narrow: YouTube's
ToS forbids downloading except as expressly authorized or with written
permission, NBA footage on third-party channels is unlicensed reproductions
of league-controlled content, and "personal use" alone satisfies neither
the ToS nor the four-factor fair-use test — the only clean inputs are your
own uploads, public-domain material, and Creative-Commons-licensed videos
(identifiable via the Data API's `status.license` field).

**Repo fit note.** Per [ADR 0002](../adr/0002-no-media-in-repo.md), the
Cache Tier holds only downloadable, re-findable Games and *never*
stream-only surfaces (official NBA free tier, YouTube), which play in place
as External Surfaces; `*.mp4`/`*.mkv` are git-ignored and no media, keys,
or bulk data enter the repo. Adopting §5's pipeline for YouTube footage
would therefore be a policy change to ADR 0002, not just tooling — recorded
here as an open decision, not a recommendation.

---

## 1. Watch URL → direct media URLs (yt-dlp resolves, ffmpeg does not)

### 1.1 yt-dlp as the resolver

`yt-dlp` is "a feature-rich command-line audio/video downloader" forked
from `youtube-dl` (via the inactive `youtube-dlc`), supporting thousands of
sites, and licensed under the Unlicense (source tree itself; bundled
release binaries pull in differently-licensed code) (yt-dlp README,
https://github.com/yt-dlp/yt-dlp#readme).

- **Install.** Via release binaries, `pip`, or a third-party package
  manager; the README defers to the Installation wiki for detail, ships
  `yt-dlp` (zipimport, Linux/BSD), `yt-dlp.exe` (Windows), and
  `yt-dlp_macos` (macOS 10.15+) as the recommended files, and notes three
  update channels (`stable` monthly, `nightly` recommended for regular
  users, `master` canary) selectable with `--update-to` (yt-dlp README,
  https://github.com/yt-dlp/yt-dlp#readme).
- **Print the direct URL instead of downloading.** `-g, --get-url` is
  documented as exactly equivalent to `--print urls` ("Simulate, quiet but
  print URL"), listed in the README's redundant-options table alongside
  `-e/--get-title` (= `--print title`), `--get-description`,
  `--get-duration`, `--get-id`, `--get-thumbnail`, `--get-filename`
  (yt-dlp README, https://github.com/yt-dlp/yt-dlp#readme).
- **General printer.** `-O, --print [WHEN:]TEMPLATE` prints a field name or
  output template, optionally prefixed with *when* to print it; the README
  advises `--print after_move:filepath` to get the filename after
  post-processing/merging renames it (yt-dlp README,
  https://github.com/yt-dlp/yt-dlp#readme).
- **Format selection.** `-f, --format FORMAT` takes a selector expression
  ("see FORMAT SELECTION for more details"); `-F, --list-formats` is
  equivalent to `--print formats_table`, and `-S/--format-sort` orders
  candidates (yt-dlp README,
  https://github.com/yt-dlp/yt-dlp#readme). With no `-f`, yt-dlp takes the
  best available quality — "generally equivalent to `-f
  bestvideo*+bestaudio/best`", falling back to `-f best/bestvideo+bestaudio`
  when ffmpeg is unavailable or streaming to stdout, or to
  `-f bestvideo+bestaudio/best` with `--audio-multistreams` (yt-dlp README,
  https://github.com/yt-dlp/yt-dlp#readme).
- **YouTube throttling notes.** The README documents a "fix for `n-sig`
  based throttling" for the YouTube extractor and `--live-from-start` for
  livestreams (yt-dlp README,
  https://github.com/yt-dlp/yt-dlp#readme); operationally, if throughput
  collapses below `--throttled-rate`, yt-dlp assumes throttling and
  re-extracts the video data, and `--hls-prefer-native` / `--hls-prefer-ffmpeg`
  (aliases for `--downloader "m3u8:native"` / `"m3u8:ffmpeg"`) choose which
  engine fetches HLS, with `--downloader [PROTO:]NAME` supporting native,
  aria2c, axel, curl, ffmpeg, httpie, and wget per protocol (yt-dlp README,
  https://github.com/yt-dlp/yt-dlp#readme).

### 1.2 What ffmpeg can and cannot take directly

`ffmpeg` "reads from an arbitrary number of inputs (which can be regular
files, pipes, network streams, grabbing devices, etc.), specified by the
`-i` option" — i.e. each `-i` URL must already be media bytes or a media
manifest (ffmpeg docs, https://ffmpeg.org/ffmpeg.html). Its demuxer catalog
ships a DASH demuxer ("Dynamic Adaptive Streaming over HTTP demuxer") and
an HLS demuxer ("Apple HTTP Live Streaming demuxer … presents all AVStreams
from all variant streams") — both consume playlist/manifest URLs, not web
pages (ffmpeg formats docs,
https://ffmpeg.org/ffmpeg-formats.html#dash-1 and
https://ffmpeg.org/ffmpeg-formats.html#hls-1).

A YouTube *watch* URL (`youtube.com/watch?v=…`) serves an HTML watch page
whose playback URLs are assembled by the player (signature deciphering,
`n`-parameter throttling — the exact countermeasures yt-dlp's extractor
carries fixes for, §1.1). ffmpeg's demuxer list contains container and
streaming-protocol demuxers and no site-specific extractor, so there is no
documented way to hand ffmpeg a watch URL; the resolver step belongs to
yt-dlp, and ffmpeg enters at the manifest/media-URL stage (ffmpeg formats
docs demuxer catalog, https://ffmpeg.org/ffmpeg-formats.html; yt-dlp
README, https://github.com/yt-dlp/yt-dlp#readme). *[INFERENCE from the
primary docs' complete demuxer list plus yt-dlp's documented extractor
role — no primary doc states this negative in one sentence.]*

---

## 2. Download mechanics

Three documented wirings, cheapest first:

1. **yt-dlp direct download** (default): `yt-dlp <watch-url>` takes best
   video+audio and merges (default `-f bestvideo*+bestaudio/best`, §1.1).
2. **Resolver → ffmpeg split**: `yt-dlp -g -f <selector> <watch-url>`
   prints direct media URLs (`--print urls`), which become ffmpeg `-i`
   inputs for a remux/fetch you control.
3. **ffmpeg fetches the manifest itself**: hand ffmpeg an HLS (`.m3u8`) or
   DASH (`.mpd`) URL from step 2's resolution and let its native demuxers
   (§1.2) pull segments.

### 2.1 Archival format: keep original vs remux, `-c copy` vs re-encode

- **Stream copy.** `-c[:stream] codec` accepts the special value `copy`
  (output only) "to indicate that the stream is not to be re-encoded",
  e.g. `ffmpeg -i INPUT -map 0 -c:v libx264 -c:a copy OUTPUT` (ffmpeg docs,
  https://ffmpeg.org/ffmpeg.html). The §3.1 "Streamcopy" pipeline copies an
  input elementary stream's packets "without decoding, filtering, or
  encoding them", demuxer straight to muxer (ffmpeg docs,
  https://ffmpeg.org/ffmpeg.html#Streamcopy).
- **Why copy wins for archival.** The alternative path runs
  decoder → filtergraph → encoder, and encoders are "typically *lossy* —
  it degrades stream quality to make the output smaller" (lossless exists
  "at the cost of much higher output size") — so re-encoding a game you
  already have in H.264/AAC only burns CPU and sheds quality (ffmpeg docs,
  https://ffmpeg.org/ffmpeg.html). Rule: `-c copy` always; re-encode only
  to change codecs for a player constraint.
- **Container choice.** When video+audio arrive as separate streams,
  yt-dlp merges them; `--merge-output-format FORMAT` picks among avi, flv,
  mkv, mov, mp4, webm ("Ignored if no merge is required"), `--remux-video`
  forces a remux container, and the `-t` preset aliases bundle this
  (`-t mp4` = `--merge-output-format mp4 --remux-video mp4 …`;
  `-t mkv` = `--merge-output-format mkv --remux-video mkv`) (yt-dlp README,
  https://github.com/yt-dlp/yt-dlp#readme). For a personal archive, MKV
  tolerates heterogeneous codecs without a second transcode while MP4
  maximizes player compatibility (notably the repo's faststart-MP4 Cache
  Tier convention, ADR 0002); either way the operation is a remux, not a
  re-encode.

### 2.2 Resume, retry, and rate-limit politeness

- **Resume** is the default: `-c, --continue` "Resume partially downloaded
  files/fragments (default)"; `--no-continue` restarts whole files, and
  `--part` stages into `.part` files instead of writing in place (yt-dlp
  README, https://github.com/yt-dlp/yt-dlp#readme).
- **Retries.** `-R, --retries RETRIES` defaults to 10 (or `"infinite"`);
  `--fragment-retries` likewise defaults to 10 for DASH/HLS-native/ISM
  fragments; `--retry-sleep [TYPE:]EXPR` spaces attempts;
  `--abort-on-unavailable-fragments` (vs skip, the default) and
  `--keep-fragments` control fragment failure policy; `-N,
  --concurrent-fragments N` (default 1) parallelizes fragment fetch
  (yt-dlp README, https://github.com/yt-dlp/yt-dlp#readme).
- **Politeness / self-throttle.** `-r, --limit-rate RATE` caps bytes/sec
  (`50K`, `4.2M`); `--sleep-requests SECONDS` sleeps between extraction
  requests, `--sleep-interval`/`--max-sleep-interval` (aliases include
  `--min-sleep-interval`) pace downloads, `--sleep-subtitles` paces
  subtitle fetches; the `-t sleep` preset bundles `--sleep-subtitles 5
  --sleep-requests 0.75 --sleep-interval 10 --max-sleep-interval 20`
  (yt-dlp README, https://github.com/yt-dlp/yt-dlp#readme). These are the
  levers for staying a quiet client; they do not change the §4 legal
  analysis.

---

## 3. Upload to Google Drive at multi-TB scale

### 3.1 rclone (the usual answer)

`rclone copy /home/source remote:backup` copies a local directory into a
Drive path (`drive:path`, arbitrarily deep); `copy` "does not transfer
files that are identical on source and destination, testing by size and
modification time or MD5SUM" and never deletes from the destination —
`sync` is the variant that also deletes to mirror (rclone Drive docs,
https://rclone.org/drive/; rclone copy docs,
https://rclone.org/commands/rclone_copy/). `rclone move` instead relocates
source contents into the destination (server-side when possible, else
copy-then-delete the original), with `--delete-empty-src-dirs` to clean up
and `--dry-run`/`-i` to rehearse since "this can cause data loss" (rclone
move docs, https://rclone.org/commands/rclone_move/). For this pipeline
`copy` (staging retained until verified on Drive) is safer than `move`;
`move` fits only once checksums on the remote confirm.

- **Auth setup.** `rclone config` walks through an interactive browser
  OAuth flow (a momentary localhost webserver on `http://127.0.0.1:53682/`
  collects the token) with scopes from full `drive` down to
  `drive.readonly`, `drive.file` (only files rclone itself created),
  `drive.appfolder`, and `drive.metadata.readonly` (rclone Drive docs,
  https://rclone.org/drive/).
- **Bring your own client ID — now effectively required.** "The shared
  client_id is being retired and will stop working during 2026, so creating
  your own is now strongly recommended", with a dedicated
  "Making your own client_id" section; new remotes warn if you proceed on
  the shared one (rclone Drive docs, https://rclone.org/drive/). A 5 TB
  personal project must therefore create a Google Cloud OAuth client and
  paste its ID/secret at the `client_id`/`client_secret` prompts.
- **Unattended alternative.** A service-account JSON key at the
  `service_account_file` prompt skips the browser flow — aimed at machines
  without logged-in users; on a Google Workspace domain, domain-wide
  delegation grants the service account's numeric client ID the
  `https://www.googleapis.com/auth/drive` scope so it can act on a user's
  Drive (rclone Drive docs, https://rclone.org/drive/). For a single
  personal 5 TB account, personal OAuth is simpler; service accounts matter
  for headless seedboxes.
- **Throughput tuning.** `--drive-chunk-size` (default 8 Mi, must be a
  power of two ≥ 256 KiB) sizes resumable-upload chunks — "larger will
  improve performance, but … each chunk is buffered in memory one per
  transfer", so raising it multiplies RAM by `--transfers`; files below
  `--drive-upload-cutoff` (default 8 Mi) upload in one shot (rclone Drive
  docs, https://rclone.org/drive/). Globally, `--transfers` (default 4
  parallel file transfers), `--checkers` (default 8), `--buffer-size`
  (16 Mi per transfer), `--retries` (default 3) with `--retries-sleep`,
  `--max-transfer` (stop after N bytes — useful for daily-cap pacing),
  `-P/--progress`, and `--dry-run` shape the run; `--bwlimit` (with
  timetable syntax) and `--bwlimit-file` cap bandwidth overall or per file
  (rclone global flags, https://rclone.org/flags/).
- **The 750 GiB/day ceiling (verified as undocumented).** "At the time of
  writing it is only possible to upload 750 GiB of data to Google Drive a
  day **(this is an undocumented limit)**" — Google publishes no figure;
  rclone detects it from error-message strings ("may break in the future",
  rclone/rclone#3857) and offers `--drive-stop-on-upload-limit` to fail
  fast instead of retry-looping into the cap; the download mirror is
  10 TiB/day with `--drive-stop-on-download-limit` (rclone Drive docs,
  https://rclone.org/drive/). Arithmetic consequence: filling 5 TiB needs
  **≈ 7 days minimum** at the cap (`5 × 1024 / 750`), before retries, so
  plan `--max-transfer 700G`-paced daily runs or a `--drive-stop-on-upload-limit`
  loop with backoff — and keep staging disk until `rclone check` confirms
  the remote.
- **API-rate posture.** "Drive has quite a lot of rate limiting. This
  causes rclone to be limited to transferring about 2 files per second
  only", tunable via the `--drive-pacer-*` options (min sleep default
  100 ms, burst 100) and `--tpslimit` globally (rclone Drive docs,
  https://rclone.org/drive/; rclone global flags, https://rclone.org/flags/).
  Large game files are throughput-bound, not transaction-bound, so this
  bites directory scans more than uploads.
- **Quota.** Drive storage is shared across Drive, Gmail, Photos (and
  WhatsApp backups); at the limit, uploads stop, and Google One raises the
  ceiling per plan (Google Drive Help,
  https://support.google.com/drive/answer/6374270). A 5 TB plan holds on
  the order of ~1,000 two-hour games at ~5 GB each (arithmetic), with trash
  still counting against quota until emptied (Google Drive Help,
  https://support.google.com/drive/answer/6374270).

### 3.2 Drive API resumable upload (the build-it-yourself alternative)

The API offers three upload types on `files.create`: **simple**
(`uploadType=media`, ≤ 5 MB, no metadata), **multipart**
(`uploadType=multipart`, ≤ 5 MB *with* metadata in one request), and
**resumable** (`uploadType=resumable`) — "for large files (greater than
5 MB) and when there's a high chance of network interruption … also a good
choice for most applications since they also work for small files at a
minimal cost of one additional HTTP request per upload"; the client
libraries implement at least one type each (Google Drive API docs,
https://developers.google.com/workspace/drive/api/guides/manage-uploads).
rclone is the usual answer because its Drive backend already implements
exactly this (chunked resumable upload + OAuth refresh + retries +
checksum/dir recursion) behind one command *[assessment — the mapping of
rclone behavior to the API's resumable type is mine; the API facts are
Google's]*.

---

## 4. Legality / Terms-of-Service boundaries (REQUIRED — read first)

This section states the rules as written by the platforms, the league, and
the statute. It is research, not legal advice (the Copyright Office itself
says of its index: "it is not a substitute for legal advice … you should
seek legal assistance as necessary", U.S. Copyright Office Fair Use Index,
https://www.copyright.gov/fair-use/).

### 4.1 YouTube ToS: downloading is forbidden by default

Under "Permissions and Restrictions" (ToS dated December 15, 2023): "You
may view or listen to Content for your personal, non-commercial use" — and
"You are not allowed to", among ten bans: (1) "access, reproduce,
download, distribute, transmit, broadcast, display, sell, license, alter,
modify or otherwise use any part of the Service or any Content **except:
(a) as expressly authorized by the Service; or (b) with prior written
permission from YouTube and, if applicable, the respective rights
holders**"; (2) circumvent anti-copying/security features; (3) automated
access "except (a) … public search engines, in accordance with YouTube's
robots.txt … or (b) with YouTube's prior written permission" (YouTube Terms
of Service, https://www.youtube.com/t/terms). Breach exposes the account:
"YouTube reserves the right to suspend or terminate your Google account or
your access … if (a) you materially or repeatedly breach this Agreement"
(YouTube Terms of Service, https://www.youtube.com/t/terms). The "expressly
authorized" lane in practice means YouTube Premium offline / the download
button where offered — third-party fetchers are not it *[the ToS text is
quoted; the characterization of the authorized lane is my reading]*.

### 4.2 NBA footage is league-controlled content, and reuploads are unlicensed copies

NBA.com's Terms of Use §1 ("Ownership and Use Restrictions"): "The
basketball-related content and materials contained within the Services
(including, but not limited to, **video, audio, photos, text, images** …)
('Basketball Content') are owned, licensed, controlled, and/or entitled to
use by the Operator. **No Basketball Content … may be reproduced,
republished, uploaded, posted, modified, reused, transmitted, reproduced,
distributed, copied, publicly displayed, linked to, or otherwise used …
without the written permission of the Operator**", where Operator = NBA
Media Ventures, NBA TV, and NBA Properties collectively; names, logos,
uniform trade dress, "game action photographs, video footage" are
"exclusive intellectual property" (NBA Terms of Use,
https://www.nba.com/termsofuse). The narrow carve-out — "Where the
function is available, you may download material displayed on the Services
to any single computer only for your personal, noncommercial use" — covers
material *on NBA Services where NBA offers the function*, not third-party
YouTube reuploads (NBA Terms of Use, https://www.nba.com/termsofuse). The
league operates a DMCA pipeline to back this up: §16 names a designated
copyright agent (DMCA Agent, NBA Media Ventures, 645 Fifth Ave, NY;
dmca@nba.com) and the ToS-side mirror at YouTube promises copyright
notices handling plus "termination, in appropriate circumstances, of repeat
infringers' access" (NBA Terms of Use, https://www.nba.com/termsofuse;
YouTube Terms of Service, https://www.youtube.com/t/terms). Uploaders get
no shelter either: "the Content you submit must not include third-party
intellectual property (such as copyrighted material) unless you have
permission from that party or are otherwise legally entitled to do so",
checked by "automated systems that analyze your Content to help detect
infringement" (YouTube Terms of Service,
https://www.youtube.com/t/terms).

### 4.3 The only clean inputs — and how to identify them by machine

- **Your own uploads.** Uploaders "retain ownership rights in your Content"
  while granting YouTube and (Service-feature-scoped) other users licenses
  (YouTube Terms of Service, https://www.youtube.com/t/terms) — downloading
  your own footage (e.g. via Google Takeout, which the ToS itself offers
  for export) is the uncontroversial case.
- **Public domain.** No rights to clear; verify status per work (age,
  federal authorship, dedication) rather than assuming — pre-1960s NBA
  telecasts are *not* automatically public domain *[general copyright
  principle flagged for counsel, not sourced here]*.
- **Creative Commons / YouTube-licensed.** `videos.list` exposes
  `status.license` with valid values "`creativeCommon` `youtube`", plus
  `contentDetails.licensedContent` ("uploaded to a channel linked to a
  YouTube content partner and then claimed by that partner") and
  `regionRestriction` allow/block lists (YouTube Data API videos resource,
  https://developers.google.com/youtube/v3/docs/videos). `status.license =
  creativeCommon` is therefore the machine-readable signal for
  uploader-elected CC licensing — but read the license deed itself for what
  reuse (commercial? derivatives? attribution?) it permits; and
  `licensedContent = true` cuts the *other* way (claimed partner content,
  stay away) *[API facts Google's; the reuse guidance is mine]*.

### 4.4 "Personal use" of an infringing reupload is still infringement

17 U.S.C. §501(a): "**Anyone who violates any of the exclusive rights of
the copyright owner** as provided by sections 106 through 122 … **is an
infringer**" (U.S. Copyright Act,
https://www.copyright.gov/title17/92chap5.html#501). The escape hatch is
fair use, 17 U.S.C. §107 — a **four-factor, case-by-case** test with "no
formula" and no percentage safe harbor: (1) purpose/character (nonprofit
educational and *transformative* uses favored — but "this does not mean …
that all nonprofit education and noncommercial uses are fair"); (2) nature
of the work (creative works like broadcasts get thicker protection than
factual ones); (3) amount used (a whole game is the entire work, and even
small takes can fail as "the heart of the work"); (4) market effect
(displacing sales; harm "if it were to become widespread") (U.S. Copyright
Office Fair Use Index, https://www.copyright.gov/fair-use/). A full-game
archival copy scores badly on factors 2–4 and factor 1's commerciality
prong is only one input — so "it's just for me" does not make an
unlicensed download lawful *[application of the cited factors to this
fact pattern is my analysis]*. Separately, ToS breach is a contract
matter independent of copyright: an account can be terminated for ToS
violation even where no infringement suit would follow (§4.1).

---

## 5. Recommended pipeline sketch (commands from the docs above)

Assumes a §4.3-clean input (own upload, public-domain, or CC-licensed
video — check `status.license` first). Staging lives **outside the repo**
(e.g. `~/staging/nba-tv/`), never under `data/` or the checkout, per ADR
0002 and the git-ignored `*.mp4`/`*.mkv` rule.

```bash
# 0. Inspect what YouTube offers (formats table; §1.1).
yt-dlp -F '<watch-url>'

# 1a. Direct download + merge to an archival container (§1.1, §2.1).
yt-dlp -f 'bv*+ba/b' --merge-output-format mkv \
  -o '~/staging/nba-tv/%(title)s [%(id)s].%(ext)s' \
  --sleep-requests 0.75 --sleep-interval 10 --max-sleep-interval 20 \
  -R 10 --continue \
  '<watch-url>'

# 1b. …or split resolver → ffmpeg when you want the remux yourself (§1.2, §2).
yt-dlp -g -f 'bv*+ba/b' '<watch-url>'   # prints direct media URLs (--print urls)
ffmpeg -i '<media-or-manifest-url>' -c copy '~/staging/nba-tv/game.mkv'

# 2. One-shot rclone setup (§3.1): personal OAuth + SELF-MADE client ID
#    (shared client_id retires during 2026), then paced upload.
rclone config                 # choose drive, paste own client_id/secret, scope: drive
rclone copy ~/staging/nba-tv/ gdrive:nba-tv/ \
  --transfers 4 --drive-chunk-size 64M --bwlimit 0 \
  --drive-stop-on-upload-limit --retries 3 -P
rclone check ~/staging/nba-tv/ gdrive:nba-tv/   # verify before clearing staging
```

Notes: raise `--drive-chunk-size` from 8 Mi toward 32–64 Mi only while RAM
allows (one chunk buffered per transfer, §3.1); expect the 750 GiB/day
ceiling to gate a 5 TiB backfill across ~7+ days — `--max-transfer 700G`
per daily invocation is the polite, resumable shape since `copy` skips
identical files on re-run; prefer `copy` + `check` over `move` until the
remote is verified. All flags above are documented at
https://github.com/yt-dlp/yt-dlp#readme, https://ffmpeg.org/ffmpeg.html,
https://rclone.org/drive/, and https://rclone.org/flags/.

## Open questions (not resolved here)

1. Whether the project *wants* this pipeline at all for grey YouTube
   footage, given ADR 0002's External-Surface stance (§4 makes most of that
   corpus unlawful to pull — likely moot).
2. Exact per-game size/quality target (1080p H.264 ≈ 4–6 GB per 2 h game
   is folk wisdom, unsourced — measure one §4.3-clean video before
   capacity planning).
3. Whether a Workspace account (higher caps, service-account uploads)
   beats personal OAuth for the backfill phase.
