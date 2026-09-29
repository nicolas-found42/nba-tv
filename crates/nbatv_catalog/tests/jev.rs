use nbatv_catalog::jev::{
    select_ia_file, CollectorNoteInput, DirectHttpJev, DisabledJevJudge, FileSelection,
    FileSelectionInput, GameTypeChoice, JevChoice, JevConfidence, JevError, JevJudge, JevNoul,
    JevQuestion, JevRequest, JevResponse, JevScore, ReviewPriority, TeamChoice,
};
use nbatv_catalog::probe::{GameContext, ProbeCandidate};
use nbatv_ladder::MatchLevel;

fn context() -> GameContext {
    GameContext {
        game_id: "199806140CHI".to_owned(),
        home_team: "CHI".to_owned(),
        away_team: "UTA".to_owned(),
        date: "1998-06-14".to_owned(),
    }
}

fn candidate() -> ProbeCandidate {
    ProbeCandidate {
        url_or_pointer: "https://example.com/watch".to_owned(),
        title: "Chicago Bulls Utah Jazz 1998 Finals full game".to_owned(),
        description: "Complete broadcast".to_owned(),
        duration_secs: None,
    }
}

fn answer(value: serde_json::Value) -> JevResponse {
    JevResponse::from_value(serde_json::json!({
        "model": "jev-1.13.0",
        "answers": {"decision": value}
    }))
    .expect("test response")
}

fn choice(criteria: serde_json::Value) -> JevQuestion {
    JevQuestion::Choice(JevChoice {
        instructions: "choose one".to_owned(),
        criteria,
    })
}

fn score() -> JevQuestion {
    JevQuestion::Score(JevScore {
        instructions: "rate it".to_owned(),
        criteria: vec![
            "unrelated".to_owned(),
            "weak".to_owned(),
            "plausible".to_owned(),
            "strong".to_owned(),
        ],
    })
}

fn noul() -> JevQuestion {
    JevQuestion::Noul(JevNoul {
        instructions: "is it true?".to_owned(),
        criteria: None,
    })
}

struct ErrorJudge;

impl JevJudge for ErrorJudge {
    fn select_file(&self, _: &FileSelectionInput) -> Result<Option<FileSelection>, JevError> {
        Err(JevError::Transport(
            "file selection test failure".to_owned(),
        ))
    }

    fn match_candidate(
        &self,
        _: &GameContext,
        _: &ProbeCandidate,
    ) -> Result<Option<nbatv_catalog::jev::CandidateVerdict>, JevError> {
        Ok(None)
    }

    fn classify_game_type(&self, _: &GameTypeChoice) -> Result<Option<String>, JevError> {
        Ok(None)
    }

    fn align_team(&self, _: &TeamChoice) -> Result<Option<String>, JevError> {
        Ok(None)
    }

    fn match_collector_note(&self, _: &CollectorNoteInput) -> Result<Option<String>, JevError> {
        Ok(None)
    }

    fn choose_search_template(
        &self,
        _: &GameContext,
        _: &str,
    ) -> Result<Option<nbatv_catalog::jev::SearchTemplateSelection>, JevError> {
        Ok(None)
    }

    fn classify_crawl_failure(
        &self,
        _: &str,
    ) -> Result<Option<nbatv_catalog::jev::CrawlFailureChoice>, JevError> {
        Ok(None)
    }

    fn classify_html_row(
        &self,
        _: &str,
        _: &str,
    ) -> Result<Option<nbatv_catalog::jev::HtmlRowChoice>, JevError> {
        Ok(None)
    }

    fn route_shell_command(&self, _: &str) -> Result<Option<String>, JevError> {
        Ok(None)
    }

    fn prioritize_review(&self, _: &[String], _: &str) -> Result<Option<u8>, JevError> {
        Ok(None)
    }
}

