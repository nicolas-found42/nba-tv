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
//! Exit codes: 0 ingested, 2 bad usage, 1 run failure. Politeness is the
//! crawler's job (see `fetch_season` etiquette): this binary only reads
//! files already on disk and writes the one db file.
//!
//! Honesty notes (mirrored on [`nbatv_ingest::ingest_snapshot_dir`]):
//! every `game_type` is `REGULAR` (the snapshots carry no round marker),
//! `ot`/`arena`/`attendance` are NULL, and games without a box snapshot
//! stay as bare 0-0 schedule rows until a later crawl upgrades them.

use nbatv_ingest::ingest_snapshot_dir;
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
    let conn = nbatv_db::open(&args.db)
        .map_err(|e| format!("cannot open archive db {:?}: {e}", args.db))?;
    let report =
        ingest_snapshot_dir(&conn, &args.raw).map_err(|e| format!("ingest failed: {e}"))?;
    println!(
        "seasons={} teams={} games={} (with_box={} without_box={} mismatched={})",
        report.seasons,
        report.teams,
        report.games,
        report.games_with_box,
        report.games_without_box,
        report.games_mismatched
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
    Ok(())
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
