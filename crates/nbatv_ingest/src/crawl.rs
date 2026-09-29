//! Season-crawl driver: enumerate seasons, derive the pending request list
//! from what is already on disk, and fetch one season at a time through
//! [`crate::fetch_season`].
//!
//! The crawl is a two-pass plan, rebuilt from disk before every attempt:
//!
//! 1. **Phase one** — the league-year schedule page and the season-totals
//!    page (`/leagues/{BAA|NBA}_{ending}_games.html` / `..._totals.html`,
//!    stored as `_games.html` / `_totals.html`).
//! 2. **Phase two** — derived from the on-disk schedule: the monthly split
//!    pages the schedule page links (`_games-{month}.html`; live BR full
//!    pages carry only a window of the season, so together with the splits
//!    they form the complete schedule), then one box-score page per game
//!    slug without a snapshot (`/boxscores/{slug}.html`). Because each
//!    landing split reveals more games, the plan is re-derived after every
//!    attempt until it is empty.
//!
//! Everything routes through [`crate::fetch_season`], which is resume-safe:
//! snapshots already on disk are skipped without touching the network, so a
//! killed run restarts for free. The driver adds three behaviors on top:
//!
//! - **404 dead-pooling:** a page the source answers with
//!   [`FetchError::NotFound`] is recorded in [`SeasonCrawl::dead`] and never
//!   re-requested within the run. A dead schedule page fails the season —
//!   without it no box jobs can be derived.
//! - **Stall bound:** three consecutive attempts with no disk progress give
//!   the season up as failed instead of spinning (two identical pending
//!   lists in a row trigger the check before the third attempt).
//! - **Per-kind bookkeeping:** fetched/skipped/missing counts land in
//!   [`SeasonCrawl`] split by schedule, monthly, totals, and box pages.
//!
//! Etiquette lives in [`crate::fetch_season`]: requests are spaced by
//! [`crate::FETCH_MIN_INTERVAL`] (≥ 3.5 s; BR robots.txt sets Crawl-delay 3).
//! At that pace the full 1946-47..2025-26 archive is ~64k requests — multi
//! days. The driver runs one season at a time; wave slicing is the caller's
//! job (`--from`/`--to` on the `nbatv-crawl` bin).

use nbatv_catalog::{CrawlFailureChoice, JevJudge};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::{
    fetch_season_with_sleeper_at, league_for_ending_year, parse_games_page, raw_snapshot_path,
    read_snapshot_page, validate_game_id, FetchClient, FetchError, FetchJob, FetchReport,
    IngestError, PageRevision,
};

/// First season-ending year with a BR league-year page: 1946-47 (BAA).
pub const FIRST_SEASON: i32 = 1947;

/// Last season-ending year complete as of this module's writing: 2025-26
/// ended with the June 2026 Finals. Later seasons are the live frontier and
/// are fetched only on explicit request (`--to` past this value).
pub const LAST_COMPLETED_SEASON: i32 = 2026;

const BR_BASE: &str = "https://www.basketball-reference.com";

/// `1947` -> `1946-47`, `2000` -> `1999-00`: the season-directory naming the
/// ingest reads back through `season_slug_to_ending_year`. Exact inverse for
/// endings `1947..=2045` (past that the two-digit century rule breaks; the
/// archive is nowhere near that frontier).
pub fn season_slug(ending: i32) -> String {
    format!("{}-{:02}", ending - 1, ending % 100)
}

/// `1947` -> `BAA` (league years through 1949), else `NBA`: the league-year
/// URL prefix. Same rule the ingest applies to `seasons.league`.
pub fn league_prefix(ending: i32) -> &'static str {
    league_for_ending_year(ending)
}

/// Phase-one jobs for one season: the full league-year schedule page and
/// the season-totals page, named as the ingest expects them on disk.
pub fn phase_one_jobs(ending: i32) -> Vec<FetchJob> {
    let lg = league_prefix(ending);
    vec![
        FetchJob::new(
            "_games.html",
            &format!("{BR_BASE}/leagues/{lg}_{ending}_games.html"),
        ),
        FetchJob::new(
            "_totals.html",
            &format!("{BR_BASE}/leagues/{lg}_{ending}_totals.html"),
        ),
    ]
}

/// `11` -> `november`, the BR split-page suffix.
fn month_name(month: u32) -> Option<&'static str> {
    crate::MONTHS
        .iter()
        .find(|(_, n)| *n == month as i32)
        .map(|(name, _)| *name)
}

/// `_games-november.html`, the snapshot name for one monthly split.
fn monthly_split_file(month: u32) -> Option<String> {
    month_name(month).map(|name| format!("_games-{name}.html"))
}

/// Months of the season the schedule page links as monthly splits
/// (`/leagues/NBA_2026_games-november.html` -> November). Live BR full
/// pages carry only a window of games, so these links — not the parsed
/// rows — enumerate the whole schedule.
fn split_months_linked(schedule_html: &str) -> BTreeSet<u32> {
    let mut months = BTreeSet::new();
    let mut rest = schedule_html;
    while let Some(at) = rest.find("_games-") {
        rest = &rest[at + "_games-".len()..];
        let name_end = rest.find('.').unwrap_or(rest.len());
        let name = &rest[..name_end];
        if let Some((_, number)) = crate::MONTHS.iter().find(|(n, _)| *n == name) {
            months.insert(*number as u32);
        }
    }
    months
}

