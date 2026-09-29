//! The headless catalog runner (issue #27): the overnight backfill path.
//!
//! [`run_backfill`] executes sweep, fetch, and Drive upload for a season
//! range without the GUI, honoring the politeness config and rescan hints.
//! It orchestrates the three merged stages behind their existing ports —
//! [`crate::sweep_game`] through a [`ProbeRegistry`], [`crate::fetch_to_cache`]
//! through a [`TapeFetcher`] + [`DurationProbe`], and
//! [`crate::mirror_ready_entries`] through an [`RcloneMirror`] — so the
//! suite stays offline: tests inject scripted fakes, the binary injects the
//! live probes.
//!
//! Stage order per run, over the games in the season range (season, then
//! date, then game):
//!
//! 1. **Sweep** each game once through [`crate::sweep_game`], threading ONE
//!    [`PolitenessConfig`] and ONE shared [`YoutubeQuota`] through every
//!    call (one quota per run, the per-sweep-day shape the config documents).
//!    self-skips fresh rows per rung, and the runner counts a game as
//!    skipped exactly when its report probed nothing AND deferred nothing
//!    (a quota-deferred rung records nothing and retries next run, so it is
//!    counted as `deferred`, never as rescan-skipped) — there is no second
//!    freshness implementation to drift.
//! 2. **Fetch** every byte-class tape row ([`PlaybackClass::ProgressiveFile`],
//!    rungs 1+4) whose game has no `Ready` cache entry yet
//!    ([`ready_cache_entry_for`] is `None`), into the research-15 cache
//!    naming through [`crate::fetch_to_cache`]. REVIEW candidates never
//!    become tape rows, so they are never fetched — they stay on the
//!    game_queries row for the review list. Already-`Ready` rows never
//!    re-fetch; `Failed` rows retry next run. The fetch set is unbounded per
//!    run by design (the whole point is the overnight catch-up); each row is
//!    still bounded by `max_retries` attempts, and `--limit` bounds the games
//!    considered.
//! 3. **Mirror** once: a single [`crate::mirror_ready_entries`] over the
//!    stored `Ready` set, with the run's `dry_run` flag.
//!
//! Pacing honesty: the runner invents no sleeps of its own. Probes read the
//! threaded [`PolitenessConfig`], the YouTube budget is shared across the
//! whole run, fetch retries stop at `max_retries` per row, and the mirror is
//! paced by [`MirrorConfig`] (`bwlimit`/`transfers`/`max-transfer`). The run
//! itself is sequential — one probe at a time — so no parallelism exists for
//! a config knob to govern.
//!
//! Season slugs (`1946-47` style) order by ending year
//! ([`season_slug_to_ending_year`]): `1946-47` → 1947, with the century
//! rollover (`1999-00` → 2000). The range is inclusive on both ends.
//!
//! Bounded smoke log 2026-09-08 (4 outbound requests total, temp paths only
//! — `--db`/`--manifest` under `/tmp/nbatv-smoke`, `NBA_TV_CACHE_DIR` at a
//! temp dir; nothing in the repo was touched):
//!
//! ```sh
//! cargo run -q -p nbatv_catalog --example seed_smoke -- /tmp/nbatv-smoke/archive.db
//! NBA_TV_CACHE_DIR=/tmp/nbatv-smoke/cache cargo run -q -p nbatv_catalog --bin nbatv-catalog-runner -- 1989-90 1989-90 \
//!   --db /tmp/nbatv-smoke/archive.db --manifest /tmp/nbatv-smoke/files-from.txt \
//!   --now 2026-09-08
//! # → backfill: games=1 swept=1 skipped=0 deferred=0 found=0 fetched_ready=0 fetched_failed=0
//! #   mirror=RemoteMissing { remote: "nbatv-drive" } dry_run=true (0.07s, 0 requests:
//! #   the 1990 Finals G5 wins rung 0 in the static catalog and stops; the rank-0
//! #   row is external-surface, never fetchable)
//! NBA_TV_CACHE_DIR=/tmp/nbatv-smoke/cache cargo run -q -p nbatv_catalog --bin nbatv-catalog-runner -- 1946-47 1946-47 \
//!   --db /tmp/nbatv-smoke/archive.db --manifest /tmp/nbatv-smoke/files-from.txt \
//!   --now 2026-09-08
//! # → backfill: games=1 swept=1 skipped=0 deferred=0 found=0 fetched_ready=0 fetched_failed=0
//! #   mirror=RemoteMissing { remote: "nbatv-drive" } dry_run=true (42.5s: rung 0
//! #   static miss, rung 1 IA live (1 inventory + 1 metadata fetch → REVIEW on a
//! #   1975 Finals item, never a tape row), rung 2 yt-dlp live search → reject,
//! #   rung 4 standing; no rung-3 probe yet)
//! curl -sS --fail -L -r 0-1023 -m 60 -A "nba-tv-runner-smoke/1.0 (personal archive research)" \
//!   -o /tmp/nbatv-smoke/range-probe.bin \
//!   -w 'http=%{http_code} bytes=%{size_download} redirects=%{num_redirects}\n' \
//!   'https://archive.org/download/1975-nba-finals-game-1/1975%20NBA%20Finals%20Game%201.mp4'
//! # → http=206 bytes=1024 redirects=1: the REVIEW candidate's bytes resume
//! #   cleanly at KB scale — the fetch transport shape, without downloading a
//! #   row that must stay human-reviewed.
//! ```
//!
//! Both runs ended in `RemoteMissing` (no Drive remote configured — the
//! acceptable smoke outcome): sweep and fetch still completed, no manifest
//! was written, no bytes moved. Re-run monthly against the stamp above; the
//! next live smoke after a Drive remote is configured doubles as the first
//! real upload preview.

