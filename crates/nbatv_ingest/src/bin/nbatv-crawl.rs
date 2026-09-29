//! `nbatv-crawl`: build the full BR snapshot crawl, one season at a time.
//!
//! The one command that grows `data/raw/br/<season>/` (schedule page, box
//! pages, monthly splits, totals page) for every season in a range, ready
//! for `nbatv-ingest`. Per-season state is derived from disk before every
//! attempt, so the run is resume-safe: kill it anywhere and re-run the same
//! command — snapshots already on disk are skipped without network traffic
//! (see `crawl::crawl_season`).
//!
//! ```sh
//! # What would be requested right now (no network):
//! cargo run -q -p nbatv_ingest --bin nbatv-crawl -- --dry-run
//!
//! # The full archive, 1946-47 through 2025-26 (multi days at 3.5 s/request):
//! cargo run -q -p nbatv_ingest --bin nbatv-crawl
//! ```
//!
//! Transport: a `curl` subprocess with an identifying UA, no compression
//! (the pipeline stores its own gzip snapshots). HTTP 404/410 pages are
//! dead-pooled for the run (reported as `missing`); 429/503 answers come
//! back as [`FetchError::Throttled`] and the driver backs off
//! exponentially; other failures retry politely and give the season up
//! after three fruitless attempts (the error is named in the season line).
//! Exit codes: 0 all seasons complete, 1 at least one season failed, 2 bad
//! usage.
//!
//! Etiquette: requests are spaced by `--interval` (default 3.5 s; BR
//! robots.txt sets Crawl-delay 3) and seasons run `--workers` at a time, so
//! the aggregate rate is workers / interval. At the default the full
//! 80-season archive is ~77k requests — days; lower the interval or raise
//! the worker count to trade throttling risk for speed (your call, your
//! IP — measured 2026-09-08: ~2 req/s sustained drew 429s within minutes).
//! Use `--from`/`--to` to run it in waves; every wave resumes where the
//! last stopped.

use nbatv_catalog::{DirectHttpJev, JevJudge};
use nbatv_ingest::crawl::{
    crawl_season_with_jev, pending_jobs, season_slug, SeasonCrawl, FIRST_SEASON,
    LAST_COMPLETED_SEASON,
};
use nbatv_ingest::{raw_snapshot_path, FetchClient, FetchError};
use std::collections::BTreeSet;
use std::io::Write as _;
use std::path::PathBuf;
use std::process::{Command, ExitCode};
use std::sync::Arc;
use std::time::Duration;

const EXIT_OK: u8 = 0;
const EXIT_RUN: u8 = 1;
const EXIT_USAGE: u8 = 2;

const DEFAULT_ROOT: &str = ".";
const SOURCE: &str = "br";

/// Identifying contact UA (the repo's private-research posture; BR's
/// data-use policy asks automated access to be identifiable and gentle).
const CRAWL_UA: &str = "nba-tv-archive-crawl/0.1 (personal research; contact local)";

/// Request timeout for one curl attempt.
const CURL_MAX_TIME_SECS: u64 = 60;

struct Args {
    from: i32,
    to: i32,
    monthly: bool,
    dry_run: bool,
    root: PathBuf,
    /// Minimum seconds between two requests (per worker).
    interval_secs: f64,
    /// Seasons crawled concurrently. The aggregate request rate is
    /// workers / interval — you own the decision to exceed BR's
    /// robots.txt Crawl-delay 3.
    workers: usize,
}

fn usage() -> String {
    format!(
        "usage: nbatv-crawl [--from ENDING] [--to ENDING] [--no-monthly] [--dry-run] [--root DIR] [--interval SECS] [--workers N]\n  --from ENDING     first season-ending year (default {FIRST_SEASON} = 1946-47)\n  --to ENDING       last season-ending year (default {LAST_COMPLETED_SEASON} = 2025-26)\n  --no-monthly      skip monthly schedule split pages (redundant with the full page)\n  --dry-run         list pending requests and pacing estimate, no network\n  --root DIR        project root holding data/ (default {DEFAULT_ROOT}); snapshots land at DIR/data/raw/br/<season>/\n  --interval SECS   minimum seconds between requests per worker (default 3.5 = BR robots.txt Crawl-delay 3 + margin; lower is your call, throttling risk is yours)\n  --workers N       seasons crawled concurrently (default 1; aggregate rate = N / interval)"
    )
}