/// Monthly split job: `/leagues/NBA_2025_games-november.html` stored as
/// `_games-november.html`.
fn monthly_split_job(ending: i32, month: u32) -> Option<FetchJob> {
    let lg = league_prefix(ending);
    month_name(month).map(|name| {
        FetchJob::new(
            &format!("_games-{name}.html"),
            &format!("{BR_BASE}/leagues/{lg}_{ending}_games-{name}.html"),
        )
    })
}

/// The directory holding one season's snapshots under the fetch pipeline's
/// layout: `{out_dir}/{source}/{slug}/`.
fn season_dir(out_dir: &Path, source: &str, slug: &str) -> PathBuf {
    let mut dir = out_dir.join(raw_snapshot_path(source, slug, "_games.html"));
    dir.pop();
    dir
}

/// Every request the season still needs, derived from disk:
///
/// - missing phase-one pages first,
/// - then missing monthly splits for the months the schedule page links
///   (live BR full pages carry only a window of the season — the splits
///   together are the complete schedule, so they are load-bearing, not
///   redundant),
/// - then box pages for every game slug on any on-disk schedule page
///   (full page plus splits; schedule order, deduped; invalid or
///   foreign-season slugs never queued — the same guard the ingest
///   applies when it drops poison rows).
///
/// `dead` pages (404s from earlier attempts) are excluded. Because box
/// derivations grow as splits land, the caller re-derives after every
/// attempt until the list is empty.
pub fn pending_jobs(
    season_dir: &Path,
    ending: i32,
    monthly: bool,
    dead: &BTreeSet<String>,
) -> Result<Vec<FetchJob>, IngestError> {
    let mut jobs: Vec<FetchJob> = Vec::new();
    for job in phase_one_jobs(ending) {
        if !dead.contains(&job.file_name) && !season_dir.join(&job.file_name).is_file() {
            jobs.push(job);
        }
    }

    let games_path = season_dir.join("_games.html");
    if !games_path.is_file() {
        return Ok(jobs);
    }
    let sched_html = read_snapshot_page(&games_path)?;
    let linked_months = split_months_linked(&sched_html);

    if monthly {
        for m in &linked_months {
            if let Some(job) = monthly_split_job(ending, *m) {
                if !dead.contains(&job.file_name) && !season_dir.join(&job.file_name).is_file() {
                    jobs.push(job);
                }
            }
        }
    }

    // Box slugs come from every on-disk schedule page: the full page
    // (a window of the season) plus each landed monthly split.
    let mut pages: Vec<String> = vec![sched_html];
    for m in &linked_months {
        if let Some(file) = monthly_split_file(*m) {
            let path = season_dir.join(&file);
            if path.is_file() {
                pages.push(read_snapshot_page(&path)?);
            }
        }
    }
    let mut slugs: BTreeSet<String> = BTreeSet::new();
    for html in &pages {
        for row in parse_games_page(html).rows {
            // The ingest's season guard: a game id whose year is neither
            // the season's start nor end year belongs to another season.
            let in_season = row
                .game_id
                .get(..4)
                .and_then(|y| y.parse::<i32>().ok())
                .map(|y| y == ending - 1 || y == ending)
                .unwrap_or(false);
            if !in_season {
                continue;
            }
            if validate_game_id(&row.game_id)
                && !season_dir.join(format!("{}.html", row.game_id)).is_file()
            {
                slugs.insert(row.game_id);
            }
        }
    }
    for slug in slugs {
        let file_name = format!("{slug}.html");
        if dead.contains(&file_name) {
            continue;
        }
        jobs.push(FetchJob::new(
            &file_name,
            &format!("{BR_BASE}/boxscores/{slug}.html"),
        ));
    }
    Ok(jobs)
}

/// Which report bucket a snapshot file name belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Schedule,
    Monthly,
    Totals,
    Box,
}

fn kind(file_name: &str) -> Kind {
    if file_name == "_games.html" {
        Kind::Schedule
    } else if file_name.starts_with("_games-") {
        Kind::Monthly
    } else if file_name == "_totals.html" {
        Kind::Totals
    } else {
        Kind::Box
    }
}

/// One decisive optional classification for an ambiguous HTTP response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassifiedCrawlFailure {
    pub file_name: String,
    pub url: String,
    pub choice: CrawlFailureChoice,
}

/// Outcome of [`crawl_season`] for one season.
#[derive(Debug, Clone, Default)]
pub struct SeasonCrawl {
    pub ending: i32,
    pub slug: String,
    pub schedule: FetchReport,
    pub monthly: FetchReport,
    pub totals: FetchReport,
    pub boxes: FetchReport,
    /// Pages the source answered "not found"; never re-requested within
    /// the run. A dead `_games.html` fails the season.
    pub dead: Vec<String>,
    /// Observed `meta-revised` stamps across the season's pages (freshness
    /// bookkeeping for a future re-crawl pass; see [`crate::recrawl_hint`]).
    pub revisions: Vec<PageRevision>,
    /// Why the season was given up, if it was.
    pub error: Option<String>,
    /// Latest decisive optional classification, for reporting/manual review.
    pub classified_failure: Option<ClassifiedCrawlFailure>,
}

impl SeasonCrawl {
    fn new(ending: i32) -> Self {
        SeasonCrawl {
            ending,
            slug: season_slug(ending),
            ..Default::default()
        }
    }