use crate::drive::{
    mirror_ready_entries, MirrorConfig, MirrorError, MirrorOutcome, MirrorReport, RcloneMirror,
};
use crate::fetch::{
    cache_path, fetch_to_cache, src_tag_for_url, DurationProbe, FetchSpec, TapeFetcher,
};
use crate::politeness::PolitenessConfig;
use crate::probe::ProbeRegistry;
use crate::sweep::{game_context_for, sweep_game};
use nbatv_db::rusqlite::Connection;
use nbatv_db::PlaybackClass;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

// ---------------------------------------------------------------------------
// Season slugs
// ---------------------------------------------------------------------------

/// Ending year for a `YYYY-YY` season slug (`1946-47` → 1947). `None` for
/// anything else shaped — including a mismatched suffix (`1946-48`), so no
/// season ever maps to the wrong year silently.
pub fn season_slug_to_ending_year(slug: &str) -> Option<i32> {
    let bytes = slug.as_bytes();
    if bytes.len() != 7 || bytes[4] != b'-' {
        return None;
    }
    if !bytes[..4].iter().all(|c| c.is_ascii_digit())
        || !bytes[5..].iter().all(|c| c.is_ascii_digit())
    {
        return None;
    }
    let start: i32 = slug[..4].parse().ok()?;
    let suffix: i32 = slug[5..].parse().ok()?;
    // Century rollover: `1999-00` ends in 2000, not 1900.
    let century = start - start % 100;
    let mut ending = century + suffix;
    if ending <= start {
        ending += 100;
    }
    (ending == start + 1).then_some(ending)
}

/// The canonical slug for an ending year ([`season_slug_to_ending_year`]
/// parses it back): `1947` → `1946-47`, `2000` → `1999-00`.
pub fn ending_year_to_slug(year: i32) -> String {
    let start = year - 1;
    let suffix = year.rem_euclid(100);
    format!("{start}-{suffix:02}")
}

/// Ending years for an inclusive slug range, oldest first. Errors name the
/// bad endpoint (or the reversal) so drivers report usage, not empty runs.
pub fn expand_season_range(start: &str, end: &str) -> Result<Vec<i32>, RunnerError> {
    let first = season_slug_to_ending_year(start)
        .ok_or_else(|| RunnerError::BadSeason(format!("not a YYYY-YY season: {start:?}")))?;
    let last = season_slug_to_ending_year(end)
        .ok_or_else(|| RunnerError::BadSeason(format!("not a YYYY-YY season: {end:?}")))?;
    if first > last {
        return Err(RunnerError::BadSeason(format!(
            "reversed season range: {start:?} is after {end:?}"
        )));
    }
    Ok((first..=last).collect())
}

// ---------------------------------------------------------------------------
// Config + ports
// ---------------------------------------------------------------------------

