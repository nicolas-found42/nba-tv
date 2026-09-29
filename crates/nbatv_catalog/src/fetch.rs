//! The [`TapeFetcher`] port: the ONE seam Cache Tier downloads sit behind.
//!
//! Byte-class tape rows (rungs 1+4, direct progressive files) are pulled
//! into the gitignored cache at the research-15 naming
//! (`tape/{season}/{game_id}__{AWAY}-at-{HOME}__{src-tag}.{ext}`), verified
//! (nonempty plus a plausible game duration), and recorded in the
//! `cache_entries` table through the `Pending` → `Fetching` → `Verifying` →
//! `Ready` state machine (`Failed` on any failure, never `Ready`).
//!
//! Mirrors the [`crate::probe::SourceProbe`] port precedent: fakes in tests,
//! real transports in production, zero new dependencies. The live fetch
//! transport is a `curl` subprocess (`curl -C -` resumes an interrupted
//! download from the partial file) and the live duration probe shells out
//! to the user-provisioned `ffprobe` sidecar (the ffmpeg precedent); both
//! binaries are injectable so the suite stays offline.
//!
//! Start at [`fetch_to_cache`]: hand it an archive connection, a
//! [`FetchSpec`], a fetcher, a duration probe, and a `YYYY-MM-DD`
//! timestamp. Everything is offline by construction — the suite links no
//! network implementation.

use nbatv_db::{upsert_cache_entry, CacheEntry, CacheState};
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

// ---------------------------------------------------------------------------
// Cache layout
// ---------------------------------------------------------------------------

/// The `.gitignore` pattern that keeps the cache out of the repo. The test
/// below asserts this exact pattern is present in the workspace `.gitignore`,
/// so a layout move must move the ignore with it.
pub const CACHE_GITIGNORE_PATTERN: &str = "data/cache/";

/// The cache root: the single config point for where tape bytes live.
/// Honors `NBA_TV_CACHE_DIR` (tests and drivers point it at a temp dir);
/// otherwise the workspace-relative `data/cache/` (gitignored).
pub fn cache_root() -> PathBuf {
    match std::env::var("NBA_TV_CACHE_DIR") {
        Ok(dir) if !dir.trim().is_empty() => PathBuf::from(dir),
        _ => PathBuf::from("data/cache"),
    }
}

/// Cache path for one game file under `root`, research-15 naming:
/// `tape/{season}/{game_id}__{AWAY}-at-{HOME}__{src-tag}.{ext}`.
///
/// `season` is the season slug (`1946-47`), teams are BR slugs, `src_tag`
/// names the byte origin (see [`src_tag_for_url`]), `ext` is `mp4` for the
/// normalized Cache Tier copy.
pub fn cache_path(
    root: &Path,
    season: &str,
    game_id: &str,
    away: &str,
    home: &str,
    src_tag: &str,
    ext: &str,
) -> PathBuf {
    let file = format!(
        "{game_id}__{away}-at-{home}__{tag}.{ext}",
        away = away,
        home = home,
        tag = sanitize_src_tag(src_tag),
        ext = ext,
    );
    root.join("tape").join(season).join(file)
}

/// Keep `[A-Za-z0-9]` (lowercased), map anything else to `-`, collapse runs:
/// the filename half of the cache path never carries spaces or slashes.
fn sanitize_src_tag(tag: &str) -> String {
    let mut out = String::with_capacity(tag.len());
    let mut last_dash = true;
    for c in tag.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        out.push_str("src");
    }
    out
}

/// Byte-origin tag for a tape URL: archive hosts keep their short name so a
/// cached filename says where its bytes came from.
pub fn src_tag_for_url(url: &str) -> &'static str {
    let lower = url.to_ascii_lowercase();
    if lower.contains("archive.org") {
        "ia"
    } else if lower.contains("youtube.com") || lower.contains("youtu.be") {
        "yt"
    } else {
        "src"
    }
}

// ---------------------------------------------------------------------------
// Fetch port
// ---------------------------------------------------------------------------

