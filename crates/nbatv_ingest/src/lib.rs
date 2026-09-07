//! Bootstrap ingest for the nba-tv personal NBA archive.
//!
//! Day-zero input is the FiveThirtyEight `nbaallelo.csv` dump (CC BY 4.0):
//! one CSV whose `game_id` is the Basketball-Reference box-score slug.
//! This crate parses that CSV into a games skeleton plus the defunct-team
//! slug crosswalk, filters ABA rows (the `league` value is kept on each
//! row for a future toggle), validates BR slugs, builds raw-snapshot
//! paths, and tracks frozen-season / `meta-revised` re-crawl hints.
//!
//! No network fetches, no media downloads. Paths only — this crate never
//! writes outside caller-supplied (test-temp) locations.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Game identity + type
// ---------------------------------------------------------------------------

/// `REGULAR` | `PLAYOFFS` | `NBA_CUP` (matches the `games.game_type` CHECK).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameType {
    Regular,
    Playoffs,
    NbaCup,
}

impl GameType {
    pub fn as_str(self) -> &'static str {
        match self {
            GameType::Regular => "REGULAR",
            GameType::Playoffs => "PLAYOFFS",
            GameType::NbaCup => "NBA_CUP",
        }
    }
}

/// BR box-score slug shape: `^\d{9}[A-Z]{3}$`, e.g. `194611010TRH`.
/// Intentional mirror of `nbatv_db::is_valid_game_id` (contract-pinned):
/// kept local so this crate stays dependency-free; change both together.
pub fn validate_game_id(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 12
        && b[..9].iter().all(|c| c.is_ascii_digit())
        && b[9..].iter().all(|c| c.is_ascii_uppercase())
}

// ---------------------------------------------------------------------------
// FTE CSV bootstrap
// ---------------------------------------------------------------------------

/// One data row of `nbaallelo.csv`. `league` is the raw `lg_id` value
/// (`BAA`/`NBA`/`ABA`) — kept verbatim so a future ABA toggle can reuse
/// the same parse.
#[derive(Debug, Clone, PartialEq)]
pub struct FteRow {
    pub game_id: String,
    pub league: String,
    pub is_copy: bool,
    pub season_year: i32,
    pub date: String,
    pub is_playoffs: bool,
    pub team: String,
    pub franchise: String,
    pub points: i32,
    pub opp_team: String,
    pub opp_franchise: String,
    pub opp_points: i32,
}

impl FteRow {
    pub fn is_aba(&self) -> bool {
        self.league == "ABA"
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ParseError {
    MissingColumn(&'static str),
    BadValue(String),
    NoRows,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::MissingColumn(c) => write!(f, "missing column: {c}"),
            ParseError::BadValue(v) => write!(f, "bad value: {v}"),
            ParseError::NoRows => write!(f, "no data rows"),
        }
    }
}

impl std::error::Error for ParseError {}

/// Split one CSV line (handles `"quoted,fields"` and `""` escapes).
fn split_csv_line(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut cur = String::new();
    let mut chars = line.chars().peekable();
    let mut in_quotes = false;
    while let Some(c) = chars.next() {
        match c {
            '"' if in_quotes => {
                if chars.peek() == Some(&'"') {
                    cur.push('"');
                    chars.next();
                } else {
                    in_quotes = false;
                }
            }
            '"' => in_quotes = true,
            ',' if !in_quotes => {
                fields.push(cur);
                cur = String::new();
            }
            _ => cur.push(c),
        }
    }
    fields.push(cur);
    fields
}

fn col_index(header: &[String], name: &'static str) -> Result<usize, ParseError> {
    header
        .iter()
        .position(|h| h.trim() == name)
        .ok_or(ParseError::MissingColumn(name))
}

fn parse_flag(cell: &str, what: &str) -> Result<bool, ParseError> {
    match cell.trim() {
        "0" => Ok(false),
        "1" => Ok(true),
        other => Err(ParseError::BadValue(format!("{what}={other}"))),
    }
}

/// Parse FTE CSV text into one row per line (both `_iscopy` sides kept;
/// see [`skeleton_games`] for per-game dedup). Column order is free —
/// lookup is by header name. Returns `NoRows` when there are no data rows.
pub fn parse_fte_csv(text: &str) -> Result<Vec<FteRow>, ParseError> {
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    let header_line = lines.next().ok_or(ParseError::NoRows)?;
    let header = split_csv_line(header_line);

    let (
        c_game,
        c_lg,
        c_copy,
        c_year,
        c_date,
        c_po,
        c_team,
        c_fran,
        c_pts,
        c_opp,
        c_oppfran,
        c_opppts,
    ) = (
        col_index(&header, "game_id")?,
        col_index(&header, "lg_id")?,
        col_index(&header, "_iscopy")?,
        col_index(&header, "year_id")?,
        col_index(&header, "date_game")?,
        col_index(&header, "is_playoffs")?,
        col_index(&header, "team_id")?,
        col_index(&header, "fran_id")?,
        col_index(&header, "pts")?,
        col_index(&header, "opp_id")?,
        col_index(&header, "opp_fran")?,
        col_index(&header, "opp_pts")?,
    );

    let mut rows = Vec::new();
    for line in lines {
        let f = split_csv_line(line);
        let get = |i: usize| f.get(i).map(|s| s.trim().to_owned()).unwrap_or_default();
        let int = |i: usize, what: String| {
            get(i)
                .parse::<i32>()
                .map_err(|_| ParseError::BadValue(what))
        };
        let game_id = get(c_game);
        if !validate_game_id(&game_id) {
            return Err(ParseError::BadValue(format!("game_id={game_id}")));
        }
        rows.push(FteRow {
            game_id,
            league: get(c_lg),
            is_copy: parse_flag(&get(c_copy), "_iscopy")?,
            season_year: int(c_year, format!("year_id={}", get(c_year)))?,
            date: get(c_date),
            is_playoffs: parse_flag(&get(c_po), "is_playoffs")?,
            team: get(c_team),
            franchise: get(c_fran),
            points: int(c_pts, format!("pts={}", get(c_pts)))?,
            opp_team: get(c_opp),
            opp_franchise: get(c_oppfran),
            opp_points: int(c_opppts, format!("opp_pts={}", get(c_opppts)))?,
        });
    }
    if rows.is_empty() {
        return Err(ParseError::NoRows);
    }
    Ok(rows)
}

/// Drop ABA rows (`lg_id == "ABA"`); keep BAA + NBA. The `league` value
/// stays on each surviving row for the future inclusion toggle.
pub fn filter_nba_only(rows: Vec<FteRow>) -> Vec<FteRow> {
    rows.into_iter().filter(|r| !r.is_aba()).collect()
}

/// Team-slug → franchise crosswalk over both sides of every row
/// (supplies the defunct-team slug list).
pub fn franchise_crosswalk(rows: &[FteRow]) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    for r in rows {
        map.insert(r.team.clone(), r.franchise.clone());
        map.insert(r.opp_team.clone(), r.opp_franchise.clone());
    }
    map
}

