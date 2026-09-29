//! Optional, post-commit Jev review for snapshot ingest.
//!
//! Snapshot parsing and SQLite reconciliation finish first. This module then
//! reviews only bounded, deterministic evidence already recorded in
//! [`IngestReport`]. Jev output is diagnostic and never changes archive rows.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use nbatv_catalog::{GameTypeChoice, HtmlRowChoice, JevJudge};
use rusqlite::Connection;

use super::{
    read_snapshot_page, row_cells, slug_from_player_href, sorted_entries, table_rows, tables,
    validate_game_id, Cell, GameTypeEvidence, HtmlRowQuarantine, IngestError, IngestReport,
    UnknownTeamEvidence,
};

const MAX_EVIDENCE_CHARS: usize = 4096;
const MAX_REVIEW_ITEMS: usize = 512;
const MAX_ROWS_PER_PAGE: usize = 64;

/// Deterministic evidence collected before the SQLite transaction opens.
pub(crate) struct SnapshotReviewInput {
    pub(crate) unknown_teams: Vec<UnknownTeamEvidence>,
    pub(crate) game_types: Vec<GameTypeEvidence>,
    pub(crate) html_quarantine: Vec<HtmlRowQuarantine>,
}

/// One bounded HTML quarantine row plus its optional Jev classification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JevHtmlRowReview {
    pub path: String,
    pub table: String,
    pub row: usize,
    pub heading: String,
    pub fragment: String,
    pub choice: Option<HtmlRowChoice>,
}

/// One row-specific Game type suggestion. It is advisory only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JevGameTypeReview {
    pub path: String,
    pub season: String,
    pub game_id: String,
    pub choice: String,
}

/// One optional historical-label alignment for an unknown Team slug.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JevTeamAlignmentReview {
    pub label: String,
    pub suggested_slug: String,
    pub seasons: Vec<String>,
}

/// Post-commit suggestions plus the number of failed review calls.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct JevIngestReview {
    pub html_rows: Vec<JevHtmlRowReview>,
    pub game_types: Vec<JevGameTypeReview>,
    pub team_alignments: Vec<JevTeamAlignmentReview>,
    pub failures: usize,
}

pub(crate) fn collect_snapshot_review_input(
    root: &Path,
) -> Result<SnapshotReviewInput, IngestError> {
    let mut unknown_labels: BTreeMap<String, BTreeMap<String, BTreeSet<String>>> = BTreeMap::new();
    let mut game_types = Vec::new();
    let mut html_quarantine = Vec::new();

    for season in sorted_entries(root)? {
        if !season.is_dir() {
            continue;
        }
        let Some(slug) = season.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if parse_season_start_year(slug).is_none() {
            continue;
        }
        for page in sorted_entries(&season)? {
            let Some(name) = page.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if !name.ends_with(".html") {
                continue;
            }
            let html = read_snapshot_page(&page)?;
            if name.starts_with("_games") {
                collect_schedule_page(
                    &page,
                    slug,
                    &html,
                    &mut unknown_labels,
                    &mut game_types,
                    &mut html_quarantine,
                );
            } else if validate_game_id(name.strip_suffix(".html").unwrap_or(name)) {
                collect_box_page(&page, &html, &mut html_quarantine);
            }
        }
    }

    let mut unknown_teams = Vec::new();
    for (slug, labels) in unknown_labels {
        for (label, seasons) in labels {
            unknown_teams.push(UnknownTeamEvidence {
                slug: slug.clone(),
                label,
                seasons: seasons.into_iter().collect(),
            });
            if unknown_teams.len() == MAX_REVIEW_ITEMS {
                break;
            }
        }
        if unknown_teams.len() == MAX_REVIEW_ITEMS {
            break;
        }
    }
    Ok(SnapshotReviewInput {
        unknown_teams,
        game_types,
        html_quarantine,
    })
}

