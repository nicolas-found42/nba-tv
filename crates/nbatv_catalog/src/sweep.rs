//! The TapeCatalog sweep engine: per-game ordered rung evaluation with
//! injected probes and persisted evidence.
//!
//! [`sweep_game`] walks rungs 0–4 in ladder order for one game, reusing the
//! existing [`evaluate_sweep`] verdicts. Per rung it either resumes from a
//! fresh [`GameQuery`] row (inside the 90-day rescan window: never
//! re-probed), calls the registered probe and records the outcome, or —
//! with no probe and no history — leaves the rung unswept. The first
//! LIKELY-or-better rung wins and stops the ascent; CONFIRMED/LIKELY write
//! `tape_sources` rows, REVIEW evidence stays on the query row for
//! [`review_list`], and Reject rows only prove the rung was consumed.
//!
//! Honesty rules, all deliberate:
//!
//! - A verified tape row is never deleted on a later failed search (one
//!   failed search is never absence — same stance as the evaluator).
//! - A deferred probe (spent quota) records nothing, so the rung retries
//!   next sweep instead of burning the rescan window.
//! - No persisted "last status" column exists: [`sweep_status_for`] derives
//!   the verdict live from `game_queries` + `tape_sources`, so restarts
//!   cannot disagree with the evidence.

use crate::jev::{CandidateVerdict, DisabledJevJudge, JevJudge};
use crate::politeness::PolitenessConfig;
use crate::probe::{GameContext, ProbeCandidate, ProbeRegistry};
use crate::scorer::{confidence_for, level_to_str, parse_level, score_candidate};
use nbatv_db::{GameQuery, TapeSource};
use nbatv_ladder::exhaustion::{
    evaluate_sweep, MatchLevel, RecordedQuery, RungEvaluation, SweepStatus,
};
use nbatv_ladder::{Rung, YoutubeQuota};
use rusqlite::{Connection, Result as SqlResult};

/// What one sweep did: the derived verdict, the rungs actually probed
/// (fresh-window skips are absent), and the rungs that deferred — a
/// deferred rung recorded nothing and retries next sweep, so callers must
/// not treat it like a rescan-window skip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SweepReport {
    pub status: SweepStatus,
    pub probed: Vec<u8>,
    pub deferred: Vec<u8>,
}

/// Sweep failure: an unparseable caller timestamp, or a database error.
#[derive(Debug)]
pub enum SweepError {
    /// `now` is not a `YYYY-MM-DD`(-prefixed) date.
    BadNow(String),
    /// SQLite failure.
    Db(rusqlite::Error),
}

impl std::fmt::Display for SweepError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SweepError::BadNow(now) => write!(f, "sweep timestamp is not a date: {now:?}"),
            SweepError::Db(err) => write!(f, "archive database error: {err}"),
        }
    }
}

impl std::error::Error for SweepError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            SweepError::BadNow(_) => None,
            SweepError::Db(err) => Some(err),
        }
    }
}

impl From<rusqlite::Error> for SweepError {
    fn from(err: rusqlite::Error) -> Self {
        SweepError::Db(err)
    }
}

/// One REVIEW candidate needing a human look. REVIEW rows never become
/// `tape_sources` and dispatch never sees them — this list is their only
/// surface (the shell review card).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewItem {
    pub game_id: String,
    pub rung: u8,
    pub rung_name: String,
    pub url_or_pointer: String,
    pub title: String,
    pub query_text: String,
    pub queried_at: String,
}

/// Load the probe/scorer context for an archived game.
pub fn game_context_for(conn: &Connection, game_id: &str) -> SqlResult<Option<GameContext>> {
    Ok(nbatv_db::game_by_id(conn, game_id)?.map(|row| GameContext {
        game_id: row.game_id,
        home_team: row.home_team,
        away_team: row.away_team,
        date: row.date,
    }))
}

/// Human rung name for tape rows and review items (`internet-archive`).
/// Out-of-range rungs (never swept) degrade to `rung-N`, never a panic.
pub fn rung_name(rung: u8) -> String {
    Rung::from_rank(rung)
        .map(|r| r.name().to_owned())
        .unwrap_or_else(|| format!("rung-{rung}"))
}

/// One candidate after deterministic scoring and optional Jev reconciliation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateRank {
    /// Original source order, retained as the final deterministic tiebreak.
    pub index: usize,
    pub level: MatchLevel,
    /// Bounded evidence quality: 0 (weakest) through 3 (strongest).
    pub quality: u8,
}