/// What one backfill run covers. `season_start`/`season_end` are inclusive
/// `YYYY-YY` slugs; `now` is the `YYYY-MM-DD` sweep stamp; `dry_run`
/// previews the Drive upload (`--dry-run`, nothing leaves the machine) while
/// `false` applies it; `limit` caps the games swept (oldest first);
/// `max_retries` bounds fetch attempts per row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackfillConfig {
    pub season_start: String,
    pub season_end: String,
    pub now: String,
    pub dry_run: bool,
    pub limit: Option<usize>,
    pub max_retries: u32,
}

/// The seams one run drives: fakes in tests, live transports in the binary.
/// `cache_root` is the cache mount the fetch destinations hang under (tests
/// point it at a temp dir; the binary uses [`crate::fetch::cache_root`]).
pub struct BackfillPorts<'a> {
    pub probes: ProbeRegistry<'a>,
    pub politeness: PolitenessConfig,
    pub fetcher: &'a dyn TapeFetcher,
    pub duration: &'a dyn DurationProbe,
    pub mirror: &'a RcloneMirror,
    pub mirror_config: MirrorConfig,
    pub manifest_path: PathBuf,
    pub cache_root: PathBuf,
}

// ---------------------------------------------------------------------------
// Report + error
// ---------------------------------------------------------------------------

/// The full account of one backfill run, in stage order: how many games the
/// range held, how many swept vs. rescan-skipped, how many byte-class rows
/// surfaced, how the fetches settled, and the single mirror outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackfillReport {
    /// Games in the range considered (after `limit`).
    pub games: usize,
    /// Games whose sweep report probed at least one rung.
    pub swept: usize,
    /// Games whose sweep probed nothing and deferred nothing (`probed` and
    /// `deferred` both empty): every rung was inside its rescan window, so
    /// the sweep was honestly skipped.
    pub skipped: usize,
    /// Games with at least one deferred rung (spent quota or backoff): the
    /// deferring rung recorded nothing and MUST be retried next run, so
    /// these are counted separately, never as rescan-skipped.
    pub deferred: usize,
    /// Byte-class tape rows without a `Ready` entry that the fetch stage ran.
    pub found: usize,
    /// Fetches that verified `Ready`.
    pub fetched_ready: usize,
    /// Fetches that failed (transport or verification — never `Ready`).
    pub fetched_failed: usize,
    /// The single end-of-run mirror over the stored `Ready` set.
    pub mirror: MirrorReport,
}

impl BackfillReport {
    /// The per-stage summary the binary prints for the overnight log:
    /// `swept/found/fetched/mirrored/skipped`, all named.
    pub fn summary(&self) -> String {
        format!(
            "backfill: games={} swept={} skipped={} deferred={} found={} fetched_ready={} fetched_failed={} mirror={:?} dry_run={}",
            self.games,
            self.swept,
            self.skipped,
            self.deferred,
            self.found,
            self.fetched_ready,
            self.fetched_failed,
            self.mirror.outcome,
            self.mirror.dry_run,
        )
    }
}

/// What can go wrong OUTSIDE a run's report: a bad season range, a bad
/// timestamp, an unreadable archive, or an unwritable manifest. Fetch
/// failures and a missing Drive remote are NOT errors — they live in the
/// report, honestly named.
#[derive(Debug)]
pub enum RunnerError {
    BadSeason(String),
    BadNow(String),
    Db(nbatv_db::rusqlite::Error),
    Mirror(MirrorError),
}

impl std::fmt::Display for RunnerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RunnerError::BadSeason(reason) => write!(f, "backfill: bad season range: {reason}"),
            RunnerError::BadNow(now) => write!(f, "backfill: bad timestamp: {now:?}"),
            RunnerError::Db(err) => write!(f, "backfill: archive database error: {err}"),
            RunnerError::Mirror(err) => write!(f, "backfill: {err}"),
        }
    }
}

impl std::error::Error for RunnerError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            RunnerError::BadSeason(_) | RunnerError::BadNow(_) => None,
            RunnerError::Db(err) => Some(err),
            RunnerError::Mirror(err) => Some(err),
        }
    }
}

impl From<nbatv_db::rusqlite::Error> for RunnerError {
    fn from(err: nbatv_db::rusqlite::Error) -> Self {
        RunnerError::Db(err)
    }
}

impl From<MirrorError> for RunnerError {
    fn from(err: MirrorError) -> Self {
        RunnerError::Mirror(err)
    }
}

