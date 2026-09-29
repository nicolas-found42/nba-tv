//! The Drive mirror stage (issue #26): a background stage that mirrors
//! `Ready` cache entries to Google Drive through the user-provisioned
//! `rclone` sidecar.
//!
//! Shape mirrors the [`crate::fetch::TapeFetcher`] port precedent: the live
//! transport shells out to a subprocess, tests inject a fake runner that
//! captures argv, and the suite stays offline. No keys, no cookies, no
//! tokens anywhere here — the user performs the one-time rclone OAuth
//! externally; the app only detects remote presence (`rclone listremotes`)
//! and otherwise prompts.
//!
//! Live sidecar note, 2026-09-08: rclone 1.75.1 is installed; the
//! single allowed read-only probe (`rclone listremotes`) returned an empty
//! list (no remote configured — rclone.conf absent), so the copy path is
//! fake-proven only. After the user configures a Drive remote, the first
//! dry run doubles as the live smoke.
//! flag. The SAME manifest file is generated first (only `Ready` rows, in
//! season-then-game order), then passed to
//! `rclone copy --files-from <manifest>` with or without `--dry-run`, so a
//! dry run previews exactly the set an apply would upload. The copy is
//! copy-only (`copy`, never `sync`/`move`/delete-family), skips identical
//! files, paces transfers, and stops on the Drive upload limit — which maps
//! to [`MirrorOutcome::LimitHit`], distinctly from [`MirrorOutcome::Failed`].
//! A missing remote short-circuits to [`MirrorOutcome::RemoteMissing`] BEFORE
//! any manifest work (no sticky state: the next run after setup resumes
//! cleanly); [`drive_sign_in_prompt`] is the exact one-time prompt text a
//! driver prints for it.
//!
//! The stage performs ZERO database writes: outcomes (including `LimitHit`)
//! live in the returned [`MirrorReport`], and cache rows stay `Ready` so an
//! interrupted or limited run resumes from what is stored.

use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::process::Command;

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

/// The single source for Drive mirror pacing and destination. Lives next to
/// [`crate::PolitenessConfig`] in spirit (one config point per sidecar), but
/// in this module because only the mirror stage reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirrorConfig {
    /// rclone remote name as configured by the user's external `rclone
    /// config` run (no credentials here, just the name). The upload root is
    /// `<remote>:` and the cache `tape/…` structure is mirrored under it.
    pub remote: String,
    /// rclone `--bwlimit` value (e.g. `"8M"`): transfer pacing.
    pub bwlimit: String,
    /// rclone `--transfers` value: how many file transfers run in parallel.
    pub transfers: u32,
    /// rclone `--max-transfer` value (e.g. `"1G"`): stop starting new
    /// transfers past this per run. Empty disables the flag.
    pub max_transfer_per_run: String,
}

impl Default for MirrorConfig {
    fn default() -> Self {
        Self {
            remote: "nbatv-drive".to_owned(),
            bwlimit: "8M".to_owned(),
            transfers: 2,
            max_transfer_per_run: "1G".to_owned(),
        }
    }
}

impl MirrorConfig {
    /// The rclone destination root (`<remote>:`).
    pub fn remote_root(&self) -> String {
        format!("{}:", self.remote)
    }
}

// ---------------------------------------------------------------------------
// rclone sidecar seam
// ---------------------------------------------------------------------------

/// What one rclone invocation produced. `Ok` from the runner means the
/// process ran (the exit `code` may still be non-zero); `Err` means it could
/// not run at all (missing binary, spawn failure).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RcloneOutput {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl RcloneOutput {
    /// A clean exit with captured stdout.
    pub fn success(stdout: &str) -> Self {
        Self {
            code: 0,
            stdout: stdout.to_owned(),
            stderr: String::new(),
        }
    }

    /// A non-zero exit with captured stderr.
    pub fn failure(code: i32, stderr: &str) -> Self {
        Self {
            code,
            stdout: String::new(),
            stderr: stderr.to_owned(),
        }
    }
}