// ---------------------------------------------------------------------------
// Games skeleton
// ---------------------------------------------------------------------------

/// One game bootstrapped from the FTE dump (no home/away flag in the
/// source, so sides are `team_a`/`team_b` in file order; BR enrichment
/// resolves home/away later).
#[derive(Debug, Clone, PartialEq)]
pub struct SkeletonGame {
    pub game_id: String,
    pub league: String,
    pub season_year: i32,
    pub date: String,
    pub game_type: GameType,
    pub team_a: String,
    pub team_b: String,
    pub team_a_pts: i32,
    pub team_b_pts: i32,
}

/// Collapse mirrored FTE rows to one skeleton game per `game_id`,
/// in first-seen (file) order.
pub fn skeleton_games(rows: &[FteRow]) -> Vec<SkeletonGame> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for r in rows {
        if !seen.insert(r.game_id.clone()) {
            continue;
        }
        out.push(SkeletonGame {
            game_id: r.game_id.clone(),
            league: r.league.clone(),
            season_year: r.season_year,
            date: r.date.clone(),
            game_type: if r.is_playoffs {
                GameType::Playoffs
            } else {
                GameType::Regular
            },
            team_a: r.team.clone(),
            team_b: r.opp_team.clone(),
            team_a_pts: r.points,
            team_b_pts: r.opp_points,
        });
    }
    out
}

// ---------------------------------------------------------------------------
// Raw snapshots, frozen seasons, re-crawl hints
// ---------------------------------------------------------------------------

/// Raw-snapshot location for one fetched page/dump:
/// `data/raw/{source}/{season}/{file_name}`. Paths only — the caller
/// decides where (never outside temp in tests).
pub fn raw_snapshot_path(source: &str, season: &str, file_name: &str) -> PathBuf {
    Path::new("data")
        .join("raw")
        .join(source)
        .join(season)
        .join(file_name)
}

/// Freeze state of one season. Completed seasons are fetched once after
/// the Finals, then frozen; only corrections re-open them.
#[derive(Debug, Clone, PartialEq)]
pub struct SeasonState {
    pub season_year: i32,
    pub frozen: bool,
}

impl SeasonState {
    pub fn current(season_year: i32) -> Self {
        SeasonState {
            season_year,
            frozen: false,
        }
    }

    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    pub fn is_frozen(&self) -> bool {
        self.frozen
    }
}

/// One scraped page plus its BR `meta-revised` timestamp (opaque string).
#[derive(Debug, Clone, PartialEq)]
pub struct PageRevision {
    pub page: String,
    pub meta_revised: Option<String>,
}

/// Re-crawl hint: true when the page carries a `meta-revised` stamp we
/// have not seen before (opaque comparison — the format is BR's own).
/// Missing stamp ⇒ no signal ⇒ false.
pub fn recrawl_hint(last_seen: Option<&str>, current: &PageRevision) -> bool {
    match (&current.meta_revised, last_seen) {
        (None, _) => false,
        (Some(_), None) => true,
        (Some(cur), Some(seen)) => cur != seen,
    }
}

// ---------------------------------------------------------------------------
// Tests (tiny inline fixtures only; never network)
// ---------------------------------------------------------------------------

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
