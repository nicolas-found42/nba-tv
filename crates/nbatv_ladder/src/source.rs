//! `TapeSource` record: one place that holds a Game Tape.
//!
//! Shape per the batch Contract (owned by `nbatv_db` §4; duplicated here so
//! this crate builds standalone — integration dedups onto `nbatv_db` later).

use crate::rung::{PlaybackClass, Rung};

/// One place that holds a Game Tape.
///
/// `game_id` is the Basketball-Reference box-score slug (e.g. `194611010TRH`).
/// `rank` is the ladder rung 0–7. `match_confidence` is the scorer output in
/// `0.0..=1.0`.
#[derive(Debug, Clone, PartialEq)]
pub struct TapeSource {
    pub game_id: String,
    pub rank: u8,
    pub source_class: String,
    pub url_or_pointer: String,
    pub match_confidence: f32,
    pub verified_at: String,
}

impl TapeSource {
    pub fn new(
        game_id: impl Into<String>,
        rank: u8,
        source_class: impl Into<String>,
        url_or_pointer: impl Into<String>,
        match_confidence: f32,
        verified_at: impl Into<String>,
    ) -> Self {
        Self {
            game_id: game_id.into(),
            rank,
            source_class: source_class.into(),
            url_or_pointer: url_or_pointer.into(),
            match_confidence,
            verified_at: verified_at.into(),
        }
    }

    /// The ladder rung, or `None` for an out-of-range rank.
    pub fn rung(&self) -> Option<Rung> {
        Rung::from_rank(self.rank)
    }

    /// Playback class inherited from the rung, if the rank is valid.
    pub fn playback(&self) -> Option<PlaybackClass> {
        self.rung().map(|r| r.playback())
    }

    /// Whether this source can yield bytes to the Player Backend / Cache Tier.
    /// Only rungs 1 and 4 serve progressive files; rungs 0/2/3 are
    /// embed-only External Surfaces and rungs 5/6/7 are pointers.
    pub fn bytes_available(&self) -> bool {
        self.rung().is_some_and(|r| r.cache_tier_eligible())
    }

    /// Pointer rows (rungs 5–7) are existence metadata, never stream URLs.
    pub fn is_pointer(&self) -> bool {
        self.playback() == Some(PlaybackClass::Pointer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(rank: u8) -> TapeSource {
        TapeSource::new(
            "194611010TRH",
            rank,
            "test",
            "pointer-or-url",
            0.9,
            "2026-09-07",
        )
    }

    #[test]
    fn contract_shape_fields_in_order() {
        let s = source(2);
        assert_eq!(s.game_id, "194611010TRH");
        assert_eq!(s.rank, 2);
        assert_eq!(s.source_class, "test");
        assert_eq!(s.url_or_pointer, "pointer-or-url");
        assert_eq!(s.match_confidence, 0.9);
        assert_eq!(s.verified_at, "2026-09-07");
    }

    #[test]
    fn bytes_only_on_rungs_1_and_4() {
        for rank in 0u8..=7 {
            assert_eq!(
                source(rank).bytes_available(),
                matches!(rank, 1 | 4),
                "rank {rank}"
            );
        }
    }

    #[test]
    fn pointer_rungs_are_marked_pointer() {
        for rank in [5u8, 6, 7] {
            let s = source(rank);
            assert!(s.is_pointer());
            assert!(!s.bytes_available());
        }
        assert!(!source(2).is_pointer());
    }
}