    fn acc_for(&mut self, file_name: &str) -> &mut FetchReport {
        match kind(file_name) {
            Kind::Schedule => &mut self.schedule,
            Kind::Monthly => &mut self.monthly,
            Kind::Totals => &mut self.totals,
            Kind::Box => &mut self.boxes,
        }
    }

    /// Fold one attempt's report into the per-kind buckets, deduping file
    /// names (an on-disk snapshot shows up as a skip in every re-derivation).
    fn absorb(&mut self, report: FetchReport) {
        let FetchReport {
            fetched,
            skipped,
            revisions,
        } = report;
        for file_name in fetched {
            let acc = self.acc_for(&file_name);
            if !acc.fetched.contains(&file_name) {
                acc.fetched.push(file_name);
            }
        }
        for file_name in skipped {
            let acc = self.acc_for(&file_name);
            if !acc.skipped.contains(&file_name) {
                acc.skipped.push(file_name);
            }
        }
        self.revisions.extend(revisions);
    }

    pub fn fetched_total(&self) -> usize {
        self.schedule.fetched.len()
            + self.monthly.fetched.len()
            + self.totals.fetched.len()
            + self.boxes.fetched.len()
    }
}

/// Crawl one season to completion (or documented failure). Never returns a
/// hard error: every failure mode lands in the [`SeasonCrawl`] so a
/// multi-season run can continue with the remaining seasons.
///
/// `interval` is the minimum spacing between two requests (the etiquette
/// floor is [`crate::FETCH_MIN_INTERVAL`]; going below it exceeds BR's
/// robots.txt Crawl-delay 3 and is the caller's call).
///
/// Loop shape: derive pending -> fetch -> re-derive. A failed attempt has
/// still landed everything before its failing job ([`crate::fetch_season`]
/// processes jobs in order), so the next derivation is strictly smaller
/// unless the same first job keeps failing — that is what the stall bound
/// catches.
pub fn crawl_season<C, S>(
    client: &C,
    source: &str,
    ending: i32,
    out_dir: &Path,
    monthly: bool,
    interval: Duration,
    sleep: &mut S,
) -> SeasonCrawl
where
    C: FetchClient,
    S: FnMut(Duration),
{
    crawl_season_with_jev(
        client, source, ending, out_dir, monthly, interval, None, sleep,
    )
}