/// Live [`RcloneMirror`]: the user-provisioned `rclone` sidecar. `rclone`
/// must be on `PATH` and the remote configured externally.
pub struct RcloneMirror {
    bin: String,
    run: Option<Box<dyn Fn(&[String]) -> Result<RcloneOutput, String> + Send + Sync>>,
}

impl RcloneMirror {
    /// Live mirror: real `rclone` subprocesses.
    pub fn live() -> Self {
        Self {
            bin: "rclone".to_owned(),
            run: None,
        }
    }

    /// Mirror over an explicit invocation runner (offline tests): the runner
    /// receives the exact argv the live transport would run and reports the
    /// scripted outcome, so tests prove the copy-only invariant, the
    /// dry-run/apply argv difference, and the outcome mapping without
    /// touching the network or a real remote.
    pub fn with_runner(
        run: Box<dyn Fn(&[String]) -> Result<RcloneOutput, String> + Send + Sync>,
    ) -> Self {
        Self {
            bin: "rclone".to_owned(),
            run: Some(run),
        }
    }

    fn invoke(&self, argv: &[String]) -> Result<RcloneOutput, String> {
        match &self.run {
            Some(run) => run(argv),
            None => {
                let (bin, args) = argv.split_first().expect("rclone argv is never empty");
                Command::new(bin)
                    .args(args)
                    .output()
                    .map_err(|err| format!("rclone: {err}"))
                    .map(|output| RcloneOutput {
                        code: output.status.code().unwrap_or(-1),
                        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
                        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
                    })
            }
        }
    }

    /// Exactly one remote-presence check per call (`rclone listremotes`,
    /// parsed for the `<remote>:` line). `Ok(false)` covers both "remote
    /// absent" and "listremotes itself failed" — either way the stage cannot
    /// confirm the remote and must prompt, never upload. `Err` is only the
    /// sidecar failing to run at all.
    fn remote_present(&self, remote: &str) -> Result<bool, String> {
        let argv = vec![self.bin.clone(), "listremotes".to_owned()];
        match self.invoke(&argv) {
            Err(reason) => Err(reason),
            Ok(out) if out.code != 0 => Ok(false),
            Ok(out) => {
                let want = format!("{remote}:");
                Ok(out.stdout.lines().any(|line| line.trim() == want))
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Report
// ---------------------------------------------------------------------------

/// One Ready file in the upload set: where its bytes are and where they go.
/// `remote_path` mirrors the cache structure under the remote root
/// (`<remote>:tape/{season}/{file}`), so source and destination cannot drift.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirrorEntry {
    pub game_id: String,
    pub local_path: String,
    pub remote_path: String,
}

/// What one [`mirror_ready_entries`] run settled on. The `cache_entries`
/// rows always still match: the stage writes nothing, so `Ready` stays
/// `Ready` through every outcome and the next run resumes cleanly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MirrorOutcome {
    /// The upload set ran (or, for a dry run, previewed) to completion.
    Completed { files: usize },
    /// rclone tripped the Drive upload limit (`--drive-stop-on-upload-limit`
    /// / `--max-transfer`): paused, NOT failed — re-run later.
    LimitHit { reason: String },
    /// The configured remote is not in `rclone listremotes`: nothing ran,
    /// no manifest was written, no state stuck. Prompt once with
    /// [`drive_sign_in_prompt`] and re-run after setup.
    RemoteMissing { remote: String },
    /// Anything else: the reason names it honestly.
    Failed { reason: String },
}

/// The full account of one mirror run: the exact upload set plus how it went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirrorReport {
    /// The upload set in manifest order (season, then game, then rank).
    pub entries: Vec<MirrorEntry>,
    /// Where the manifest was written, `None` when the run short-circuited
    /// before any manifest work (`RemoteMissing`, unrunnable sidecar).
    pub manifest_path: Option<PathBuf>,
    /// Whether this run previewed (`--dry-run`, nothing moved) or applied.
    pub dry_run: bool,
    pub outcome: MirrorOutcome,
}

/// What can go wrong OUTSIDE a run's outcome: the archive read failed, or
/// the manifest file could not be written. A run that starts always lands in
/// a [`MirrorReport`] instead (including `Failed`).
#[derive(Debug)]
pub enum MirrorError {
    Db(rusqlite::Error),
    Io(std::io::Error),
}

impl From<rusqlite::Error> for MirrorError {
    fn from(err: rusqlite::Error) -> Self {
        MirrorError::Db(err)
    }
}

impl From<std::io::Error> for MirrorError {
    fn from(err: std::io::Error) -> Self {
        MirrorError::Io(err)
    }
}

impl std::fmt::Display for MirrorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MirrorError::Db(err) => write!(f, "mirror: archive read failed: {err}"),
            MirrorError::Io(err) => write!(f, "mirror: manifest write failed: {err}"),
        }
    }
}