/// Reconcile one deterministic verdict with the optional Jev sidecar.
///
/// Jev can only demote a positive deterministic match to REVIEW after an
/// explicit contradiction. It never promotes a candidate, and an already
/// rejected candidate stays rejected.
pub fn reconcile_candidate_level(
    deterministic: MatchLevel,
    verdict: Option<&CandidateVerdict>,
) -> MatchLevel {
    let Some(verdict) = verdict.filter(|verdict| verdict.decisive) else {
        return deterministic;
    };
    if verdict.same_game == Some(false) && deterministic > MatchLevel::Reject {
        MatchLevel::Review
    } else {
        deterministic
    }
}

/// Rank candidates without ever granting the model promotion authority.
///
/// Every candidate is scored deterministically first. Jev errors and
/// uncertain answers leave that verdict intact; a decisive contradiction
/// demotes a positive match to REVIEW. Ordering is level, bounded quality,
/// then original source index.
pub fn rank_candidates(
    game: &GameContext,
    candidates: &[ProbeCandidate],
    judge: &dyn JevJudge,
) -> Vec<CandidateRank> {
    let mut ranked: Vec<CandidateRank> = candidates
        .iter()
        .enumerate()
        .map(|(index, candidate)| {
            let deterministic = score_candidate(game, candidate);
            let verdict = judge.match_candidate(game, candidate).ok().flatten();
            let level = reconcile_candidate_level(deterministic, verdict.as_ref());
            let quality = verdict
                .filter(|verdict| verdict.decisive)
                .and_then(|verdict| verdict.quality)
                .filter(|quality| quality.is_finite())
                .map(|quality| quality.clamp(0.0, 3.0).round() as u8)
                .unwrap_or(match deterministic {
                    MatchLevel::Confirmed => 3,
                    MatchLevel::Likely => 2,
                    MatchLevel::Review => 1,
                    MatchLevel::Reject => 0,
                });
            CandidateRank {
                index,
                level,
                quality,
            }
        })
        .collect();
    ranked.sort_by(|left, right| {
        right
            .level
            .cmp(&left.level)
            .then_with(|| right.quality.cmp(&left.quality))
            .then_with(|| left.index.cmp(&right.index))
    });
    ranked
}

/// Sweep one game through rungs 0–4 in ladder order.
///
/// Consults `game_queries` to skip rungs swept inside the rescan window,
/// probes each remaining rung once, scores candidates with the catalog
/// scorer, and persists everything: every executed query becomes a row
/// (replacing stale ones), CONFIRMED/LIKELY winners become `tape_sources`
/// rows, REVIEW evidence stays on the query row. Stops ascending at the
/// first LIKELY-or-better rung. Returns the derived [`SweepStatus`] plus
/// which rungs were actually probed.
///
/// `now` is the sweep timestamp (`YYYY-MM-DD`, an RFC 3339 prefix also
/// works): it stamps new rows and anchors the 90-day freshness check.
pub fn sweep_game(
    conn: &Connection,
    game: &GameContext,
    probes: &ProbeRegistry<'_>,
    politeness: &PolitenessConfig,
    quota: &mut YoutubeQuota,
    now: &str,
) -> Result<SweepReport, SweepError> {
    sweep_game_with_judge(
        conn,
        game,
        probes,
        politeness,
        quota,
        now,
        &DisabledJevJudge,
    )
}

