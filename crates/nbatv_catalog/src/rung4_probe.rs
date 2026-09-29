//! Rung-4 probe: the standing empty-corpus sweep (issue #24).
//!
//! Rung 4 holds the two bytes-reachable-but-empty platforms (Odysee via
//! JSON-RPC `claim_search`, PeerTube via Sepia Search REST — both keyless,
//! per ladder v2 §rung 4 and research 07 §2.6–2.7). Measured classic-NBA
//! yield is ≈ 0 (noise: 2K gameplay, vlogs, TikTok rips), and the rung stays
//! because enumeration is free and re-scans are cheap.
//!
//! This probe therefore never claims a find: every call records one standing
//! query whose text documents the method, with zero candidates expected. That
//! standing record is what makes an all-rungs-empty sweep read **Unavailable**
//! (honest: the rung was consumed) instead of **Sweeping** (never checked).
//! It spends no YouTube quota and needs no politeness pacing (no HTTP here —
//! the live keyless endpoints are re-checked by hand, not per game).
//!
//! Playback note: rung 4 is file-class (progressive MP4, Cache
//! Tier-eligible) *if content ever appears*; legality is Method clean (any
//! content found would be the uploader's licensing problem).

use crate::politeness::PolitenessConfig;
use crate::probe::{GameContext, ProbeOutcome, SourceProbe};
use nbatv_ladder::YoutubeQuota;

/// Standing rung-4 query text for one game: names both keyless endpoints so
/// the recorded row documents the method that was swept.
pub fn rung4_query_text(game: &GameContext) -> String {
    format!(
        "standing empty-corpus sweep for {} ({} @ {} {}): \
         Odysee claim_search + PeerTube Sepia Search; \
         zero candidates expected, corpus ~0 per ladder v2 rung 4",
        game.game_id, game.away_team, game.home_team, game.date,
    )
}

/// The rung-4 probe: records the standing query, finds nothing, spends
/// nothing.
#[derive(Debug, Clone, Copy, Default)]
pub struct Rung4Probe;

impl Rung4Probe {
    /// Stateless standing probe.
    pub fn new() -> Self {
        Self
    }
}

impl SourceProbe for Rung4Probe {
    fn rung(&self) -> u8 {
        4
    }

    fn name(&self) -> &'static str {
        "standing-empty-corpus"
    }

    fn probe(
        &self,
        game: &GameContext,
        _politeness: &PolitenessConfig,
        _quota: &mut YoutubeQuota,
    ) -> ProbeOutcome {
        ProbeOutcome::empty(rung4_query_text(game))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::probe::{ProbeRegistry, ScriptedProbe};

    fn game() -> GameContext {
        GameContext {
            game_id: "194611010TRH".to_owned(),
            home_team: "TRH".to_owned(),
            away_team: "NYK".to_owned(),
            date: "1946-11-01".to_owned(),
        }
    }

    #[test]
    fn standing_query_is_recorded_with_zero_candidates() {
        let outcome = Rung4Probe::new().probe(
            &game(),
            &PolitenessConfig::default(),
            &mut YoutubeQuota::new(),
        );
        assert!(!outcome.deferred, "the query ran; the rung is consumed");
        assert!(outcome.candidates.is_empty(), "corpus ~0: nothing expected");
        assert!(outcome.query_text.contains("Odysee"), "{outcome:?}");
        assert!(outcome.query_text.contains("PeerTube"), "{outcome:?}");
        assert!(outcome.query_text.contains("194611010TRH"), "{outcome:?}");
    }

    #[test]
    fn rung_and_name_match_the_ladder() {
        let probe = Rung4Probe::new();
        assert_eq!(probe.rung(), 4);
        assert_eq!(probe.name(), "standing-empty-corpus");
    }

    #[test]
    fn probe_spends_no_youtube_quota() {
        let g = game();
        let mut quota = YoutubeQuota::new();
        let before = quota.remaining();
        Rung4Probe::new().probe(&g, &PolitenessConfig::default(), &mut quota);
        assert_eq!(quota.remaining(), before);
    }

    #[test]
    fn full_empty_sweep_with_recorded_rung4_is_honest_unavailable() {
        let conn = nbatv_db::open_in_memory().unwrap();
        nbatv_db::create_schema(&conn).unwrap();
        let g = game();
        let nba = crate::nba_probe::NbaProbe::new();
        let empties: Vec<ScriptedProbe> = (1u8..=3)
            .map(|rung| {
                ScriptedProbe::responding(rung, ProbeOutcome::empty(format!("fake rung {rung}")))
            })
            .collect();
        let standing = Rung4Probe::new();
        let mut registry = ProbeRegistry::new();
        registry.register(&nba);
        for fake in &empties {
            registry.register(fake);
        }
        registry.register(&standing);
        let report = crate::sweep::sweep_game(
            &conn,
            &g,
            &registry,
            &PolitenessConfig::default(),
            &mut YoutubeQuota::new(),
            "2026-09-08",
        )
        .unwrap();
        assert_eq!(report.probed, vec![0, 1, 2, 3, 4]);
        assert_eq!(
            report.status,
            nbatv_ladder::SweepStatus::Unavailable {
                consumed_label: format!("ladder consumed {}", g.game_id),
            }
        );
        let queries = nbatv_db::game_queries_for(&conn, &g.game_id).unwrap();
        let rungs: Vec<u8> = queries.iter().map(|q| q.rung).collect();
        assert_eq!(
            rungs,
            vec![0, 1, 2, 3, 4],
            "every rung recorded: honest Unavailable"
        );
        let rung4 = queries.iter().find(|q| q.rung == 4).unwrap();
        assert!(rung4.query_text.contains("Odysee"), "{rung4:?}");
    }
}
