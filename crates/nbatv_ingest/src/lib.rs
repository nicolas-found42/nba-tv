//! Bootstrap ingest for the nba-tv personal NBA archive.
//!
//! Day-zero input is the FiveThirtyEight `nbaallelo.csv` dump (CC BY 4.0):
//! one CSV whose `game_id` is the Basketball-Reference box-score slug.
//! This crate parses that CSV into a games skeleton plus the defunct-team
//! slug crosswalk, filters ABA rows (the `league` value is kept on each
//! row for a future toggle), validates BR slugs, builds raw-snapshot
//! paths, and tracks frozen-season / `meta-revised` re-crawl hints.
//!
//! No network fetches, no media downloads. Paths only — this crate never
//! writes outside caller-supplied (test-temp) locations.
//!
//! Module map: [`csv`] (FTE CSV bootstrap, games skeleton, snapshot paths),
//! [`br_html`] (dependency-free BR HTML parsers), [`fetch`] (polite resumable
//! fetch pipeline + snapshot gzip codec), [`snapshot`] (`data/raw/br` ->
//! archive db), [`crawl`] (season crawl driver).

pub mod br_html;
pub mod crawl;
pub mod csv;
pub mod fetch;
pub mod snapshot;

// Public API re-exports: every external call site (bins, integration tests,
// other crates) keeps its pre-split `nbatv_ingest::` paths unchanged.
pub use crate::br_html::{
    parse_box_page, parse_games_page, parse_totals_page, BoxPlayerInput, BoxTeamInput,
    GameIndexRow, GamesPage, SeasonTotalInput,
};
pub use crate::csv::{
    filter_nba_only, franchise_crosswalk, parse_fte_csv, raw_snapshot_path, recrawl_hint,
    skeleton_games, validate_game_id, FteRow, GameType, PageRevision, ParseError, SeasonState,
    SkeletonGame,
};
pub use crate::fetch::{
    etiquette_delay, fetch_season, fetch_season_with_sleeper, fetch_season_with_sleeper_at,
    gzip_decode, gzip_encode, parse_meta_revised, read_snapshot_gz, write_snapshot_gz, FetchClient,
    FetchError, FetchJob, FetchReport, FETCH_MIN_INTERVAL,
};
pub use crate::snapshot::{ingest_snapshot_dir, IngestError, IngestReport};

#[cfg(test)]
mod tests_mod;
