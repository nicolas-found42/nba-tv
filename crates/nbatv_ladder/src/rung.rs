//! Rung order 0–7 with per-rung playback class and legality mark.
//!
//! Playback classes (CONTEXT.md + ladder v2 §1): **ProgressiveFile** = bytes
//! reachable over HTTP by the app; **ExternalSurface** = plays only in a
//! vendor player; **Pointer** = existence metadata only, never playable.

/// How a rung's tape can be played, if at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlaybackClass {
    /// Progressive file bytes reachable by the app (MP4 over Range).
    ProgressiveFile,
    /// Plays only in a vendor/browser player, never as app bytes.
    ExternalSurface,
    /// Existence metadata only; never a stream URL, never bytes.
    Pointer,
}

/// Acquisition-legality mark per ladder v2 §1 table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Legality {
    /// League-owned / licensed channel (rungs 0, 6, 7).
    Clean,
    /// Public retrieval of third-party uploads; enforcement out of scope (rung 1).
    GreyNote,
    /// Unlicensed rehost; stream-a-public-URL posture (rungs 2, 3).
    Grey,
    /// Keyless public mechanism; corpus ~0 today (rung 4).
    MethodClean,
    /// Unlicensed copies; pointer only, never bytes (rung 5 catalogs/trade
    /// channels; the Lost Media Wiki lane inside rung 5 is clean metadata).
    GreyPointerOnly,
}

/// One rung of the Source Ladder. Discriminant == ladder rank.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Rung {
    /// 0 — Official NBA free tier (NBA ID catalog; Classic Games playlist proxy).
    Official = 0,
    /// 1 — Internet Archive (advancedsearch metadata sweeps).
    InternetArchive = 1,
    /// 2 — YouTube fan/team corpus (+ choucisan/nba_games crosswalk).
    YouTube = 2,
    /// 3 — Non-Anglo rehost cluster (VK, OK.ru, CDA.pl, Bilibili; Dailymotion pass).
    RehostCluster = 3,
    /// 4 — Standing empty-corpus sweep (Odysee, PeerTube).
    EmptyCorpus = 4,
    /// 5 — Collector catalogs & fan networks (existence pointers only).
    Collector = 5,
    /// 6 — Purchase-only (licensed physical media; League Pass excluded by $0).
    Purchase = 6,
    /// 7 — Institutional & newsreel archives (Paley, UCLA FTVA, Pathé, Getty).
    Institutional = 7,
}

impl Rung {
    /// Ladder rank 0–7.
    pub fn rank(self) -> u8 {
        self as u8
    }

    /// Rung for a rank, or `None` outside 0–7.
    pub fn from_rank(rank: u8) -> Option<Self> {
        match rank {
            0 => Some(Self::Official),
            1 => Some(Self::InternetArchive),
            2 => Some(Self::YouTube),
            3 => Some(Self::RehostCluster),
            4 => Some(Self::EmptyCorpus),
            5 => Some(Self::Collector),
            6 => Some(Self::Purchase),
            7 => Some(Self::Institutional),
            _ => None,
        }
    }

    /// Short human name for UI/shell glyphs.
    pub fn name(self) -> &'static str {
        match self {
            Self::Official => "official-nba-free-tier",
            Self::InternetArchive => "internet-archive",
            Self::YouTube => "youtube",
            Self::RehostCluster => "non-anglo-rehost-cluster",
            Self::EmptyCorpus => "standing-empty-corpus",
            Self::Collector => "collector-catalogs",
            Self::Purchase => "purchase-only",
            Self::Institutional => "institutional",
        }
    }

    /// Playback class per the batch Contract: 1+4 file; 0+2+3 surface; 5+6+7 pointer.
    pub fn playback(self) -> PlaybackClass {
        match self {
            Self::InternetArchive | Self::EmptyCorpus => PlaybackClass::ProgressiveFile,
            Self::Official | Self::YouTube | Self::RehostCluster => PlaybackClass::ExternalSurface,
            Self::Collector | Self::Purchase | Self::Institutional => PlaybackClass::Pointer,
        }
    }

    /// Legality mark per ladder v2 §1.
    pub fn legality(self) -> Legality {
        match self {
            Self::Official | Self::Purchase | Self::Institutional => Legality::Clean,
            Self::InternetArchive => Legality::GreyNote,
            Self::YouTube | Self::RehostCluster => Legality::Grey,
            Self::EmptyCorpus => Legality::MethodClean,
            Self::Collector => Legality::GreyPointerOnly,
        }
    }

    /// A Game moves down the ladder only after the higher rung returned no
    /// CONFIRMED/LIKELY match; byte-eligible rungs are 1 and 4 (plus licensed
    /// purchase via 6). Embed-only rungs 0, 2, 3 never feed the Cache Tier.
    pub fn cache_tier_eligible(self) -> bool {
        matches!(self, Self::InternetArchive | Self::EmptyCorpus)
    }
}

