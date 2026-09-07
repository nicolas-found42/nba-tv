//! Rung 2 crosswalk: choucisan/nba_games YouTube identity bootstrap.
//!
//! Per ladder v2 §2 (rung 2): the MIT-licensed choucisan/nba_games list maps
//! 189 verified full-length YouTube games to official NBA.com game IDs
//! (`0021500874`-style), records named `YYYY-MM-DD-away-vs-home`. This is
//! the drop-in bootstrap for per-game YouTube identity matching. The table
//! below is a tiny inline fixture in that shape — tests only, no network.

/// One crosswalk row: YouTube video ↔ NBA.com game ↔ archive game.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CrosswalkRow {
    /// 11-char YouTube video id.
    pub youtube_id: &'static str,
    /// Official NBA.com game id (`0021500874`-style).
    pub nba_game_id: &'static str,
    /// Archive `game_id`: Basketball-Reference box-score slug.
    pub game_id: &'static str,
}

/// Tiny inline crosswalk fixture (3 rows, choucisan shape). Tests only.
pub const CHOUCISAN_FIXTURE: &[CrosswalkRow] = &[
    CrosswalkRow {
        youtube_id: "XGS8aqV9XhU",
        nba_game_id: "0021401229",
        game_id: "201506160CLE",
    },
    CrosswalkRow {
        youtube_id: "mK7q2pL4zQwA",
        nba_game_id: "0041400306",
        game_id: "199806140CHI",
    },
    CrosswalkRow {
        youtube_id: "fT3nR8sK1vBm",
        nba_game_id: "0047900306",
        game_id: "198005160PHI",
    },
];

/// Lookup table over the crosswalk fixture.
pub struct Crosswalk;

impl Crosswalk {
    /// All rows.
    pub fn rows() -> &'static [CrosswalkRow] {
        CHOUCISAN_FIXTURE
    }

    /// Find a row by YouTube video id.
    pub fn by_youtube(youtube_id: &str) -> Option<&'static CrosswalkRow> {
        CHOUCISAN_FIXTURE
            .iter()
            .find(|row| row.youtube_id == youtube_id)
    }

    /// Find a row by official NBA.com game id.
    pub fn by_nba_id(nba_game_id: &str) -> Option<&'static CrosswalkRow> {
        CHOUCISAN_FIXTURE
            .iter()
            .find(|row| row.nba_game_id == nba_game_id)
    }

    /// Find a row by archive `game_id` (BR slug).
    pub fn by_game_id(game_id: &str) -> Option<&'static CrosswalkRow> {
        CHOUCISAN_FIXTURE.iter().find(|row| row.game_id == game_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crosswalk_fixture_resolves_all_rows_both_directions() {
        assert_eq!(Crosswalk::rows().len(), 3);
        for row in Crosswalk::rows() {
            // youtube ↔ game_id
            assert_eq!(
                Crosswalk::by_youtube(row.youtube_id)
                    .expect("youtube row")
                    .game_id,
                row.game_id
            );
            assert_eq!(
                Crosswalk::by_game_id(row.game_id)
                    .expect("game row")
                    .youtube_id,
                row.youtube_id
            );
            // nba_game_id ↔ game_id
            assert_eq!(
                Crosswalk::by_nba_id(row.nba_game_id)
                    .expect("nba row")
                    .game_id,
                row.game_id
            );
            assert_eq!(
                Crosswalk::by_game_id(row.game_id)
                    .expect("game row")
                    .nba_game_id,
                row.nba_game_id
            );
            // youtube ↔ nba_game_id
            assert_eq!(
                Crosswalk::by_youtube(row.youtube_id)
                    .expect("youtube row")
                    .nba_game_id,
                row.nba_game_id
            );
            assert_eq!(
                Crosswalk::by_nba_id(row.nba_game_id)
                    .expect("nba row")
                    .youtube_id,
                row.youtube_id
            );
        }
    }

    #[test]
    fn crosswalk_misses_return_none() {
        assert_eq!(Crosswalk::by_youtube("AAAAAAAAAAA"), None);
        assert_eq!(Crosswalk::by_nba_id("0000000000"), None);
        assert_eq!(Crosswalk::by_game_id("19000101XXX"), None);
    }
    #[test]
    fn crosswalk_fixture_game_ids_match_br_slug_shape() {
        // Contract shape ^\d{9}[A-Z]{3}$ (12 chars): the fixture must join
        // games.game_id, so every row is held to the validator shape.
        for row in Crosswalk::rows() {
            let b = row.game_id.as_bytes();
            assert_eq!(b.len(), 12, "slug length: {}", row.game_id);
            assert!(
                b[..9].iter().all(|c| c.is_ascii_digit()),
                "date part: {}",
                row.game_id
            );
            assert!(
                b[9..].iter().all(|c| c.is_ascii_uppercase()),
                "team part: {}",
                row.game_id
            );
        }
    }
}