/// Sweep one game with deterministic candidate scoring plus an optional Jev
/// reconciliation sidecar. Per-candidate Jev failures fall back to the
/// deterministic verdict and never fail the sweep.
pub fn sweep_game_with_judge(
    conn: &Connection,
    game: &GameContext,
    probes: &ProbeRegistry<'_>,
    politeness: &PolitenessConfig,
    quota: &mut YoutubeQuota,
    now: &str,
    judge: &dyn JevJudge,
) -> Result<SweepReport, SweepError> {
    let now_days = parse_date(now).ok_or_else(|| SweepError::BadNow(now.to_owned()))?;
    let stored: std::collections::HashMap<u8, GameQuery> =
        nbatv_db::game_queries_for(conn, &game.game_id)?
            .into_iter()
            .map(|row| (row.rung, row))
            .collect();
    let mut probed: Vec<u8> = Vec::new();
    let mut deferred: Vec<u8> = Vec::new();

    let mut evals: Vec<RungEvaluation> = Vec::with_capacity(5);

    for rung in 0u8..=4 {
        // Fresh rows inside the rung's rescan window resume without
        // re-probing: the NBA catalog re-enumerates monthly (rung 0), the
        // rest of the ladder re-checks after the ladder's 90-day cadence.
        if let Some(row) = stored.get(&rung) {
            if is_fresh(&row.queried_at, now_days, rescan_days(rung)) {
                let best = parse_level(&row.best_match_level).unwrap_or(MatchLevel::Reject);
                evals.push(stored_eval(&game.game_id, row, best));
                if best >= MatchLevel::Likely {
                    break; // Fresh win: stop ascending, never re-probed.
                }
                continue;
            }
        }
        let Some(probe) = probes.get(rung) else {
            // No probe for this rung: keep prior knowledge when a stale row
            // exists, else leave the rung unswept (Sweeping, never absent).
            evals.push(cached_or_missing(&game.game_id, stored.get(&rung), rung));
            continue;
        };
        let outcome = probe.probe(game, politeness, quota);
        if outcome.deferred {
            // The query could not run: record nothing so the rung retries
            // next sweep instead of burning the rescan window.
            deferred.push(rung);
            evals.push(cached_or_missing(&game.game_id, stored.get(&rung), rung));
            continue;
        }
        probed.push(rung);
        let ranking = rank_candidates(game, &outcome.candidates, judge);
        let best = ranking
            .first()
            .map(|candidate| candidate.level)
            .unwrap_or(MatchLevel::Reject);
        let (review_url, review_title) = ranking
            .iter()
            .find(|candidate| candidate.level == MatchLevel::Review)
            .map(|candidate| {
                let candidate = &outcome.candidates[candidate.index];
                (
                    Some(candidate.url_or_pointer.clone()),
                    Some(candidate.title.clone()),
                )
            })
            .unwrap_or((None, None));
        let row = GameQuery {
            game_id: game.game_id.clone(),
            rung,
            query_text: outcome.query_text.clone(),
            queried_at: now.to_owned(),
            best_match_level: level_to_str(best).to_owned(),
            review_url,
            review_title,
        };
        nbatv_db::upsert_game_query(conn, &row)?;
        evals.push(stored_eval(&game.game_id, &row, best));
        if best >= MatchLevel::Likely {
            let winner = ranking
                .first()
                .map(|candidate| &outcome.candidates[candidate.index])
                .expect("best verdict came from these candidates");
            nbatv_db::upsert_tape_source(
                conn,
                &TapeSource {
                    game_id: game.game_id.clone(),
                    rank: rung,
                    source_class: rung_name(rung),
                    url_or_pointer: winner.url_or_pointer.clone(),
                    match_confidence: confidence_for(best),
                    verified_at: now.to_owned(),
                },
            )?;
            break;
        }
    }
    // Rungs never reached (early win) count as unconsumed — the win itself
    // still forces Playable in the evaluator regardless.
    for rung in 0u8..=4 {
        if !evals.iter().any(|e| e.rank == rung) {
            evals.push(RungEvaluation::new(rung, vec![], MatchLevel::Reject));
        }
    }
    Ok(SweepReport {
        status: status_from(conn, &game.game_id, &evals)?,
        probed,
        deferred,
    })
}

/// Derive one game's verdict from preloaded evidence: the game's stored
/// `game_queries` rows plus a caller-computed `pointers_only` flag (true
/// when every tape row is rung 5+). Callers already holding the rows skip
/// the re-reads [`sweep_status_for`] would make. No probes, no network, no
/// clock.
pub fn sweep_status_from(
    game_id: &str,
    queries: Vec<GameQuery>,
    pointers_only: bool,
) -> SweepStatus {
    let stored: std::collections::HashMap<u8, GameQuery> =
        queries.into_iter().map(|row| (row.rung, row)).collect();
    let mut evals: Vec<RungEvaluation> = Vec::with_capacity(5);
    for rung in 0u8..=4 {
        evals.push(cached_or_missing(game_id, stored.get(&rung), rung));
    }
    // The evaluator reports ExistsNotStreamable only for a literally empty
    // evaluation: five unconsumed rungs and no evidence are the same claim
    // ("never swept"), so pass the empty slice in that case.
    let evals: &[RungEvaluation] = if evals.iter().any(RungEvaluation::consumed) {
        &evals
    } else {
        &[]
    };
    evaluate_sweep(game_id, evals, pointers_only)
}

