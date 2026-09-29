# 10 — In-window playback on an ffmpeg foundation (nicolas-found42/nba-tv#10)

Resolves the #10 question: how every Tape Source class from the Source Ladder plays inside one
Shell window, on one Player Backend, without violating the map's acquisition-method legality
gate. Design + verdicts only — no code, no media downloaded. Feeds the open Shell ticket **#5,
which stays open**; nothing here closes or decides it. Inputs: #4's source classes S0–S5, #2's
streaming decision (progressive MP4 over Range, no HLS; YouTube + NBA App as External Surfaces),
and CONTEXT.md terms (Shell, Player Backend, Tape Source, Cache Tier, External Surface).

**Verdict in one paragraph.** Every Tape Source class can play inside the Shell window, but not
through one decoder: the honest architecture is **two lanes**. Lane A (ffmpeg-native) covers the
Cache Tier, Internet Archive and any progressive file URL — decoded frames become egui textures,
full pixel-level uniformity, working reference code exists (egui-video). Lane B (vendor-player
embed) covers YouTube-class sources: a platform webview hosts the vendor's own sanctioned player
inside the window; lawful and in-window, but the pixels live in the webview, not an egui texture.
The **NBA App has no lane**: its streams are Widevine-encrypted (first-party config observed
2026-09-07), decrypting them for ffmpeg is DMCA §1201 circumvention, and its Terms bar deep
linking — however the free NBA ID catalog mirrors onto the official NBA Classic Games YouTube
playlist, so its content still reaches Lane B. Stream-extraction (yt-dlp-style URLs fed to
ffmpeg) is **gate-failing as an app feature**: YouTube's ToS and API Developer Policies prohibit
it outright. The Cache Tier normalizes every Lane A ingest to **MP4 / H.264 / AAC, moov-first
(`+faststart`)** so tapes stream progressively over Range with no HLS, exactly per #2.

---

## 1. Player Backend: the two-lane pipeline

```
Tape Source (S0–S3) ── resolve at play time (per #4 ladder / #2 registry)
        │
        ├─ progressive file URL / Cache Tier file ──► LANE A ──► ffmpeg ─► RGBA frames ─► egui texture
        │                                              │                    └► PCM samples ─► audio out
        │                                              └─ cache miss → normalize to MP4/H.264/AAC/+faststart
        │
        └─ embeddable platform player (YouTube et al.) ► LANE B ──► wry child webview ─► vendor player
                                                       (egui draws chrome + controls around it)
```

### 1.1 Lane A — ffmpeg-native (the default lane)

