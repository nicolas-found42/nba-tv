// Playoffs-ledger fixture, reduced from the real crawled page
// `data/raw/br/1946-47/194704220PHW.html` (1947 BAA Finals Game 5, the
// championship clincher — 1947-04-22, Philadelphia Warriors 83, Chicago
// Stags 80). Everything below is that page's own markup (scorebox_meta
// date, the two `box-*-game-basic` tables reduced to one player row plus
// Team Totals, and the `game_summaries playoffs` section with its real
// `1947 BAA Finals` round label) — whitespace and attributes preserved.
// Tiny text fixture per ADR 0002: no bulk media, no network.
//
// The paired negative control is the regular-season opener page
// `194611010TRH.html` (November 1, 1946), whose only game_summaries div is
// `game_summaries compressed` (no `playoffs` class).

use nbatv_db::rusqlite::Connection;
use nbatv_ingest::{ingest_snapshot_dir, parse_box_page};

const FINALS_PAGE_HTML: &str = r#"<div class="scorebox_meta">
		<div>April 22, 1947</div><div>Philadelphia Arena, Philadelphia, Pennsylvania</div>
</div>
<div class="game_summaries playoffs compressed">
   <h2>1947 BAA Finals</h2>
   <div class="game_summary expanded nohover current "> <table class="teams poptip" data-tip="Chicago Stags at Philadelphia Warriors"> <tbody> <tr class="date"><td colspan=3>Game 5, Apr 22</td></tr> <tr class="loser"> <td><a href="/teams/CHS/1947.html">CHS</a></td> <td class="right">80</td> <td class="right gamelink"> <a href="/boxscores/194704220PHW.html">F<span class="no_mobile">inal</span></a> </td> </tr> <tr class="winner"> <td><strong><a href="/teams/PHW/1947.html">PHW</a></strong></td> <td class="right">83</td> <td class="right">&nbsp; </td> </tr> </tbody> </table> </div>
</div>
<table class="sortable stats_table" id="box-PHW-game-basic" data-cols-to-freeze=",1">
<tbody>
<tr ><th scope="row" class="left " data-append-csv="fulksjo01" data-stat="player" csk="Fulks,Joe" ><a href="/players/f/fulksjo01.html">Joe Fulks</a></th><td class="right iz" data-stat="mp" ></td><td class="right " data-stat="fg" >10</td><td class="right " data-stat="fga" >34</td><td class="right " data-stat="fg_pct" >.294</td><td class="right " data-stat="ft" >14</td><td class="right " data-stat="fta" >18</td><td class="right " data-stat="ft_pct" >.778</td><td class="right iz" data-stat="orb" ></td><td class="right iz" data-stat="drb" ></td><td class="right iz" data-stat="trb" ></td><td class="right iz" data-stat="ast" >0</td><td class="right iz" data-stat="stl" ></td><td class="right iz" data-stat="blk" ></td><td class="right iz" data-stat="tov" ></td><td class="right " data-stat="pf" >4</td><td class="right " data-stat="pts" >34</td></tr>
<tr ><th scope="row" class="left " data-stat="player" >Team Totals</th><td class="right " data-stat="mp" >240</td><td class="right " data-stat="fg" >26</td><td class="right " data-stat="fga" >101</td><td class="right " data-stat="fg_pct" >.257</td><td class="right " data-stat="ft" >31</td><td class="right " data-stat="fta" >44</td><td class="right " data-stat="ft_pct" >.705</td><td class="right iz" data-stat="orb" ></td><td class="right iz" data-stat="drb" ></td><td class="right iz" data-stat="trb" ></td><td class="right " data-stat="ast" >10</td><td class="right iz" data-stat="stl" ></td><td class="right iz" data-stat="blk" ></td><td class="right iz" data-stat="tov" ></td><td class="right " data-stat="pf" >21</td><td class="right " data-stat="pts" >83</td></tr>
</tbody></table>
<table class="sortable stats_table" id="box-CHS-game-basic" data-cols-to-freeze=",1">
<tbody>
<tr ><th scope="row" class="left " data-stat="player" >Team Totals</th><td class="right " data-stat="mp" >240</td><td class="right " data-stat="fg" >30</td><td class="right " data-stat="fga" >117</td><td class="right " data-stat="fg_pct" >.256</td><td class="right " data-stat="ft" >20</td><td class="right " data-stat="fta" >29</td><td class="right " data-stat="ft_pct" >.690</td><td class="right iz" data-stat="orb" ></td><td class="right iz" data-stat="drb" ></td><td class="right iz" data-stat="trb" ></td><td class="right " data-stat="ast" >8</td><td class="right iz" data-stat="stl" ></td><td class="right iz" data-stat="blk" ></td><td class="right iz" data-stat="tov" ></td><td class="right " data-stat="pf" >33</td><td class="right " data-stat="pts" >80</td></tr>
</tbody></table>"#;

