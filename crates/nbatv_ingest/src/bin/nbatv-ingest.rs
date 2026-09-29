//! `nbatv-ingest`: fill the archive db from a BR snapshot crawl.
//!
//! The one command that turns `data/raw/br` into the database the Shell
//! reads. Idempotent — run it again after the crawl grows and the new
//! snapshots land while everything already ingested stays put (scores
//! upgrade from bare 0-0 to box-derived, team spans widen, nothing
//! duplicates and catalog-owned tape sources survive).
//!
//! ```sh
//! cargo run -q -p nbatv_ingest --bin nbatv-ingest -- --raw data/raw/br --db data/archive.db
//! ```
//!
//! crawler's job (see `fetch_season` etiquette): this binary reads files
//! already on disk and writes the one db file. If Jev environment variables
//! are configured, its optional review runs only after that write commits.
//!
//! Honesty notes (mirrored on [`nbatv_ingest::ingest_snapshot_dir`]):
//! every `game_type` is `REGULAR` (the snapshots carry no round marker),
//! `ot`/`arena`/`attendance` are NULL, and games without a box snapshot
//! stay as bare 0-0 schedule rows until a later crawl upgrades them.
//!
//! The sidecar is advisory: it cannot rewrite Game identity, Team slugs, or
//! any persisted value.

use nbatv_catalog::{DirectHttpJev, JevJudge};
use nbatv_ingest::{ingest_snapshot_dir_with_jev, JevIngestReview};
use rusqlite::Connection;
use std::path::PathBuf;
use std::process::ExitCode;

const EXIT_OK: u8 = 0;
const EXIT_RUN: u8 = 1;
const EXIT_USAGE: u8 = 2;

const DEFAULT_RAW: &str = "data/raw/br";
const DEFAULT_DB: &str = "data/archive.db";

struct Args {
    raw: PathBuf,
    db: PathBuf,
}

fn usage() -> String {
    format!(
        "usage: nbatv-ingest [--raw DIR] [--db FILE]\n  --raw  crawl root (default {DEFAULT_RAW})\n  --db   archive db to fill (default {DEFAULT_DB})"
    )
}

fn parse_argv(argv: &[String]) -> Result<Args, String> {
    let mut args = Args {
        raw: PathBuf::from(DEFAULT_RAW),
        db: PathBuf::from(DEFAULT_DB),
    };
    let mut values = argv.iter();
    while let Some(arg) = values.next() {
        let flag = arg.as_str();
        if !matches!(flag, "--raw" | "--db") {
            return Err(format!("unknown argument {flag:?}\n{}", usage()));
        }
        let Some(value) = values.next() else {
            return Err(format!("{flag} needs a value\n{}", usage()));
        };
        if value.starts_with("--") {
            return Err(format!(
                "{flag} needs a value (a lone -- flag is not a value)"
            ));
        }
        match flag {
            "--raw" => args.raw = PathBuf::from(value),
            "--db" => args.db = PathBuf::from(value),
            _ => unreachable!("flag matched the takes-value set above"),
        }
    }
    Ok(args)
}

fn run(args: &Args) -> Result<(), String> {
    if !args.raw.is_dir() {
        return Err(format!(
            "crawl root {:?} does not exist — fetch snapshots first (see the fetch_season docs in nbatv_ingest)",
            args.raw
        ));
    }
    let conn = Connection::open(&args.db)
        .map_err(|e| format!("cannot open archive db {:?}: {e}", args.db))?;
    let judge = match DirectHttpJev::from_env() {
        Ok(judge) => Some(judge),
        Err(e) => {
            eprintln!("Jev disabled; deterministic ingest will continue: {e}");
            None
        }
    };
    let (report, review) = ingest_snapshot_dir_with_jev(
        &conn,
        &args.raw,
        judge.as_ref().map(|judge| judge as &dyn JevJudge),
    )
    .map_err(|e| format!("ingest failed: {e}"))?;
    println!(
        "seasons={} teams={} games={} (with_box={} without_box={} mismatched={} regular_fallback={})",
        report.seasons,
        report.teams,
        report.games,
        report.games_with_box,
        report.games_without_box,
        report.games_mismatched,
        report.regular_fallback_games
    );
    println!(
        "writes: box_teams={} box_players={} season_totals={} upgraded_games={}",
        report.inserted_box_teams,
        report.inserted_box_players,
        report.inserted_season_total_rows,
        report.upgraded_games
    );
    println!(
        "skips: bad_slugs={} season_mismatch={} orphan_box_pages={}",
        report.skipped_bad_slugs, report.skipped_season_mismatch, report.skipped_orphan_box_pages
    );
    if !report.unknown_team_slugs.is_empty() {
        println!(
            "unknown team slugs (stored with slug-shaped names): {}",
            report.unknown_team_slugs.join(", ")
        );
    }
    print_jev_review(&review);
    Ok(())
}

fn print_jev_review(review: &JevIngestReview) {
    if review.html_rows.is_empty()
        && review.game_types.is_empty()
        && review.team_alignments.is_empty()
        && review.failures == 0
    {
        return;
    }
    println!(
        "jev review (advisory): html_rows={} game_types={} team_alignments={} failures={}",
        review.html_rows.len(),
        review.game_types.len(),
        review.team_alignments.len(),
        review.failures
    );
    for suggestion in review.game_types.iter().take(20) {
        println!(
            "jev game type: {} => {} ({})",
            suggestion.season, suggestion.choice, suggestion.path
        );
    }
    for suggestion in &review.team_alignments {
        println!(
            "jev team alignment: {} => {}",
            suggestion.label, suggestion.suggested_slug
        );
    }
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = match parse_argv(&argv) {
        Ok(args) => args,
        Err(message) => {
            eprintln!("nbatv-ingest: {message}");
            return ExitCode::from(EXIT_USAGE);
        }
    };
    match run(&args) {
        Ok(()) => ExitCode::from(EXIT_OK),
        Err(message) => {
            eprintln!("nbatv-ingest: {message}");
            ExitCode::from(EXIT_RUN)
        }
    }
}