#[test]
fn request_uses_documented_shape() {
    let request = JevRequest::batch(
        serde_json::json!({
            "candidate": {
                "title": candidate().title,
                "description": candidate().description,
                "duration_secs": candidate().duration_secs,
                "url_or_pointer": candidate().url_or_pointer
            }
        }),
        vec![
            (
                "first",
                choice(serde_json::json!({"a": "first", "b": "second"})),
            ),
            ("second", noul()),
        ],
    )
    .expect("request");
    let value = serde_json::to_value(request).expect("serialize request");
    assert_eq!(value["state"]["candidate"]["title"], candidate().title);
    assert_eq!(value["model"], "jev-1.13.0");
    assert_eq!(value["questions"]["first"]["type"], "choice");
    assert_eq!(value["questions"]["second"]["type"], "noul");
}

#[test]
fn batch_rejects_duplicate_ids() {
    let error = JevRequest::batch(
        serde_json::json!({}),
        vec![("same", noul()), ("same", score())],
    )
    .expect_err("duplicate ids");
    assert!(matches!(error, JevError::InvalidAnswer(_)));
}

#[test]
fn choice_rejects_an_invented_option() {
    let question = choice(serde_json::json!({"a": "first", "b": "second"}));
    let response = answer(serde_json::json!({
        "type": "choice",
        "choice": "invented",
        "probabilities": {"invented": 1.0},
        "confidence": 0.99
    }));
    let error = JevResponse::validate_answer(
        &question,
        response.answer("decision").expect("decision answer"),
    )
    .expect_err("invented option must fail");
    assert!(matches!(error, JevError::InvalidAnswer(_)));
}

#[test]
fn score_and_noul_validate_against_their_schema() {
    let noul_answer =
        JevResponse::answer_value(&noul(), serde_json::json!({"type": "noul", "noul": 0.92}))
            .expect("noul answer");
    assert!(noul_answer.get("noul").is_some());

    let score_answer = JevResponse::answer_value(
        &score(),
        serde_json::json!({
            "type": "score",
            "score": 1.0,
            "legend": {"0": "unrelated", "1": "weak", "2": "plausible", "3": "strong"},
            "probabilities": {"0": 0.0, "1": 0.1, "2": 0.1, "3": 0.8},
            "confidence": 0.9
        }),
    )
    .expect("score answer");
    assert_eq!(
        score_answer.get("type").and_then(serde_json::Value::as_str),
        Some("score")
    );
}

#[test]
fn disabled_judge_keeps_deterministic_ia_fallback() {
    let input = FileSelectionInput {
        target: context(),
        item_title: "1996 NBA Finals".to_owned(),
        files: vec![
            ("Game 1.mp4".to_owned(), Some(8_453)),
            ("Game 2.mp4".to_owned(), Some(8_494)),
            ("Game 3.mp4".to_owned(), Some(8_286)),
        ],
    };
    let selected = select_ia_file(&DisabledJevJudge, &input).expect("fallback");
    assert_eq!(
        selected.map(|(name, _)| name),
        Some("Game 2.mp4".to_owned())
    );
}

#[test]
fn ia_selection_error_keeps_the_deterministic_fallback() {
    let input = FileSelectionInput {
        target: context(),
        item_title: "1996 NBA Finals".to_owned(),
        files: vec![
            ("Game 1.mp4".to_owned(), Some(8_453)),
            ("Game 2.mp4".to_owned(), Some(8_494)),
        ],
    };

    let selected = select_ia_file(&ErrorJudge, &input).expect("fallback");

    assert_eq!(
        selected.map(|(name, _)| name),
        Some("Game 2.mp4".to_owned())
    );
}

#[test]
fn low_confidence_file_selection_keeps_deterministic_fallback() {
    struct ReviewJudge;
    impl JevJudge for ReviewJudge {
        fn select_file(&self, _: &FileSelectionInput) -> Result<Option<FileSelection>, JevError> {
            Ok(Some(FileSelection::Review {
                name: Some("Game 1.mp4".to_owned()),
                confidence: 0.5,
            }))
        }
    }
    let input = FileSelectionInput {
        target: context(),
        item_title: "Finals".to_owned(),
        files: vec![
            ("Game 1.mp4".to_owned(), Some(100)),
            ("Game 2.mp4".to_owned(), Some(90)),
        ],
    };
    assert_eq!(
        select_ia_file(&ReviewJudge, &input).expect("review path"),
        Some(("Game 1.mp4".to_owned(), Some(100)))
    );
}

