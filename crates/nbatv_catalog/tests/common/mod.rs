//! Shared test helpers for fake-binary (subprocess sidecar) tests.
//!
//! Why this module exists: CI (ubuntu, `cargo test --workspace`) flaked in
//! `tests/ytdlp_probe.rs` with a different test failing each run, always
//! 12 passed / 1 failed. Root cause is a Linux `ETXTBSY` spawn race: the fake
//! binary is an `sh` script whose write fd, until closed, is inherited by
//! *every* concurrent `fork` (O_CLOEXEC only affects `exec`, not fork). While
//! any such child lives, `execve` of the script fails with ETXTBSY, which maps
//! to `ProbeOutcome::deferred` and fails the `!deferred` assertions.
//!
//! The fix is a process-wide spawn lock plus install-by-rename:
//!
//! - [`spawn_lock`]/[`with_spawn_lock`]: a process-wide mutex held around
//!   (a) the fake-binary write window and (b) every test call that spawns a
//!   fake binary. Forks then can never overlap a write window, so no child can
//!   inherit a live write fd on the script inode.
//! - [`install_fake`]: writes the script to a unique temp name, closes the
//!   fd, then `rename`s onto the final path — the final path never carries an
//!   open write fd in this process.
//!
//! Children exit before the lock releases because library calls
//! (`SourceProbe::probe`, `sweep_game`, …) block on `Command::output()`.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};

/// Collision-proof unique temp dir: `NEXT_FAKE_DIR_ID` + pid (repo
/// convention: best-effort cleanup, no tempdir crate).
static NEXT_FAKE_DIR_ID: AtomicU64 = AtomicU64::new(0);

/// Process-wide spawn lock. See the module docs for the ETXTBSY race.
static SPAWN_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

/// The process-wide spawn-lock mutex, created on first use.
pub(crate) fn spawn_lock() -> &'static Mutex<()> {
    SPAWN_LOCK.get_or_init(|| Mutex::new(()))
}

/// Hold the spawn lock while running `f`.
///
/// Convention: wrap every test call that causes a fake binary to be spawned
/// (`probe`, `sweep_game`, `run_backfill`, fetch/mirror calls, …).
pub(crate) fn with_spawn_lock<T>(f: impl FnOnce() -> T) -> T {
    let _guard: MutexGuard<'_, ()> = spawn_lock().lock().unwrap_or_else(|poisoned| {
        // A panic inside another test's spawn window poisons the lock; the
        // test binary is failing anyway, so the guard's extra readers are
        // harmless. Resume rather than cascade.
        poisoned.into_inner()
    });
    f()
}

/// Install a fake binary and return `(binary path, argv-log path, guard)`.
///
/// The script logs its argv (one arg per line) to a sibling file, then runs
/// `body`. Installed by write-to-unique-temp-name + `rename`, so the final
/// path never has an open write fd in this process; `chmod 755` happens after
/// the rename. The dir is removed when the guard drops.
pub(crate) fn install_fake(hint: &str, body: &str) -> (PathBuf, PathBuf, TempDirGuard) {
    // Hold the spawn lock across the whole write window so no concurrent
    // fork can pick up the script's write fd.
    with_spawn_lock(|| {
        let id = NEXT_FAKE_DIR_ID.fetch_add(1, Ordering::Relaxed);
        let dir = loop {
            let dir =
                std::env::temp_dir().join(format!("nbatv-fake-{hint}-{}-{id}", std::process::id()));
            match std::fs::create_dir(&dir) {
                Ok(()) => break dir,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("create fake binary temp dir: {error}"),
            }
        };
        let bin = dir.join(format!("fake-{hint}.sh"));
        let log = dir.join("argv.log");
        // `echo "$@"` would join with spaces; one-arg-per-line keeps
        // assertions exact.
        let script = format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"{}\"\n{}",
            log.display(),
            body
        );
        // Write to a unique staging name first: File::create on the *final*
        // path would leave that inode with an open write fd until drop.
        let staging = dir.join(format!(".staging-{id}"));
        let mut f = std::fs::File::create(&staging).expect("write fake");
        std::io::Write::write_all(&mut f, script.as_bytes()).expect("write fake");
        drop(f);
        // Rename closes the race: the final path never had a live write fd.
        std::fs::rename(&staging, &bin).expect("rename fake into place");
        let mut perms = std::fs::metadata(&bin).expect("meta").permissions();
        use std::os::unix::fs::PermissionsExt;
        perms.set_mode(0o755);
        std::fs::set_permissions(&bin, perms).expect("chmod");
        (bin, log, TempDirGuard { dir })
    })
}

/// Removes the fake binary's temp dir on drop.
pub(crate) struct TempDirGuard {
    dir: PathBuf,
}

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Read the fake binary's argv log, panicking if the fake never ran.
pub(crate) fn argv_of(log: &Path) -> Vec<String> {
    std::fs::read_to_string(log)
        .expect("fake ran and logged argv")
        .lines()
        .map(str::to_owned)
        .collect()
}
