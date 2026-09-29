//! Optional Jev decisions for Shell commands and collector-note matching.
//!
//! Both paths are advisory. Disabled/error answers return `None`; collector
//! suggestions must name one of the caller-supplied candidate Game ids.

use crate::db_store::Store;
use crate::route::Route;
use crate::store::PaletteKind;
use nbatv_catalog::{CollectorNoteInput, JevJudge};
use nbatv_ladder::GreggNote;

const MAX_COLLECTOR_NOTE_CHARS: usize = 4096;

/// Closed set of Shell actions recognized by the optional command router.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellCommandRoute {
    FindGame,
    OpenSeason,
    OpenTeam,
    PlayGame,
    ShowAvailability,
    ShowTapeSources,
    OpenReview,
    Other,
}

/// Classify a natural-language Shell command without executing it.
pub fn route_shell_command(
    command: &str,
    judge: Option<&dyn JevJudge>,
) -> Option<ShellCommandRoute> {
    let label = judge?.route_shell_command(command).ok().flatten()?;
    route_from_label(&label)
}

fn route_from_label(label: &str) -> Option<ShellCommandRoute> {
    match label {
        "find_game" => Some(ShellCommandRoute::FindGame),
        "open_season" => Some(ShellCommandRoute::OpenSeason),
        "open_team" => Some(ShellCommandRoute::OpenTeam),
        "play_game" => Some(ShellCommandRoute::PlayGame),
        "show_availability" => Some(ShellCommandRoute::ShowAvailability),
        "show_tape_sources" => Some(ShellCommandRoute::ShowTapeSources),
        "open_review" => Some(ShellCommandRoute::OpenReview),
        "other" => Some(ShellCommandRoute::Other),
        _ => None,
    }
}

/// The screen section a game-focused command should reveal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameSection {
    Availability,
    TapeSources,
    Review,
}

/// A side-effect-free Shell command plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellCommandAction {
    Navigate(Route),
    Play(String),
    FocusGame {
        game_id: String,
        section: GameSection,
    },
    OpenPalette(String),
    Unhandled,
}

/// Plan a command using a previously classified intent and the caller's
/// archive. The model chooses only the intent; this function resolves names
/// and ids against deterministic Store data and never performs I/O.
pub fn plan_shell_command(
    store: &Store,
    command: &str,
    jev_intent: Option<&str>,
) -> ShellCommandAction {
    let Some(intent) = jev_intent.and_then(route_from_label) else {
        return ShellCommandAction::Unhandled;
    };
    let game = || unique_game_id(store, command);
    let fallback = || ShellCommandAction::OpenPalette(command.trim().to_owned());
    match intent {
        ShellCommandRoute::FindGame => game()
            .map(|game_id| ShellCommandAction::Navigate(Route::Game { game_id }))
            .unwrap_or_else(fallback),
        ShellCommandRoute::OpenSeason => unique_season(store, command)
            .map(ShellCommandAction::Navigate)
            .unwrap_or_else(fallback),
        ShellCommandRoute::OpenTeam => unique_team(store, command)
            .map(ShellCommandAction::Navigate)
            .unwrap_or_else(fallback),
        ShellCommandRoute::PlayGame => game()
            .map(ShellCommandAction::Play)
            .unwrap_or_else(fallback),
        ShellCommandRoute::ShowAvailability => game()
            .map(|game_id| ShellCommandAction::FocusGame {
                game_id,
                section: GameSection::Availability,
            })
            .unwrap_or_else(fallback),
        ShellCommandRoute::ShowTapeSources => game()
            .map(|game_id| ShellCommandAction::FocusGame {
                game_id,
                section: GameSection::TapeSources,
            })
            .unwrap_or_else(fallback),
        ShellCommandRoute::OpenReview => game()
            .map(|game_id| ShellCommandAction::FocusGame {
                game_id,
                section: GameSection::Review,
            })
            .unwrap_or_else(fallback),
        ShellCommandRoute::Other => ShellCommandAction::Unhandled,
    }
}

fn unique_game_id(store: &Store, command: &str) -> Option<String> {
    let mut ids = store
        .palette_search("")
        .into_iter()
        .filter(|item| item.kind == PaletteKind::Game)
        .filter_map(|item| match item.route {
            Route::Game { game_id } if label_matches(command, &game_id) => Some(game_id),
            Route::Game { game_id } if label_matches(command, &item.label) => Some(game_id),
            _ => None,
        })
        .collect::<Vec<_>>();
    ids.sort();
    ids.dedup();
    (ids.len() == 1).then(|| ids.remove(0))
}