fn parse_season_ending(flag: &str, value: &str) -> Result<i32, String> {
    value
        .parse::<i32>()
        .map_err(|_| format!("{flag}: {value:?} is not a year\n{}", usage()))
}

fn parse_argv(argv: &[String]) -> Result<Args, String> {
    let mut args = Args {
        from: FIRST_SEASON,
        to: LAST_COMPLETED_SEASON,
        monthly: true,
        dry_run: false,
        root: PathBuf::from(DEFAULT_ROOT),
        interval_secs: 3.5,
        workers: 1,
    };
    let mut values = argv.iter();
    while let Some(arg) = values.next() {
        let flag = arg.as_str();
        match flag {
            "--from" | "--to" | "--root" | "--interval" | "--workers" => {
                let Some(value) = values.next() else {
                    return Err(format!("{flag} needs a value\n{}", usage()));
                };
                if value.starts_with("--") {
                    return Err(format!(
                        "{flag} needs a value (a lone -- flag is not a value)"
                    ));
                }
                match flag {
                    "--from" => args.from = parse_season_ending(flag, value)?,
                    "--to" => args.to = parse_season_ending(flag, value)?,
                    "--root" => args.root = PathBuf::from(value),
                    "--interval" => {
                        args.interval_secs = value.parse::<f64>().map_err(|_| {
                            format!("{flag}: {value:?} is not a number of seconds\n{}", usage())
                        })?;
                    }
                    "--workers" => {
                        args.workers = value.parse::<usize>().map_err(|_| {
                            format!("{flag}: {value:?} is not a worker count\n{}", usage())
                        })?;
                    }
                    _ => unreachable!("flag matched the takes-value set above"),
                }
            }
            "--no-monthly" => args.monthly = false,
            "--dry-run" => args.dry_run = true,
            _ => return Err(format!("unknown argument {flag:?}\n{}", usage())),
        }
    }
    if args.from < FIRST_SEASON {
        return Err(format!(
            "--from {}: no BR league-year pages before {FIRST_SEASON} (1946-47)\n{}",
            args.from,
            usage()
        ));
    }
    if args.to < args.from {
        return Err(format!(
            "--to {} is before --from {}\n{}",
            args.to,
            args.from,
            usage()
        ));
    }
    if args.interval_secs <= 0.0 {
        return Err(format!(
            "--interval {} must be positive\n{}",
            args.interval_secs,
            usage()
        ));
    }
    if args.workers == 0 || args.workers > 8 {
        return Err(format!(
            "--workers {} must be 1..=8\n{}",
            args.workers,
            usage()
        ));
    }
    Ok(args)
}

/// Live [`FetchClient`]: `curl -sS --fail-with-body -L` with the archive
/// contact UA, page body on stdout, and the HTTP status appended via `-w`.
/// `--fail-with-body` preserves 403/5xx evidence; `--max-filesize` bounds
/// the captured body before `Command::output()` buffers it.
struct CurlClient {
    curl_bin: String,
    user_agent: String,
    max_time_secs: u64,
}

impl CurlClient {
    fn live() -> Self {
        CurlClient {
            curl_bin: "curl".to_owned(),
            user_agent: CRAWL_UA.to_owned(),
            max_time_secs: CURL_MAX_TIME_SECS,
        }
    }
}

const CURL_MAX_RESPONSE_BYTES: u64 = 16 * 1024 * 1024;

fn curl_command(curl_bin: &str, user_agent: &str, max_time_secs: u64, url: &str) -> Command {
    let mut command = Command::new(curl_bin);
    command
        .args(["-sS", "--fail-with-body", "-L"])
        .arg("--max-time")
        .arg(max_time_secs.to_string())
        .arg("--max-filesize")
        .arg(CURL_MAX_RESPONSE_BYTES.to_string())
        .arg("-A")
        .arg(user_agent)
        .arg("-o")
        .arg("-")
        .arg("-w")
        .arg("\n%{http_code}")
        .arg(url);
    command
}

impl FetchClient for CurlClient {
    fn fetch(&self, url: &str) -> Result<String, FetchError> {
        let output = curl_command(&self.curl_bin, &self.user_agent, self.max_time_secs, url)
            .output()
            .map_err(|e| FetchError::Client(format!("curl for {url}: {e}")))?;
        decode_curl_output(url, output.status.code().unwrap_or(-1), &output.stdout)
    }
}

