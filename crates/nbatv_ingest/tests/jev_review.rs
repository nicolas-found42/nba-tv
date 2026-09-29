use std::fs;
use std::path::Path;
use std::sync::Mutex;

use nbatv_catalog::{
    CandidateVerdict, CollectorNoteInput, CrawlFailureChoice, FileSelection, FileSelectionInput,
    GameContext, GameTypeChoice, HtmlRowChoice, JevError, JevJudge, ProbeCandidate,
    SearchTemplateSelection, TeamChoice,
};
use nbatv_ingest::{ingest_snapshot_dir_with_jev, review_ingest_snapshots};
use std::path::PathBuf;

struct TestDir(PathBuf);

impl TestDir {
    fn fresh(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("nbatv-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("test directory");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
use rusqlite::Connection;

struct ReviewJudge {
    game_type: Option<&'static str>,
    team: Option<&'static str>,
    observed_game_type_evidence: Mutex<Vec<String>>,
    observed_html_chars: Mutex<Vec<usize>>,
}

impl ReviewJudge {
    fn new(game_type: Option<&'static str>, team: Option<&'static str>) -> Self {
        Self {
            game_type,
            team,
            observed_game_type_evidence: Mutex::new(Vec::new()),
            observed_html_chars: Mutex::new(Vec::new()),
        }
    }
}

impl JevJudge for ReviewJudge {
    fn select_file(&self, _: &FileSelectionInput) -> Result<Option<FileSelection>, JevError> {
        Ok(None)
    }

    fn match_candidate(
        &self,
        _: &GameContext,
        _: &ProbeCandidate,
    ) -> Result<Option<CandidateVerdict>, JevError> {
        Ok(None)
    }

    fn classify_game_type(&self, input: &GameTypeChoice) -> Result<Option<String>, JevError> {
        self.observed_game_type_evidence
            .lock()
            .expect("observed game type evidence lock")
            .push(input.page_text.clone());
        Ok(self.game_type.map(str::to_owned))
    }

    fn align_team(&self, _: &TeamChoice) -> Result<Option<String>, JevError> {
        Ok(self.team.map(str::to_owned))
    }

    fn match_collector_note(&self, _: &CollectorNoteInput) -> Result<Option<String>, JevError> {
        Ok(None)
    }

    fn choose_search_template(
        &self,
        _: &GameContext,
        _: &str,
    ) -> Result<Option<SearchTemplateSelection>, JevError> {
        Ok(None)
    }

    fn classify_crawl_failure(&self, _: &str) -> Result<Option<CrawlFailureChoice>, JevError> {
        Ok(None)
    }

    fn classify_html_row(
        &self,
        fragment: &str,
        _: &str,
    ) -> Result<Option<HtmlRowChoice>, JevError> {
        self.observed_html_chars
            .lock()
            .expect("observed lengths lock")
            .push(fragment.chars().count());
        Ok(Some(HtmlRowChoice::HeaderOrFooter))
    }

    fn route_shell_command(&self, _: &str) -> Result<Option<String>, JevError> {
        Ok(None)
    }

    fn prioritize_review(&self, _: &[String], _: &str) -> Result<Option<u8>, JevError> {
        Ok(None)
    }
}

fn schedule_page() -> &'static str {
    r#"<html><body><table id="games"><tbody>
<tr><th>Date</th><th>Visitor</th><th>Home</th><th>Round</th><th>Box Score</th></tr>
<tr><td data-stat="date_game" csk="199706110PQR">June 11, 1997</td>
<td data-stat="visitor_team_name"><a href="/teams/PQR/1997.html">Toronto Raptors</a></td>
<td data-stat="home_team_name"><a href="/teams/BOS/1997.html">Boston Celtics</a></td>
<td>First Round</td><td data-stat="box_score_text"><a href="/boxscores/199706110BOS.html">Box Score</a></td></tr>
</tbody></table></body></html>"#
}

fn box_page() -> &'static str {
    r#"<html><body><table id="box-PQR-game-basic"><tbody>
<tr><td colspan="10" data-stat="player">Starters</td></tr>
<tr><td colspan="10">ambiguous placeholder</td></tr>
</tbody></table><table id="box-BOS-game-basic"><tbody>
<tr><td colspan="10" data-stat="player">Reserves</td></tr>
<tr><td colspan="10">ambiguous placeholder</td></tr>
</tbody></table></body></html>"#
}

fn fixture(root: &Path, with_box: bool) {
    let season = root.join("1996-97");
    fs::create_dir_all(&season).expect("season directory");
    fs::write(season.join("_games.html"), schedule_page()).expect("schedule snapshot");
    if with_box {
        fs::write(season.join("199706110PQR.html"), box_page()).expect("box snapshot");
    }
}

#[test]
fn snapshot_review_preserves_bounded_quarantine_and_uses_row_evidence() {
    let dir = TestDir::fresh("ingest-jev-review");
    fixture(dir.path(), true);
    let conn = Connection::open_in_memory().expect("in-memory archive");

    let (report, disabled) =
        ingest_snapshot_dir_with_jev(&conn, dir.path(), None).expect("deterministic ingest");
    assert_eq!(report.unknown_team_slugs, vec!["PQR".to_owned()]);
    assert_eq!(report.html_quarantine.len(), 3, "{report:?}");
    assert_eq!(disabled.html_rows.len(), 3);
    assert!(disabled.html_rows.iter().all(|row| row.choice.is_none()));
    assert!(report
        .html_quarantine
        .iter()
        .all(|row| row.classification.is_none() && !row.fragment.is_empty()));

    let judge = ReviewJudge::new(Some("PLAYOFFS"), Some("BOS"));
    let review = review_ingest_snapshots(&conn, dir.path(), &report, Some(&judge));

    assert_eq!(review.game_types.len(), 1, "{review:?}");
    assert_eq!(review.game_types[0].game_id, "199706110PQR");
    assert_eq!(review.game_types[0].choice, "PLAYOFFS");
    assert!(judge
        .observed_game_type_evidence
        .lock()
        .expect("observed game type evidence lock")
        .iter()
        .all(|evidence| evidence.contains("First Round") && !evidence.contains("Date Visitor")));
    assert_eq!(review.team_alignments.len(), 1);
    assert_eq!(review.team_alignments[0].label, "Toronto Raptors");
    assert_eq!(review.team_alignments[0].suggested_slug, "BOS");
    assert_eq!(
        review.team_alignments[0].seasons,
        vec!["1996-97".to_owned()]
    );
    assert_eq!(review.html_rows.len(), 3);
    assert!(review
        .html_rows
        .iter()
        .all(|row| row.choice == Some(HtmlRowChoice::HeaderOrFooter)));
    assert!(judge
        .observed_html_chars
        .lock()
        .expect("observed lengths lock")
        .iter()
        .all(|length| *length <= 4096));
    assert_eq!(review.failures, 0);
}

#[test]
fn advisory_game_type_never_rewrites_the_committed_archive_row() {
    let dir = TestDir::fresh("ingest-jev-readonly");
    fixture(dir.path(), false);
    let conn = Connection::open_in_memory().expect("in-memory archive");
    let judge = ReviewJudge::new(Some("PLAYOFFS"), None);

    let (report, review) =
        ingest_snapshot_dir_with_jev(&conn, dir.path(), Some(&judge)).expect("ingest succeeds");

    assert_eq!(report.games, 1);
    let game_type: String = conn
        .query_row(
            "SELECT game_type FROM games WHERE game_id = '199706110BOS'",
            [],
            |row| row.get(0),
        )
        .expect("committed game");
    assert_eq!(game_type, "REGULAR");
    assert_eq!(review.game_types[0].choice, "PLAYOFFS");
}