impl std::error::Error for MirrorError {}

// ---------------------------------------------------------------------------
// Manifest + orchestration
// ---------------------------------------------------------------------------

/// The `--files-from` line for one cached file: the cache-root-relative path
/// (`tape/{season}/{file}`), which is also the remote-relative destination.
/// Derived from the `tape/` component so it never depends on where the cache
/// root is mounted; a path with no `tape/` component degrades to its file
/// name (rclone then fails honestly on that line, surfaced as `Failed`).
fn manifest_rel(local_path: &str) -> String {
    let parts: Vec<String> = Path::new(local_path)
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    if let Some(at) = parts.iter().position(|c| c == "tape") {
        return parts[at..].join("/");
    }
    Path::new(local_path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| local_path.to_owned())
}

/// Whether an rclone failure means the Drive upload limit tripped (pause and
/// re-run later) rather than a real failure. Matches the sidecar's
/// stop-on-upload-limit wording plus the Drive `userRateLimitExceeded` API
/// reason and the `--max-transfer` stop line, case-insensitively.
fn is_limit_hit(reason: &str) -> bool {
    const MARKERS: &[&str] = &[
        "drive-stop-on-upload-limit",
        "stop on upload limit",
        "upload limit",
        "userratelimitexceeded",
        "max transfer limit",
    ];
    let lower = reason.to_ascii_lowercase();
    MARKERS.iter().any(|m| lower.contains(m))
}

/// The exact one-time prompt a driver prints when a run reports
/// [`MirrorOutcome::RemoteMissing`]: names the missing remote and the
/// external setup command. The sign-in itself always completes outside the
/// app — this string carries no credentials because there are none to carry.
pub fn drive_sign_in_prompt(config: &MirrorConfig) -> String {
    format!(
        "Drive mirror: remote '{}' is missing — run `rclone config` once to sign in, \
         then re-run the mirror. The app never touches credentials.",
        config.remote
    )
}

