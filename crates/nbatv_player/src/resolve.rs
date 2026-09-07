//! Play-time resolution: Cache Tier entry → `tape_sources` by rank → unavailable.
//!
//! Mirrors research 02 section 3.3. The player resolves public URLs at play
//! time and rehosts nothing. The app is fully functional with zero Cache
//! Tier usage: the cache is an accelerator, never a dependency.
//!
//! [`TapeSource`] intentionally repeats the cross-crate contract shape
//! (`tape_sources(game_id, rank, source_class, url_or_pointer,
//! match_confidence, verified_at)`); integration dedups onto `nbatv_db`
//! later, so each crate keeps an identical local copy for now.

/// One row of the `tape_sources` table (local contract copy).
///
/// `game_id` is the Basketball-Reference box-score slug (also the FTE CSV
/// id), e.g. `194611010TRH`.
#[derive(Debug, Clone, PartialEq)]
pub struct TapeSource {
    pub game_id: String,
    pub rank: u8,
    pub source_class: String,
    pub url_or_pointer: String,
    pub match_confidence: f32,
    pub verified_at: String,
}

/// A Cache Tier entry for one game: a normalized MP4 (H.264/AAC,
/// moov-first) held in the driver's personal file store and streamable back
/// over progressive HTTP `Range`. Never a stream-only surface: S0/S2/S3
/// bytes are played in place and are never pulled into the cache.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheEntry {
    pub game_id: String,
    /// Local path or direct progressive URL of the normalized MP4.
    pub location: String,
}

/// What play-time resolution settled on for one game.
#[derive(Debug, Clone, PartialEq)]
pub enum Resolved {
    /// A Cache Tier copy exists; play it (most stable, user-owned).
    CacheTier { game_id: String, location: String },
    /// No cache entry; play the best-ranked ladder row in place.
    Tape(TapeSource),
    /// Cache missed and the ladder is consumed: honestly unavailable.
    /// (Only tape can be unavailable; the Box Score always renders.)
    Unavailable { game_id: String },
}

/// Resolve what to play for `game_id`.
///
/// Order: `cache` (when it names this game) first, then the lowest-`rank`
/// row of `sources` naming this game, then [`Resolved::Unavailable`].
/// Rows naming other games are ignored. Rank ties keep the caller's order
/// (stable minimum).
pub fn resolve(game_id: &str, cache: Option<CacheEntry>, sources: &[TapeSource]) -> Resolved {
    if let Some(entry) = cache {
        if entry.game_id == game_id {
            return Resolved::CacheTier {
                game_id: entry.game_id,
                location: entry.location,
            };
        }
    }
    let best = sources
        .iter()
        .filter(|s| s.game_id == game_id)
        .min_by_key(|s| s.rank)
        .cloned();
    match best {
        Some(source) => Resolved::Tape(source),
        None => Resolved::Unavailable {
            game_id: game_id.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(game_id: &str, rank: u8, url: &str) -> TapeSource {
        TapeSource {
            game_id: game_id.to_string(),
            rank,
            source_class: "test".to_string(),
            url_or_pointer: url.to_string(),
            match_confidence: 1.0,
            verified_at: "2026-09-07".to_string(),
        }
    }

    #[test]
    fn cache_entry_wins_over_ranked_sources() {
        let cache = Some(CacheEntry {
            game_id: "194611010TRH".to_string(),
            location: "/cache/194611010TRH.mp4".to_string(),
        });
        let sources = vec![
            source("194611010TRH", 0, "https://example.com/official"),
            source("194611010TRH", 1, "https://archive.org/download/x/y.mp4"),
        ];
        assert_eq!(
            resolve("194611010TRH", cache, &sources),
            Resolved::CacheTier {
                game_id: "194611010TRH".to_string(),
                location: "/cache/194611010TRH.mp4".to_string(),
            }
        );
    }

    #[test]
    fn lowest_rank_wins_without_cache() {
        let sources = vec![
            source("194611010TRH", 3, "https://example.com/rehost"),
            source("194611010TRH", 1, "https://archive.org/download/x/y.mp4"),
            source("194611010TRH", 2, "https://www.youtube.com/watch?v=abc"),
        ];
        let resolved = resolve("194611010TRH", None, &sources);
        match resolved {
            Resolved::Tape(best) => {
                assert_eq!(best.rank, 1);
                assert_eq!(best.url_or_pointer, "https://archive.org/download/x/y.mp4");
            }
            other => panic!("expected Tape, got {other:?}"),
        }
    }

    #[test]
    fn empty_ladder_resolves_unavailable() {
        assert_eq!(
            resolve("194611010TRH", None, &[]),
            Resolved::Unavailable {
                game_id: "194611010TRH".to_string(),
            }
        );
    }

    #[test]
    fn foreign_game_rows_are_ignored() {
        let sources = vec![source("OTHER00000000", 0, "https://example.com/official")];
        assert_eq!(
            resolve("194611010TRH", None, &sources),
            Resolved::Unavailable {
                game_id: "194611010TRH".to_string(),
            }
        );
    }

    #[test]
    fn foreign_game_cache_entry_does_not_win() {
        let cache = Some(CacheEntry {
            game_id: "OTHER00000000".to_string(),
            location: "/cache/OTHER00000000.mp4".to_string(),
        });
        let sources = vec![source("194611010TRH", 2, "https://example.com/rehost")];
        match resolve("194611010TRH", cache, &sources) {
            Resolved::Tape(best) => assert_eq!(best.rank, 2),
            other => panic!("expected Tape, got {other:?}"),
        }
    }
}