// ---------------------------------------------------------------------------
// Orchestration: sweep → fetch → mirror
// ---------------------------------------------------------------------------

/// Sweep, fetch, and mirror one season range headlessly.
///
/// Loads every game in the inclusive slug range (season, then date, then
/// game; `limit` keeps the oldest N), sweeps each through `ports.probes`
/// with the shared politeness config and quota, fetches every byte-class
/// tape row still missing its `Ready` entry, then mirrors the stored `Ready`
/// set once with the run's `dry_run` flag.
pub fn run_backfill(
    conn: &Connection,
    config: &BackfillConfig,
    ports: &BackfillPorts<'_>,
) -> Result<BackfillReport, RunnerError> {
    let years = expand_season_range(&config.season_start, &config.season_end)?;
    let mut games = Vec::new();
    for year in years {
        games.extend(nbatv_db::games_in_season(conn, year)?);
    }
    if let Some(limit) = config.limit {
        games.truncate(limit);
    }

    // One quota for the whole run: the per-sweep-day shape the politeness
    // config documents, so the YouTube budget paces the night, not one game.
    let mut quota = ports.politeness.youtube_quota();

    let mut swept = 0usize;
    let mut skipped = 0usize;
    let mut deferred = 0usize;
    for row in &games {
        let Some(game) = game_context_for(conn, &row.game_id)? else {
            // The row vanished mid-run: nothing to sweep, honestly skipped.
            skipped += 1;
            continue;
        };
        let report = sweep_game(
            conn,
            &game,
            &ports.probes,
            &ports.politeness,
            &mut quota,
            &config.now,
        )
        .map_err(|err| match err {
            crate::sweep::SweepError::BadNow(now) => RunnerError::BadNow(now),
            crate::sweep::SweepError::Db(err) => RunnerError::Db(err),
        })?;
        // The skip rule: a game is rescan-skipped only when nothing was
        // probed AND nothing deferred — a deferred rung records nothing and
        // must be retried next run, so it is counted separately (no second
        // freshness implementation; both come off the report).
        if !report.deferred.is_empty() {
            deferred += 1;
        } else if report.probed.is_empty() {
            skipped += 1;
        } else {
            swept += 1;
        }
    }

    let mut found = 0usize;
    let mut fetched_ready = 0usize;
    let mut fetched_failed = 0usize;
    for row in &games {
        for tape in nbatv_db::tape_sources_for(conn, &row.game_id)? {
            if PlaybackClass::of_rank(tape.rank) != Some(PlaybackClass::ProgressiveFile) {
                continue;
            }
            if nbatv_db::ready_cache_entry_for(conn, &row.game_id)?.is_some() {
                continue;
            }
            found += 1;
            let season_slug = ending_year_to_slug(row.season);
            let dest = cache_path(
                &ports.cache_root,
                &season_slug,
                &row.game_id,
                &row.away_team,
                &row.home_team,
                src_tag_for_url(&tape.url_or_pointer),
                "mp4",
            );
            let spec = FetchSpec {
                game_id: nbatv_db::GameId(row.game_id.clone()),
                rank: tape.rank,
                source_class: tape.source_class.clone(),
                url: tape.url_or_pointer.clone(),
                dest,
                max_retries: config.max_retries.max(1),
            };
            match fetch_to_cache(conn, &spec, ports.fetcher, ports.duration, &config.now)? {
                crate::fetch::CacheFetchReport::Ready { .. } => fetched_ready += 1,
                crate::fetch::CacheFetchReport::Failed { .. } => fetched_failed += 1,
            }
        }
    }

    let mirror = mirror_ready_entries(
        conn,
        &ports.mirror_config,
        ports.mirror,
        &ports.manifest_path,
        config.dry_run,
    )?;

    Ok(BackfillReport {
        games: games.len(),
        swept,
        skipped,
        deferred,
        found,
        fetched_ready,
        fetched_failed,
        mirror,
    })
}

// ---------------------------------------------------------------------------
// Argv: the tiny clap-free surface
// ---------------------------------------------------------------------------

/// Default archive path (the Shell's live database).
pub const DEFAULT_DB_PATH: &str = "data/archive.db";
/// Default Drive manifest path (under the gitignored Cache Tier dir —
/// generated artifact, never committed).
pub const DEFAULT_MANIFEST_PATH: &str = "data/cache/drive-manifest.txt";
/// Fetch attempts per row when `--max-retries` is absent.
pub const DEFAULT_MAX_RETRIES: u32 = 3;

