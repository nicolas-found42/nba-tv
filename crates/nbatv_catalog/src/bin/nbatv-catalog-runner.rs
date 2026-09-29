//! `nbatv-catalog-runner`: the headless overnight backfill (issue #27).
//!
//! Thin argv wrapper over [`nbatv_catalog::run_backfill`]: std only, no new
//! dependencies. Parses the season range with [`nbatv_catalog::parse_argv`],
//! opens (creating) the archive database, registers the LIVE probe set
//! (rung 0 NBA catalog, rung 1 Internet Archive, rung 2 yt-dlp sidecar, rung
//! 4 standing empty-corpus — rung 3 has no probe and stays unswept until one
//! lands), and runs sweep → fetch → one dry-run/apply mirror behind the live
//! `curl`/`ffprobe`/`rclone` transports.
//!
//! Exit codes ([`nbatv_catalog::exit_code_for_report`] computes it from the
//! report; [`nbatv_catalog::EXIT_USAGE`] covers unusable argv, and any
//! `run_backfill` error is RUN):
//!
//! - `0` OK: the run completed. Under `--dry-run` (the default) this is
//!   every completed run — fetch failures and a missing Drive remote live
//!   in the printed summary, honestly named, because nothing was supposed
//!   to move.
//! - `1` USAGE: the argv was unusable.
//! - `2` RUN: the run itself failed. Under `--apply` this fires when the
//!   mirror did not complete — including `RemoteMissing`, so an overnight
//!   apply that uploads nothing pages the driver instead of looking green —
//!   or when any fetch failed, or when `run_backfill` errored.
//!
//! YouTube quota note: the rung-2 budget is per-run. `run_backfill` mints
//! one fresh [`nbatv_catalog::YoutubeQuota`] per process (from the
//! politeness config's daily limit); nothing is persisted across runs, so
//! each invocation gets the full daily budget and running the binary twice
//! in a day may spend up to twice the single-run limit.

use nbatv_catalog::{
    exit_code_for_report, fetch, parse_argv, run_backfill, usage, BackfillPorts, FfprobeDuration,
    IaProbe, MirrorConfig, NbaProbe, PolitenessConfig, ProbeRegistry, RcloneMirror, Rung4Probe,
    YtdlpProbe, EXIT_RUN, EXIT_USAGE,
};
use std::path::Path;

fn main() {
    let argv: Vec<String> = std::env::args().collect();
    if argv.iter().any(|a| a == "--help" || a == "-h") {
        print!("{}", usage());
        return;
    }
    let args = match parse_argv(&argv) {
        Ok(args) => args,
        Err(reason) => {
            eprintln!("{reason}");
            if !reason.contains("usage:") {
                eprintln!("{}", usage());
            }
            std::process::exit(EXIT_USAGE);
        }
    };
    let report = match run(&args.db_path, &args.manifest_path, &args) {
        Ok(report) => report,
        Err(reason) => {
            eprintln!("nbatv-catalog-runner: {reason}");
            std::process::exit(EXIT_RUN);
        }
    };
    println!("{}", report.summary());
    let mirror_config = MirrorConfig::default();
    if let nbatv_catalog::MirrorOutcome::RemoteMissing { remote } = &report.mirror.outcome {
        let prompt = nbatv_catalog::drive_sign_in_prompt(&mirror_config);
        println!("{prompt} (missing remote: {remote})");
    }
    std::process::exit(exit_code_for_report(&report));
}

fn run(
    db_path: &Path,
    manifest_path: &Path,
    args: &nbatv_catalog::RunnerArgs,
) -> Result<nbatv_catalog::BackfillReport, String> {
    if let Some(parent) = db_path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .map_err(|err| format!("cannot create archive dir {}: {err}", parent.display()))?;
        }
    }
    let conn = nbatv_db::open(db_path)
        .map_err(|err| format!("cannot open archive {}: {err}", db_path.display()))?;
    nbatv_db::create_schema(&conn).map_err(|err| format!("cannot create schema: {err}"))?;

    // Live probe set: one probe per rung with an implementation. Rung 3 has
    // none yet — its rungs stay unswept (Sweeping, never absent) until one
    // lands, exactly as the sweep documents.
    let nba = NbaProbe::new();
    let ia = IaProbe::live();
    let ytdlp = YtdlpProbe::new();
    let rung4 = Rung4Probe::new();
    let mut registry = ProbeRegistry::new();
    registry
        .register(&nba)
        .register(&ia)
        .register(&ytdlp)
        .register(&rung4);

    let fetcher = fetch::CurlFetcher::live();
    let duration = FfprobeDuration::live();
    let mirror = RcloneMirror::live();
    let ports = BackfillPorts {
        probes: registry,
        politeness: PolitenessConfig::default(),
        fetcher: &fetcher,
        duration: &duration,
        mirror: &mirror,
        mirror_config: MirrorConfig::default(),
        manifest_path: manifest_path.to_owned(),
        cache_root: fetch::cache_root(),
    };

    run_backfill(&conn, &args.backfill_config(), &ports).map_err(|err| err.to_string())
}