/// The fixed ladder: rungs 0–7 in search order.
pub struct Ladder;

impl Ladder {
    /// Rungs 0–7 in the exact order a Game is searched.
    pub fn ordered() -> impl DoubleEndedIterator<Item = Rung> {
        (0u8..=7).filter_map(Rung::from_rank)
    }

    /// Free-stream rungs whose recorded queries gate exhaustion (0–4).
    pub fn stream_rungs() -> impl DoubleEndedIterator<Item = Rung> {
        (0u8..=4).filter_map(Rung::from_rank)
    }

    /// Pointer-only rungs: existence metadata, never bytes (5–7).
    pub fn pointer_rungs() -> impl DoubleEndedIterator<Item = Rung> {
        (5u8..=7).filter_map(Rung::from_rank)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rung_order_is_exactly_0_through_7() {
        let ranks: Vec<u8> = Ladder::ordered().map(|r| r.rank()).collect();
        assert_eq!(ranks, vec![0, 1, 2, 3, 4, 5, 6, 7]);
        assert_eq!(Ladder::ordered().count(), 8);
    }

    #[test]
    fn playback_marks_match_contract() {
        let classes: Vec<(u8, PlaybackClass)> = Ladder::ordered()
            .map(|r| (r.rank(), r.playback()))
            .collect();
        assert_eq!(
            classes,
            vec![
                (0, PlaybackClass::ExternalSurface),
                (1, PlaybackClass::ProgressiveFile),
                (2, PlaybackClass::ExternalSurface),
                (3, PlaybackClass::ExternalSurface),
                (4, PlaybackClass::ProgressiveFile),
                (5, PlaybackClass::Pointer),
                (6, PlaybackClass::Pointer),
                (7, PlaybackClass::Pointer),
            ]
        );
    }

    #[test]
    fn legality_marks_match_ladder_v2_table() {
        let marks: Vec<(u8, Legality)> = Ladder::ordered()
            .map(|r| (r.rank(), r.legality()))
            .collect();
        assert_eq!(
            marks,
            vec![
                (0, Legality::Clean),
                (1, Legality::GreyNote),
                (2, Legality::Grey),
                (3, Legality::Grey),
                (4, Legality::MethodClean),
                (5, Legality::GreyPointerOnly),
                (6, Legality::Clean),
                (7, Legality::Clean),
            ]
        );
    }

    #[test]
    fn from_rank_round_trips_and_rejects_8() {
        for rank in 0u8..=7 {
            assert_eq!(Rung::from_rank(rank).unwrap().rank(), rank);
        }
        assert_eq!(Rung::from_rank(8), None);
        assert_eq!(Rung::from_rank(255), None);
    }

    #[test]
    fn names_are_unique_and_cache_eligibility_is_rungs_1_and_4() {
        let mut names: Vec<&str> = Ladder::ordered().map(|r| r.name()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), 8);
        for rung in Ladder::ordered() {
            assert_eq!(
                rung.cache_tier_eligible(),
                matches!(rung, Rung::InternetArchive | Rung::EmptyCorpus)
            );
        }
    }
}