/// Mirror every `Ready` cache entry to Google Drive.
///
/// 1. Checks the remote exactly once (`rclone listremotes`). Missing (or
///    unlistable) → [`MirrorOutcome::RemoteMissing`] BEFORE any manifest
///    work: no file written, no state stuck, so the run after setup resumes
///    cleanly.
/// 2. Generates the manifest: one cache-root-relative path per `Ready` row
///    in season-then-game order, so dry-run and apply cannot diverge.
/// 3. Runs `rclone copy <cache-root> <remote>: --files-from <manifest>`
///    (copy-only, paced, stop-on-upload-limit) with or without
///    `--dry-run`. Identical files are never re-transferred — that is
///    `rclone copy`'s own default (size/modtime/MD5), and `--skip-identical`
///    does not exist in rclone 1.75. An empty upload set writes the empty
///    manifest and completes without invoking the copy at all.
pub fn mirror_ready_entries(
    conn: &Connection,
    config: &MirrorConfig,
    mirror: &RcloneMirror,
    manifest_path: &Path,
    dry_run: bool,
) -> Result<MirrorReport, MirrorError> {
    let no_manifest = |outcome: MirrorOutcome| MirrorReport {
        entries: Vec::new(),
        manifest_path: None,
        dry_run,
        outcome,
    };
    let present = match mirror.remote_present(&config.remote) {
        Ok(present) => present,
        Err(reason) => {
            return Ok(no_manifest(MirrorOutcome::Failed {
                reason: format!("rclone listremotes: {reason}"),
            }));
        }
    };
    if !present {
        return Ok(no_manifest(MirrorOutcome::RemoteMissing {
            remote: config.remote.clone(),
        }));
    }
    let rows = nbatv_db::ready_cache_entries(conn)?;
    let mut entries = Vec::with_capacity(rows.len());
    let mut manifest = String::new();
    for row in &rows {
        let rel = manifest_rel(&row.local_path);
        manifest.push_str(&rel);
        manifest.push('\n');
        entries.push(MirrorEntry {
            game_id: row.game_id.clone(),
            local_path: row.local_path.clone(),
            remote_path: format!("{}:{rel}", config.remote),
        });
    }
    if let Some(parent) = manifest_path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    std::fs::write(manifest_path, manifest)?;
    if entries.is_empty() {
        return Ok(MirrorReport {
            entries,
            manifest_path: Some(manifest_path.to_owned()),
            dry_run,
            outcome: MirrorOutcome::Completed { files: 0 },
        });
    }
    let mut argv = vec![
        "rclone".to_owned(),
        "copy".to_owned(),
        crate::fetch::cache_root().to_string_lossy().into_owned(),
        config.remote_root(),
        "--files-from".to_owned(),
        manifest_path.to_string_lossy().into_owned(),
        "--drive-stop-on-upload-limit".to_owned(),
        "--bwlimit".to_owned(),
        config.bwlimit.clone(),
        "--transfers".to_owned(),
        config.transfers.to_string(),
    ];
    if !config.max_transfer_per_run.trim().is_empty() {
        argv.push("--max-transfer".to_owned());
        argv.push(config.max_transfer_per_run.clone());
    }
    if dry_run {
        argv.push("--dry-run".to_owned());
    }
    let files = entries.len();
    let outcome = match mirror.invoke(&argv) {
        Err(reason) => MirrorOutcome::Failed {
            reason: format!("rclone copy: {reason}"),
        },
        Ok(out) if out.code == 0 => MirrorOutcome::Completed { files },
        Ok(out) => {
            let stderr = out.stderr.trim();
            let reason = if stderr.is_empty() {
                format!("rclone exited {}", out.code)
            } else {
                format!("rclone exited {}: {stderr}", out.code)
            };
            if is_limit_hit(&reason) {
                MirrorOutcome::LimitHit { reason }
            } else {
                MirrorOutcome::Failed { reason }
            }
        }
    };
    Ok(MirrorReport {
        entries,
        manifest_path: Some(manifest_path.to_owned()),
        dry_run,
        outcome,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use parking_lot::Mutex;
    use std::sync::Arc;

    fn memdb() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        nbatv_db::create_schema(&conn).expect("create_schema");
        conn
    }

    fn game(season: i32, game_id: &str) -> nbatv_db::GameRow {
        nbatv_db::GameRow {
            game_id: game_id.to_owned(),
            nba_game_id: None,
            league: "BAA".to_owned(),
            season,
            date: "1946-11-01".to_owned(),
            game_type: "REGULAR".to_owned(),
            home_team: "TRH".to_owned(),
            away_team: "NYK".to_owned(),
            home_pts: 68,
            away_pts: 66,
            ot: None,
            arena: None,
            attendance: None,
            br_url: format!("https://example.com/{game_id}"),
            sources: "[]".to_owned(),
        }
    }

    fn cache(game_id: &str, rank: u8, state: nbatv_db::CacheState) -> nbatv_db::CacheEntry {
        // Local paths use the research-15 naming; the season directory
        // follows the game, not the insertion order.
        let season = if game_id < "194711010TRH" {
            "1946-47"
        } else {
            "1947-48"
        };
        nbatv_db::CacheEntry {
            game_id: game_id.to_owned(),
            rank,
            source_class: "internet-archive".to_owned(),
            local_path: format!("data/cache/tape/{season}/{game_id}__NYK-at-TRH__ia.mp4"),
            bytes: 1024,
            verified_at: (state == nbatv_db::CacheState::Ready).then(|| "2026-09-08".to_owned()),
            state,
        }
    }

    /// Scratch dir that removes itself on drop (repo convention: best-
    /// effort cleanup, no tempdir crate).
    fn temp_dir(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("nbatv-drive-{name}-{}", std::process::id()));
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

    /// Fake rclone over an explicit script: `listremotes` answers from
    /// `remotes_stdout`/`remotes_code`, every `copy` is captured and answers
    /// `copy_code`/`copy_stderr`. The caller owns `calls` and asserts argv.
    fn fake_mirror(
        calls: &Arc<Mutex<Vec<Vec<String>>>>,
        remotes_stdout: &str,
        remotes_code: i32,
        copy_code: i32,
        copy_stderr: &str,
    ) -> RcloneMirror {
        let calls = Arc::clone(calls);
        let remotes_stdout = remotes_stdout.to_owned();
        let copy_stderr = copy_stderr.to_owned();
        RcloneMirror::with_runner(Box::new(move |argv: &[String]| {
            calls.lock().push(argv.to_vec());
            if argv.get(1).map(String::as_str) == Some("listremotes") {
                if remotes_code == 0 {
                    Ok(RcloneOutput::success(&remotes_stdout))
                } else {
                    Ok(RcloneOutput::failure(remotes_code, "config read failed"))
                }
            } else {
                assert_eq!(
                    argv.get(1).map(String::as_str),
                    Some("copy"),
                    "every upload invocation is a copy: {argv:?}"
                );
                if copy_code == 0 {
                    Ok(RcloneOutput::success("transferred"))
                } else {
                    Ok(RcloneOutput::failure(copy_code, &copy_stderr))
                }
            }
        }))
    }

    fn remote_line(remote: &str) -> String {
        format!("other-remote:\n{remote}:\n")
    }

    fn copy_argv(calls: &[Vec<String>]) -> &Vec<String> {
        calls
            .iter()
            .find(|argv| argv.get(1).map(String::as_str) == Some("copy"))
            .expect("one copy invocation ran")
    }

    /// Three Ready rows across two seasons plus non-Ready decoys, inserted
    /// out of order so the manifest must sort, not echo insertion.
    fn seeded_two_seasons() -> Connection {
        let conn = memdb();
        nbatv_db::insert_game(&conn, &game(1948, "194711010TRH")).unwrap();
        nbatv_db::insert_game(&conn, &game(1947, "194611010TRH")).unwrap();
        nbatv_db::insert_game(&conn, &game(1947, "194611020CHS")).unwrap();
        // Insertion order is deliberately NOT manifest order.
        nbatv_db::upsert_cache_entry(
            &conn,
            &cache("194711010TRH", 1, nbatv_db::CacheState::Ready),
        )
        .unwrap();
        nbatv_db::upsert_cache_entry(
            &conn,
            &cache("194611020CHS", 1, nbatv_db::CacheState::Failed),
        )
        .unwrap();
        nbatv_db::upsert_cache_entry(
            &conn,
            &cache("194611020CHS", 4, nbatv_db::CacheState::Ready),
        )
        .unwrap();
        nbatv_db::upsert_cache_entry(
            &conn,
            &cache("194611010TRH", 1, nbatv_db::CacheState::Pending),
        )
        .unwrap();
        nbatv_db::upsert_cache_entry(
            &conn,
            &cache("194611010TRH", 4, nbatv_db::CacheState::Ready),
        )
        .unwrap();
        conn
    }

    #[test]
    fn empty_ready_set_writes_empty_manifest_without_a_copy() {
        // The fresh-clone first-run path: an empty archive mirrors nothing
        // and still completes, never invoking the copy.
        let conn = memdb();
        let dir = temp_dir("empty");
        let manifest = dir.join("files-from.txt");
        let calls = Arc::new(Mutex::new(Vec::new()));
        let config = MirrorConfig::default();
        let mirror = fake_mirror(&calls, &remote_line(&config.remote), 0, 0, "");
        let report = mirror_ready_entries(&conn, &config, &mirror, &manifest, false).unwrap();
        assert_eq!(report.entries, Vec::new());
        assert_eq!(report.outcome, MirrorOutcome::Completed { files: 0 });
        assert_eq!(
            std::fs::read(&manifest).unwrap(),
            Vec::<u8>::new(),
            "the empty manifest file exists with zero bytes"
        );
        assert!(
            !calls
                .lock()
                .iter()
                .any(|argv| argv.get(1).map(String::as_str) == Some("copy")),
            "no copy runs for an empty upload set: {calls:?}"
        );
    }

    #[test]
    fn manifest_lists_only_ready_rows_in_season_then_game_order() {
        let conn = seeded_two_seasons();
        let dir = temp_dir("manifest");
        let manifest = dir.join("files-from.txt");
        let calls = Arc::new(Mutex::new(Vec::new()));
        let config = MirrorConfig::default();
        let mirror = fake_mirror(&calls, &remote_line(&config.remote), 0, 0, "");
        let report = mirror_ready_entries(&conn, &config, &mirror, &manifest, true).unwrap();
        let expected = vec![
            "tape/1946-47/194611010TRH__NYK-at-TRH__ia.mp4",
            "tape/1946-47/194611020CHS__NYK-at-TRH__ia.mp4",
            "tape/1947-48/194711010TRH__NYK-at-TRH__ia.mp4",
        ];
        assert_eq!(report.entries.len(), 3, "only Ready rows mirror");
        for (entry, rel) in report.entries.iter().zip(&expected) {
            assert_eq!(
                entry.remote_path,
                format!("{}:{rel}", config.remote),
                "the remote mirrors the cache structure"
            );
        }
        assert_eq!(
            report
                .entries
                .iter()
                .map(|e| e.game_id.clone())
                .collect::<Vec<_>>(),
            vec!["194611010TRH", "194611020CHS", "194711010TRH"],
            "season first, then game"
        );
        let on_disk = std::fs::read_to_string(&manifest).unwrap();
        assert_eq!(
            on_disk,
            expected.join("\n") + "\n",
            "the manifest file is exactly the upload set"
        );
        assert_eq!(report.manifest_path.as_deref(), Some(manifest.as_path()));
        assert!(report.dry_run);
        assert_eq!(report.outcome, MirrorOutcome::Completed { files: 3 });
        // The decoy states never surface, whatever their rank.
        assert!(
            nbatv_db::cache_entries_for(&conn, "194611020CHS")
                .unwrap()
                .iter()
                .any(|e| e.state == nbatv_db::CacheState::Failed),
            "the Failed decoy is still Failed (the stage writes nothing)"
        );
    }

    #[test]
    fn dry_run_and_apply_share_one_manifest_and_differ_by_one_flag() {
        let conn = seeded_two_seasons();
        let dir = temp_dir("dryapply");
        let manifest = dir.join("files-from.txt");
        let config = MirrorConfig::default();
        let dry_calls = Arc::new(Mutex::new(Vec::new()));
        let dry = fake_mirror(&dry_calls, &remote_line(&config.remote), 0, 0, "");
        let dry_report = mirror_ready_entries(&conn, &config, &dry, &manifest, true).unwrap();
        let dry_manifest = std::fs::read(&manifest).unwrap();
        let apply_calls = Arc::new(Mutex::new(Vec::new()));
        let apply = fake_mirror(&apply_calls, &remote_line(&config.remote), 0, 0, "");
        let apply_report = mirror_ready_entries(&conn, &config, &apply, &manifest, false).unwrap();
        let apply_manifest = std::fs::read(&manifest).unwrap();
        assert_eq!(
            dry_manifest, apply_manifest,
            "the SAME manifest feeds preview and apply"
        );
        assert_eq!(
            dry_report.entries, apply_report.entries,
            "both reports list the same upload set"
        );
        let dry_argv = copy_argv(&dry_calls.lock()).clone();
        let apply_argv = copy_argv(&apply_calls.lock()).clone();
        assert!(
            dry_argv.contains(&"--dry-run".to_owned()),
            "the preview passes --dry-run: {dry_argv:?}"
        );
        assert!(
            !apply_argv.contains(&"--dry-run".to_owned()),
            "the apply does not: {apply_argv:?}"
        );
        let stripped: Vec<String> = dry_argv.into_iter().filter(|a| a != "--dry-run").collect();
        assert_eq!(
            stripped, apply_argv,
            "the ONLY argv difference is the --dry-run flag"
        );
    }

    #[test]
    fn copy_is_copy_only_skip_identical_paced_and_limited() {
        let conn = seeded_two_seasons();
        let dir = temp_dir("flags");
        let manifest = dir.join("files-from.txt");
        let config = MirrorConfig {
            remote: "nbatv-drive".to_owned(),
            bwlimit: "8M".to_owned(),
            transfers: 2,
            max_transfer_per_run: "1G".to_owned(),
        };
        let calls = Arc::new(Mutex::new(Vec::new()));
        let mirror = fake_mirror(&calls, &remote_line(&config.remote), 0, 0, "");
        mirror_ready_entries(&conn, &config, &mirror, &manifest, false).unwrap();
        let argv = copy_argv(&calls.lock()).clone();
        assert_eq!(argv[1], "copy", "never sync, move, or delete");
        for banned in ["sync", "move", "delete", "purge"] {
            assert!(
                !argv.iter().any(|a| a == banned),
                "no remote deletes are possible ({banned} in {argv:?})"
            );
            assert!(
                !argv.iter().any(|a| a.starts_with("--delete")),
                "no delete-family flag either: {argv:?}"
            );
        }
        // --skip-identical does not exist in rclone 1.75 (and would fail
        // every run): identical-skip is `rclone copy`'s default behavior.
        assert!(
            !argv.iter().any(|a| a.contains("skip-identical")),
            "the nonexistent flag must never appear: {argv:?}"
        );
        for flag in [
            "--drive-stop-on-upload-limit",
            "--bwlimit",
            "--transfers",
            "--max-transfer",
            "--files-from",
        ] {
            assert!(argv.contains(&flag.to_owned()), "{flag} is set: {argv:?}");
        }
        let at = |flag: &str| {
            argv.iter()
                .position(|a| a == flag)
                .map(|i| argv[i + 1].clone())
                .unwrap_or_else(|| panic!("{flag} carries a value: {argv:?}"))
        };
        assert_eq!(at("--bwlimit"), "8M");
        assert_eq!(at("--transfers"), "2");
        assert_eq!(at("--max-transfer"), "1G");
        assert_eq!(at("--files-from"), manifest.to_string_lossy());
        assert_eq!(
            argv[3], "nbatv-drive:",
            "the remote root is the destination"
        );
    }

    #[test]
    fn copy_outcome_maps_limit_hit_and_failure_distinctly() {
        let conn = seeded_two_seasons();
        let dir = temp_dir("outcome");
        let config = MirrorConfig::default();
        // The Drive upload limit trips: rclone exits non-zero under
        // --drive-stop-on-upload-limit. Paused, never Failed.
        let limit_calls = Arc::new(Mutex::new(Vec::new()));
        let limited = fake_mirror(
            &limit_calls,
            &remote_line(&config.remote),
            0,
            1,
            "Failed to copy: googleapi: Error 403: User rate limit exceeded., userRateLimitExceeded",
        );
        let limit_report =
            mirror_ready_entries(&conn, &config, &limited, &dir.join("limit.txt"), false).unwrap();
        match limit_report.outcome {
            MirrorOutcome::LimitHit { reason } => {
                assert!(
                    reason.contains("rclone exited 1"),
                    "the reason names the exit: {reason}"
                );
            }
            other => panic!("upload-limit trip must pause distinctly, got {other:?}"),
        }
        assert_eq!(
            limit_report.entries.len(),
            3,
            "the report still lists what ran"
        );
        // A real failure stays a failure.
        let fail_calls = Arc::new(Mutex::new(Vec::new()));
        let failing = fake_mirror(&fail_calls, &remote_line(&config.remote), 0, 1, "boom");
        let fail_report =
            mirror_ready_entries(&conn, &config, &failing, &dir.join("fail.txt"), false).unwrap();
        assert_eq!(
            fail_report.outcome,
            MirrorOutcome::Failed {
                reason: "rclone exited 1: boom".to_owned()
            }
        );
        // Neither outcome touches the rows: Ready stays Ready, so a later
        // run resumes from what is stored.
        assert_eq!(nbatv_db::ready_cache_entries(&conn).unwrap().len(), 3);
    }

    #[test]
    fn missing_remote_short_circuits_and_resumes_after_setup() {
        let conn = seeded_two_seasons();
        let dir = temp_dir("missing");
        let manifest = dir.join("files-from.txt");
        let config = MirrorConfig::default();
        // No remote configured: exactly one listremotes check, then stop —
        // no manifest, no copy, no stuck state.
        let calls = Arc::new(Mutex::new(Vec::new()));
        let missing = fake_mirror(&calls, "other-remote:\n", 0, 0, "");
        let report = mirror_ready_entries(&conn, &config, &missing, &manifest, false).unwrap();
        assert_eq!(
            report.outcome,
            MirrorOutcome::RemoteMissing {
                remote: config.remote.clone()
            }
        );
        assert_eq!(report.entries, Vec::new(), "no upload set runs");
        assert_eq!(report.manifest_path, None, "no manifest work happens");
        assert!(!manifest.exists(), "no manifest file is written");
        assert!(
            calls
                .lock()
                .iter()
                .all(|argv| argv.get(1).map(String::as_str) != Some("copy")),
            "no upload is attempted"
        );
        assert_eq!(
            calls.lock().len(),
            1,
            "the remote is checked exactly once per run (the prompt fires once, never in a loop)"
        );
        // A failing listremotes is the same prompt, not a crash.
        let err_calls = Arc::new(Mutex::new(Vec::new()));
        let err_calls_clone = Arc::clone(&err_calls);
        let unlistable = RcloneMirror::with_runner(Box::new(move |argv: &[String]| {
            err_calls_clone.lock().push(argv.to_vec());
            Ok(RcloneOutput::failure(1, "config read failed"))
        }));
        let unlistable_report = mirror_ready_entries(
            &conn,
            &config,
            &unlistable,
            &dir.join("unlistable.txt"),
            false,
        )
        .unwrap();
        assert!(
            matches!(
                unlistable_report.outcome,
                MirrorOutcome::RemoteMissing { .. }
            ),
            "an unlistable remote prompts too: {:?}",
            unlistable_report.outcome
        );
        drop(err_calls);
        // After the user sets the remote up externally, the next run
        // resumes cleanly: same rows, full upload set, no stuck failure.
        let resume_calls = Arc::new(Mutex::new(Vec::new()));
        let present = fake_mirror(&resume_calls, &remote_line(&config.remote), 0, 0, "");
        let resume = mirror_ready_entries(&conn, &config, &present, &manifest, false).unwrap();
        assert_eq!(resume.outcome, MirrorOutcome::Completed { files: 3 });
        assert!(
            manifest.exists(),
            "the manifest is written on the resumed run"
        );
    }

    #[test]
    fn sign_in_prompt_names_the_remote_and_the_external_setup() {
        let config = MirrorConfig::default();
        let prompt = drive_sign_in_prompt(&config);
        assert!(
            prompt.contains("nbatv-drive"),
            "names the missing remote: {prompt}"
        );
        assert!(
            prompt.contains("rclone config"),
            "names the external setup: {prompt}"
        );
        assert!(
            prompt.contains("never touches credentials"),
            "states the credential boundary: {prompt}"
        );
        for secret_word in ["password", "token", "secret", "cookie"] {
            assert!(
                !prompt.to_ascii_lowercase().contains(secret_word),
                "no credential material in the prompt ({secret_word}): {prompt}"
            );
        }
    }

    #[test]
    fn mirror_config_defaults_match_the_documented_sidecar() {
        let config = MirrorConfig::default();
        assert_eq!(config.remote, "nbatv-drive");
        assert_eq!(config.bwlimit, "8M");
        assert_eq!(config.transfers, 2);
        assert_eq!(config.max_transfer_per_run, "1G");
        assert_eq!(config.remote_root(), "nbatv-drive:");
    }
}
