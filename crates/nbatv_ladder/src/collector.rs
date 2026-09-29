//! Rung 5: collector catalogs & fan networks — existence pointers only.
//!
//! Per research doc 08: USA Sports on DVD (20,901 NBA games, each record
//! carrying a Basketball-Reference boxscore URL → `game_id` crosswalk) and
//! Gregg's Sports Archive (score-grammar lines, no dates). These networks
//! are existence metadata and human request channels, never stream URLs:
//! rung 5 never yields bytes — enforced by `bytes_available() == false` on
//! every type in this module.

/// Extract the `game_id` slug from a Basketball-Reference boxscore URL.
///
/// `https://www.basketball-reference.com/boxscores/200304120CLE.html`
/// → `Some("200304120CLE")`. Returns `None` for non-boxscore URLs.
pub fn parse_br_url_to_game_id(url: &str) -> Option<String> {
    let (_, after) = url.split_once("/boxscores/")?;
    let slug = after.split(['?', '#']).next()?.strip_suffix(".html")?;
    if slug.is_empty() || !slug.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return None;
    }
    // Slugs are date + home-abbrev, e.g. `200304120CLE`, `194611010TRH`.
    if slug.len() < 9 || !slug.as_bytes()[..8].iter().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(slug.to_string())
}

/// One USA-Sports-on-DVD-style catalog record: BR URL identity plus the
/// trader's network/duration/completeness notes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsasdRecord {
    pub br_url: String,
    pub game_id: String,
    pub network: String,
    pub completeness: String,
}

impl UsasdRecord {
    /// Parse one catalog record; `None` when the BR URL has no game slug.
    pub fn parse(
        br_url: impl Into<String>,
        network: impl Into<String>,
        completeness: impl Into<String>,
    ) -> Option<Self> {
        let br_url = br_url.into();
        let game_id = parse_br_url_to_game_id(&br_url)?;
        Some(Self {
            br_url,
            game_id,
            network: network.into(),
            completeness: completeness.into(),
        })
    }

    /// Pointer-only marker: rung 5 records never yield bytes.
    pub fn bytes_available(&self) -> bool {
        false
    }
}

/// Tiny inline catalog fixture (shape per 08 §1.1: BR URL + network +
/// completeness notes). Tests only — never crawled, never downloaded.
pub const USASD_FIXTURE: &[(&str, &str, &str)] = &[
    (
        "https://www.basketball-reference.com/boxscores/200304120CLE.html",
        "Fox Sports Net Cavs TV",
        "Full broadcast with no halftime, commercials included",
    ),
    (
        "https://www.basketball-reference.com/boxscores/196304240LAL.html",
        "NBC",
        "1962-63 Finals Game 6; INCOMPLETE GAME — broadcast starts with 11:20 left in 2nd quarter",
    ),
    (
        "https://www.basketball-reference.com/boxscores/199806140CHI.html",
        "NBC",
        "1998 Finals Game 6; complete",
    ),
];

/// Parse the whole inline fixture into records.
pub fn parse_usasd_fixture() -> Vec<UsasdRecord> {
    USASD_FIXTURE
        .iter()
        .filter_map(|(url, network, notes)| UsasdRecord::parse(*url, *network, *notes))
        .collect()
}

/// One Gregg's-style score-grammar note (08 §1.2):
/// `{season} {round} Game {n} {away} {away_score} @ {home} {home_score}
/// ({grade})(defects)` — identity by season + round + game number + teams +
/// final score, no date. Matching is score+round+teams → playoff index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GreggNote {
    pub season_label: String,
    pub round_label: String,
    pub game_no: u8,
    pub away_team: String,
    pub away_score: u16,
    pub home_team: String,
    pub home_score: u16,
    pub grade: String,
    pub defects: String,
}

impl GreggNote {
    /// Pointer-only marker: rung 5 notes never yield bytes.
    pub fn bytes_available(&self) -> bool {
        false
    }
}

/// Parse one Gregg score-grammar line, e.g.
/// `1962 NBA Finals Game 7 L.A. Lakers 107 @ Boston 110 (VG)(Edited possessions, B&W)`.
/// Returns `None` when the line does not follow the grammar.
pub fn parse_gregg_line(line: &str) -> Option<GreggNote> {
    let (left, right) = line.split_once(" Game ")?;
    let mut left_words = left.split_whitespace();
    let season_label = left_words.next()?.to_string();
    let round_label = left_words.collect::<Vec<_>>().join(" ");
    if round_label.is_empty() {
        return None;
    }
    let (game_no_text, rest) = right.split_once(char::is_whitespace)?;
    let game_no: u8 = game_no_text.parse().ok()?;
    let (away_part, home_part) = rest.split_once(" @ ")?;
    let (away_team, away_score) = split_team_score(away_part)?;
    let (home_team, home_score, tail) = split_home_score(home_part)?;
    let (grade, defects) = split_grade_defects(tail);
    Some(GreggNote {
        season_label,
        round_label,
        game_no,
        away_team,
        away_score,
        home_team,
        home_score,
        grade,
        defects,
    })
}

/// Split `{team words} {score}` at the trailing number.
fn split_team_score(part: &str) -> Option<(String, u16)> {
    let idx = part.rfind(char::is_whitespace)?;
    let (team, score_text) = part.split_at(idx);
    let score = score_text.trim().parse().ok()?;
    if team.trim().is_empty() {
        return None;
    }
    Some((team.trim().to_string(), score))
}

