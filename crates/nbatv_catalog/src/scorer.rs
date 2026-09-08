//! Catalog scorer: [`MatchLevel`] verdicts over probe evidence.
//!
//! Probes return evidence, never verdicts — this module is the single place
//! those verdicts are computed, so fakes and (later) real probes are judged
//! by the same rule. The heuristics reuse the ladder's CONFIRMED / LIKELY /
//! REVIEW / Reject semantics over title + description + duration:
//!
//! - both teams named + full-tape evidence → CONFIRMED
//! - both teams named, no full-tape marker → LIKELY
//! - one team, or a full-tape claim naming no team, or a highlights/short
//!   clip of the game → REVIEW (needs a human look, never auto-plays)
//! - neither team named and no full-tape claim → Reject
//!
//! Full-tape evidence is a "full game" marker in the text or a reported
//! duration of at least 90 minutes; a clip is a "highlight" marker or a
//! reported duration under 20 minutes. Unknown duration is neither.

use crate::probe::{GameContext, ProbeCandidate};
use nbatv_ladder::exhaustion::MatchLevel;

/// Full-length tape threshold: 90 minutes in seconds.
pub const FULL_TAPE_SECS: u64 = 5_400;
/// Short-clip threshold: 20 minutes in seconds.
pub const CLIP_SECS: u64 = 1_200;

/// Score one probe candidate for one game.
pub fn score_candidate(game: &GameContext, candidate: &ProbeCandidate) -> MatchLevel {
    let hay = format!("{} {}", candidate.title, candidate.description).to_lowercase();
    let tokens = game.team_tokens();
    let hits = tokens
        .iter()
        .filter(|t| !t.is_empty() && hay.contains(t.as_str()))
        .count();
    let full = hay.contains("full game")
        || hay.contains("full-game")
        || hay.contains("fullgame")
        || candidate.duration_secs.is_some_and(|d| d >= FULL_TAPE_SECS);
    let clip = hay.contains("highlight") || candidate.duration_secs.is_some_and(|d| d < CLIP_SECS);
    match (hits, full, clip) {
        (2, true, false) => MatchLevel::Confirmed,
        (2, false, false) => MatchLevel::Likely,
        (2, _, _) => MatchLevel::Review,
        (1, _, _) => MatchLevel::Review,
        (0, true, _) => MatchLevel::Review,
        _ => MatchLevel::Reject,
    }
}

/// Tape-row confidence for a playable verdict (CONFIRMED 1.0, LIKELY 0.85).
/// REVIEW/Reject never reach tape rows; mapping them is a bug, so they
/// share the 0.0 floor.
pub fn confidence_for(level: MatchLevel) -> f32 {
    match level {
        MatchLevel::Confirmed => 1.0,
        MatchLevel::Likely => 0.85,
        MatchLevel::Review | MatchLevel::Reject => 0.0,
    }
}

/// Database spelling of a verdict (`game_queries.best_match_level`).
pub fn level_to_str(level: MatchLevel) -> &'static str {
    match level {
        MatchLevel::Confirmed => "confirmed",
        MatchLevel::Likely => "likely",
        MatchLevel::Review => "review",
        MatchLevel::Reject => "reject",
    }
}

/// Parse a stored verdict; `None` for rows predating the vocabulary.
pub fn parse_level(s: &str) -> Option<MatchLevel> {
    match s {
        "confirmed" => Some(MatchLevel::Confirmed),
        "likely" => Some(MatchLevel::Likely),
        "review" => Some(MatchLevel::Review),
        "reject" => Some(MatchLevel::Reject),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game() -> GameContext {
        GameContext {
            game_id: "194611010TRH".to_owned(),
            home_team: "TRH".to_owned(),
            away_team: "NYK".to_owned(),
            date: "1946-11-01".to_owned(),
        }
    }

    fn candidate(title: &str, description: &str, duration_secs: Option<u64>) -> ProbeCandidate {
        ProbeCandidate {
            url_or_pointer: "https://example.com/x".to_owned(),
            title: title.to_owned(),
            description: description.to_owned(),
            duration_secs,
        }
    }

    #[test]
    fn scorer_reaches_all_four_verdicts() {
        let game = game();
        assert_eq!(
            score_candidate(
                &game,
                &candidate(
                    "NYK at TRH Full Game 1946",
                    "Complete broadcast",
                    Some(7_200)
                )
            ),
            MatchLevel::Confirmed
        );
        assert_eq!(
            score_candidate(
                &game,
                &candidate("NYK at TRH 1946-11-01", "Archive upload", None)
            ),
            MatchLevel::Likely
        );
        assert_eq!(
            score_candidate(
                &game,
                &candidate("NYK highlights 1946", "Short reel", Some(600))
            ),
            MatchLevel::Review
        );
        assert_eq!(
            score_candidate(
                &game,
                &candidate("Cats playing piano compilation", "Unrelated", None)
            ),
            MatchLevel::Reject
        );
    }

    #[test]
    fn clips_and_bare_claims_need_a_human() {
        let game = game();
        // Both teams but highlights-only: the game, not the tape.
        assert_eq!(
            score_candidate(
                &game,
                &candidate("NYK at TRH highlights", "Top plays", Some(7_200))
            ),
            MatchLevel::Review
        );
        // Full-tape claim naming no team: possible match, human look.
        assert_eq!(
            score_candidate(
                &game,
                &candidate("Full game 1946", "Unknown teams", Some(7_200))
            ),
            MatchLevel::Review
        );
    }

    #[test]
    fn levels_round_trip_through_their_database_spelling() {
        for level in [
            MatchLevel::Confirmed,
            MatchLevel::Likely,
            MatchLevel::Review,
            MatchLevel::Reject,
        ] {
            assert_eq!(parse_level(level_to_str(level)), Some(level));
        }
        assert_eq!(parse_level("maybe"), None);
    }

    #[test]
    fn confidence_maps_playable_verdicts_only() {
        assert_eq!(confidence_for(MatchLevel::Confirmed), 1.0);
        assert_eq!(confidence_for(MatchLevel::Likely), 0.85);
        assert_eq!(confidence_for(MatchLevel::Review), 0.0);
        assert_eq!(confidence_for(MatchLevel::Reject), 0.0);
    }
}