#[allow(clippy::too_many_arguments)]
fn collect_schedule_page(
    path: &Path,
    season: &str,
    html: &str,
    unknown_labels: &mut BTreeMap<String, BTreeMap<String, BTreeSet<String>>>,
    game_types: &mut Vec<GameTypeEvidence>,
    quarantine: &mut Vec<HtmlRowQuarantine>,
) {
    let mut page_rows = 0usize;
    for (table, body) in schedule_tables(html) {
        for (index, row) in table_rows(body).into_iter().enumerate() {
            let cells = row_cells(row);
            collect_team_labels(&cells, season, unknown_labels);
            let game_id = schedule_game_id(&cells);
            if let Some(game_id) = game_id {
                if has_competition_marker(&cells) && game_types.len() < MAX_REVIEW_ITEMS {
                    game_types.push(GameTypeEvidence {
                        path: path.display().to_string(),
                        season: season.to_owned(),
                        game_id,
                        row_text: bounded(row),
                    });
                }
                continue;
            }
            if page_rows >= MAX_ROWS_PER_PAGE || quarantine.len() >= MAX_REVIEW_ITEMS {
                break;
            }
            quarantine.push(HtmlRowQuarantine {
                path: path.display().to_string(),
                table: table.clone(),
                row: index + 1,
                heading: row_heading(&cells, &table),
                fragment: bounded(row),
                classification: None,
            });
            page_rows += 1;
        }
        if page_rows >= MAX_ROWS_PER_PAGE || quarantine.len() >= MAX_REVIEW_ITEMS {
            break;
        }
    }
}

fn collect_box_page(path: &Path, html: &str, quarantine: &mut Vec<HtmlRowQuarantine>) {
    let mut page_rows = 0usize;
    for (table, body) in tables(html) {
        if !table.starts_with("box-") || !table.ends_with("-game-basic") {
            continue;
        }
        for (index, row) in table_rows(body).into_iter().enumerate() {
            if page_rows >= MAX_ROWS_PER_PAGE || quarantine.len() >= MAX_REVIEW_ITEMS {
                return;
            }
            let cells = row_cells(row);
            let parsed = match cells.iter().find(|cell| cell.stat == "player") {
                None => false,
                Some(player) => {
                    let label = player.text.trim();
                    label.eq_ignore_ascii_case("starters")
                        || label.eq_ignore_ascii_case("reserves")
                        || label.eq_ignore_ascii_case("team totals")
                        || player
                            .href
                            .as_deref()
                            .and_then(slug_from_player_href)
                            .is_some()
                }
            };
            if parsed {
                continue;
            }
            quarantine.push(HtmlRowQuarantine {
                path: path.display().to_string(),
                table: table.clone(),
                row: index + 1,
                heading: row_heading(&cells, &table),
                fragment: bounded(row),
                classification: None,
            });
            page_rows += 1;
        }
    }
}

fn schedule_tables(html: &str) -> Vec<(String, &str)> {
    let tables = tables(html);
    let selected = tables
        .iter()
        .filter(|(id, _)| id == "games")
        .map(|(id, body)| (id.clone(), *body))
        .collect::<Vec<_>>();
    if selected.is_empty() {
        vec![("document".to_owned(), html)]
    } else {
        selected
    }
}

fn collect_team_labels(
    cells: &[Cell],
    season: &str,
    labels: &mut BTreeMap<String, BTreeMap<String, BTreeSet<String>>>,
) {
    for cell in cells {
        if cell.stat != "visitor_team_name" && cell.stat != "home_team_name" {
            continue;
        }
        let Some(href) = cell.href.as_deref() else {
            continue;
        };
        let Some(slug) = team_slug_from_href(href) else {
            continue;
        };
        let label = cell.text.trim();
        if label.is_empty() || label == slug {
            continue;
        }
        labels
            .entry(slug)
            .or_default()
            .entry(label.to_owned())
            .or_default()
            .insert(season.to_owned());
    }
}

fn team_slug_from_href(href: &str) -> Option<String> {
    let href = href.split('?').next()?;
    let segment = href
        .rsplit('/')
        .nth(1)
        .or_else(|| href.rsplit('/').next())?;
    let slug = segment.strip_suffix(".html").unwrap_or(segment);
    if slug.is_empty()
        || !slug
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
    {
        return None;
    }
    Some(slug.to_owned())
}

fn schedule_game_id(cells: &[Cell]) -> Option<String> {
    let date = cells
        .iter()
        .find(|cell| cell.stat == "date_game")
        .and_then(|cell| cell.csk.as_deref())?;
    if validate_game_id(date) {
        return Some(date.to_owned());
    }
    if date.len() != 8 || !date.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let slug = |stat: &str| {
        cells
            .iter()
            .find(|cell| cell.stat == stat)
            .and_then(|cell| cell.href.as_deref())
            .and_then(team_slug_from_href)
    };
    let game_id = format!(
        "{date}{}{}",
        slug("visitor_team_name")?,
        slug("home_team_name")?
    );
    validate_game_id(&game_id).then_some(game_id)
}