/// curl's output (page body + trailing `-w` status line) → page text.
/// Exact 404/410 and 429/503 handling preempts the optional classifier.
/// Other access/policy responses and 5xx responses retain bounded body
/// evidence in [`FetchError::Unclassified`]; transport failures stay typed
/// as [`FetchError::Client`].
fn decode_curl_output(url: &str, exit: i32, stdout: &[u8]) -> Result<String, FetchError> {
    let text = String::from_utf8_lossy(stdout);
    let text: &str = &text;
    let (body, status) = text.rsplit_once('\n').unwrap_or((text, ""));
    let status = status.trim();
    if status == "404" || status == "410" {
        return Err(FetchError::NotFound(url.to_owned()));
    }
    if status == "429" || status == "503" {
        return Err(FetchError::Throttled(format!("HTTP {status} for {url}")));
    }
    if exit == 0 && (status.starts_with('2') || status.starts_with('3')) {
        return Ok(body.to_owned());
    }
    if status.is_empty() {
        return Err(FetchError::Client(format!(
            "curl exited {exit} for {url} (no status line)"
        )));
    }
    let status_code = status.parse::<u16>().ok();
    if matches!(status_code, Some(401 | 403 | 418) | Some(500..=599)) {
        return Err(FetchError::Unclassified {
            status: status_code,
            evidence: body.chars().take(4096).collect(),
        });
    }
    Err(FetchError::Client(format!(
        "HTTP {status} for {url} (curl exited {exit})"
    )))
}

fn one_line(summary: &SeasonCrawl) -> String {
    // Access/policy and 5xx responses remain deterministic crawl
    // failures; Jev may classify the bounded evidence for manual review.
    let mut line = format!(
        "{}: +{} fetched (boxes {}, monthly {}, totals {}), missing {}",
        summary.slug,
        summary.fetched_total(),
        summary.boxes.fetched.len(),
        summary.monthly.fetched.len(),
        summary.totals.fetched.len(),
        summary.dead.len(),
    );
    if let Some(classified) = &summary.classified_failure {
        line.push_str(&format!(" — Jev={:?}", classified.choice));
    }
    if let Some(e) = &summary.error {
        line.push_str(&format!(" — FAILED: {e}"));
    }
    line
}

fn print_line(line: &str) {
    println!("{line}");
    let _ = std::io::stdout().flush();
}

fn dry_run(args: &Args) -> Result<(), String> {
    let empty_dead = BTreeSet::new();
    let mut total = 0usize;
    for ending in args.from..=args.to {
        let slug = season_slug(ending);
        let season_dir = {
            let mut dir = args
                .root
                .join(raw_snapshot_path(SOURCE, &slug, "_games.html"));
            dir.pop();
            dir
        };
        let jobs = pending_jobs(&season_dir, ending, args.monthly, &empty_dead)
            .map_err(|e| format!("{slug}: {e}"))?;
        let boxes = jobs
            .iter()
            .filter(|j| !j.file_name.starts_with('_'))
            .count();
        let split_like = jobs.len() - boxes;
        println!(
            "{slug}: {} pending (phase-one/splits {split_like}, boxes {boxes})",
            jobs.len()
        );
        total += jobs.len();
    }
    let hours = total as f64 * args.interval_secs / args.workers.max(1) as f64 / 3600.0;
    println!(
        "TOTAL: {total} requests ≈ {hours:.1} h at {:.1} s/request × {} worker(s)",
        args.interval_secs, args.workers
    );
    Ok(())
}