/// Derive one game's verdict live from stored evidence: `game_queries`
/// rows feed the exhaustion evaluator, pointer-only tape rows feed its
/// `pointers_only` flag. No probes, no network, no clock.
pub fn sweep_status_for(conn: &Connection, game_id: &str) -> SqlResult<SweepStatus> {
    let queries = nbatv_db::game_queries_for(conn, game_id)?;
    let sources = nbatv_db::tape_sources_for(conn, game_id)?;
    let pointers_only = !sources.is_empty() && sources.iter().all(|s| s.rank >= 5);
    Ok(sweep_status_from(game_id, queries, pointers_only))
}

/// REVIEW candidates for one game, in rung order: the shell review card.
pub fn review_list(conn: &Connection, game_id: &str) -> SqlResult<Vec<ReviewItem>> {
    Ok(nbatv_db::game_queries_for(conn, game_id)?
        .into_iter()
        .filter(|row| parse_level(&row.best_match_level) == Some(MatchLevel::Review))
        .map(review_item)
        .collect())
}

/// REVIEW candidates across all games, in game then rung order.
pub fn review_list_all(conn: &Connection) -> SqlResult<Vec<ReviewItem>> {
    Ok(nbatv_db::game_queries_all(conn)?
        .into_iter()
        .filter(|row| parse_level(&row.best_match_level) == Some(MatchLevel::Review))
        .map(review_item)
        .collect())
}

/// Maximum REVIEW items sent to the optional sidecar in one prioritization
/// pass. Later items retain their deterministic order without unbounded calls.
const MAX_REVIEW_PRIORITY_ITEMS: usize = 50;
const REVIEW_EVIDENCE_CHARS: usize = 512;

/// REVIEW candidates for one game, optionally prioritized in memory.
pub fn review_list_with_judge(
    conn: &Connection,
    game_id: &str,
    judge: &dyn JevJudge,
    query: &str,
) -> SqlResult<Vec<ReviewItem>> {
    Ok(prioritize_reviews(
        review_list(conn, game_id)?,
        judge,
        query,
    ))
}

/// REVIEW candidates across all games, optionally prioritized in memory.
pub fn review_list_all_with_judge(
    conn: &Connection,
    judge: &dyn JevJudge,
    query: &str,
) -> SqlResult<Vec<ReviewItem>> {
    Ok(prioritize_reviews(review_list_all(conn)?, judge, query))
}

/// Prioritize an already-loaded REVIEW list without mutating persistence.
/// Disabled, uncertain, failed, invalid, and unscored items all retain their
/// original relative order.
pub fn prioritize_reviews(
    items: Vec<ReviewItem>,
    judge: &dyn JevJudge,
    query: &str,
) -> Vec<ReviewItem> {
    let mut ranked: Vec<(usize, u8, ReviewItem)> = items
        .into_iter()
        .enumerate()
        .map(|(index, item)| {
            let score = if index < MAX_REVIEW_PRIORITY_ITEMS {
                let evidence = review_evidence(&item);
                judge
                    .prioritize_review(&[evidence], query)
                    .ok()
                    .flatten()
                    .filter(|score| *score <= 3)
                    .unwrap_or(0)
            } else {
                0
            };
            (index, score, item)
        })
        .collect();
    ranked.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    ranked.into_iter().map(|(_, _, item)| item).collect()
}

fn review_evidence(item: &ReviewItem) -> String {
    format!(
        "{} | {} | {} | {} | {}",
        item.game_id, item.rung_name, item.title, item.query_text, item.url_or_pointer
    )
    .chars()
    .take(REVIEW_EVIDENCE_CHARS)
    .collect()
}

fn status_from(
    conn: &Connection,
    game_id: &str,
    evals: &[RungEvaluation],
) -> SqlResult<SweepStatus> {
    let sources = nbatv_db::tape_sources_for(conn, game_id)?;
    let pointers_only = !sources.is_empty() && sources.iter().all(|s| s.rank >= 5);
    // The evaluator reports ExistsNotStreamable only for a literally empty
    // evaluation: five unconsumed rungs and no evidence are the same claim
    // ("never swept"), so pass the empty slice in that case.
    let evals: &[RungEvaluation] = if evals.iter().any(RungEvaluation::consumed) {
        evals
    } else {
        &[]
    };
    Ok(evaluate_sweep(game_id, evals, pointers_only))
}
fn stored_eval(game_id: &str, row: &GameQuery, best: MatchLevel) -> RungEvaluation {
    RungEvaluation::new(
        row.rung,
        vec![RecordedQuery::new(
            game_id,
            row.rung,
            row.query_text.clone(),
            row.queried_at.clone(),
        )],
        best,
    )
}