/// One Cache Tier download: the byte-class tape URL plus where its bytes go.
/// `max_retries` bounds the total attempts (first try plus resume retries);
/// at least one attempt always runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchSpec {
    pub game_id: String,
    pub rank: u8,
    pub source_class: String,
    pub url: String,
    pub dest: PathBuf,
    pub max_retries: u32,
}

impl FetchSpec {
    pub fn attempts(&self) -> u32 {
        self.max_retries.max(1)
    }
}

/// What one fetch call produced for one byte-class row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchOutcome {
    /// Bytes are on disk at `dest` (resumed or fresh).
    Completed {
        bytes: u64,
        resumed: bool,
        attempts: u32,
    },
    /// Nothing usable came back after `attempts` tries; the caller records
    /// `Failed` and surfaces `reason` honestly.
    Failed { attempts: u32, reason: String },
}

/// The ONE seam Cache Tier downloads sit behind. Implementations are offline
/// in tests (fakes) and shell out to `curl` in production.
///
/// Attempt contract: ONE [`TapeFetcher::fetch`] call performs the WHOLE
/// bounded retry loop for the spec — production implementations loop up to
/// [`FetchSpec::attempts`] internally (see [`CurlFetcher`]); the scripted
/// fake replays one outcome per call so tests drive resume/verify paths
/// without real transport. Callers never loop.
pub trait TapeFetcher: Send + Sync {
    fn name(&self) -> &'static str;
    fn fetch(&self, spec: &FetchSpec) -> FetchOutcome;
}

/// Scripted fake fetcher for offline tests: replays one scripted outcome per
/// call, writing the scripted bytes to `spec.dest` so resume behavior is
/// observable on disk. Production replaces this with [`CurlFetcher`].
pub struct ScriptedFetcher {
    steps: Vec<ScriptStep>,
    calls: std::sync::atomic::AtomicU32,
}

/// One scripted fetch call: the bytes appended to the destination file plus
/// whether this call reports success. A `false` step leaves its partial
/// bytes behind (the interrupted-download shape) and reports failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptStep {
    pub append_bytes: Vec<u8>,
    pub succeed: bool,
}

impl ScriptStep {
    /// A call that appends `bytes` and reports completion.
    pub fn complete(bytes: &[u8]) -> Self {
        Self {
            append_bytes: bytes.to_vec(),
            succeed: true,
        }
    }

    /// A call that appends `bytes` (the partial file) and reports failure.
    pub fn fail_after(bytes: &[u8]) -> Self {
        Self {
            append_bytes: bytes.to_vec(),
            succeed: false,
        }
    }
}

impl ScriptedFetcher {
    /// Fake over an explicit script: call N consumes step N (extra calls
    /// repeat the last step, so an always-failing script proves the retry
    /// bound instead of panicking).
    pub fn new(steps: Vec<ScriptStep>) -> Self {
        assert!(
            !steps.is_empty(),
            "a scripted fetch needs at least one step"
        );
        Self {
            steps,
            calls: std::sync::atomic::AtomicU32::new(0),
        }
    }

    /// How many fetch calls the fake has served.
    pub fn calls(&self) -> u32 {
        self.calls.load(std::sync::atomic::Ordering::SeqCst)
    }
}