#[test]
fn direct_client_rejects_incomplete_https_endpoint() {
    let error = DirectHttpJev::from_env_or("test-key", "https://").expect_err("empty endpoint");
    assert!(matches!(error, JevError::Transport(_)));
}

#[test]
fn explicit_client_rejects_non_https_endpoint() {
    let error =
        DirectHttpJev::from_env_or("test-key", "http://127.0.0.1:1").expect_err("http endpoint");
    assert!(matches!(error, JevError::Transport(_)));
}

#[test]
fn direct_client_constructor_never_exposes_key_in_debug() {
    let client =
        DirectHttpJev::from_env_or("secret", "https://127.0.0.1:1").expect("configured client");
    let rendered = format!("{client:?}");
    assert!(!rendered.contains("secret"));
}

#[test]
fn request_requires_at_least_one_question() {
    let error = JevRequest::batch(serde_json::json!({}), Vec::new()).expect_err("empty batch");
    assert!(matches!(error, JevError::InvalidAnswer(_)));
}

#[test]
fn all_ten_workflows_have_typed_closed_set_adapters() {
    let judge = DisabledJevJudge;
    let game = context();
    let candidate = candidate();
    let files = FileSelectionInput {
        target: game.clone(),
        item_title: "1998 Finals".to_owned(),
        files: vec![("Game 1.mp4".to_owned(), Some(100))],
    };
    let note = CollectorNoteInput {
        note: "Finals game".to_owned(),
        candidate_games: vec!["199806140CHI".to_owned()],
    };
    let game_type = GameTypeChoice::new(
        "Conference Finals",
        "2025-26",
        &["REGULAR", "PLAYOFFS", "NBA_CUP", "UNCERTAIN"],
    );
    let team = TeamChoice {
        label: "Philadelphia Warriors".to_owned(),
        season: "1946-47".to_owned(),
        known_teams: vec![("PHW".to_owned(), "Philadelphia Warriors".to_owned())],
    };

    assert!(select_ia_file(&judge, &files)
        .expect("ia fallback")
        .is_some());
    assert!(judge
        .match_candidate(&game, &candidate)
        .expect("candidate")
        .is_none());
    assert!(judge
        .prioritize_review(&["candidate".to_owned()], "1998 Finals")
        .expect("review")
        .is_none());
    assert!(judge
        .classify_game_type(&game_type)
        .expect("game type")
        .is_none());
    assert!(judge.align_team(&team).expect("team").is_none());
    assert!(judge.match_collector_note(&note).expect("note").is_none());
    assert!(judge
        .choose_search_template(&game, "youtube")
        .expect("template")
        .is_none());
    assert!(judge
        .classify_crawl_failure("403")
        .expect("failure")
        .is_none());
    assert!(judge
        .classify_html_row("row", "heading")
        .expect("html")
        .is_none());
    assert!(judge
        .route_shell_command("show availability")
        .expect("route")
        .is_none());
}

#[test]
fn confidence_and_review_priority_are_typed() {
    assert!(!JevConfidence(0.74).is_decisive(0.75));
    assert!(JevConfidence(0.75).is_decisive(0.75));
    let priority = ReviewPriority::from_jev(2.82, true);
    assert_eq!(priority.score, 3);
    assert!(priority.decisive);
}

#[test]
fn candidate_verdict_cannot_promote_without_decisive_semantics() {
    let uncertain = nbatv_catalog::jev::CandidateVerdict {
        same_game: Some(true),
        both_teams: Some(true),
        date_or_round: Some(true),
        full_game: Some(true),
        quality: Some(3.0),
        decisive: false,
    };
    assert_eq!(uncertain.level(), None);
    let strong = nbatv_catalog::jev::CandidateVerdict {
        decisive: true,
        ..uncertain
    };
    assert_eq!(strong.level(), Some(MatchLevel::Confirmed));
}