fn unique_season(store: &Store, command: &str) -> Option<Route> {
    let mut routes = store
        .seasons()
        .into_iter()
        .filter(|season| {
            label_matches(command, &season.slug) || label_matches(command, &season.label)
        })
        .map(|season| Route::Season {
            season: season.slug,
        })
        .collect::<Vec<_>>();
    routes.sort_by_key(|route| match route {
        Route::Season { season } => season.clone(),
        _ => String::new(),
    });
    (routes.len() == 1).then(|| routes.remove(0))
}

fn unique_team(store: &Store, command: &str) -> Option<Route> {
    let mut routes = store
        .palette_search("")
        .into_iter()
        .filter(|item| item.kind == PaletteKind::Team)
        .filter(|item| label_matches(command, &item.label))
        .map(|item| item.route)
        .collect::<Vec<_>>();
    routes.sort_by_key(|route| format!("{route:?}"));
    routes.dedup();
    (routes.len() == 1).then(|| routes.remove(0))
}

fn label_matches(command: &str, label: &str) -> bool {
    let command = normalize(command);
    let label = normalize(label);
    !label.is_empty()
        && (command.contains(&label)
            || label
                .split_whitespace()
                .filter(|word| word.len() >= 4)
                .any(|word| command.split_whitespace().any(|token| token == word)))
}