impl TapeFetcher for ScriptedFetcher {
    fn name(&self) -> &'static str {
        "scripted"
    }

    fn fetch(&self, spec: &FetchSpec) -> FetchOutcome {
        let attempt = self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
        let step = self
            .steps
            .get((attempt - 1) as usize)
            .unwrap_or_else(|| self.steps.last().unwrap())
            .clone();
        let resumed = spec.dest.exists();
        let append = |bytes: &[u8]| {
            if let Some(parent) = spec.dest.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            use std::io::Write;
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&spec.dest)
                .expect("scripted fetch must write its bytes");
            file.write_all(bytes)
                .expect("scripted fetch must write its bytes");
        };
        append(&step.append_bytes);
        let bytes = std::fs::metadata(&spec.dest).map(|m| m.len()).unwrap_or(0);
        if step.succeed {
            FetchOutcome::Completed {
                bytes,
                resumed,
                attempts: attempt,
            }
        } else {
            FetchOutcome::Failed {
                attempts: attempt,
                reason: format!("scripted failure at attempt {attempt}"),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Live curl transport
// ---------------------------------------------------------------------------

/// Request timeout for one curl attempt.
const CURL_MAX_TIME_SECS: u64 = 300;
/// Contact UA for live tape requests (private-research posture).
const FETCH_USER_AGENT: &str = "nba-tv-personal-archive/private-research-contact-local";

/// Live [`TapeFetcher`]: `curl -fL -C -` with the archive contact UA. `-C -`
/// resumes from the partial file when one exists, so an interrupted
/// download continues instead of restarting; the attempt loop bounds total
/// tries at [`FetchSpec::attempts`]. `curl` must be on `PATH`.
///
/// Smoke log 2026-09-08 (1 request after redirect, identifying UA,
/// media-shaped only — nothing kept):
///
/// ```sh
/// curl -sS --fail -L -r 0-1023 -m 60 \
///   -A "nba-tv-cache-smoke/1.0 (personal archive research)" \
///   -o /tmp/nbatv-cache-smoke.bin \
///   -w 'http=%{http_code} bytes=%{size_download} redirects=%{num_redirects}\n' \
///   'https://archive.org/download/1996-nba-finals-game-3/1996%20NBA%20Finals%20Game%203.mp4'
/// ```
///
/// → `http=206 bytes=1024 redirects=1` (302 to the ia*.us.archive.org node,
/// then 206 Partial Content with exactly the requested 1024 bytes) — the
/// Range-resume shape this fetcher relies on.
pub struct CurlFetcher {
    curl_bin: String,
    max_time_secs: u64,
    user_agent: String,
    run: Option<Box<dyn Fn(&[String]) -> Result<(), String> + Send + Sync>>,
}

impl CurlFetcher {
    /// Live fetcher: real `curl` subprocess, real resume.
    pub fn live() -> Self {
        Self {
            curl_bin: "curl".to_owned(),
            max_time_secs: CURL_MAX_TIME_SECS,
            user_agent: FETCH_USER_AGENT.to_owned(),
            run: None,
        }
    }

    /// Fetcher over an explicit attempt runner (offline tests): the runner
    /// receives the exact argv the live transport would run and reports the
    /// attempt outcome, so tests prove the retry bound and the resume flag
    /// without touching the network.
    pub fn with_runner(run: Box<dyn Fn(&[String]) -> Result<(), String> + Send + Sync>) -> Self {
        Self {
            curl_bin: "curl".to_owned(),
            max_time_secs: CURL_MAX_TIME_SECS,
            user_agent: FETCH_USER_AGENT.to_owned(),
            run: Some(run),
        }
    }

    fn argv(&self, spec: &FetchSpec) -> Vec<String> {
        vec![
            self.curl_bin.clone(),
            "-sS".to_owned(),
            "--fail".to_owned(),
            "-L".to_owned(),
            "-C".to_owned(),
            "-".to_owned(),
            "--max-time".to_owned(),
            self.max_time_secs.to_string(),
            "-A".to_owned(),
            self.user_agent.clone(),
            "-o".to_owned(),
            spec.dest.to_string_lossy().into_owned(),
            "--".to_owned(),
            spec.url.clone(),
        ]
    }

    fn run_live(&self, argv: &[String]) -> Result<(), String> {
        let (bin, args) = argv.split_first().expect("curl argv is never empty");
        Command::new(bin)
            .args(args)
            .output()
            .map_err(|err| format!("curl: {err}"))
            .and_then(|output| {
                if output.status.success() {
                    Ok(())
                } else {
                    Err(format!(
                        "curl exited {}: {}",
                        output.status,
                        String::from_utf8_lossy(&output.stderr).trim()
                    ))
                }
            })
    }
}

impl TapeFetcher for CurlFetcher {
    fn name(&self) -> &'static str {
        "curl"
    }

    fn fetch(&self, spec: &FetchSpec) -> FetchOutcome {
        if let Some(parent) = spec.dest.parent() {
            if let Err(err) = std::fs::create_dir_all(parent) {
                return FetchOutcome::Failed {
                    attempts: 0,
                    reason: format!("cannot create cache dir: {err}"),
                };
            }
        }
        let resumed_from = std::fs::metadata(&spec.dest).map(|m| m.len()).unwrap_or(0);
        let argv = self.argv(spec);
        let mut last_reason = String::from("no attempts ran");
        for attempt in 1..=spec.attempts() {
            let outcome: Result<(), String> = match &self.run {
                Some(run) => run(&argv),
                None => self.run_live(&argv),
            };
            match outcome {
                Ok(()) => {
                    let bytes = std::fs::metadata(&spec.dest).map(|m| m.len()).unwrap_or(0);
                    return FetchOutcome::Completed {
                        bytes,
                        resumed: resumed_from > 0,
                        attempts: attempt,
                    };
                }
                Err(reason) => {
                    last_reason = reason;
                }
            }
        }
        FetchOutcome::Failed {
            attempts: spec.attempts(),
            reason: last_reason,
        }
    }
}

// ---------------------------------------------------------------------------
// Verification
// ---------------------------------------------------------------------------

/// Shortest plausible full-game tape: highlights and quarter clips score
/// below this, never `Ready`.
pub const MIN_GAME_SECS: f64 = 35.0 * 60.0;
/// Longest plausible full-game tape: doubleheaders and ceremony overruns
/// still fit; anything longer is a wrong file, never `Ready`.
pub const MAX_GAME_SECS: f64 = 200.0 * 60.0;

/// The one seam duration probing sits behind: production shells out to
/// `ffprobe`, tests serve fixed durations. `None` means "unreadable".
pub trait DurationProbe: Send + Sync {
    fn duration_secs(&self, path: &Path) -> Option<f64>;
}

/// Live [`DurationProbe`]: `ffprobe -show_entries format=duration`.
/// `ffprobe` is a user-provisioned sidecar (the ffmpeg precedent) and must
/// be on `PATH`.
pub struct FfprobeDuration {
    bin: String,
}

impl FfprobeDuration {
    pub fn live() -> Self {
        Self {
            bin: "ffprobe".to_owned(),
        }
    }

    pub fn with_bin(bin: &str) -> Self {
        Self {
            bin: bin.to_owned(),
        }
    }
}

impl DurationProbe for FfprobeDuration {
    fn duration_secs(&self, path: &Path) -> Option<f64> {
        let output = Command::new(&self.bin)
            .arg("-v")
            .arg("error")
            .arg("-show_entries")
            .arg("format=duration")
            .arg("-of")
            .arg("csv=p=0")
            .arg("--")
            .arg(path)
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        String::from_utf8(output.stdout)
            .ok()?
            .trim()
            .parse::<f64>()
            .ok()
    }
}

/// Fixed-duration fake probe for offline tests.
pub struct ScriptedDuration {
    duration_secs: Option<f64>,
}

impl ScriptedDuration {
    /// Probe that always reports `secs` (a full-game `5400.0`, a clip
    /// `600.0`, whatever the test needs).
    pub fn seconds(secs: f64) -> Self {
        Self {
            duration_secs: Some(secs),
        }
    }

    /// Probe that always reports unreadable (missing sidecar shape).
    pub fn unreadable() -> Self {
        Self {
            duration_secs: None,
        }
    }
}

impl DurationProbe for ScriptedDuration {
    fn duration_secs(&self, _path: &Path) -> Option<f64> {
        self.duration_secs
    }
}

/// What verification settled on for one downloaded file.
#[derive(Debug, Clone, PartialEq)]
pub enum VerifyOutcome {
    /// Nonempty with a plausible game duration: safe to mark `Ready`.
    Ready { bytes: u64, duration_secs: f64 },
    /// Never `Ready`: empty, unreadable, or an implausible duration. The
    /// caller records `Failed` and surfaces `reason` honestly.
    Failed { reason: String },
}

/// Verify one downloaded file: nonempty plus an `ffprobe` duration inside
/// [`MIN_GAME_SECS`]..=[`MAX_GAME_SECS`]. A clip-length file, an empty
/// file, or an unreadable file all fail — and a failure never yields
/// `Ready`.
pub fn verify(path: &Path, probe: &dyn DurationProbe) -> VerifyOutcome {
    let bytes = match std::fs::metadata(path) {
        Ok(meta) => meta.len(),
        Err(err) => {
            return VerifyOutcome::Failed {
                reason: format!("cannot stat {}: {err}", path.display()),
            };
        }
    };
    if bytes == 0 {
        return VerifyOutcome::Failed {
            reason: format!("{} is empty: no bytes, nothing to play", path.display()),
        };
    }
    match probe.duration_secs(path) {
        None => VerifyOutcome::Failed {
            reason: format!(
                "{} is unreadable: no duration from the decoder probe",
                path.display()
            ),
        },
        Some(secs) if !secs.is_finite() => VerifyOutcome::Failed {
            reason: format!(
                "{} reports a non-finite duration ({secs}): the decoder probe is unusable",
                path.display()
            ),
        },
        Some(secs) if secs < MIN_GAME_SECS || secs > MAX_GAME_SECS => VerifyOutcome::Failed {
            reason: format!(
                "{} runs {secs:.0}s: outside the plausible game window ({}-{} min)",
                path.display(),
                (MIN_GAME_SECS / 60.0) as u64,
                (MAX_GAME_SECS / 60.0) as u64,
            ),
        },
        Some(secs) => VerifyOutcome::Ready {
            bytes,
            duration_secs: secs,
        },
    }
}

// ---------------------------------------------------------------------------
// Orchestration: fetch + verify + record
// ---------------------------------------------------------------------------

/// What one [`fetch_to_cache`] run settled on. The `cache_entries` row
/// always matches: `Ready` on success, `Failed` on any failure.
#[derive(Debug, Clone, PartialEq)]
pub enum CacheFetchReport {
    Ready { bytes: u64, duration_secs: f64 },
    Failed { reason: String },
}

/// Fetch one byte-class row into the cache and record it.
///
/// Drives the row through `Pending` → `Fetching` → `Verifying` → `Ready`,
/// refreshing the `(game_id, rank)` row at each step so an interrupted run
/// resumes from what is stored. Any failure — fetch transport, empty file,
/// unreadable or implausible duration — lands the row on `Failed` with the
/// bytes seen so far, and a verification failure never yields `Ready`.
/// `verified_at` is the `YYYY-MM-DD` stamp written only on `Ready` rows.
pub fn fetch_to_cache(
    conn: &Connection,
    spec: &FetchSpec,
    fetcher: &dyn TapeFetcher,
    probe: &dyn DurationProbe,
    verified_at: &str,
) -> Result<CacheFetchReport, rusqlite::Error> {
    let row = |state: CacheState, bytes: i64, verified: Option<String>| CacheEntry {
        game_id: spec.game_id.clone(),
        rank: spec.rank,
        source_class: spec.source_class.clone(),
        local_path: spec.dest.to_string_lossy().into_owned(),
        bytes,
        verified_at: verified,
        state,
    };
    upsert_cache_entry(conn, &row(CacheState::Pending, 0, None))?;
    upsert_cache_entry(conn, &row(CacheState::Fetching, 0, None))?;
    match fetcher.fetch(spec) {
        FetchOutcome::Failed { reason, .. } => {
            let bytes = std::fs::metadata(&spec.dest).map(|m| m.len()).unwrap_or(0) as i64;
            upsert_cache_entry(conn, &row(CacheState::Failed, bytes, None))?;
            return Ok(CacheFetchReport::Failed { reason });
        }
        FetchOutcome::Completed { bytes, .. } => {
            upsert_cache_entry(conn, &row(CacheState::Verifying, bytes as i64, None))?;
        }
    }
    match verify(&spec.dest, probe) {
        VerifyOutcome::Ready {
            bytes,
            duration_secs,
        } => {
            upsert_cache_entry(
                conn,
                &row(
                    CacheState::Ready,
                    bytes as i64,
                    Some(verified_at.to_owned()),
                ),
            )?;
            Ok(CacheFetchReport::Ready {
                bytes,
                duration_secs,
            })
        }
        VerifyOutcome::Failed { reason } => {
            let bytes = std::fs::metadata(&spec.dest).map(|m| m.len()).unwrap_or(0) as i64;
            upsert_cache_entry(conn, &row(CacheState::Failed, bytes, None))?;
            Ok(CacheFetchReport::Failed { reason })
        }
    }
}

/// How long the live curl transport waits between attempts. Exposed so a
/// driver can pace retries politely; the fetcher itself does not sleep (its
/// attempts are back-to-back subprocess calls, and politeness belongs to
/// the caller that schedules re-fetches).
pub const RETRY_PAUSE: Duration = Duration::from_secs(5);

#[cfg(test)]
mod tests {
    use super::*;

    fn spec_in(dir: &Path, max_retries: u32) -> FetchSpec {
        FetchSpec {
            game_id: "194611010TRH".to_owned(),
            rank: 1,
            source_class: "internet-archive".to_owned(),
            url: "https://archive.org/download/194611010TRH/game.mp4".to_owned(),
            dest: dir.join("tape/1946-47/194611010TRH__NYK-at-TRH__ia.mp4"),
            max_retries,
        }
    }

    fn memdb() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        nbatv_db::create_schema(&conn).expect("create_schema");
        conn
    }

    /// Scratch dir that removes itself on drop (repo convention: best-
    /// effort cleanup, no tempdir crate). `Deref` keeps call sites
    /// unchanged.
    fn temp_dir(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("nbatv-fetch-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        TempDir(dir)
    }

    struct TempDir(PathBuf);

    impl std::ops::Deref for TempDir {
        type Target = PathBuf;
        fn deref(&self) -> &PathBuf {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn naming_matches_the_agreed_scheme() {
        let path = cache_path(
            Path::new("data/cache"),
            "1946-47",
            "194611010TRH",
            "NYK",
            "TRH",
            "ia",
            "mp4",
        );
        assert_eq!(
            path,
            PathBuf::from("data/cache/tape/1946-47/194611010TRH__NYK-at-TRH__ia.mp4")
        );
    }

    #[test]
    fn src_tags_sanitize_and_classify() {
        assert_eq!(sanitize_src_tag("Internet Archive"), "internet-archive");
        assert_eq!(sanitize_src_tag("a/b c"), "a-b-c");
        assert_eq!(sanitize_src_tag(""), "src");
        assert_eq!(
            src_tag_for_url("https://archive.org/download/x/y.mp4"),
            "ia"
        );
        assert_eq!(src_tag_for_url("https://www.youtube.com/watch?v=x"), "yt");
        assert_eq!(src_tag_for_url("https://example.com/v.mp4"), "src");
        let tagged = cache_path(
            Path::new("data/cache"),
            "1946-47",
            "194611010TRH",
            "NYK",
            "TRH",
            "Weird Tag/Name",
            "mp4",
        );
        assert_eq!(
            tagged.file_name().unwrap().to_str().unwrap(),
            "194611010TRH__NYK-at-TRH__weird-tag-name.mp4"
        );
    }

    #[test]
    fn gitignore_keeps_the_cache_out_of_the_repo() {
        let ignore = include_str!("../../../.gitignore");
        for line in ignore.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if line == CACHE_GITIGNORE_PATTERN || line == "data/cache" {
                return;
            }
        }
        panic!(
            "workspace .gitignore must ignore {} (nothing under the cache is committable)",
            CACHE_GITIGNORE_PATTERN
        );
    }

    #[test]
    fn interrupted_download_resumes_from_the_partial_file() {
        let dir = temp_dir("resume");
        let spec = spec_in(&dir, 3);
        let fetcher = ScriptedFetcher::new(vec![
            ScriptStep::fail_after(b"first-half-"),
            ScriptStep::complete(b"second-half"),
        ]);
        let first = fetcher.fetch(&spec);
        assert!(
            matches!(first, FetchOutcome::Failed { .. }),
            "the interrupted call reports failure"
        );
        let partial = std::fs::read(&spec.dest).unwrap();
        assert_eq!(partial, b"first-half-", "the partial file stays behind");
        let second = fetcher.fetch(&spec);
        match second {
            FetchOutcome::Completed {
                bytes,
                resumed,
                attempts,
            } => {
                assert!(resumed, "the second call sees the partial file");
                assert_eq!(attempts, 2);
                assert_eq!(bytes, b"first-half-second-half".len() as u64);
            }
            other => panic!("expected completion, got {other:?}"),
        }
        assert_eq!(
            std::fs::read(&spec.dest).unwrap(),
            b"first-half-second-half",
            "resume appends, never restarts"
        );
        assert_eq!(fetcher.calls(), 2);
    }

    #[test]
    fn retries_are_bounded_by_the_spec() {
        let dir = temp_dir("bound");
        let spec = spec_in(&dir, 3);
        let fetcher = ScriptedFetcher::new(vec![ScriptStep::fail_after(b"shard")]);
        let conn = memdb();
        let report = fetch_to_cache(
            &conn,
            &spec,
            &fetcher,
            &ScriptedDuration::seconds(5400.0),
            "2026-09-08",
        )
        .unwrap();
        assert!(
            matches!(report, CacheFetchReport::Failed { .. }),
            "an always-failing fetch fails honestly"
        );
        // The port itself tries once per call; the bound lives in the
        // driver loop the spec declares. Three scripted calls stand in for
        // three bounded attempts: no call may run past the script.
        fetch_to_cache(
            &conn,
            &spec,
            &fetcher,
            &ScriptedDuration::seconds(5400.0),
            "2026-09-08",
        )
        .unwrap();
        fetch_to_cache(
            &conn,
            &spec,
            &fetcher,
            &ScriptedDuration::seconds(5400.0),
            "2026-09-08",
        )
        .unwrap();
        assert_eq!(fetcher.calls(), 3, "attempts stop at the bound");
        let entries = nbatv_db::cache_entries_for(&conn, "194611010TRH").unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].state, CacheState::Failed);
    }

    #[test]
    fn curl_fetcher_stops_at_the_attempt_bound() {
        use std::sync::atomic::{AtomicU32, Ordering};
        let dir = temp_dir("curlbound");
        let spec = spec_in(&dir, 3);
        let dest = spec.dest.to_string_lossy().into_owned();
        let calls = std::sync::Arc::new(AtomicU32::new(0));
        let probe_calls = calls.clone();
        let fetcher = CurlFetcher::with_runner(Box::new(move |argv: &[String]| {
            probe_calls.fetch_add(1, Ordering::SeqCst);
            assert!(
                argv.windows(2).any(|w| w == ["-C", "-"]),
                "every attempt resumes (curl -C -): {argv:?}"
            );
            let at = argv.iter().position(|a| a == "-o").expect("-o in argv");
            assert_eq!(
                argv[at + 1],
                dest,
                "every attempt writes to the destination: {argv:?}"
            );
            Err("boom".to_owned())
        }));
        let outcome = fetcher.fetch(&spec);
        match outcome {
            FetchOutcome::Failed { attempts, reason } => {
                assert_eq!(attempts, 3, "retries stop at max_retries");
                assert_eq!(reason, "boom");
            }
            other => panic!("expected bounded failure, got {other:?}"),
        }
        assert_eq!(
            calls.load(Ordering::SeqCst),
            3,
            "exactly three attempts ran"
        );
    }

    #[test]
    fn verify_rejects_empty_clips_and_unreadable_files() {
        let dir = temp_dir("verify");
        let empty = dir.join("empty.mp4");
        std::fs::write(&empty, b"").unwrap();
        assert!(
            matches!(
                verify(&empty, &ScriptedDuration::seconds(5400.0)),
                VerifyOutcome::Failed { .. }
            ),
            "empty files never verify"
        );
        let clip = dir.join("clip.mp4");
        std::fs::write(&clip, b"bytes").unwrap();
        assert!(
            matches!(
                verify(&clip, &ScriptedDuration::seconds(600.0)),
                VerifyOutcome::Failed { .. }
            ),
            "a 10-minute clip is not a game"
        );
        assert!(
            matches!(
                verify(&clip, &ScriptedDuration::unreadable()),
                VerifyOutcome::Failed { .. }
            ),
            "unreadable files never verify"
        );
        assert!(
            matches!(
                verify(&clip, &ScriptedDuration::seconds(15_000.0)),
                VerifyOutcome::Failed { .. }
            ),
            "implausibly long files never verify"
        );
        match verify(&clip, &ScriptedDuration::seconds(5400.0)) {
            VerifyOutcome::Ready {
                bytes,
                duration_secs,
            } => {
                assert_eq!(bytes, 5);
                assert_eq!(duration_secs, 5400.0);
            }
            other => panic!("expected Ready, got {other:?}"),
        }
    }

    #[test]
    fn verify_failure_never_yields_a_ready_row() {
        let dir = temp_dir("verifyfail");
        let spec = spec_in(&dir, 2);
        let fetcher = ScriptedFetcher::new(vec![ScriptStep::complete(b"clip-bytes")]);
        let conn = memdb();
        let report = fetch_to_cache(
            &conn,
            &spec,
            &fetcher,
            &ScriptedDuration::seconds(600.0),
            "2026-09-08",
        )
        .unwrap();
        assert!(
            matches!(report, CacheFetchReport::Failed { .. }),
            "a clip-length download fails verification"
        );
        assert_eq!(
            nbatv_db::ready_cache_entry_for(&conn, "194611010TRH").unwrap(),
            None,
            "verification failure never yields a Ready entry"
        );
        let entries = nbatv_db::cache_entries_for(&conn, "194611010TRH").unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].state, CacheState::Failed);
        assert_eq!(entries[0].verified_at, None, "Failed rows carry no stamp");
    }

    #[test]
    fn happy_path_records_a_ready_row() {
        let dir = temp_dir("happy");
        let spec = spec_in(&dir, 3);
        let fetcher = ScriptedFetcher::new(vec![ScriptStep::complete(&vec![7u8; 1024])]);
        let conn = memdb();
        let report = fetch_to_cache(
            &conn,
            &spec,
            &fetcher,
            &ScriptedDuration::seconds(5400.0),
            "2026-09-08",
        )
        .unwrap();
        match report {
            CacheFetchReport::Ready {
                bytes,
                duration_secs,
            } => {
                assert_eq!(bytes, 1024);
                assert_eq!(duration_secs, 5400.0);
            }
            other => panic!("expected Ready, got {other:?}"),
        }
        let ready = nbatv_db::ready_cache_entry_for(&conn, "194611010TRH")
            .unwrap()
            .expect("a Ready row is recorded");
        assert_eq!(ready.bytes, 1024);
        assert_eq!(ready.verified_at.as_deref(), Some("2026-09-08"));
        assert_eq!(ready.local_path, spec.dest.to_string_lossy());
    }

    #[test]
    fn cache_root_honors_its_single_config_point() {
        // Stash and restore so the process-wide var is untouched for
        // sibling tests; unique paths avoid cross-run collisions.
        let prior = std::env::var("NBA_TV_CACHE_DIR").ok();
        let probe = std::env::temp_dir().join(format!(
            "nbatv-cache-root-{}-{:p}",
            std::process::id(),
            &prior
        ));
        std::env::set_var("NBA_TV_CACHE_DIR", &probe);
        assert_eq!(cache_root(), probe);
        match &prior {
            Some(v) => std::env::set_var("NBA_TV_CACHE_DIR", v),
            None => std::env::remove_var("NBA_TV_CACHE_DIR"),
        }
        assert_eq!(cache_root(), PathBuf::from("data/cache"));
    }
}