/// Split `{team words} {score} ({grade})(defects...)` at the score token.
///
/// Splits the grade tail off first (byte-safe: `(` is ASCII, so both halves
/// are char boundaries), then reuses [`split_team_score`] on the remainder.
/// Never slices by recomputed byte lengths, so double-spaced or non-ASCII
/// team words cannot mis-slice or panic.
fn split_home_score(part: &str) -> Option<(String, u16, &str)> {
    let (team_part, tail) = match part.find('(') {
        Some(i) => (&part[..i], &part[i..]),
        None => (part, ""),
    };
    let (team, score) = split_team_score(team_part.trim_end())?;
    Some((team, score, tail))
}

/// Split `(GRADE)(defects...)` into grade + defect notes.
fn split_grade_defects(tail: &str) -> (String, String) {
    let tail = tail.trim();
    if !tail.starts_with('(') {
        return (String::new(), tail.to_string());
    }
    match tail.find(')') {
        Some(end) => {
            let grade = tail[1..end].trim().to_string();
            (grade, tail[end + 1..].trim().to_string())
        }
        None => (String::new(), tail.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn br_urls_reduce_to_game_ids() {
        assert_eq!(
            parse_br_url_to_game_id(
                "https://www.basketball-reference.com/boxscores/200304120CLE.html"
            ),
            Some("200304120CLE".to_string())
        );
        assert_eq!(
            parse_br_url_to_game_id(
                "https://www.basketball-reference.com/boxscores/194611010TRH.html"
            ),
            Some("194611010TRH".to_string())
        );
    }

    #[test]
    fn br_parse_rejects_non_boxscore_urls() {
        assert_eq!(parse_br_url_to_game_id("https://example.com/nba"), None);
        assert_eq!(
            parse_br_url_to_game_id("https://www.basketball-reference.com/teams/CHI/"),
            None
        );
        assert_eq!(
            parse_br_url_to_game_id(
                "https://www.basketball-reference.com/boxscores/not-a-date.html"
            ),
            None
        );
    }

    #[test]
    fn rung_5_fixture_parses_br_urls_to_game_ids_and_never_yields_bytes() {
        let records = parse_usasd_fixture();
        assert_eq!(records.len(), USASD_FIXTURE.len());
        let game_ids: Vec<&str> = records.iter().map(|r| r.game_id.as_str()).collect();
        assert_eq!(
            game_ids,
            vec!["200304120CLE", "196304240LAL", "199806140CHI"]
        );
        for record in &records {
            assert!(!record.bytes_available(), "rung 5 must never yield bytes");
        }
    }

    #[test]
    fn gregg_finals_line_parses() {
        let note = parse_gregg_line(
            "1962 NBA Finals Game 7 L.A. Lakers 107 @ Boston 110 (VG)(Edited possessions, B&W with natural arena sounds 0:38)",
        )
        .expect("grammar line must parse");
        assert_eq!(note.season_label, "1962");
        assert_eq!(note.round_label, "NBA Finals");
        assert_eq!(note.game_no, 7);
        assert_eq!(note.away_team, "L.A. Lakers");
        assert_eq!(note.away_score, 107);
        assert_eq!(note.home_team, "Boston");
        assert_eq!(note.home_score, 110);
        assert_eq!(note.grade, "VG");
        assert!(note.defects.contains("Edited possessions"));
        assert!(!note.bytes_available(), "rung 5 must never yield bytes");
    }

    #[test]
    fn gregg_era_true_round_label_parses() {
        let note = parse_gregg_line("1966 NBA ECSF Game 3 Baltimore 115 @ Philadelphia 121 (EX)")
            .expect("era-true round label must parse");
        assert_eq!(note.round_label, "NBA ECSF");
        assert_eq!(note.game_no, 3);
        assert_eq!(note.away_team, "Baltimore");
        assert_eq!(note.home_team, "Philadelphia");
        assert_eq!(note.grade, "EX");
    }

    #[test]
    fn gregg_garbage_lines_rejected() {
        assert_eq!(parse_gregg_line("just a forum post"), None);
        assert_eq!(parse_gregg_line("1962 NBA Finals, no game number"), None);
        assert_eq!(
            parse_gregg_line("1978 Game 4 Washington @ Seattle (EX)"),
            None
        );
    }
    #[test]
    fn gregg_double_spaced_multword_team_keeps_score_and_grade() {
        let note =
            parse_gregg_line("1962 NBA Finals Game 7 L.A. Lakers 107 @ Los  Angeles 110 (VG)")
                .expect("double-spaced team words must parse");
        assert_eq!(note.away_team, "L.A. Lakers");
        assert_eq!(note.away_score, 107);
        assert_eq!(note.home_team, "Los  Angeles");
        assert_eq!(note.home_score, 110);
        assert_eq!(note.grade, "VG");
    }

    #[test]
    fn gregg_non_ascii_team_does_not_panic() {
        let note = parse_gregg_line("1962 NBA Finals Game 7 Boston 107 @ AB  ä 100 (VG)")
            .expect("non-ASCII team words must parse without panicking");
        assert_eq!(note.away_team, "Boston");
        assert_eq!(note.home_team, "AB  ä");
        assert_eq!(note.home_score, 100);
        assert_eq!(note.grade, "VG");
    }
}
