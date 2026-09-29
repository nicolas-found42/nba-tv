//! In-file unit tests for the FTE CSV bootstrap + games skeleton +
//! snapshot-path/hint bookkeeping (moved verbatim from the pre-split
//! single-file lib.rs).

#![allow(unused_imports)]

use std::collections::BTreeSet;
use std::path::PathBuf;

use super::super::csv::*;
use super::super::GameType;

#[cfg(test)]
mod tests {
    use super::*;

    /// Mini FTE-shaped fixture: first game ever (both `_iscopy` sides),
    /// a second BAA game, one ABA pair, one 2015 playoff game.
    const FIXTURE: &str = "\
gameorder,game_id,lg_id,_iscopy,year_id,date_game,seasongame,is_playoffs,team_id,fran_id,pts,opp_id,opp_fran,opp_pts
1,194611010TRH,BAA,0,1947,11/1/1946,1,0,NYK,Knicks,68,TRH,Huskies,66
2,194611010TRH,BAA,1,1947,11/1/1946,1,0,TRH,Huskies,66,NYK,Knicks,68
3,194611020CHS,BAA,0,1947,11/2/1946,1,0,CHS,Stags,63,PIT,Ironmen,55
4,194611020CHS,BAA,1,1947,11/2/1946,1,0,PIT,Ironmen,55,CHS,Stags,63
5,196710130OAK,ABA,0,1968,10/13/1967,1,0,OAK,Oaks,109,ANA,Amigos,99
6,196710130OAK,ABA,1,1968,10/13/1967,1,0,ANA,Amigos,99,OAK,Oaks,109
7,201506170CLE,NBA,0,2015,6/16/2015,102,1,GSW,Warriors,105,CLE,Cavaliers,97
8,201506170CLE,NBA,1,2015,6/16/2015,102,1,CLE,Cavaliers,97,GSW,Warriors,105
";

    #[test]
    fn fte_fixture_parses_to_skeleton_games_with_correct_slugs() {
        let rows = parse_fte_csv(FIXTURE).unwrap();
        assert_eq!(rows.len(), 8);

        let kept = filter_nba_only(rows);
        // ABA pair (2 rows) dropped, BAA + NBA kept.
        assert_eq!(kept.len(), 6);
        assert!(kept.iter().all(|r| !r.is_aba()));

        let games = skeleton_games(&kept);
        let ids: Vec<&str> = games.iter().map(|g| g.game_id.as_str()).collect();
        assert_eq!(ids, vec!["194611010TRH", "194611020CHS", "201506170CLE"]);

        let first = &games[0];
        assert_eq!(first.team_a, "NYK");
        assert_eq!(first.team_b, "TRH");
        assert_eq!((first.team_a_pts, first.team_b_pts), (68, 66));
        assert_eq!(first.league, "BAA");
        assert_eq!(first.season_year, 1947);
        assert_eq!(first.game_type, GameType::Regular);

        let last = &games[2];
        assert_eq!(last.game_type, GameType::Playoffs);
        assert_eq!(last.league, "NBA");
        assert_eq!((last.team_a.as_str(), last.team_b.as_str()), ("GSW", "CLE"));
    }

    #[test]
    fn aba_rows_filtered_nba_baa_kept() {
        let rows = parse_fte_csv(FIXTURE).unwrap();
        let aba: Vec<&FteRow> = rows.iter().filter(|r| r.is_aba()).collect();
        assert_eq!(aba.len(), 2);
        // League value is kept verbatim on the struct for the future toggle.
        assert!(aba.iter().all(|r| r.league == "ABA"));
        assert_eq!(aba[0].game_id, "196710130OAK");

        let kept = filter_nba_only(rows);
        let leagues: BTreeSet<&str> = kept.iter().map(|r| r.league.as_str()).collect();
        assert_eq!(leagues, BTreeSet::from(["BAA", "NBA"]));
    }