fn has_competition_marker(cells: &[Cell]) -> bool {
    cells
        .iter()
        .filter(|cell| !cell.text.is_empty())
        .any(|cell| {
            let text = cell.text.to_ascii_lowercase();
            ["first round", "semifinal", "finals", "playoffs", "nba cup"]
                .iter()
                .any(|marker| text.contains(marker))
        })
}

fn row_heading(cells: &[Cell], table: &str) -> String {
    cells
        .iter()
        .find(|cell| cell.stat.is_empty() && !cell.text.trim().is_empty())
        .map(|cell| cell.text.trim().to_owned())
        .unwrap_or_else(|| format!("{table} row"))
}

fn bounded(value: &str) -> String {
    value.chars().take(MAX_EVIDENCE_CHARS).collect()
}

fn parse_season_start_year(season: &str) -> Option<i32> {
    let year = season.get(..4)?.parse::<i32>().ok()?;
    (1946..=2100).contains(&year).then_some(year)
}

/// Review precomputed evidence without mutating the committed archive.
pub fn review_ingest_snapshots(
    conn: &Connection,
    _root: &Path,
    report: &IngestReport,
    judge: Option<&dyn JevJudge>,
) -> JevIngestReview {
    let mut review = JevIngestReview::default();
    for row in &report.html_quarantine {
        let choice =
            judge.and_then(
                |judge| match judge.classify_html_row(&row.fragment, &row.heading) {
                    Ok(Some(choice)) if choice != HtmlRowChoice::Unknown => Some(choice),
                    Ok(_) => None,
                    Err(_) => {
                        review.failures += 1;
                        None
                    }
                },
            );
        review.html_rows.push(JevHtmlRowReview {
            path: row.path.clone(),
            table: row.table.clone(),
            row: row.row,
            heading: row.heading.clone(),
            fragment: row.fragment.clone(),
            choice,
        });
    }

    let Some(judge) = judge else {
        return review;
    };

    for row in &report.game_type_evidence {
        let input = GameTypeChoice::new(
            &row.row_text,
            &row.season,
            &["REGULAR", "PLAYOFFS", "NBA_CUP", "FINALS", "UNCERTAIN"],
        );
        match judge.classify_game_type(&input) {
            Ok(Some(choice)) if is_game_type(&choice) => {
                review.game_types.push(JevGameTypeReview {
                    path: row.path.clone(),
                    season: row.season.clone(),
                    game_id: row.game_id.clone(),
                    choice,
                });
            }
            Ok(_) => {}
            Err(_) => review.failures += 1,
        }
    }

    let known_teams = match nbatv_db::list_teams(conn) {
        Ok(teams) => teams,
        Err(_) => {
            review.failures += 1;
            return review;
        }
    };
    let unknown_slugs = report.unknown_team_slugs.iter().collect::<BTreeSet<_>>();
    for evidence in report
        .unknown_team_evidence
        .iter()
        .filter(|evidence| unknown_slugs.contains(&evidence.slug))
    {
        let Some(first) = evidence
            .seasons
            .first()
            .and_then(|season| parse_season_start_year(season))
        else {
            continue;
        };
        let last = evidence
            .seasons
            .last()
            .and_then(|season| parse_season_start_year(season))
            .unwrap_or(first);
        let candidates = known_teams
            .iter()
            .filter(|team| team.br_slug != evidence.slug)
            .filter(|team| {
                team.active_from.unwrap_or(i32::MIN) <= last
                    && team.active_to.unwrap_or(i32::MAX) >= first
            })
            .map(|team| {
                (
                    team.br_slug.clone(),
                    format!(
                        "{} {} ({}–{})",
                        team.city,
                        team.name,
                        team.active_from.unwrap_or_default(),
                        team.active_to.unwrap_or(i32::MAX)
                    ),
                )
            })
            .collect::<Vec<_>>();
        if candidates.is_empty() {
            continue;
        }
        let input = nbatv_catalog::TeamChoice {
            label: evidence.label.clone(),
            season: format!("observed {first} through {last}"),
            known_teams: candidates.clone(),
        };
        match judge.align_team(&input) {
            Ok(Some(suggested_slug))
                if candidates.iter().any(|(slug, _)| slug == &suggested_slug) =>
            {
                review.team_alignments.push(JevTeamAlignmentReview {
                    label: evidence.label.clone(),
                    suggested_slug,
                    seasons: evidence.seasons.clone(),
                });
            }
            Ok(_) => {}
            Err(_) => review.failures += 1,
        }
    }
    review
}

fn is_game_type(choice: &str) -> bool {
    matches!(choice, "REGULAR" | "PLAYOFFS" | "NBA_CUP" | "FINALS")
}
