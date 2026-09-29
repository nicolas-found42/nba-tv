//! Exhaustion runner: per-game ordered candidate evaluation over rungs 0–4.
//!
//! Per ladder v2 §3: a Game is marked **unavailable** only when rungs 0–4
//! have each been queried with the Game's query set, each query recorded
//! with timestamp and query text, and no candidate reached LIKELY or better
//! — or when only rung 5–7 pointers exist (`EXISTS_NOT_STREAMABLE`).
//! Nothing is marked absent on a single failed search.

use crate::rung::Ladder;

/// One recorded outbound query against a rung. No crawling happens here;
/// this is the record the runner evaluates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedQuery {
    pub game_id: String,
    pub rank: u8,
    pub query_text: String,
    pub verified_at: String,
}

impl RecordedQuery {
    pub fn new(
        game_id: impl Into<String>,
        rank: u8,
        query_text: impl Into<String>,
        verified_at: impl Into<String>,
    ) -> Self {
        Self {
            game_id: game_id.into(),
            rank,
            query_text: query_text.into(),
            verified_at: verified_at.into(),
        }
    }
}

/// Scorer verdict for one candidate (ladder v2 §3: CONFIRMED / LIKELY /
/// REVIEW / Reject over title + description + duration).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MatchLevel {
    Reject,
    Review,
    Likely,
    Confirmed,
}

/// Evaluation outcome for one rung: the recorded queries plus the best
/// candidate verdict seen on that rung.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RungEvaluation {
    pub rank: u8,
    pub queries: Vec<RecordedQuery>,
    pub best: MatchLevel,
}

impl RungEvaluation {
    pub fn new(rank: u8, queries: Vec<RecordedQuery>, best: MatchLevel) -> Self {
        Self {
            rank,
            queries,
            best,
        }
    }

    /// A rung counts as consumed only when at least one query was recorded.
    pub fn consumed(&self) -> bool {
        !self.queries.is_empty()
    }
}

/// Sweep outcome for one Game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SweepStatus {
    /// A rung 0–4 candidate reached LIKELY+; UI shows the tape button.
    Playable { rank: u8 },
    /// Rungs 0–4 not all consumed yet — keep sweeping, never mark absent.
    Sweeping,
    /// Rungs 0–4 all consumed with no LIKELY+ candidate.
    Unavailable { consumed_label: String },
    /// Only rung 5–7 pointers exist: tape exists, not streamable.
    ExistsNotStreamable,
}

/// Hint for the next sweep. Absent Games are re-checked after 90 days
/// (ladder v2 §3); rung 0 re-enumerated monthly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RescanHint {
    pub game_id: String,
    pub not_before_days: u64,
}

/// Days before an unavailable Game is re-checked.
pub const RESCAN_AFTER_DAYS: u64 = 90;

/// 90-day re-scan hint for a Game.
pub fn rescan_hint(game_id: impl Into<String>) -> RescanHint {
    RescanHint {
        game_id: game_id.into(),
        not_before_days: RESCAN_AFTER_DAYS,
    }
}

/// Evaluate one Game's sweep over rungs 0–4, in ladder order.
///
/// * Lowest rung with `best >= Likely` wins → `Playable`.
/// * Otherwise, if any rung 0–4 has no recorded query → `Sweeping`.
/// * Otherwise → `Unavailable`.
/// * Empty evaluation with `pointers_only` → `ExistsNotStreamable`.
pub fn evaluate_sweep(game_id: &str, evals: &[RungEvaluation], pointers_only: bool) -> SweepStatus {
    let mut by_rank: [Option<&RungEvaluation>; 5] = [None, None, None, None, None];
    for eval in evals {
        if eval.rank <= 4 {
            by_rank[eval.rank as usize] = Some(eval);
        }
    }
    for rung in Ladder::stream_rungs() {
        let rank = rung.rank() as usize;
        if let Some(eval) = by_rank[rank] {
            if eval.best >= MatchLevel::Likely {
                return SweepStatus::Playable { rank: rank as u8 };
            }
        }
    }
    let all_consumed = by_rank
        .iter()
        .all(|slot| slot.is_some_and(|eval| eval.consumed()));
    if all_consumed {
        return SweepStatus::Unavailable {
            consumed_label: format!("ladder consumed {game_id}"),
        };
    }
    if pointers_only && evals.is_empty() {
        return SweepStatus::ExistsNotStreamable;
    }
    SweepStatus::Sweeping
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recorded(game_id: &str, rank: u8) -> RecordedQuery {
        RecordedQuery::new(
            game_id,
            rank,
            format!("query for rung {rank}"),
            "2026-09-07",
        )
    }

    fn eval(rank: u8, best: MatchLevel) -> RungEvaluation {
        RungEvaluation::new(rank, vec![recorded("194611010TRH", rank)], best)
    }

    fn full_reject_sweep() -> Vec<RungEvaluation> {
        (0u8..=4)
            .map(|rank| eval(rank, MatchLevel::Reject))
            .collect()
    }

    #[test]
    fn unavailable_only_after_rungs_0_to_4_all_recorded() {
        // Full sweep, nothing LIKELY+ → unavailable.
        assert!(matches!(
            evaluate_sweep("194611010TRH", &full_reject_sweep(), false),
            SweepStatus::Unavailable { .. }
        ));
        // REVIEW-best full sweep is still unavailable (needs LIKELY+).
        let review: Vec<RungEvaluation> = (0u8..=4)
            .map(|rank| eval(rank, MatchLevel::Review))
            .collect();
        assert!(matches!(
            evaluate_sweep("194611010TRH", &review, false),
            SweepStatus::Unavailable { .. }
        ));
    }

    #[test]
    fn partial_sweep_never_marks_unavailable() {
        // Rungs 0–3 consumed, rung 4 untouched → still sweeping.
        let partial: Vec<RungEvaluation> = (0u8..=3)
            .map(|rank| eval(rank, MatchLevel::Reject))
            .collect();
        assert_eq!(
            evaluate_sweep("194611010TRH", &partial, false),
            SweepStatus::Sweeping
        );
        // A rung entry with zero recorded queries does not count as consumed.
        let mut gap = full_reject_sweep();
        gap[4] = RungEvaluation::new(4, vec![], MatchLevel::Reject);
        assert_eq!(
            evaluate_sweep("194611010TRH", &gap, false),
            SweepStatus::Sweeping
        );
        // Single failed search is never absence.
        assert_eq!(
            evaluate_sweep("194611010TRH", &[eval(0, MatchLevel::Reject)], false),
            SweepStatus::Sweeping
        );
    }

    #[test]
    fn likely_or_better_plays_lowest_rung_first() {
        let mut evals = full_reject_sweep();
        evals[2] = eval(2, MatchLevel::Likely);
        evals[4] = eval(4, MatchLevel::Confirmed);
        assert_eq!(
            evaluate_sweep("194611010TRH", &evals, false),
            SweepStatus::Playable { rank: 2 }
        );
    }

    #[test]
    fn pointer_only_games_are_exists_not_streamable() {
        assert_eq!(
            evaluate_sweep("194611010TRH", &[], true),
            SweepStatus::ExistsNotStreamable
        );
        assert_eq!(
            evaluate_sweep("194611010TRH", &[], false),
            SweepStatus::Sweeping
        );
    }

    #[test]
    fn rescan_hint_is_90_days() {
        let hint = rescan_hint("194611010TRH");
        assert_eq!(hint.game_id, "194611010TRH");
        assert_eq!(hint.not_before_days, 90);
        assert_eq!(RESCAN_AFTER_DAYS, 90);
    }
}