**Decode-to-texture loop.** The proven shape in the wild is
[egui-video](https://github.com/n00kii/egui-video) (v0.9.0, MIT): per stream, pull packets,
`video_decoder.receive_frame()`, convert through the software scaler to RGB
(`Context::get(format, w, h, ...)` + `scaler.run(&frame, &mut rgb_frame)`), wrap as
`ColorImage`, and paint via a persistent `TextureHandle`
([src/lib.rs](https://github.com/n00kii/egui-video/blob/main/src/lib.rs), lines ~1295–1448,
`texture_handle.set(frame, texture_options)` at line 400). egui's texture contract matches this
directly: `egui::Context::load_texture` to create and `TextureHandle::set` / `set_partial` to
update ([egui TextureHandle docs](https://docs.rs/egui/latest/egui/struct.TextureHandle.html),
epaint-backed).

**Binding choice — two viable crates:**

| | [`ffmpeg-sidecar`](https://crates.io/crates/ffmpeg-sidecar) v2.5.2 | [`ffmpeg-the-third`](https://crates.io/crates/ffmpeg-the-third) v6.0.0 |
|---|---|---|
| Mechanism | spawns a standalone ffmpeg CLI, exposes decoded output as an **iterator of raw RGB frames** ("interact with any video *as if it were an array of raw RGB frames*" — [README](https://github.com/nathanbabcock/ffmpeg-sidecar)) | libav* bindings; **FFmpeg 5.1–9.0 supported** ([README](https://github.com/shssoichiro/ffmpeg-the-third)); what egui-video actually uses |
| Workspace cost | pure-Rust code, **no C linkage**; ships/auto-downloads an ffmpeg binary (auto_download, per README) | C linkage + build complexity (FFmpeg headers/libs per platform); unsafe FFI boundary |
| Frame access | stdout pipe → frame iterator (no PTS on the pipe; sync by wall clock) | in-process decode with full PTS/packet control |
| Verdict | **start here** — the same binary powers the Cache Tier normalizer (1.3), one ffmpeg install serves both; matches the driver's pure-Rust lean for workspace code | adopt only if sidecar pipe overhead or timestamp control becomes a problem; egui-video is the reference for that port |

Both need a native ffmpeg somewhere — "pure Rust" holds for our code, not for the media stack;
that is already implied by the ticket's own premise ("on an ffmpeg foundation").

**Audio + sync.** egui-video decodes audio to resampled PCM and plays it through an SDL2
callback, syncing video against elapsed wall time (lib.rs: `AudioDevice`, `resampler`,
`video_elapsed_ms`). For our shell, cpal (v0.18.2) or rodio (v0.22.2) replace SDL2 to avoid the
extra native dep; the sync model (decode-ahead ring buffer + wall clock, video texture updated on
egui repaint) carries over. Lane A is "good enough" sync, not mpv-grade: acceptable for archive
tape viewing, and one reason Lane A defaults to the sidecar design where a second
`-f s16le`/`-f f32le` pipe is trivial to add.

**Seeking.** Sidecar: restart ffmpeg with `-ss <t>` before `-i` (fast input seek); egui-video's
equivalent is a streamer seek that resets decoder state (lib.rs `seek()`, lines 335–371). Either
way seek is O(1) for our normal MP4s (moov-first index, see 1.3).

**Hardware acceleration.** Decode: `ffmpeg -hwaccel videotoolbox` on macOS — Homebrew's ffmpeg
formula builds with `--enable-videotoolbox` on macOS
([Formula/f/ffmpeg.rb](https://github.com/Homebrew/homebrew-core/blob/HEAD/Formula/f/ffmpeg.rb),
line 89) — verified locally: this machine's `ffmpeg` is 9.0.1 (Homebrew) with
`h264_videotoolbox` / `hevc_videotoolbox` encoders present. Encode side uses them for
normalization (1.3). Caveat, stated honestly: in the sidecar design the decoded frame crosses a
pipe as RGBA regardless of how it was decoded, so hw accel cuts *decode* cost (matters for
HEVC/4K-era tape) but not the per-frame CPU→GPU texture upload. At 1080p/24 (≈8.3 MB RGBA per
frame, ≈200 MB/s upload) this is unremarkable on Apple Silicon; 4K/60 (≈2 GB/s) is where the
libmpv render-API fallback (below) earns its place.

**mpv fallback (optional, not the plan of record).** libmpv embeds its own libav* and offers a
render API that draws video straight into an OpenGL FBO, with hardware decoding
([mpv `render_gl.h`](https://github.com/mpv-player/mpv/blob/master/include/mpv/render_gl.h):
"Use mpv_render_context_create() … Call mpv_render_context_render() with MPV_RENDER_PARAM_OPENGL_FBO
to render the video frame to an FBO"). The Rust crate [`libmpv2`](https://crates.io/crates/libmpv2)
v6.0.0 (libmpv ≥ 2.0 / mpv 0.35+) ships a working OpenGL example
([examples/opengl.rs](https://github.com/kohsine/libmpv-rs/blob/master/examples/opengl.rs):
`create_render_context(vec![RenderParam::ApiType(OpenGl), …])`). Cost: a C library dependency
that duplicates ffmpeg — worth prototyping only if Lane A measurably can't hold 4K-era tape.

### 1.2 Lane B — vendor-player embed (External Surfaces, pulled in-window)

Mechanism: [`wry`](https://crates.io/crates/wry) v0.56.1 can create a webview **as a child inside
another window with explicit bounds** — `WebViewBuilder::build_as_child` + `with_bounds(Rect)`
([README](https://github.com/tauri-apps/wry/blob/dev/README.md), supported on macOS, Windows,
X11; per-platform engines: WKWebView / WebView2 / WebKitGTK). The egui Shell sizes an egui rect
for the tape view; the webview is bound to that rect and loads the vendor's player page.

- **YouTube**: the IFrame Player API is the sanctioned embed surface — "Embed a YouTube player in
  your application" ([IFrame Player API
  reference](https://developers.google.com/youtube/iframe_api_reference)); the embed URL carries
  `enablejsapi=1`, and the API exposes programmatic control (`loadVideoById(videoId,
  startSeconds)`, `setPlaybackRate`, state events) — enough for egui-side transport controls
  (play/pause/seek/rate) to drive the embedded player uniformly. The window shows one coherent
  UI: egui chrome + controls, vendor video in the bounded child webview.
- **S3 platforms**: Dailymotion documents iframe embed players via its developer tools
  ([developer.dailymotion.com/tools](https://developer.dailymotion.com/tools)); Vimeo ships a
  Player SDK over iframe embeds ([developer.vimeo.com/player/sdk/basics](https://developer.vimeo.com/player/sdk/basics)).
  OK.ru and Bilibili have embed players but were not verified against primary docs in this
  ticket — treat them as best-effort Lane B, same as #4's S3 priority.

**Cost to the Rust plan (stated plainly).** Lane B means: a per-platform webview dependency
(system engines — WKWebView needs macOS entitlements care, WebKitGTK needs Linux packages); no
pixel access to the video (egui cannot composite, filter, or screenshot the tape; subtitles and
frame-exact UI must come from the vendor's player, not ours); an extra z-order/focus surface
inside the window (egui paints around it, never over it); and playback controls only as rich as
the vendor's player API allows. What it buys: the only lawful way to play YouTube-class sources
at all (see §3.1), zero transcoding or decode cost, and vendor-side adaptive streaming.

**Uniformity verdict:** "one uniform view" holds at the UX level (one window, one control scheme
across lanes); it cannot hold at the texture level for Lane B. That is the price of the legality
gate, and it is the right trade.

### 1.3 Cache Tier normalization (transcode-on-cache-miss)

Every ingest into the Cache Tier is normalized on first fetch (cache miss) to exactly one
format, so playback code has a single happy path and #2's "progressive MP4 over Range, no HLS"
holds:

- **Container**: MP4 (mov muxer). **Video**: H.264. **Audio**: AAC.
- **moov-first**: `-movflags +faststart` — ffmpeg docs: "Run a second pass moving the index (moov
  atom) to the beginning of the file"
  ([ffmpeg-formats](https://ffmpeg.org/ffmpeg-formats.html), mov/mp4 muxer options). Without it,
  a server can't serve a playable prefix and Range-based progressive streaming degrades to
  download-then-play.
- Normalizer command (normalization, not playback — runs once per tape):

  ```
  ffmpeg -i <src> -c:v libx264 -crf 20 -preset medium -c:a aac -b:a 160k -movflags +faststart out.mp4
  ```

  `h264_videotoolbox` may replace libx264 for throughput on macOS (verified present locally);
  libx264 remains the compatibility default since every Lane A build decodes it. ffmpeg's http
  client itself issues ranged requests for seekable transfers (request_size: "split a seekable
  transfer into ranged requests" — [ffmpeg-protocols](https://ffmpeg.org/ffmpeg-protocols.html)),
  so the same file streams progressively whether played by ffmpeg or served to a browser.

### 1.4 ffmpeg capability matrix (what the shipped build must enable)

| Tape Source class | Protocols | Demuxers | Codecs (decode) | Notes |
|---|---|---|---|---|
| Cache Tier (MP4) | `file`, `https` (TLS) | mov/mp4 | h264, aac | the normalized happy path |
| Internet Archive (S1) | `https` | mov/mp4, matroska, avi, mpegts | h264, mpeg2/mpeg4, vp9 + aac/mp3/vorbis | IA items expose direct MP4 files (e.g. the 1996 Finals G3 item carries 638×360 MPEG4 originals — [metadata API](https://archive.org/metadata/1996-nba-finals-game-3)) |
| HLS/DASH fan mirrors (rare) | `https` | hls, dash | + hevc | ffmpeg ships both demuxers ([ffmpeg-formats](https://ffmpeg.org/ffmpeg-formats.html) §3.6 dash, §3.12 hls); only needed if a non-DRM segmented source ever enters the ladder — #2 says the *player* never consumes HLS directly; normalization converts it |
| Lane B (YouTube/S3) | none (webview) | none (webview) | none (webview) | vendor player decodes; ffmpeg idle |

TLS is the only build-sensitive protocol ([ffmpeg-protocols §3.43
tls](https://ffmpeg.org/ffmpeg-protocols.html)); every mainstream ffmpeg build (Homebrew,
sidecar-bundled) ships it. The sidecar's auto-download targets all three desktop OSes, per its
README.

## 2. Per-source verdicts

| Source (#4 class) | Lane | In-window? | Legality (acquisition-method gate) | Basis |
|---|---|---|---|---|
| S0 NBA App free tier | none | no — stays External Surface | playing = lawful; **decoding = gate-failing** | §3.2 below (Widevine observed; §1201) |
| S0 mirror — NBA Classic Games YouTube playlist | B | **yes** | **lawful** (sanctioned embed) | IFrame API = "Embed a YouTube player in your application"; ToS allows showing videos "through the embeddable YouTube player" |
| S1 Internet Archive | A | **yes, full texture uniformity** | **lawful** (public file; per map gate) | direct MP4s over https; ffmpeg file/https + mov |
| S2 YouTube fan archives | B | **yes** (embed) | **lawful** as embed; extraction path gate-failing | §3.1 |
| S3 Dailymotion/Vimeo/OK.ru/Bilibili | B | **yes** (embed, verified Dailymotion+Vimeo docs) | **lawful** as embed | platform embed docs |
| S4 purchase-only / S5 institutions | none | no (nothing to play) | n/a — pointer records only | #4: not $0-streamable |

## 3. The hard two

### 3.1 YouTube — verdict: lawful in-window playback exists (Lane B); ffmpeg decode of YouTube does not

Facts (all primary, accessed 2026-09-07):

1. **The Data API has no playback streams.** The YouTube Data API returns metadata only; the
   documented playback surface for apps is the embed player (IFrame Player API reference,
   above). #4's playlistItems-based catalog enumeration is unaffected — that use stays legal and
   cheap.
2. **Extraction into ffmpeg is prohibited by the ToS stack.**
   - YouTube Terms of Service: "You may view or listen to Content for your personal,
     non-commercial use. You may also show YouTube videos through the embeddable YouTube
     player. … You are not allowed to: access, reproduce, download, distribute, … or otherwise
     use any part of the Service or any Content except (a) as expressly authorized by the
     Service; or (b) with prior written permission"
     ([YouTube ToS](https://www.youtube.com/t/terms)).
   - Using the Data API binds the app to the Developer Policies ("required to comply with" —
     [API Services ToS](https://developers.google.com/youtube/terms/api-services-terms-of-service),
     §Definitions/Agreement), which prohibit API Clients from any "download, import, backup,
     cache, or store copies of YouTube audiovisual content without YouTube's prior written
     approval" ([Developer Policies §III.E.1](https://developers.google.com/youtube/terms/developer-policies))
     and from creating "a substitute for, or substantially similar service to" YouTube's own
     applications (§I).
   - Extraction tools are actively enforced against: the RIAA's DMCA notice to GitHub demanded
     removal of the youtube-dl source as "Copyright Violations"
     ([RIAA notice, 2020-10-23](https://github.com/github/dmca/blob/master/2020/10/2020-10-23-RIAA.md)).
   - **Mark: gate-failing as an app feature.** A personal, manual yt-dlp run by the driver is
     grey (ToS breach, arguably not §1201 circumvention for non-DRM streams) — reported grey,
     never endorsed by the app.
3. **libmpv-for-YouTube is the same path with extra steps**: mpv plays YouTube only via its
   external extractor hook (ytdl_hook invoking yt-dlp-class tooling) — it inherits the same
   prohibition. Mark: gate-failing in-app.
4. **Headless-browser frames (CDP `Page.startScreencast`) into egui textures**: technically real
   — CDP streams each rendered frame as an event
   ([CDP Page domain](https://chromedevtools.github.io/devtools-protocol/tot/Page/#method-startScreencast),
   marked *Experimental*). But the video arrives while audio stays inside the browser engine
   (no supported audio capture path), forcing desync or a second capture hack; it needs a full
   browser per playback; and peeling the player's pixels out through a screencast is at best
   grey (circumventing the sanctioned player interface). Mark: **grey + engineering-heavy —
   rejected.**
5. **The lawful in-window answer is the embed lane** (1.2): YouTube's ToS explicitly permits
   showing content "through the embeddable YouTube player", and the IFrame API gives uniform
   transport control. **Cost to the Rust plan:** one platform webview dependency; the YouTube
   lane never becomes an egui texture; subtitles/quality control stay vendor-side; nothing about
   Lane A changes for every other source.

### 3.2 NBA App — verdict: no lawful ffmpeg path; irreconcilable with in-window decode; content coverage comes through its YouTube mirror

1. **The streams are DRM-encrypted.** The watch.nba.com web app ships its own runtime config
   with `"videoPlayerConfig":{"enableWidevineEncryption":true, …}` (observed in the page's
   embedded config JSON, [watch.nba.com](https://watch.nba.com/), 2026-09-07). Widevine DRM
   requires a license agreement to use at all ("A license agreement is required for the use of
   Widevine products or services" — [Widevine overview](https://developers.google.com/widevine/drm/overview))
   and license fulfillment happens on vendor-operated proxies/CDMs; there is no open
   implementation a Rust app could legally or practically use.
2. **Therefore native decode is circumvention.** 17 U.S.C. §1201(a)(1)(A): "No person shall
   circumvent a technological measure that effectively controls access to a work protected under
   this title", where circumvention is "to decrypt an encrypted work … without the authority of
   the copyright owner" (§1201(a)(3)(A)–(B)) ([copyright.gov, Title 17 ch. 12](https://www.copyright.gov/title17/92chap12.html)).
   ffmpeg cannot decode Widevine CENC without a CDM; obtaining one outside an approved
   integration fails both the law and the engineering test. **Mark: gate-failing. There is no
   grey path here.**
3. **The ToS download allowance doesn't help.** NBA.com's Terms do permit personal downloads
   "Where the function is available" (Terms of Use §1, [nba.com/termsofuse](https://www.nba.com/termsofuse)),
   but a DRM-encrypted stream cannot be lawfully decrypted and remuxed regardless — the
   allowance never reaches ffmpeg.
4. **Embedding the vendor's web player in-window is the only conceivable path, and it's grey.**
   A wry child webview loading watch.nba.com keeps the vendor's EME/CDM in charge (no
   circumvention), but the Terms' Linkage Restrictions permit Permissible Sites to link only to
   "the Services' home page or to the homepage of a particular team — links to internal pages
   … are not permitted" (§6.E), and allow framing only when the frame "contains any sponsorship,
   advertising, or other commercial text or graphics" is absent (§6.D). A game watch page is an
   internal page; a personal, non-commercial shell arguably qualifies as Permissible, but the
   design leans on that reading. It also requires the login + DRM flow to work inside the
   platform webview (EME/FairPlay availability differs per engine — an unverified build-time
   risk to test before relying on it). **Mark: grey; fragile; not the plan.**
5. **The practical resolution is the mirror**: the official NBA Classic Games YouTube playlist
   self-describes as "More than 500 Classic Games now available in the Watch tab of the NBA App"
   (documented in #4, §S0) — the free NBA ID catalog is mirrored where Lane B already plays
   lawfully. NBA App playback stays what #2 decided: an **External Surface**, opened in the
   vendor app/browser when the YouTube mirror doesn't cover a game. #4's ladder uses the
   playlist for catalog enumeration; #10 adds the playback verdict: **the NBA App itself is
   irreconcilable with in-window ffmpeg playback, and that is a settled-by-evidence dead end,
   not an open problem.**

## 4. Legality summary (map gate: acquisition method)

| Path | Mark | Reason (source) |
|---|---|---|
| Lane A: play public direct-file tape (IA, Cache Tier) through ffmpeg | **lawful** | acquisition = public file; playback method neutral per map gate |
| Lane B: YouTube / S3 embed players in a webview | **lawful** | sanctioned embed surface (YouTube ToS; platform embed docs) |
| Lane B variant: watch.nba.com web player in-window | **grey** | no circumvention, but internal-page deep link + conditional framing (NBA ToS §6) |
| Headless CDP screencast of an embed player | **grey** | experimental video-only capture; audio workaround pushes past the sanctioned interface (CDP docs) |
| Personal manual yt-dlp use by the driver | **grey** | ToS breach, not endorsed by the app (YouTube ToS) |
| yt-dlp-style extraction built into the app | **gate-failing** | ToS + API Developer Policies III.E.1 + substitute-service prohibition; RIAA takedown precedent |
| NBA App stream decryption / CDM bypass | **gate-failing** | §1201(a)(1)(A); no open CDM; Widevine license-only (copyright.gov; Widevine docs) |

## 5. Handoff to #5 (not deciding — options with evidence)

- **#2's decisions are confirmed by this research**: progressive MP4 over Range, no HLS in the
  player; YouTube + NBA App as External Surfaces. The one refinement #5 may adopt: "External
  Surface" can still be *in-window* via Lane B (wry child webview + IFrame API) — external to
  the Player Backend, not to the Shell.
- **Prototype order for #5** (cheapest validation first): ① Lane A sidecar → egui texture on a
  local MP4 (hours); ② normalization command round-trip (mkv/webm → +faststart MP4, verify
  moov offset + Range play); ③ wry `build_as_child` inside an eframe window with the IFrame
  embed (the risky integration — z-order/focus); ④ only if ④ matters, the libmpv2 OpenGL
  fallback for 4K-era tape.
- **Open build-time risks for #5 to verify, not decide here**: EME/FairPlay DRM support inside
  the chosen platform webviews (blocks the grey NBA-webview path, irrelevant to everything
  else); egui-video's egui version pin (crate pins egui 0.29 vs current 0.36 — the sidecar
  route avoids that pin); sidecar binary licensing/distribution choice (auto-download at
  runtime vs brew/system ffmpeg).
- **Dead ends, so #5 doesn't reopen them**: any YouTube-via-ffmpeg path (ToS stack + takedown
  precedent); any NBA-App-via-ffmpeg path (§1201; no open CDM); CDP-screencast uniforms.