/// Exit codes for the binary: `0` OK, `1` USAGE (unusable argv), `2` RUN
/// (the run itself failed). See [`exit_code_for_report`].
pub const EXIT_OK: i32 = 0;
pub const EXIT_USAGE: i32 = 1;
pub const EXIT_RUN: i32 = 2;

/// The process exit code for a completed backfill report (ticket #27 gap,
/// fixed): a run is not "successful" when it uploaded nothing.
///
/// * `--dry-run` runs always exit [`EXIT_OK`] — nothing was supposed to
///   move; fetch failures and a missing remote are report content.
/// * Under `--apply`, a missing Drive remote ([`MirrorOutcome::RemoteMissing`])
///   or any non-completed mirror outcome, as well as any fetch failure,
///   exit [`EXIT_RUN`]: an overnight apply that uploads nothing or drops
///   rows must page the driver, not look green. Everything else is
///   [`EXIT_OK`].
pub fn exit_code_for_report(report: &BackfillReport) -> i32 {
    if report.mirror.dry_run {
        return EXIT_OK;
    }
    let fetch_failed = report.fetched_failed > 0;
    let mirror_failed = !matches!(report.mirror.outcome, MirrorOutcome::Completed { .. });
    if fetch_failed || mirror_failed {
        EXIT_RUN
    } else {
        EXIT_OK
    }
}

/// The parsed binary argv: the season range plus the run knobs. `dry_run` is
/// true unless `--apply` is given — bytes never leave the machine by
/// accident.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunnerArgs {
    pub season_start: String,
    pub season_end: String,
    pub now: String,
    pub db_path: PathBuf,
    pub manifest_path: PathBuf,
    pub dry_run: bool,
    pub limit: Option<usize>,
    pub max_retries: u32,
}

impl RunnerArgs {
    /// The library config this argv asks for.
    pub fn backfill_config(&self) -> BackfillConfig {
        BackfillConfig {
            season_start: self.season_start.clone(),
            season_end: self.season_end.clone(),
            now: self.now.clone(),
            dry_run: self.dry_run,
            limit: self.limit,
            max_retries: self.max_retries,
        }
    }
}

/// Usage text for `--help` and usage errors.
pub fn usage() -> String {
    format!(
        "usage: nbatv-catalog-runner <season-start> <season-end> [options]\n\
         \n\
         Sweep, fetch, and mirror one inclusive season range headlessly.\n\
         Seasons are YYYY-YY slugs (1946-47..1950-51).\n\
         \n\
         options:\n\
         \u{20} --now DATE        sweep stamp YYYY-MM-DD (default: today)\n\
         \u{20} --db PATH         archive database (default: {DEFAULT_DB_PATH})\n\
         \u{20} --manifest PATH   drive manifest (default: {DEFAULT_MANIFEST_PATH})\n\
         \u{20} --dry-run         preview the upload, move nothing (default)\n\
         \u{20} --apply           perform the upload\n\
         \u{20} --limit N         sweep at most N games, oldest first\n\
         \u{20} --max-retries N   fetch attempts per row (default: {DEFAULT_MAX_RETRIES})\n\
         \u{20} --help            print this text\n"
    )
}