fn run(args: &Args) -> Result<(), String> {
    let interval = Duration::from_secs_f64(args.interval_secs);
    let endings: Vec<i32> = (args.from..=args.to).collect();
    let judge = match DirectHttpJev::from_env() {
        Ok(judge) => Some(Arc::new(judge)),
        Err(e) => {
            eprintln!("Jev disabled; deterministic crawl will continue: {e}");
            None
        }
    };
    // Season-level parallelism: exactly `workers` threads, each taking a
    // contiguous block of seasons, one season batch at a time each
    // (aggregate request rate = workers / interval).
    let workers = args.workers.min(endings.len()).max(1);
    let chunk_len = endings.len().div_ceil(workers);
    let failed = std::thread::scope(|scope| {
        let mut handles = Vec::new();
        for chunk in endings.chunks(chunk_len) {
            let chunk: Vec<i32> = chunk.to_vec();
            let judge = judge.clone();
            handles.push(scope.spawn(move || {
                let client = CurlClient::live();
                let mut failed = 0usize;
                for ending in chunk {
                    let summary = crawl_season_with_jev(
                        &client,
                        SOURCE,
                        ending,
                        &args.root,
                        args.monthly,
                        interval,
                        judge.as_deref().map(|judge| judge as &dyn JevJudge),
                        &mut std::thread::sleep,
                    );
                    if summary.error.is_some() {
                        failed += 1;
                    }
                    print_line(&one_line(&summary));
                }
                failed
            }));
        }
        handles
            .into_iter()
            .map(|h| h.join().unwrap_or(1))
            .sum::<usize>()
    });
    if failed > 0 {
        return Err(format!(
            "{failed} season(s) failed — re-run the same command to retry (resume skips completed work)"
        ));
    }
    Ok(())
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = match parse_argv(&argv) {
        Ok(args) => args,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(EXIT_USAGE);
        }
    };
    let result = if args.dry_run {
        dry_run(&args)
    } else {
        run(&args)
    };
    match result {
        Ok(()) => ExitCode::from(EXIT_OK),
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(EXIT_RUN)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_curl_preserves_http_error_bodies() {
        let command = curl_command("curl", "archive-contact", 30, "https://example.test/game");
        let args = command
            .get_args()
            .map(|arg| arg.to_string_lossy())
            .collect::<Vec<_>>();
        assert!(args.iter().any(|arg| arg == "--fail-with-body"), "{args:?}");
        assert!(args.iter().any(|arg| arg == "--max-filesize"), "{args:?}");
        assert!(
            args.iter()
                .any(|arg| arg == &CURL_MAX_RESPONSE_BYTES.to_string()),
            "{args:?}"
        );
        assert!(!args.iter().any(|arg| arg == "--fail"), "{args:?}");
    }

    #[test]
    fn curl_output_decode_names_the_page_404_throttle_and_status_failures() {
        // Happy path: body + trailing status line.
        assert_eq!(
            decode_curl_output("u", 0, b"<html>page</html>\n200").unwrap(),
            "<html>page</html>"
        );
        // 404/410 → NotFound regardless of exit code.
        assert!(matches!(
            decode_curl_output("u", 22, b"\n404"),
            Err(FetchError::NotFound(_))
        ));
        assert!(matches!(
            decode_curl_output("u", 0, b"gone\n410"),
            Err(FetchError::NotFound(_))
        ));
        // 429/503 → Throttled (the driver backs off instead of stall-failing).
        assert!(matches!(
            decode_curl_output("u", 22, b"\n429"),
            Err(FetchError::Throttled(_))
        ));
        assert!(matches!(
            decode_curl_output("u", 22, b"\n503"),
            Err(FetchError::Throttled(_))
        ));
        // Other policy-sensitive HTTP responses retain bounded evidence for
        // the optional typed classifier.
        let err = decode_curl_output("u", 22, b"blocked by policy\n403").unwrap_err();
        assert!(matches!(
            err,
            FetchError::Unclassified {
                status: Some(403),
                ref evidence
            } if evidence == "blocked by policy"
        ));
        // Transport failure without a status line.
        let err = decode_curl_output("u", 7, b"").unwrap_err();
        assert!(err.to_string().contains("exited 7"), "{err}");
        // Multi-line bodies survive: only the LAST line is the status.
        assert_eq!(
            decode_curl_output("u", 0, b"<a>\n<b>\n200").unwrap(),
            "<a>\n<b>"
        );
    }

    #[test]
    fn argv_parses_ranges_flags_and_rejects_bad_order() {
        let ok =
            parse_argv(&["--from".into(), "1980".into(), "--to".into(), "1989".into()]).unwrap();
        assert_eq!((ok.from, ok.to), (1980, 1989));
        assert!(ok.monthly && !ok.dry_run);

        let ok = parse_argv(&["--no-monthly".into(), "--dry-run".into()]).unwrap();
        assert!(!ok.monthly && ok.dry_run);

        let ok = parse_argv(&[
            "--interval".into(),
            "1.5".into(),
            "--workers".into(),
            "2".into(),
        ])
        .unwrap();
        assert_eq!(ok.interval_secs, 1.5);
        assert_eq!(ok.workers, 2);

        assert!(parse_argv(&["--from".into(), "1946".into()]).is_err());
        assert!(
            parse_argv(&["--to".into(), "1950".into(), "--from".into(), "1980".into()]).is_err()
        );
        assert!(parse_argv(&["--interval".into(), "0".into()]).is_err());
        assert!(parse_argv(&["--workers".into(), "9".into()]).is_err());
        assert!(parse_argv(&["--bogus".into()]).is_err());
        assert!(parse_argv(&["--from".into()]).is_err());
    }
}