/// Crawl one season with an optional typed classifier for ambiguous HTTP
/// responses. Exact 404/410 and 429/503 handling preempts Jev; transport
/// failures never call it. Decisions are cached per file for the run.
pub fn crawl_season_with_jev<C, S>(
    client: &C,
    source: &str,
    ending: i32,
    out_dir: &Path,
    monthly: bool,
    interval: Duration,
    judge: Option<&dyn JevJudge>,
    sleep: &mut S,
) -> SeasonCrawl
where
    C: FetchClient,
    S: FnMut(Duration),
{
    let mut out = SeasonCrawl::new(ending);
    let dir = season_dir(out_dir, source, &out.slug);
    let mut dead: BTreeSet<String> = BTreeSet::new();
    let mut classification_cache: BTreeMap<String, Option<CrawlFailureChoice>> = BTreeMap::new();
    // Content equality, not length: landing a split both drains fetched
    // jobs and adds newly derived boxes in the same re-derivation.
    let mut last_pending: Option<Vec<FetchJob>> = None;
    let mut stalls = 0usize;
    let mut throttle_wait = Duration::from_secs(30);
    let mut throttles = 0usize;

    loop {
        let jobs = match pending_jobs(&dir, ending, monthly, &dead) {
            Ok(jobs) => jobs,
            Err(e) => {
                out.error = Some(format!("pending derivation failed: {e}"));
                return out;
            }
        };
        if jobs.is_empty() {
            break;
        }
        match fetch_season_with_sleeper_at(
            client, source, &out.slug, &jobs, out_dir, interval, sleep,
        ) {
            Ok(report) => {
                // No break here: this attempt may have landed the schedule
                // or splits that make further derivations possible.
                out.absorb(report);
                stalls = 0;
                throttles = 0;
                throttle_wait = Duration::from_secs(30);
            }
            Err(e) => {
                // Everything before the failing job landed, so the failing
                // job is the first pending entry whose file is still absent.
                let Some(job) = jobs
                    .iter()
                    .find(|j| !dir.join(&j.file_name).is_file())
                    .cloned()
                else {
                    out.error = Some(format!("fetch failed after full progress: {e}"));
                    break;
                };
                // Everything before the failing job landed: fold those files
                // into the buckets so run counts reflect disk state.
                let landed = FetchReport {
                    fetched: jobs
                        .iter()
                        .filter(|j| dir.join(&j.file_name).is_file())
                        .map(|j| j.file_name.clone())
                        .collect(),
                    skipped: Vec::new(),
                    revisions: Vec::new(),
                };
                out.absorb(landed);

                let decision = match &e {
                    FetchError::NotFound(_) => Some(CrawlFailureChoice::NotFound),
                    FetchError::Throttled(_) => Some(CrawlFailureChoice::RateLimited),
                    FetchError::Unclassified { status, evidence } => {
                        let decision =
                            if let Some(cached) = classification_cache.get(&job.file_name) {
                                *cached
                            } else {
                                let status = status
                                    .map(|status| status.to_string())
                                    .unwrap_or_else(|| "unknown".to_owned());
                                let response = format!("HTTP status: {status}\n\n{evidence}");
                                let decision = judge
                                    .and_then(|judge| {
                                        judge.classify_crawl_failure(&response).ok().flatten()
                                    })
                                    .filter(|choice| *choice != CrawlFailureChoice::Unknown);
                                if let Some(choice) = decision {
                                    out.classified_failure = Some(ClassifiedCrawlFailure {
                                        file_name: job.file_name.clone(),
                                        url: job.url.clone(),
                                        choice,
                                    });
                                }
                                classification_cache.insert(job.file_name.clone(), decision);
                                decision
                            };
                        decision
                    }
                    FetchError::UnsafePath(_)
                    | FetchError::Io(_)
                    | FetchError::Client(_)
                    | FetchError::Decode(_) => None,
                };

                match decision {
                    Some(CrawlFailureChoice::NotFound) => {
                        dead.insert(job.file_name.clone());
                        out.dead.push(job.file_name.clone());
                        stalls = 0;
                        if job.file_name == "_games.html" {
                            out.error = Some(format!("schedule page not found: {}", job.url));
                            break;
                        }
                    }
                    Some(CrawlFailureChoice::RateLimited) => {
                        // Rate budget exhausted: wait it out with a growing
                        // backoff. This never counts as a stall; eight straight
                        // throttled re-derivations give the season up.
                        stalls = 0;
                        sleep(throttle_wait);
                        throttle_wait = (throttle_wait * 2).min(Duration::from_secs(300));
                        throttles += 1;
                        if throttles >= 8 {
                            out.error = Some(format!(
                                "still throttled after {throttles} backoffs (last wait {throttle_wait:?}): {e}"
                            ));
                            break;
                        }
                    }
                    Some(CrawlFailureChoice::AccessBlocked | CrawlFailureChoice::ParserChange) => {
                        out.error = Some(format!(
                            "{} needs manual review after {:?}: {e}",
                            job.file_name,
                            decision.expect("terminal classification")
                        ));
                        break;
                    }
                    Some(CrawlFailureChoice::TransientServer | CrawlFailureChoice::Unknown)
                    | None => {
                        if last_pending.as_deref() == Some(&jobs) {
                            stalls += 1;
                            if stalls >= 2 {
                                out.error = Some(format!(
                                    "no progress across {} attempts; {} request(s) keep failing",
                                    stalls + 2,
                                    jobs.len()
                                ));
                                break;
                            }
                        } else {
                            stalls = 1;
                        }
                        last_pending = Some(jobs.clone());
                        // Transient-looking failure: pause once before the
                        // polite retry; the stall bound ends persistent ones.
                        sleep(interval);
                    }
                }
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Tests (tiny inline fixtures only; never network)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{season_slug_to_ending_year, write_snapshot_gz, FETCH_MIN_INTERVAL};
    use nbatv_catalog::{
        CollectorNoteInput, CrawlFailureChoice, FileSelection, FileSelectionInput, GameTypeChoice,
        JevError, JevJudge, SearchTemplateSelection, TeamChoice,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn season_slug_matches_the_ingest_reader_through_the_century_rollover() {
        assert_eq!(season_slug(1947), "1946-47");
        assert_eq!(season_slug(1999), "1998-99");
        assert_eq!(season_slug(2000), "1999-00");
        assert_eq!(season_slug(2026), "2025-26");
        for ending in FIRST_SEASON..=2045 {
            assert_eq!(
                season_slug_to_ending_year(&season_slug(ending)),
                Some(ending),
                "round trip {ending}"
            );
        }
    }

    #[test]
    fn phase_one_jobs_use_the_right_league_prefix_per_era() {
        for (ending, lg) in [(1947, "BAA"), (1949, "BAA"), (1950, "NBA"), (2026, "NBA")] {
            let jobs = phase_one_jobs(ending);
            assert_eq!(jobs.len(), 2);
            assert_eq!(jobs[0].file_name, "_games.html");
            assert_eq!(
                jobs[0].url,
                format!("https://www.basketball-reference.com/leagues/{lg}_{ending}_games.html")
            );
            assert_eq!(jobs[1].file_name, "_totals.html");
            assert_eq!(
                jobs[1].url,
                format!("https://www.basketball-reference.com/leagues/{lg}_{ending}_totals.html")
            );
        }
    }

    #[test]
    fn month_helpers_cover_the_split_page_naming() {
        assert_eq!(month_name(11), Some("november"));
        assert_eq!(month_name(13), None);
        let job = monthly_split_job(2026, 11).unwrap();
        assert_eq!(job.file_name, "_games-november.html");
        assert_eq!(
            job.url,
            "https://www.basketball-reference.com/leagues/NBA_2026_games-november.html"
        );
        assert_eq!(
            monthly_split_file(11).as_deref(),
            Some("_games-november.html")
        );
    }

    #[test]
    fn split_months_linked_reads_the_schedule_navigation() {
        // Live-shaped full page: one window table plus the split nav links.
        let html = "<html><body>\
<a href=\"/leagues/BAA_1947_games-november.html\">November</a>\
<a href=\"/leagues/BAA_1947_games-december.html\">December</a>\
<a href=\"/leagues/BAA_1947_games.html\">Full schedule</a>\
<a href=\"/boxscores/195511050ROC.html\">Box</a>\
</body></html>";
        let mut want = BTreeSet::new();
        want.insert(11u32);
        want.insert(12u32);
        assert_eq!(split_months_linked(html), want);
        assert_eq!(
            split_months_linked("<html>no links</html>"),
            BTreeSet::new()
        );
    }

    /// Minimal BR `_games.html` in the live shape (`date_game`'s `csk`
    /// carries the game slug; the visible text is the date) with the split
    /// navigation: a November game with a slug and a December game with a
    /// slug on the window page, one slugless row, one foreign-season row,
    /// and links to the november/december split pages.
    const SCHED_NAV: &str = "\
<div class=\"schedule\">\
<a href=\"/leagues/BAA_1947_games-november.html\">November</a> · \
<a href=\"/leagues/BAA_1947_games-december.html\">December</a>\
</div>\
<table class=\"stats_table\" id=\"schedule\">\
<tbody>\
<tr><th data-stat=\"date_game\" csk=\"194611010TRH\">Fri, Nov 1, 1946</th>\
<td data-stat=\"visitor_team_name\"><a href=\"/teams/NYK/1947.html\">New York Knicks</a></td>\
<td data-stat=\"home_team_name\"><a href=\"/teams/TRH/1947.html\">Toronto Huskies</a></td>\
<td data-stat=\"box_score_text\"><a href=\"/boxscores/194611010TRH.html\">Box Score</a></td></tr>\
<tr><th data-stat=\"date_game\" csk=\"194612050CHS\">Thu, Dec 5, 1946</th>\
<td data-stat=\"visitor_team_name\"><a href=\"/teams/TRH/1947.html\">Toronto Huskies</a></td>\
<td data-stat=\"home_team_name\"><a href=\"/teams/CHS/1947.html\">Chicago Stags</a></td>\
<td data-stat=\"box_score_text\"><a href=\"/boxscores/194612050CHS.html\">Box Score</a></td></tr>\
<tr><th data-stat=\"date_game\" csk=\"194612080NYK\">Sun, Dec 8, 1946</th>\
<td data-stat=\"visitor_team_name\"><a href=\"/teams/NYK/1947.html\">New York Knicks</a></td>\
<td data-stat=\"home_team_name\"><a href=\"/teams/PIT/1947.html\">Pittsburgh Ironmen</a></td>\
<td data-stat=\"box_score_text\"></td></tr>\
<tr><th data-stat=\"date_game\" csk=\"201506160GSW\">Tue, Jun 16, 2015</th>\
<td data-stat=\"visitor_team_name\"><a href=\"/teams/CLE/2016.html\">Cavaliers</a></td>\
<td data-stat=\"home_team_name\"><a href=\"/teams/GSW/2016.html\">Warriors</a></td>\
<td data-stat=\"box_score_text\"><a href=\"/boxscores/201506160GSW.html\">Box Score</a></td></tr>\
</tbody></table>";

    const SCHED: &str = SCHED_NAV;

    static DIR_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    /// Unique caller-supplied dir under the system temp dir (std only).
    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        /// The pipeline layout: `crawl_season`'s `out_dir` is the directory
        /// that *contains* `data/` (see `raw_snapshot_path`), so the season
        /// dir is `{path}/data/raw/br/1946-47`.
        fn fresh(tag: &str) -> Self {
            let n = DIR_SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let mut path = std::env::temp_dir();
            path.push(format!("nbatv-crawl-{tag}-{}-{n}", std::process::id()));
            std::fs::create_dir_all(path.join("data/raw/br/1946-47")).expect("create season dir");
            TempDir { path }
        }

        fn season(&self) -> PathBuf {
            self.path.join("data/raw/br/1946-47")
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    fn no_dead() -> BTreeSet<String> {
        BTreeSet::new()
    }

    #[test]
    fn pending_on_empty_dir_is_just_the_phase_one_pages() {
        let dir = TempDir::fresh("empty");
        let jobs = pending_jobs(&dir.season(), 1947, true, &no_dead()).unwrap();
        assert_eq!(jobs, phase_one_jobs(1947));
    }

    #[test]
    fn pending_derives_missing_boxes_months_and_skips_present_and_foreign() {
        let dir = TempDir::fresh("derive");
        let season = dir.season();
        write_snapshot_gz(&season.join("_games.html"), SCHED).unwrap();
        write_snapshot_gz(&season.join("_totals.html"), "<html>t</html>").unwrap();
        write_snapshot_gz(&season.join("194611010TRH.html"), "<html>box</html>").unwrap();

        let jobs = pending_jobs(&season, 1947, true, &no_dead()).unwrap();
        let names: Vec<&str> = jobs.iter().map(|j| j.file_name.as_str()).collect();
        // TRH already on disk; CHS queued; foreign 201506160GSW and the
        // slugless row never queued; linked monthly splits come first.
        assert_eq!(
            names,
            vec![
                "_games-november.html",
                "_games-december.html",
                "194612050CHS.html"
            ]
        );
        assert_eq!(
            jobs[2].url,
            "https://www.basketball-reference.com/boxscores/194612050CHS.html"
        );

        // With monthly splits off, only the box remains.
        let jobs = pending_jobs(&season, 1947, false, &no_dead()).unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].file_name, "194612050CHS.html");
    }

    #[test]
    fn pending_excludes_dead_pages() {
        let dir = TempDir::fresh("dead");
        let season = dir.season();
        write_snapshot_gz(&season.join("_games.html"), SCHED).unwrap();
        write_snapshot_gz(&season.join("194611010TRH.html"), "x").unwrap();
        write_snapshot_gz(&season.join("194612050CHS.html"), "x").unwrap();
        write_snapshot_gz(&season.join("_games-november.html"), "x").unwrap();

        let dead: BTreeSet<String> = ["_totals.html", "_games-december.html"]
            .into_iter()
            .map(str::to_owned)
            .collect();
        let jobs = pending_jobs(&season, 1947, true, &dead).unwrap();
        assert_eq!(jobs.len(), 0, "{jobs:?}");
    }

    /// Canned-HTML client with a set of URLs to answer "not found".
    struct FakeClient {
        pages: std::collections::HashMap<String, String>,
        not_found: BTreeSet<String>,
        fail_all: bool,
        throttle_all: bool,
        unclassified: Option<(u16, String)>,
        calls: std::cell::RefCell<Vec<String>>,
    }

    impl FakeClient {
        fn with(pages: &[(&str, &str)]) -> Self {
            FakeClient {
                pages: pages
                    .iter()
                    .map(|(u, h)| ((*u).to_owned(), (*h).to_owned()))
                    .collect(),
                not_found: BTreeSet::new(),
                fail_all: false,
                throttle_all: false,
                unclassified: None,
                calls: std::cell::RefCell::new(Vec::new()),
            }
        }
    }

    impl FetchClient for FakeClient {
        fn fetch(&self, url: &str) -> Result<String, FetchError> {
            self.calls.borrow_mut().push(url.to_owned());
            if self.fail_all {
                return Err(FetchError::Client("network down".to_owned()));
            }
            if self.throttle_all {
                return Err(FetchError::Throttled("HTTP 429 for you".to_owned()));
            }
            if self.not_found.contains(url) {
                return Err(FetchError::NotFound(url.to_owned()));
            }
            if let Some((status, evidence)) = &self.unclassified {
                return Err(FetchError::Unclassified {
                    status: Some(*status),
                    evidence: evidence.clone(),
                });
            }
            self.pages
                .get(url)
                .cloned()
                .ok_or_else(|| FetchError::Client(format!("no fixture for {url}")))
        }
    }

    struct CrawlJudge {
        choice: CrawlFailureChoice,
        calls: AtomicUsize,
    }

    impl JevJudge for CrawlJudge {
        fn select_file(&self, _: &FileSelectionInput) -> Result<Option<FileSelection>, JevError> {
            Ok(None)
        }

        fn match_candidate(
            &self,
            _: &nbatv_catalog::GameContext,
            _: &nbatv_catalog::ProbeCandidate,
        ) -> Result<Option<nbatv_catalog::CandidateVerdict>, JevError> {
            Ok(None)
        }

        fn classify_game_type(&self, _: &GameTypeChoice) -> Result<Option<String>, JevError> {
            Ok(None)
        }

        fn align_team(&self, _: &TeamChoice) -> Result<Option<String>, JevError> {
            Ok(None)
        }

        fn match_collector_note(&self, _: &CollectorNoteInput) -> Result<Option<String>, JevError> {
            Ok(None)
        }

        fn choose_search_template(
            &self,
            _: &nbatv_catalog::GameContext,
            _: &str,
        ) -> Result<Option<SearchTemplateSelection>, JevError> {
            Ok(None)
        }

        fn classify_crawl_failure(&self, _: &str) -> Result<Option<CrawlFailureChoice>, JevError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(Some(self.choice))
        }

        fn classify_html_row(
            &self,
            _: &str,
            _: &str,
        ) -> Result<Option<nbatv_catalog::HtmlRowChoice>, JevError> {
            Ok(None)
        }

        fn route_shell_command(&self, _: &str) -> Result<Option<String>, JevError> {
            Ok(None)
        }

        fn prioritize_review(&self, _: &[String], _: &str) -> Result<Option<u8>, JevError> {
            Ok(None)
        }
    }

    #[test]
    fn crawl_fetches_a_season_end_to_end_and_resumes_on_a_second_run() {
        let dir = TempDir::fresh("happy");
        // Seed only the schedule: totals + the two boxes + the two monthly
        // splits should all be fetched from the schedule's slugs.
        write_snapshot_gz(&dir.season().join("_games.html"), SCHED).unwrap();
        let pages: Vec<(String, String)> = [
            (
                "https://www.basketball-reference.com/leagues/BAA_1947_totals.html",
                "<html>totals</html>",
            ),
            (
                "https://www.basketball-reference.com/boxscores/194611010TRH.html",
                "<html>194611010TRH</html>",
            ),
            (
                "https://www.basketball-reference.com/boxscores/194612050CHS.html",
                "<html>194612050CHS</html>",
            ),
            (
                "https://www.basketball-reference.com/leagues/BAA_1947_games-november.html",
                "<html>november</html>",
            ),
            (
                "https://www.basketball-reference.com/leagues/BAA_1947_games-december.html",
                "<html>december</html>",
            ),
        ]
        .into_iter()
        .map(|(u, h)| (u.to_owned(), h.to_owned()))
        .collect();
        let client = FakeClient::with(
            &pages
                .iter()
                .map(|(u, h)| (u.as_str(), h.as_str()))
                .collect::<Vec<_>>(),
        );

        let out = crawl_season(
            &client,
            "br",
            1947,
            &dir.path,
            true,
            FETCH_MIN_INTERVAL,
            &mut |_| {},
        );
        assert_eq!(out.error, None);
        assert_eq!(out.fetched_total(), 5);
        assert_eq!(out.boxes.fetched.len(), 2);
        assert_eq!(out.monthly.fetched.len(), 2);
        assert_eq!(out.totals.fetched.len(), 1);
        // The seeded schedule is never queued (pending derives from disk),
        // so nothing lands in the schedule bucket at all.
        assert!(out.schedule.fetched.is_empty() && out.schedule.skipped.is_empty());
        assert!(out.dead.is_empty());
        assert_eq!(client.calls.borrow().len(), 5);

        // Second run: everything resume-skipped, zero network.
        let fresh = FakeClient::with(&[]);
        let out = crawl_season(
            &fresh,
            "br",
            1947,
            &dir.path,
            true,
            FETCH_MIN_INTERVAL,
            &mut |_| {},
        );
        assert_eq!(out.error, None);
        assert_eq!(out.fetched_total(), 0);
        assert_eq!(fresh.calls.borrow().len(), 0);
    }

    #[test]
    fn crawl_dead_pools_missing_box_pages_and_keeps_going() {
        let dir = TempDir::fresh("notfound");
        write_snapshot_gz(&dir.season().join("_games.html"), SCHED).unwrap();
        let mut client = FakeClient::with(&[
            (
                "https://www.basketball-reference.com/leagues/BAA_1947_totals.html",
                "<html>totals</html>",
            ),
            (
                "https://www.basketball-reference.com/boxscores/194611010TRH.html",
                "<html>box</html>",
            ),
            (
                "https://www.basketball-reference.com/leagues/BAA_1947_games-november.html",
                "<html>november</html>",
            ),
            (
                "https://www.basketball-reference.com/leagues/BAA_1947_games-december.html",
                "<html>december</html>",
            ),
        ]);
        client
            .not_found
            .insert("https://www.basketball-reference.com/boxscores/194612050CHS.html".to_owned());

        let out = crawl_season(
            &client,
            "br",
            1947,
            &dir.path,
            true,
            FETCH_MIN_INTERVAL,
            &mut |_| {},
        );
        assert_eq!(out.error, None);
        assert_eq!(out.dead, vec!["194612050CHS.html".to_owned()]);
        assert_eq!(out.boxes.fetched, vec!["194611010TRH.html".to_owned()]);
        // The dead page was requested exactly once across the run.
        assert_eq!(
            client
                .calls
                .borrow()
                .iter()
                .filter(|u| u.contains("194612050CHS"))
                .count(),
            1
        );
    }

    #[test]
    fn crawl_fails_the_season_when_the_schedule_page_is_missing() {
        let dir = TempDir::fresh("nogames");
        let mut client = FakeClient::with(&[]);
        client
            .not_found
            .insert("https://www.basketball-reference.com/leagues/BAA_1947_games.html".to_owned());

        let out = crawl_season(
            &client,
            "br",
            1947,
            &dir.path,
            true,
            FETCH_MIN_INTERVAL,
            &mut |_| {},
        );
        assert_eq!(
            out.error.as_deref(),
            Some(
                "schedule page not found: https://www.basketball-reference.com/leagues/BAA_1947_games.html"
            )
        );
        assert_eq!(out.dead, vec!["_games.html".to_owned()]);
        // The run stops instead of deriving boxes from nothing.
        assert_eq!(client.calls.borrow().len(), 1);
    }

    #[test]
    fn crawl_continues_after_phase_one_to_splits_and_their_boxes() {
        // Regression: a successful attempt used to end the season, so the
        // boxes and splits derived from a just-fetched schedule never ran.
        let dir = TempDir::fresh("multiturn");
        let pages: Vec<(String, String)> = [
            (
                "https://www.basketball-reference.com/leagues/BAA_1947_games.html",
                SCHED_NAV,
            ),
            (
                "https://www.basketball-reference.com/leagues/BAA_1947_totals.html",
                "<html>totals</html>",
            ),
            (
                "https://www.basketball-reference.com/leagues/BAA_1947_games-november.html",
                // The split carries a game the window page did not.
                "<tr><th data-stat=\"date_game\" csk=\"194611020PHW\">Sat, Nov 2, 1946</th>\
<td data-stat=\"visitor_team_name\"><a href=\"/teams/PHW/1947.html\">Warriors</a></td>\
<td data-stat=\"home_team_name\"><a href=\"/teams/NYK/1947.html\">Knicks</a></td>\
<td data-stat=\"box_score_text\"><a href=\"/boxscores/194611020NYK.html\">Box Score</a></td></tr>",
            ),
            (
                "https://www.basketball-reference.com/leagues/BAA_1947_games-december.html",
                "<html><table id=\"schedule\"><tbody></tbody></table></html>",
            ),
            (
                "https://www.basketball-reference.com/boxscores/194611010TRH.html",
                "<html>box TRH</html>",
            ),
            (
                "https://www.basketball-reference.com/boxscores/194612050CHS.html",
                "<html>box CHS</html>",
            ),
            (
                "https://www.basketball-reference.com/boxscores/194611020NYK.html",
                "<html>box NYK</html>",
            ),
        ]
        .into_iter()
        .map(|(u, h)| (u.to_owned(), h.to_owned()))
        .collect();
        let client = FakeClient::with(
            &pages
                .iter()
                .map(|(u, h)| (u.as_str(), h.as_str()))
                .collect::<Vec<_>>(),
        );

        let out = crawl_season(
            &client,
            "br",
            1947,
            &dir.path,
            true,
            FETCH_MIN_INTERVAL,
            &mut |_| {},
        );
        assert_eq!(out.error, None, "{:?}", out.error);
        // Both linked splits land (december carries no rows), and the
        // november split's extra game gets its box in a later attempt.
        assert_eq!(out.monthly.fetched.len(), 2);
        assert_eq!(out.boxes.fetched.len(), 3);
        assert!(out.dead.is_empty());
    }

    #[test]
    fn crawl_gives_up_after_bounded_stalls_instead_of_spinning() {
        let dir = TempDir::fresh("stall");
        let mut fail = FakeClient::with(&[]);
        fail.fail_all = true;

        let mut pauses = 0usize;
        let out = crawl_season(
            &fail,
            "br",
            1947,
            &dir.path,
            true,
            FETCH_MIN_INTERVAL,
            &mut |_| pauses += 1,
        );
        assert!(out.error.is_some());
        // Attempts: two failing fetches; one polite pause before the retry,
        // none before the give-up.
        assert_eq!(fail.calls.borrow().len(), 2);
        assert_eq!(pauses, 1);
        assert!(!dir.season().join("_games.html").exists());
    }

    #[test]
    fn crawl_backs_off_exponentially_while_throttled_then_gives_up() {
        let dir = TempDir::fresh("throttle");
        let mut throttled = FakeClient::with(&[]);
        throttled.throttle_all = true;

        let mut waits: Vec<Duration> = Vec::new();
        let out = crawl_season(
            &throttled,
            "br",
            1947,
            &dir.path,
            true,
            FETCH_MIN_INTERVAL,
            &mut |d| waits.push(d),
        );
        // Eight backoffs (30s doubling to the 300s cap), then the season
        // fails with the throttle named — throttling never counts as stall
        // and never trips the fast-fail path.
        assert_eq!(
            out.error.as_deref(),
            Some("still throttled after 8 backoffs (last wait 300s): throttled: HTTP 429 for you")
        );
        assert_eq!(waits.len(), 8);
        assert_eq!(
            waits,
            [
                Duration::from_secs(30),
                Duration::from_secs(60),
                Duration::from_secs(120),
                Duration::from_secs(240),
                Duration::from_secs(300),
                Duration::from_secs(300),
                Duration::from_secs(300),
                Duration::from_secs(300),
            ]
        );
    }

    #[test]
    fn access_block_is_terminal_manual_review_not_dead_pool() {
        let dir = TempDir::fresh("jev-access");
        let mut client = FakeClient::with(&[]);
        client.unclassified = Some((403, "Access denied by source".to_owned()));
        let judge = CrawlJudge {
            choice: CrawlFailureChoice::AccessBlocked,
            calls: AtomicUsize::new(0),
        };

        let out = crawl_season_with_jev(
            &client,
            "br",
            1947,
            &dir.path,
            true,
            FETCH_MIN_INTERVAL,
            Some(&judge),
            &mut |_| {},
        );

        assert!(out.dead.is_empty());
        assert!(out
            .error
            .as_deref()
            .is_some_and(|error| error.contains("manual review")));
        let classified = out.classified_failure.expect("typed classification");
        assert_eq!(classified.file_name, "_games.html");
        assert_eq!(classified.choice, CrawlFailureChoice::AccessBlocked);
        assert_eq!(judge.calls.load(Ordering::SeqCst), 1);
        assert_eq!(client.calls.borrow().len(), 1);
    }

    #[test]
    fn crawl_failure_classification_is_cached_across_retries() {
        let dir = TempDir::fresh("jev-cache");
        let mut client = FakeClient::with(&[]);
        client.unclassified = Some((502, "upstream unavailable".to_owned()));
        let judge = CrawlJudge {
            choice: CrawlFailureChoice::TransientServer,
            calls: AtomicUsize::new(0),
        };

        let out = crawl_season_with_jev(
            &client,
            "br",
            1947,
            &dir.path,
            true,
            FETCH_MIN_INTERVAL,
            Some(&judge),
            &mut |_| {},
        );

        assert!(out.error.is_some());
        assert_eq!(client.calls.borrow().len(), 2);
        assert_eq!(judge.calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            out.classified_failure.map(|failure| failure.choice),
            Some(CrawlFailureChoice::TransientServer)
        );
    }

    #[test]
    fn transport_failure_without_response_never_calls_jev() {
        let dir = TempDir::fresh("jev-transport");
        let mut client = FakeClient::with(&[]);
        client.fail_all = true;
        let judge = CrawlJudge {
            choice: CrawlFailureChoice::AccessBlocked,
            calls: AtomicUsize::new(0),
        };

        let out = crawl_season_with_jev(
            &client,
            "br",
            1947,
            &dir.path,
            true,
            FETCH_MIN_INTERVAL,
            Some(&judge),
            &mut |_| {},
        );

        assert!(out.error.is_some());
        assert_eq!(judge.calls.load(Ordering::SeqCst), 0);
        assert!(out.classified_failure.is_none());
    }
}