/// Parse the binary argv (including `argv[0]`). `Err` is a usage error: the
/// caller prints it with [`usage`] and exits [`EXIT_USAGE`].
pub fn parse_argv(argv: &[String]) -> Result<RunnerArgs, String> {
    let mut positional: Vec<String> = Vec::new();
    let mut now: Option<String> = None;
    let mut db_path = PathBuf::from(DEFAULT_DB_PATH);
    let mut manifest_path = PathBuf::from(DEFAULT_MANIFEST_PATH);
    let mut dry_run = true;
    let mut limit: Option<usize> = None;
    let mut max_retries = DEFAULT_MAX_RETRIES;

    let mut rest = argv.iter().skip(1).peekable();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--help" | "-h" => return Err(usage()),
            "--dry-run" => dry_run = true,
            "--apply" => dry_run = false,
            "--now" => {
                let raw = rest
                    .next()
                    .cloned()
                    .ok_or_else(|| "--now needs a DATE".to_owned())?;
                if raw.starts_with("--") {
                    return Err(format!("--now needs a DATE, got flag {raw:?}"));
                }
                now = Some(raw);
            }
            "--db" => {
                let raw = rest
                    .next()
                    .cloned()
                    .ok_or_else(|| "--db needs a PATH".to_owned())?;
                if raw.starts_with("--") {
                    return Err(format!("--db needs a PATH, got flag {raw:?}"));
                }
                db_path = PathBuf::from(raw);
            }
            "--manifest" => {
                let raw = rest
                    .next()
                    .cloned()
                    .ok_or_else(|| "--manifest needs a PATH".to_owned())?;
                if raw.starts_with("--") {
                    return Err(format!("--manifest needs a PATH, got flag {raw:?}"));
                }
                manifest_path = PathBuf::from(raw);
            }
            "--limit" => {
                let raw = rest
                    .next()
                    .cloned()
                    .ok_or_else(|| "--limit needs an N".to_owned())?;
                let parsed: usize = raw
                    .parse()
                    .map_err(|_| format!("--limit needs a positive N, got {raw:?}"))?;
                if parsed == 0 {
                    return Err("--limit must be at least 1".to_owned());
                }
                limit = Some(parsed);
            }
            "--max-retries" => {
                let raw = rest
                    .next()
                    .cloned()
                    .ok_or_else(|| "--max-retries needs an N".to_owned())?;
                let parsed: u32 = raw
                    .parse()
                    .map_err(|_| format!("--max-retries needs an N, got {raw:?}"))?;
                if parsed == 0 {
                    return Err("--max-retries must be at least 1".to_owned());
                }
                max_retries = parsed;
            }
            flag if flag.starts_with("--") => {
                return Err(format!("unknown flag {flag:?}\n\n{}", usage()));
            }
            value => positional.push(value.to_owned()),
        }
    }
    if positional.len() != 2 {
        return Err(format!(
            "need exactly 2 season slugs, got {}\n\n{}",
            positional.len(),
            usage()
        ));
    }
    // Validate the range now so usage errors never open the database.
    expand_season_range(&positional[0], &positional[1]).map_err(|err| match err {
        RunnerError::BadSeason(reason) => format!("{reason}\n\n{}", usage()),
        other => other.to_string(),
    })?;
    Ok(RunnerArgs {
        season_start: positional[0].clone(),
        season_end: positional[1].clone(),
        now: now.unwrap_or_else(today_ymd),
        db_path,
        manifest_path,
        dry_run,
        limit,
        max_retries,
    })
}

/// Today's `YYYY-MM-DD` in UTC (std-only days-from-civil inverse): the
/// default `--now` stamp.
fn today_ymd() -> String {
    let days = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() / 86_400)
        .unwrap_or(0) as i64;
    let (year, month, day) = civil_from_days(days + 719_468);
    format!("{year:04}-{month:02}-{day:02}")
}

/// Howard Hinnant's civil-from-days inverse via [`crate::civil`] (std-only,
/// no date dependency). Takes Hinnant's `z` form (the epoch shift already
/// applied) so the call site stays unchanged.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    crate::civil::civil_from_days(z - 719_468)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn today_stamp_is_a_parseable_date() {
        let today = today_ymd();
        assert_eq!(today.len(), 10, "YYYY-MM-DD: {today}");
        assert_eq!(&today[4..5], "-");
        assert_eq!(&today[7..8], "-");
        // civil_from_days inverts the sweep's days-from-civil: the stamp the
        // runner defaults to must satisfy the sweep's own date parser shape.
        assert!(today[..4].parse::<i32>().is_ok());
        assert!(today[5..7].parse::<u32>().is_ok());
        assert!(today[8..10].parse::<u32>().is_ok());
    }

    #[test]
    fn civil_roundtrip_matches_known_dates() {
        assert_eq!(civil_from_days(days_to_z(2026, 9, 8)), (2026, 9, 8));
        assert_eq!(civil_from_days(days_to_z(1946, 11, 1)), (1946, 11, 1));
        assert_eq!(civil_from_days(days_to_z(2000, 1, 1)), (2000, 1, 1));
    }

    fn days_to_z(year: i64, month: i64, day: i64) -> i64 {
        let y = if month <= 2 { year - 1 } else { year };
        let era = if y >= 0 { y } else { y - 399 } / 400;
        let yoe = y - era * 400;
        let doy = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146097 + doe - 719468 + 719_468
    }
}