fn normalize(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Match a Gregg collector note against deterministic candidate Game ids.
///
/// USA-Sports-on-DVD records already carry their BR URL and must not call this
/// function. A successful result is advisory and is revalidated against the
/// candidate set before return.
pub fn match_collector_note(
    note: &GreggNote,
    candidates: &[String],
    judge: Option<&dyn JevJudge>,
) -> Option<String> {
    if candidates.is_empty() {
        return None;
    }
    let evidence = format!(
        "{} {} Game {} {} {} @ {} {} ({}){}",
        note.season_label,
        note.round_label,
        note.game_no,
        note.away_team,
        note.away_score,
        note.home_team,
        note.home_score,
        note.grade,
        note.defects
    );
    let input = CollectorNoteInput {
        note: evidence.chars().take(MAX_COLLECTOR_NOTE_CHARS).collect(),
        candidate_games: candidates.to_vec(),
    };
    let selected = judge?.match_collector_note(&input).ok().flatten()?;
    candidates
        .iter()
        .find(|candidate| candidate.as_str() == selected)
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::FixtureStore;
    use nbatv_catalog::{
        CandidateVerdict, CrawlFailureChoice, FileSelection, FileSelectionInput, GameContext,
        GameTypeChoice, HtmlRowChoice, JevError, ProbeCandidate, SearchTemplateSelection,
        TeamChoice,
    };
    use std::sync::Mutex;

    fn fixture_store() -> Store {
        Store::Fixture(FixtureStore::fixture())
    }

    #[test]
    fn shell_planner_resolves_only_stored_entities_and_falls_back_to_palette() {
        let store = fixture_store();
        assert_eq!(
            plan_shell_command(&store, "open 1946-47", Some("open_season")),
            ShellCommandAction::Navigate(Route::Season {
                season: "1946-47".to_owned()
            })
        );
        assert_eq!(
            plan_shell_command(&store, "show the Celtics", Some("open_team")),
            ShellCommandAction::Navigate(Route::Team {
                season: "1946-47".to_owned(),
                team: "BOS".to_owned()
            })
        );
        assert_eq!(
            plan_shell_command(&store, "find game 194611010TRH", Some("find_game")),
            ShellCommandAction::Navigate(Route::Game {
                game_id: "194611010TRH".to_owned()
            })
        );
        assert_eq!(
            plan_shell_command(&store, "play 194611010TRH", Some("play_game")),
            ShellCommandAction::Play("194611010TRH".to_owned())
        );
        assert_eq!(
            plan_shell_command(
                &store,
                "availability 194611010TRH",
                Some("show_availability")
            ),
            ShellCommandAction::FocusGame {
                game_id: "194611010TRH".to_owned(),
                section: GameSection::Availability
            }
        );
        assert_eq!(
            plan_shell_command(&store, "sources 194611010TRH", Some("show_tape_sources")),
            ShellCommandAction::FocusGame {
                game_id: "194611010TRH".to_owned(),
                section: GameSection::TapeSources
            }
        );
        assert_eq!(
            plan_shell_command(&store, "review 194611010TRH", Some("open_review")),
            ShellCommandAction::FocusGame {
                game_id: "194611010TRH".to_owned(),
                section: GameSection::Review
            }
        );
        assert_eq!(
            plan_shell_command(&store, "show 1946", Some("open_team")),
            ShellCommandAction::OpenPalette("show 1946".to_owned())
        );
        assert_eq!(
            plan_shell_command(&store, "do nothing", None),
            ShellCommandAction::Unhandled
        );
        assert_eq!(
            plan_shell_command(&store, "do something else", Some("other")),
            ShellCommandAction::Unhandled
        );
    }

    struct FixedJudge {
        command: Option<&'static str>,
        collector: Option<&'static str>,
        observed_note_chars: Mutex<Vec<usize>>,
    }

    impl FixedJudge {
        fn new(command: Option<&'static str>, collector: Option<&'static str>) -> Self {
            Self {
                command,
                collector,
                observed_note_chars: Mutex::new(Vec::new()),
            }
        }
    }

    impl JevJudge for FixedJudge {
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

        fn classify_game_type(&self, _: &GameTypeChoice) -> Result<Option<String>, JevError> {
            Ok(None)
        }

        fn align_team(&self, _: &TeamChoice) -> Result<Option<String>, JevError> {
            Ok(None)
        }

        fn match_collector_note(
            &self,
            input: &CollectorNoteInput,
        ) -> Result<Option<String>, JevError> {
            self.observed_note_chars
                .lock()
                .expect("observed note lengths lock")
                .push(input.note.chars().count());
            Ok(self.collector.map(str::to_owned))
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

        fn classify_html_row(&self, _: &str, _: &str) -> Result<Option<HtmlRowChoice>, JevError> {
            Ok(None)
        }

        fn route_shell_command(&self, _: &str) -> Result<Option<String>, JevError> {
            Ok(self.command.map(str::to_owned))
        }

        fn prioritize_review(&self, _: &[String], _: &str) -> Result<Option<u8>, JevError> {
            Ok(None)
        }
    }

    fn finals_note() -> GreggNote {
        GreggNote {
            season_label: "1962".to_owned(),
            round_label: "NBA Finals".to_owned(),
            game_no: 7,
            away_team: "L.A. Lakers".to_owned(),
            away_score: 107,
            home_team: "Boston".to_owned(),
            home_score: 110,
            grade: "VG".to_owned(),
            defects: "Edited possessions".to_owned(),
        }
    }

    #[test]
    fn shell_command_route_accepts_only_the_typed_closed_set() {
        let judge = FixedJudge::new(Some("open_team"), None);
        assert_eq!(
            route_shell_command("show me the Celtics", Some(&judge)),
            Some(ShellCommandRoute::OpenTeam)
        );

        let invalid = FixedJudge::new(Some("run shell command"), None);
        assert_eq!(route_shell_command("anything", Some(&invalid)), None);
        assert_eq!(route_shell_command("show me the Celtics", None), None);
    }

    #[test]
    fn collector_match_must_name_a_supplied_candidate() {
        let candidates = vec!["194612250BOS".to_owned(), "194612260BOS".to_owned()];
        let judge = FixedJudge::new(None, Some("194612250BOS"));
        assert_eq!(
            match_collector_note(&finals_note(), &candidates, Some(&judge)),
            Some("194612250BOS".to_owned())
        );

        let invalid = FixedJudge::new(None, Some("invented-game"));
        assert_eq!(
            match_collector_note(&finals_note(), &candidates, Some(&invalid)),
            None
        );
    }

    #[test]
    fn collector_evidence_is_bounded() {
        let candidates = vec!["194612250BOS".to_owned()];
        let judge = FixedJudge::new(None, Some("194612250BOS"));
        let mut note = finals_note();
        note.defects = "x".repeat(10_000);

        assert_eq!(
            match_collector_note(&note, &candidates, Some(&judge)),
            Some("194612250BOS".to_owned())
        );
        assert_eq!(
            judge
                .observed_note_chars
                .lock()
                .expect("observed note lengths lock")
                .as_slice(),
            &[4096]
        );
    }
}
