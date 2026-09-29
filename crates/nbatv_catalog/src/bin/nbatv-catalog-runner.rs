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
//! Exit codes: `0` the run completed (fetch failures and a missing Drive
//! remote live in the printed summary, honestly named), `1` a usage error,
//! `2` the run itself failed.

use nbatv_catalog::{
    fetch, parse_argv, review_list_all_with_judge, run_backfill_with_judge, usage, BackfillPorts,
    DirectHttpJev, DisabledJevJudge, FfprobeDuration, IaProbe, JevError, JevJudge, MirrorConfig,
    NbaProbe, PolitenessConfig, ProbeRegistry, RcloneMirror, Rung4Probe, YtdlpProbe, EXIT_OK,
    EXIT_RUN, EXIT_USAGE,
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
    if let Err(reason) = run(&args.db_path, &args.manifest_path, &args) {
        eprintln!("nbatv-catalog-runner: {reason}");
        std::process::exit(EXIT_RUN);
    }
}

fn run(
    db_path: &Path,
    manifest_path: &Path,
    args: &nbatv_catalog::RunnerArgs,
) -> Result<(), String> {
    if let Some(parent) = db_path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .map_err(|err| format!("cannot create archive dir {}: {err}", parent.display()))?;
        }
    }
    let conn = rusqlite::Connection::open(db_path)
        .map_err(|err| format!("cannot open archive {}: {err}", db_path.display()))?;
    nbatv_db::create_schema(&conn).map_err(|err| format!("cannot create schema: {err}"))?;

    let judge: Box<dyn JevJudge> = match DirectHttpJev::from_env() {
        Ok(judge) => Box::new(judge),
        Err(JevError::MissingApiKey) => Box::new(DisabledJevJudge),
        Err(error) => {
            eprintln!("nbatv-catalog: Jev disabled: {error}");
            Box::new(DisabledJevJudge)
        }
    };

    // Live probe set: one probe per rung with an implementation. Rung 3 has
    // none yet — its rungs stay unswept (Sweeping, never absent) until one
    // lands, exactly as the sweep documents.
    let nba = NbaProbe::new();
    let ia = IaProbe::live_with_env_jev();
    let ytdlp = YtdlpProbe::new_with_env_jev();
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

    let report = run_backfill_with_judge(&conn, &args.backfill_config(), &ports, judge.as_ref())
        .map_err(|err| err.to_string())?;
    println!("{}", report.summary());
    let reviews = review_list_all_with_judge(&conn, judge.as_ref(), "")
        .map_err(|error| format!("cannot prioritize review list: {error}"))?;
    if !reviews.is_empty() {
        println!("review priorities ({} total):", reviews.len());
        for item in reviews.iter().take(20) {
            println!(
                "  {} | {} | {} | {}",
                item.game_id, item.rung_name, item.title, item.url_or_pointer
            );
        }
    }
    if let nbatv_catalog::MirrorOutcome::RemoteMissing { remote } = &report.mirror.outcome {
        let prompt = nbatv_catalog::drive_sign_in_prompt(&ports.mirror_config);
        println!("{prompt} (missing remote: {remote})");
    }
    let _ = EXIT_OK;
    Ok(())
}