const OPENER_PAGE_HTML: &str = r#"<div class="scorebox_meta">
		<div>November 1, 1946</div><div>Maple Leaf Gardens, Toronto, Canada</div>
</div>
<div class="game_summaries compressed">
   <h2>BAA Scores &mdash; Nov 1, 1946</h2>
</div>
<table class="sortable stats_table" id="box-NYK-game-basic" data-cols-to-freeze=",1">
<tbody>
<tr ><th scope="row" class="left " data-stat="player" >Team Totals</th><td class="right " data-stat="mp" >240</td><td class="right " data-stat="fg" >22</td><td class="right " data-stat="fga" >60</td><td class="right " data-stat="ft" >24</td><td class="right " data-stat="fta" >30</td><td class="right " data-stat="pf" >22</td><td class="right " data-stat="pts" >68</td></tr>
</tbody></table>
<table class="sortable stats_table" id="box-TRH-game-basic" data-cols-to-freeze=",1">
<tbody>
<tr ><th scope="row" class="left " data-stat="player" >Team Totals</th><td class="right " data-stat="mp" >240</td><td class="right " data-stat="fg" >20</td><td class="right " data-stat="fga" >55</td><td class="right " data-stat="ft" >26</td><td class="right " data-stat="fta" >34</td><td class="right " data-stat="pf" >20</td><td class="right " data-stat="pts" >66</td></tr>
</tbody></table>"#;

/// The Finals fixture parses through the unchanged box parser: two team
/// rows, the Fulks player row, and the real page date (season 1947).
#[test]
fn finals_fixture_parses_like_the_real_page() {
    let (teams, players) = parse_box_page(FINALS_PAGE_HTML);
    assert_eq!(teams.len(), 2);
    let phw = teams.iter().find(|t| t.team_br == "PHW").unwrap();
    assert_eq!((phw.fga, phw.ft, phw.pts), (Some(101), Some(31), Some(83)));
    // Era clamp (1947): rebounds/steals/blocks/turnovers/threes are None.
    assert_eq!(phw.reb, None);
    assert_eq!(phw.stl, None);
    let chs = teams.iter().find(|t| t.team_br == "CHS").unwrap();
    assert_eq!(chs.pts, Some(80));
    let fulks = players.iter().find(|p| p.player_br == "fulksjo01").unwrap();
    assert_eq!(
        (fulks.pts, fulks.ast),
        (Some(34), None),
        "per-game assists pre-1950-51 are era-blanked"
    );
}

/// Scratch dir that removes itself on drop (repo convention: no tempdir crate).
struct TempDir(std::path::PathBuf);