    #[test]
    fn franchise_crosswalk_covers_defunct_slugs() {
        let rows = parse_fte_csv(FIXTURE).unwrap();
        let map = franchise_crosswalk(&rows);
        assert_eq!(map.get("TRH").map(String::as_str), Some("Huskies"));
        assert_eq!(map.get("PIT").map(String::as_str), Some("Ironmen"));
        assert_eq!(map.get("GSW").map(String::as_str), Some("Warriors"));
        // ABA side present pre-filter (toggle source material).
        assert_eq!(map.get("OAK").map(String::as_str), Some("Oaks"));
    }

    #[test]
    fn game_id_validator_accepts_slug_rejects_garbage() {
        assert!(validate_game_id("194611010TRH"));
        assert!(validate_game_id("201506170CLE"));
        for bad in [
            "",
            "194611010TR",
            "194611010TRHX",
            "19461101OTRH",
            "194611010trh",
            "0024600001",
            "194611010TR ",
            " 194611010TRH",
            "1946-11010TRH",
        ] {
            assert!(!validate_game_id(bad), "must reject {bad:?}");
        }
    }

    #[test]
    fn parse_rejects_garbage_game_id_and_missing_columns() {
        let bad_id = FIXTURE.replacen("194611010TRH", "not-a-game", 1);
        assert!(matches!(
            parse_fte_csv(&bad_id),
            Err(ParseError::BadValue(_))
        ));
        assert!(matches!(
            parse_fte_csv("game_id,lg_id\n194611010TRH,BAA\n"),
            Err(ParseError::MissingColumn(_))
        ));
        assert!(matches!(parse_fte_csv(""), Err(ParseError::NoRows)));
        assert!(matches!(
            parse_fte_csv("game_id,lg_id,_iscopy,year_id,date_game,is_playoffs,team_id,fran_id,pts,opp_id,opp_fran,opp_pts\n"),
            Err(ParseError::NoRows)
        ));
    }

    #[test]
    fn raw_snapshot_path_shape() {
        assert_eq!(
            raw_snapshot_path("br-box", "BAA_1947", "194611010TRH.html.gz"),
            PathBuf::from("data/raw/br-box/BAA_1947/194611010TRH.html.gz")
        );
        assert_eq!(
            raw_snapshot_path("fte", "bootstrap", "nbaallelo.csv"),
            PathBuf::from("data/raw/fte/bootstrap/nbaallelo.csv")
        );
    }

    #[test]
    fn frozen_season_and_recrawl_hint() {
        let mut s = SeasonState::current(1947);
        assert!(!s.is_frozen());
        s.freeze();
        assert!(s.is_frozen());

        let page = |rev: Option<&str>| PageRevision {
            page: "BAA_1947_games.html".to_owned(),
            meta_revised: rev.map(str::to_owned),
        };
        // Never seen + stamp present => fetch/hint true.
        assert!(recrawl_hint(None, &page(Some("16:31:52 03-Sep-2026"))));
        // Same stamp as last fetch => frozen, no re-crawl.
        assert!(!recrawl_hint(
            Some("16:31:52 03-Sep-2026"),
            &page(Some("16:31:52 03-Sep-2026"))
        ));
        // BR revised the page after our fetch => re-crawl.
        assert!(recrawl_hint(
            Some("16:31:52 03-Sep-2026"),
            &page(Some("09:00:00 04-Sep-2026"))
        ));
        // No stamp on page => no signal, never hint.
        assert!(!recrawl_hint(None, &page(None)));
        assert!(!recrawl_hint(Some("x"), &page(None)));
    }

    #[test]
    fn game_type_labels_match_db_check() {
        assert_eq!(GameType::Regular.as_str(), "REGULAR");
        assert_eq!(GameType::Playoffs.as_str(), "PLAYOFFS");
        assert_eq!(GameType::NbaCup.as_str(), "NBA_CUP");
    }
}