/// Prior knowledge when a stale row exists, else an unconsumed rung.
fn cached_or_missing(game_id: &str, cached: Option<&GameQuery>, rung: u8) -> RungEvaluation {
    match cached {
        Some(row) => {
            let best = parse_level(&row.best_match_level).unwrap_or(MatchLevel::Reject);
            stored_eval(game_id, row, best)
        }
        None => RungEvaluation::new(rung, vec![], MatchLevel::Reject),
    }
}

fn review_item(row: GameQuery) -> ReviewItem {
    ReviewItem {
        game_id: row.game_id,
        rung: row.rung,
        rung_name: rung_name(row.rung),
        url_or_pointer: row.review_url.unwrap_or_default(),
        title: row.review_title.unwrap_or_default(),
        query_text: row.query_text,
        queried_at: row.queried_at,
    }
}

/// Fresh inside the rung's rescan window: the NBA catalog (rung 0)
/// re-enumerates monthly, every other rung re-checks on the ladder's
/// 90-day cadence. Future stamps (clock skew) stay fresh rather than
/// forcing a re-probe; unparseable stamps re-probe.
fn is_fresh(queried_at: &str, now_days: i64, window_days: i64) -> bool {
    match parse_date(queried_at) {
        Some(queried) if queried <= now_days => now_days - queried < window_days,
        Some(_) => true,
        None => false,
    }
}

/// Rescan window for one rung, in days.
fn rescan_days(rung: u8) -> i64 {
    if rung == 0 {
        crate::nba_probe::NBA_CATALOG_REFRESH_DAYS as i64
    } else {
        nbatv_ladder::exhaustion::RESCAN_AFTER_DAYS as i64
    }
}

/// Parse a `YYYY-MM-DD`(-prefixed) date to days since the Unix epoch
/// (Howard Hinnant's days-from-civil; std-only, no date dependency).
fn parse_date(s: &str) -> Option<i64> {
    let bytes = s.as_bytes();
    if bytes.len() < 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    let year: i64 = s[0..4].parse().ok()?;
    let month: i64 = s[5..7].parse().ok()?;
    let day: i64 = s[8..10].parse().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    Some(days_from_civil(year, month, day))
}

fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let adjusted_year = if month <= 2 { year - 1 } else { year };
    let era = if adjusted_year >= 0 {
        adjusted_year
    } else {
        adjusted_year - 399
    } / 400;
    let year_of_era = adjusted_year - era * 400;
    let month_prime = (month + 9) % 12;
    let day_of_year = (153 * month_prime + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146097 + day_of_era - 719468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_math_matches_the_rescan_windows() {
        let (d90, d30) = (rescan_days(1), rescan_days(0));
        assert_eq!(d90, 90);
        assert_eq!(d30, 30);
        // Rungs 1-4: 90-day cadence.
        assert!(is_fresh(
            "2026-01-01",
            parse_date("2026-01-15").unwrap(),
            d90
        ));
        assert!(is_fresh(
            "2026-01-01",
            parse_date("2026-03-31").unwrap(),
            d90
        ));
        assert!(!is_fresh(
            "2026-01-01",
            parse_date("2026-04-01").unwrap(),
            d90
        ));
        // Rung 0: the NBA catalog re-enumerates monthly — a row 30+ days
        // old is due even though the 90-day ladder window would call it
        // fresh.
        assert!(is_fresh(
            "2026-01-01",
            parse_date("2026-01-30").unwrap(),
            d30
        ));
        assert!(!is_fresh(
            "2026-01-01",
            parse_date("2026-01-31").unwrap(),
            d30
        ));
        assert!(!is_fresh(
            "2026-01-01",
            parse_date("2026-02-15").unwrap(),
            d30
        ));
        assert!(parse_date("2026-13-01").is_none());
        assert!(parse_date("2026-1-1").is_none());
        // RFC 3339 timestamps work: only the date prefix is read.
        assert_eq!(parse_date("2026-01-01T00:00:00Z"), parse_date("2026-01-01"));
    }
    #[test]
    fn rung_names_come_from_the_ladder() {
        assert_eq!(rung_name(1), "internet-archive");
        assert_eq!(rung_name(2), "youtube");
        assert_eq!(rung_name(9), "rung-9");
    }
}