impl TempDir {
    fn fresh(tag: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("nbatv-ingest-po-{tag}-{}", std::process::id()));
        // The ingest walks season directories: 1946-47 under the crawl root.
        std::fs::create_dir_all(dir.join("1946-47")).expect("temp dir");
        TempDir(dir)
    }

    fn write(&self, name: &str, body: &str) {
        std::fs::write(
            self.0.join("1946-47").join(name),
            nbatv_ingest::gzip_encode(body.as_bytes()),
        )
        .expect("fixture file");
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The Finals snapshot ingests as `PLAYOFFS` while the regular-season
/// opener stays `REGULAR` — both from the pages' own `game_summaries`
/// class, the marker every archived crawl page carries.
#[test]
fn box_playoff_marker_sets_the_ledger() {
    let conn = Connection::open_in_memory().expect("in-memory db");
    let crawl = TempDir::fresh("playoffs");
    // Schedule pages (the authority for which games exist) carry no round
    // marker — real shape: both games on one monthly page.
    crawl.write(
        "_games-april.html",
        r#"<table class="stats_table" id="schedule"><tbody>
<tr><th data-stat="date_game" csk="194704220PHW">Tue, Apr 22, 1947</th>
<td data-stat="visitor_team_name"><a href="/teams/CHS/1947.html">Chicago Stags</a></td>
<td data-stat="home_team_name"><a href="/teams/PHW/1947.html">Philadelphia Warriors</a></td>
<td data-stat="box_score_text"><a href="/boxscores/194704220PHW.html">Box Score</a></td></tr>
<tr><th data-stat="date_game" csk="194611010TRH">Fri, Nov 1, 1946</th>
<td data-stat="visitor_team_name"><a href="/teams/NYK/1947.html">New York Knicks</a></td>
<td data-stat="home_team_name"><a href="/teams/TRH/1947.html">Toronto Huskies</a></td>
<td data-stat="box_score_text"><a href="/boxscores/194611010TRH.html">Box Score</a></td></tr>
</tbody></table>"#,
    );
    crawl.write("194704220PHW.html", FINALS_PAGE_HTML);
    crawl.write("194611010TRH.html", OPENER_PAGE_HTML);

    let report = ingest_snapshot_dir(&conn, crawl.0.as_path()).expect("ingest");
    assert_eq!(report.games, 2);
    assert_eq!(report.games_with_box, 2);

    let game_type = |id: &str| -> String {
        conn.query_row(
            "SELECT game_type FROM games WHERE game_id = ?1",
            [id],
            |r| r.get(0),
        )
        .expect("game row")
    };
    assert_eq!(
        game_type("194704220PHW"),
        "PLAYOFFS",
        "the Finals page carries the marker"
    );
    assert_eq!(
        game_type("194611010TRH"),
        "REGULAR",
        "the opener page does not"
    );
}

/// A game whose box snapshot is absent has no signal at all: it stays
/// `REGULAR` (never guessed from the calendar) until the crawl adds the page.
#[test]
fn bare_rows_without_box_snapshot_stay_regular() {
    let conn = Connection::open_in_memory().expect("in-memory db");
    let crawl = TempDir::fresh("bare");
    crawl.write(
        "_games-april.html",
        r#"<table class="stats_table" id="schedule"><tbody>
<tr><th data-stat="date_game" csk="194704160PHW">Wed, Apr 16, 1947</th>
<td data-stat="visitor_team_name"><a href="/teams/CHS/1947.html">Chicago Stags</a></td>
<td data-stat="home_team_name"><a href="/teams/PHW/1947.html">Philadelphia Warriors</a></td>
<td data-stat="box_score_text"><a href="/boxscores/194704160PHW.html">Box Score</a></td></tr>
</tbody></table>"#,
    );

    let report = ingest_snapshot_dir(&conn, crawl.0.as_path()).expect("ingest");
    assert_eq!(report.games_without_box, 1);
    let game_type: String = conn
        .query_row(
            "SELECT game_type FROM games WHERE game_id = '194704160PHW'",
            [],
            |r| r.get(0),
        )
        .expect("game row");
    assert_eq!(game_type, "REGULAR");
}
