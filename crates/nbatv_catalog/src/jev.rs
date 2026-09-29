//! TypeSafe Jev decision sidecar.
//!
//! This module owns the external contract and deterministic policy. It
//! never lets Jev choose a rung, URL, path, identifier, download, upload,
//! or legality decision. Every `Choice` answer is checked against the exact
//! closed set supplied by Rust.

use crate::probe::{GameContext, ProbeCandidate};
use nbatv_ladder::MatchLevel;
use rustls::pki_types::ServerName;
use rustls::{ClientConfig, ClientConnection, RootCertStore, StreamOwned};
use serde_json::{Map, Value};
use std::fmt;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::time::Duration;

pub const API_URL: &str = "https://api.typesafe.ai/v1/systemone";
pub const MODEL: &str = "jev-1.13.0";
pub const OPENROUTER_API_URL: &str = "https://openrouter.ai/api/v1/chat/completions";

const OPENROUTER_SYSTEM_PROMPT: &str = "You are a strict typed decision engine. Return JSON only, with exactly this shape: {\"answers\":{\"decision\":<answer>}}. The answer must follow the supplied question schema and use only supplied closed-set values. Do not invent identifiers, URLs, or actions.";
pub const DEFAULT_THRESHOLD: f64 = 0.75;

#[derive(Debug, Clone, PartialEq)]
pub enum JevError {
    MissingApiKey,
    MissingModel,
    Transport(String),
    Http { status: u16, body: String },
    InvalidResponse(String),
    InvalidAnswer(String),
}

impl fmt::Display for JevError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingApiKey => write!(
                f,
                "Jev API key is not set (TYPESAFE_API_KEY or OPENROUTER_API_KEY)"
            ),
            Self::MissingModel => write!(f, "OPENROUTER_MODEL is not set"),
            Self::Transport(message) => write!(f, "Jev transport: {message}"),
            Self::Http { status, body } => write!(f, "Jev HTTP {status}: {body}"),
            Self::InvalidResponse(message) => write!(f, "Jev response: {message}"),
            Self::InvalidAnswer(message) => write!(f, "Jev answer: {message}"),
        }
    }
}

impl std::error::Error for JevError {}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum JevQuestion {
    Noul(JevNoul),
    Choice(JevChoice),
    Score(JevScore),
}

impl JevQuestion {
    pub fn id(&self) -> &'static str {
        "decision"
    }
}

impl JevRequest {
    /// Build a request from one or more typed questions. Independent
    /// questions about the same evidence are batched into one HTTP call.
    pub fn batch(
        state: Value,
        questions: Vec<(&'static str, JevQuestion)>,
    ) -> Result<Self, JevError> {
        let mut iter = questions.into_iter();
        let (first_id, first) = iter
            .next()
            .ok_or_else(|| JevError::InvalidAnswer("request has no questions".to_owned()))?;
        let mut request = Self {
            state,
            model: MODEL.to_owned(),
            questions: std::collections::BTreeMap::new(),
        };
        request.questions.insert(first_id.to_owned(), first);
        for (id, question) in iter {
            if request.questions.insert(id.to_owned(), question).is_some() {
                return Err(JevError::InvalidAnswer(format!(
                    "duplicate question id {id:?}"
                )));
            }
        }
        Ok(request)
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct JevNoul {
    pub instructions: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub criteria: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct JevChoice {
    pub instructions: String,
    pub criteria: Value,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct JevScore {
    pub instructions: String,
    pub criteria: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct JevRequest {
    pub state: Value,
    pub model: String,
    pub questions: std::collections::BTreeMap<String, JevQuestion>,
}

impl JevRequest {
    pub fn new(state: Value, question: JevQuestion) -> Self {
        let mut questions = std::collections::BTreeMap::new();
        questions.insert("decision".to_owned(), question);
        Self {
            state,
            model: MODEL.to_owned(),
            questions,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct JevResponse {
    pub model: String,
    pub answers: Map<String, Value>,
}

impl JevResponse {
    pub fn from_value(value: Value) -> Result<Self, JevError> {
        let object = value
            .as_object()
            .ok_or_else(|| JevError::InvalidResponse("top-level value is not an object".into()))?;
        let model = object
            .get("model")
            .and_then(Value::as_str)
            .ok_or_else(|| JevError::InvalidResponse("missing model".into()))?
            .to_owned();
        let answers = object
            .get("answers")
            .and_then(Value::as_object)
            .ok_or_else(|| JevError::InvalidResponse("missing answers object".into()))?
            .clone();
        Ok(Self { model, answers })
    }

    pub fn answer(&self, id: &str) -> Option<&Value> {
        self.answers.get(id)
    }

    pub fn answer_value(question: &JevQuestion, value: Value) -> Result<Value, JevError> {
        Self::validate_answer(question, &value)?;
        Ok(value)
    }

    pub fn validate_answer(question: &JevQuestion, value: &Value) -> Result<(), JevError> {
        let object = value
            .as_object()
            .ok_or_else(|| JevError::InvalidAnswer("answer is not an object".into()))?;
        let expected = match question {
            JevQuestion::Noul(_) => "noul",
            JevQuestion::Choice(_) => "choice",
            JevQuestion::Score(_) => "score",
        };
        if object.get("type").and_then(Value::as_str) != Some(expected) {
            return Err(JevError::InvalidAnswer(format!(
                "expected type {expected}, got {:?}",
                object.get("type")
            )));
        }
        match question {
            JevQuestion::Noul(_) => {
                probability(object.get("noul"), "noul")?;
            }
            JevQuestion::Choice(choice) => {
                let selected = object
                    .get("choice")
                    .and_then(Value::as_str)
                    .ok_or_else(|| JevError::InvalidAnswer("choice is missing".into()))?;
                let criteria = choice.criteria.as_object().ok_or_else(|| {
                    JevError::InvalidAnswer("choice criteria is not an object".into())
                })?;
                if !criteria.contains_key(selected) {
                    return Err(JevError::InvalidAnswer(format!(
                        "selected option {selected:?} was not supplied"
                    )));
                }
                confidence(object)?;
            }
            JevQuestion::Score(score) => {
                let value = object
                    .get("score")
                    .and_then(Value::as_f64)
                    .ok_or_else(|| JevError::InvalidAnswer("score is missing".into()))?;
                if !(0.0..=(score.criteria.len() as f64 - 1.0)).contains(&value) {
                    return Err(JevError::InvalidAnswer(format!(
                        "score {value} is outside the supplied rubric"
                    )));
                }
                confidence(object)?;
            }
        }
        Ok(())
    }
}

fn probability(value: Option<&Value>, field: &str) -> Result<(), JevError> {
    let value = value
        .and_then(Value::as_f64)
        .ok_or_else(|| JevError::InvalidAnswer(format!("{field} is missing")))?;
    if !(0.0..=1.0).contains(&value) {
        return Err(JevError::InvalidAnswer(format!("{field}={value}")));
    }
    Ok(())
}

fn confidence(object: &Map<String, Value>) -> Result<f64, JevError> {
    let value = object
        .get("confidence")
        .and_then(Value::as_f64)
        .ok_or_else(|| JevError::InvalidAnswer("confidence is missing".into()))?;
    if !(0.0..=1.0).contains(&value) {
        return Err(JevError::InvalidAnswer(format!("confidence={value}")));
    }
    Ok(value)
}

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct JevConfidence(pub f64);

impl JevConfidence {
    pub fn is_decisive(self, threshold: f64) -> bool {
        self.0 >= threshold
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Answer {
    pub noul: Option<f64>,
    pub choice: Option<String>,
    pub score: Option<f64>,
    pub confidence: f64,
}

impl Answer {
    fn from_value(value: &Value) -> Result<Self, JevError> {
        let object = value
            .as_object()
            .ok_or_else(|| JevError::InvalidAnswer("answer is not an object".into()))?;
        Ok(Self {
            noul: object.get("noul").and_then(Value::as_f64),
            choice: object
                .get("choice")
                .and_then(Value::as_str)
                .map(str::to_owned),
            score: object.get("score").and_then(Value::as_f64),
            confidence: object
                .get("confidence")
                .and_then(Value::as_f64)
                .ok_or_else(|| JevError::InvalidAnswer("confidence is missing".into()))?,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct FileSelectionInput {
    pub target: GameContext,
    pub item_title: String,
    pub files: Vec<(String, Option<u64>)>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FileSelection {
    Selected {
        name: String,
        confidence: f64,
    },
    Review {
        name: Option<String>,
        confidence: f64,
    },
    Fallback {
        name: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameTypeChoice {
    pub page_text: String,
    pub season: String,
    pub choices: Vec<String>,
}

impl GameTypeChoice {
    pub fn new(page_text: &str, season: &str, choices: &[&str]) -> Self {
        Self {
            page_text: page_text.to_owned(),
            season: season.to_owned(),
            choices: choices.iter().map(|s| (*s).to_owned()).collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamChoice {
    pub label: String,
    pub season: String,
    pub known_teams: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollectorNoteInput {
    pub note: String,
    pub candidate_games: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrawlFailureChoice {
    NotFound,
    RateLimited,
    AccessBlocked,
    TransientServer,
    ParserChange,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HtmlRowChoice {
    HeaderOrFooter,
    TeamTotal,
    PlayerTotal,
    DnpRow,
    Separator,
    Unknown,
}

/// One bounded search-query template chosen from a closed set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchTemplate {
    TeamsAndDate,
    FinalsGameNumber,
    EraRoundLabel,
    TeamArchive,
    SourceSpecific,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChoiceResult {
    pub choice: String,
    pub confidence: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SearchTemplateSelection {
    pub template: SearchTemplate,
    pub confidence: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReviewPriority {
    pub score: u8,
    pub decisive: bool,
}

impl ReviewPriority {
    pub fn from_jev(score: f64, decisive: bool) -> Self {
        let bounded = score.clamp(0.0, 3.0);
        Self {
            score: bounded.round() as u8,
            decisive,
        }
    }

    pub fn round(self) -> u8 {
        self.score
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CandidateVerdict {
    pub same_game: Option<bool>,
    pub both_teams: Option<bool>,
    pub date_or_round: Option<bool>,
    pub full_game: Option<bool>,
    pub quality: Option<f64>,
    pub decisive: bool,
}

impl CandidateVerdict {
    pub fn level(&self) -> Option<MatchLevel> {
        if !self.decisive {
            return None;
        }
        match (self.same_game, self.full_game) {
            (Some(true), Some(true)) => Some(MatchLevel::Confirmed),
            (Some(true), _) => Some(MatchLevel::Likely),
            _ => Some(MatchLevel::Review),
        }
    }
}

pub trait JevJudge: Send + Sync {
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
    fn classify_html_row(&self, _: &str, _: &str) -> Result<Option<HtmlRowChoice>, JevError> {
        Ok(None)
    }
    fn route_shell_command(&self, _: &str) -> Result<Option<String>, JevError> {
        Ok(None)
    }
    fn prioritize_review(&self, _: &[String], _: &str) -> Result<Option<u8>, JevError> {
        Ok(None)
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct DisabledJevJudge;

impl JevJudge for DisabledJevJudge {}

#[derive(Debug, Clone, PartialEq, Eq)]
struct JevEndpoint {
    host: String,
    port: u16,
    authority: String,
    target: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JevWireFormat {
    TypeSafe,
    OpenRouter,
}

pub struct DirectHttpJev {
    endpoint: JevEndpoint,
    api_key: String,
    tls: Arc<ClientConfig>,
    model: String,
    threshold: f64,
    retries: u32,
    wire_format: JevWireFormat,
}

impl fmt::Debug for DirectHttpJev {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DirectHttpJev")
            .field("endpoint", &self.endpoint)
            .field("api_key", &"[REDACTED]")
            .field("model", &self.model)
            .field("threshold", &self.threshold)
            .field("wire_format", &self.wire_format)
            .field("retries", &self.retries)
            .finish()
    }
}

impl DirectHttpJev {
    pub fn from_env() -> Result<Self, JevError> {
        if let Ok(api_key) = std::env::var("OPENROUTER_API_KEY") {
            if !api_key.trim().is_empty() {
                return Self::openrouter_from_env();
            }
        }
        let api_key = std::env::var("TYPESAFE_API_KEY")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .ok_or(JevError::MissingApiKey)?;
        Self::with_endpoint(parse_https_endpoint(API_URL)?, &api_key)
    }

    /// Construct an OpenRouter client from the local environment.
    pub fn openrouter_from_env() -> Result<Self, JevError> {
        let api_key = std::env::var("OPENROUTER_API_KEY")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .ok_or(JevError::MissingApiKey)?;
        let model = std::env::var("OPENROUTER_MODEL")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .ok_or(JevError::MissingModel)?;
        Self::openrouter_from_env_or(&api_key, &model)
    }

    /// Construct an OpenRouter client with out-of-band credentials for tests
    /// or local adapters. The model ID is always explicit.
    pub fn openrouter_from_env_or(api_key: &str, model: &str) -> Result<Self, JevError> {
        if model.trim().is_empty() {
            return Err(JevError::MissingModel);
        }
        if model.contains(['\r', '\n']) {
            return Err(JevError::Transport(
                "OpenRouter model contains an invalid newline".to_owned(),
            ));
        }
        Self::with_endpoint_and_format(
            parse_https_endpoint(OPENROUTER_API_URL)?,
            api_key,
            model,
            JevWireFormat::OpenRouter,
        )
    }

    /// Construct a client with an explicit endpoint. Intended for tests
    /// and local adapters; credentials are still supplied out of band.
    pub fn from_env_or(api_key: &str, endpoint: &str) -> Result<Self, JevError> {
        Self::with_endpoint(parse_https_endpoint(endpoint)?, api_key)
    }

    fn with_endpoint(endpoint: JevEndpoint, api_key: &str) -> Result<Self, JevError> {
        Self::with_endpoint_and_format(endpoint, api_key, MODEL, JevWireFormat::TypeSafe)
    }

    fn with_endpoint_and_format(
        endpoint: JevEndpoint,
        api_key: &str,
        model: &str,
        wire_format: JevWireFormat,
    ) -> Result<Self, JevError> {
        if api_key.trim().is_empty() {
            return Err(JevError::MissingApiKey);
        }
        if api_key.contains(['\r', '\n']) {
            return Err(JevError::Transport(
                "Jev API key contains an invalid newline".to_owned(),
            ));
        }
        Ok(Self {
            endpoint,
            api_key: api_key.to_owned(),
            tls: rustls_client_config()?,
            model: model.to_owned(),
            threshold: DEFAULT_THRESHOLD,
            retries: 2,
            wire_format,
        })
    }

    fn ask_transport(
        &self,
        state: Value,
        questions: Vec<(&'static str, JevQuestion)>,
    ) -> Result<Vec<(String, Answer)>, JevError> {
        let question_count = questions.len();
        let mut request = JevRequest::batch(state, questions)?;
        request.model = self.model.clone();
        let mut attempt = 0;
        loop {
            let request_body = match self.wire_format {
                JevWireFormat::TypeSafe => serde_json::to_vec(&request)
                    .map_err(|error| JevError::InvalidResponse(error.to_string()))?,
                JevWireFormat::OpenRouter => openrouter_request_body(&request)?,
            };
            let (status, body) = self.post_json(&request_body)?;
            if (200..300).contains(&status) {
                let response = match self.wire_format {
                    JevWireFormat::TypeSafe => {
                        let value: Value = serde_json::from_str(&body)
                            .map_err(|error| JevError::InvalidResponse(error.to_string()))?;
                        JevResponse::from_value(value)?
                    }
                    JevWireFormat::OpenRouter => parse_openrouter_response(&body, &self.model)?,
                };
                let mut answers = Vec::with_capacity(question_count);
                for (id, answer) in request.questions.iter() {
                    let value = response.answer(id).ok_or_else(|| {
                        JevError::InvalidResponse(format!("missing answer {id:?}"))
                    })?;
                    JevResponse::validate_answer(answer, value)?;
                    answers.push((id.clone(), Answer::from_value(value)?));
                }
                return Ok(answers);
            }
            let retryable = status == 429 || status == 529;
            if !retryable || attempt >= self.retries {
                return Err(JevError::Http { status, body });
            }
            attempt += 1;
            std::thread::sleep(Duration::from_millis(250 * u64::from(attempt)));
        }
    }

    fn accepted_choice(
        &self,
        state: Value,
        instructions: &str,
        options: &[&str],
    ) -> Result<Option<String>, JevError> {
        Ok(self
            .choose(state, instructions, options)?
            .filter(|choice| self.decisive(choice.confidence))
            .map(|choice| choice.choice))
    }

    /// Ask a transport client directly. This is the single transport seam;
    /// all workflow methods below compose its typed answers.
    pub fn ask(
        &self,
        state: Value,
        questions: Vec<(&'static str, JevQuestion)>,
    ) -> Result<Vec<(String, Answer)>, JevError> {
        self.ask_transport(state, questions)
    }

    fn ask_one(
        &self,
        state: Value,
        question: JevQuestion,
    ) -> Result<Option<(Answer, f64)>, JevError> {
        Ok(self
            .ask_transport(state, vec![("decision", question)])?
            .into_iter()
            .next()
            .map(|(_, answer)| {
                let confidence = answer.confidence;
                (answer, confidence)
            }))
    }

    fn choose(
        &self,
        state: Value,
        instructions: &str,
        options: &[&str],
    ) -> Result<Option<ChoiceResult>, JevError> {
        let criteria = options
            .iter()
            .map(|option| ((*option).to_owned(), Value::Null))
            .collect::<Map<String, Value>>();
        let question = JevQuestion::Choice(JevChoice {
            instructions: instructions.to_owned(),
            criteria: Value::Object(criteria),
        });
        Ok(self
            .ask_one(state, question)?
            .map(|(answer, confidence)| ChoiceResult {
                choice: answer.choice.unwrap_or_default(),
                confidence,
            }))
    }

    fn decisive(&self, confidence: f64) -> bool {
        JevConfidence(confidence).is_decisive(self.threshold)
    }

    fn post_json(&self, body: &[u8]) -> Result<(u16, String), JevError> {
        let request = render_jev_request(&self.endpoint, &self.api_key, body);
        let mut stream = connect_tls(&self.endpoint, Arc::clone(&self.tls))?;
        stream
            .write_all(&request)
            .map_err(|error| JevError::Transport(format!("writing Jev request: {error}")))?;
        stream
            .flush()
            .map_err(|error| JevError::Transport(format!("flushing Jev request: {error}")))?;
        let response = read_limited(&mut stream, 2 * 1024 * 1024)?;
        parse_http_response(&response)
    }
}

fn openrouter_request_body(request: &JevRequest) -> Result<Vec<u8>, JevError> {
    let request_json = serde_json::to_string(request)
        .map_err(|error| JevError::InvalidResponse(error.to_string()))?;
    serde_json::to_vec(&serde_json::json!({
        "model": request.model,
        "messages": [
            {"role": "system", "content": OPENROUTER_SYSTEM_PROMPT},
            {"role": "user", "content": request_json},
        ],
        "response_format": {"type": "json_object"},
    }))
    .map_err(|error| JevError::InvalidResponse(error.to_string()))
}

fn parse_openrouter_response(body: &str, fallback_model: &str) -> Result<JevResponse, JevError> {
    let root: Value =
        serde_json::from_str(body).map_err(|error| JevError::InvalidResponse(error.to_string()))?;
    let content = root
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            JevError::InvalidResponse("OpenRouter response omitted message content".into())
        })?;
    let content = parse_openrouter_content(content)?;
    let object = content.as_object().ok_or_else(|| {
        JevError::InvalidResponse("OpenRouter message content is not an object".into())
    })?;
    let answers = object
        .get("answers")
        .cloned()
        .or_else(|| {
            object
                .get("decision")
                .cloned()
                .map(|decision| serde_json::json!({"decision": decision}))
        })
        .ok_or_else(|| JevError::InvalidResponse("OpenRouter content omitted answers".into()))?;
    let model = root
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or(fallback_model);
    JevResponse::from_value(serde_json::json!({"model": model, "answers": answers}))
}

fn parse_openrouter_content(content: &str) -> Result<Value, JevError> {
    let trimmed = content.trim();
    let unfenced = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .unwrap_or(trimmed);
    let unfenced = unfenced.strip_suffix("```").unwrap_or(unfenced).trim();
    serde_json::from_str(unfenced)
        .map_err(|error| JevError::InvalidResponse(format!("OpenRouter content: {error}")))
}

fn parse_https_endpoint(endpoint: &str) -> Result<JevEndpoint, JevError> {
    let rest = endpoint
        .strip_prefix("https://")
        .ok_or_else(|| JevError::Transport("Jev endpoint must use https".to_owned()))?;
    if rest.contains('#') {
        return Err(JevError::Transport(
            "Jev endpoint must not contain a fragment".to_owned(),
        ));
    }
    let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
    if authority.is_empty() || authority.contains('@') || authority.chars().any(char::is_whitespace)
    {
        return Err(JevError::Transport("invalid Jev endpoint".to_owned()));
    }
    let (host, port_text) = if let Some(bracketed) = authority.strip_prefix('[') {
        let close = bracketed
            .find(']')
            .ok_or_else(|| JevError::Transport("invalid Jev IPv6 endpoint".to_owned()))?;
        let host = &bracketed[..close];
        let remainder = &bracketed[close + 1..];
        if remainder.is_empty() {
            (host, None)
        } else {
            let port = remainder
                .strip_prefix(':')
                .ok_or_else(|| JevError::Transport("invalid Jev endpoint port".to_owned()))?;
            (host, Some(port))
        }
    } else {
        match authority.rsplit_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (authority, None),
        }
    };
    if host.is_empty() {
        return Err(JevError::Transport("invalid Jev endpoint host".to_owned()));
    }
    let port = match port_text {
        Some("") | None => 443,
        Some(port) => port
            .parse::<u16>()
            .ok()
            .filter(|port| *port != 0)
            .ok_or_else(|| JevError::Transport("invalid Jev endpoint port".to_owned()))?,
    };
    Ok(JevEndpoint {
        host: host.to_owned(),
        port,
        authority: authority.to_owned(),
        target: if path.is_empty() {
            "/".to_owned()
        } else {
            format!("/{path}")
        },
    })
}

fn rustls_client_config() -> Result<Arc<ClientConfig>, JevError> {
    let roots = RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    let config = ClientConfig::builder_with_provider(Arc::new(rustls_graviola::default_provider()))
        .with_safe_default_protocol_versions()
        .map_err(|error| JevError::Transport(format!("configuring Jev TLS: {error}")))?
        .with_root_certificates(roots)
        .with_no_client_auth();
    Ok(Arc::new(config))
}

fn connect_tls(
    endpoint: &JevEndpoint,
    config: Arc<ClientConfig>,
) -> Result<StreamOwned<ClientConnection, TcpStream>, JevError> {
    let addresses = (endpoint.host.as_str(), endpoint.port)
        .to_socket_addrs()
        .map_err(|error| JevError::Transport(format!("resolving Jev endpoint: {error}")))?;
    let mut last_error = None;
    let socket = addresses
        .into_iter()
        .find_map(
            |address| match TcpStream::connect_timeout(&address, Duration::from_secs(30)) {
                Ok(socket) => Some(socket),
                Err(error) => {
                    last_error = Some(error);
                    None
                }
            },
        )
        .ok_or_else(|| {
            JevError::Transport(format!(
                "connecting to Jev endpoint: {}",
                last_error
                    .map(|error| error.to_string())
                    .unwrap_or_else(|| "no addresses".to_owned())
            ))
        })?;
    socket
        .set_read_timeout(Some(Duration::from_secs(30)))
        .map_err(|error| JevError::Transport(format!("setting Jev read timeout: {error}")))?;
    socket
        .set_write_timeout(Some(Duration::from_secs(30)))
        .map_err(|error| JevError::Transport(format!("setting Jev write timeout: {error}")))?;
    let server_name = ServerName::try_from(endpoint.host.clone())
        .map_err(|error| JevError::Transport(format!("invalid Jev TLS server name: {error}")))?;
    let connection = ClientConnection::new(config, server_name)
        .map_err(|error| JevError::Transport(format!("initializing Jev TLS: {error}")))?;
    Ok(StreamOwned::new(connection, socket))
}

fn render_jev_request(endpoint: &JevEndpoint, api_key: &str, body: &[u8]) -> Vec<u8> {
    let mut request = format!(
        "POST {} HTTP/1.1\r\nHost: {}\r\nAuthorization: Bearer {}\r\nUser-Agent: nba-tv-catalog-jev/0.1 (personal archive research)\r\nAccept: application/json\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        endpoint.target,
        endpoint.authority,
        api_key,
        body.len()
    )
    .into_bytes();
    request.extend_from_slice(body);
    request
}

fn read_limited(
    stream: &mut StreamOwned<ClientConnection, TcpStream>,
    max_bytes: usize,
) -> Result<Vec<u8>, JevError> {
    let mut response = Vec::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let read = stream
            .read(&mut buffer)
            .map_err(|error| JevError::Transport(format!("reading Jev response: {error}")))?;
        if read == 0 {
            return Ok(response);
        }
        if response.len().saturating_add(read) > max_bytes {
            return Err(JevError::Transport(
                "Jev response exceeded size limit".to_owned(),
            ));
        }
        response.extend_from_slice(&buffer[..read]);
    }
}

fn parse_http_response(raw: &[u8]) -> Result<(u16, String), JevError> {
    let header_end = raw
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| JevError::Transport("Jev response omitted HTTP headers".to_owned()))?;
    let mut headers = [httparse::EMPTY_HEADER; 64];
    let mut response = httparse::Response::new(&mut headers);
    response
        .parse(&raw[..header_end + 4])
        .map_err(|error| JevError::Transport(format!("invalid Jev HTTP response: {error}")))?;
    let status = response
        .code
        .ok_or_else(|| JevError::Transport("Jev response omitted HTTP status".to_owned()))?;
    let body = &raw[header_end + 4..];
    let transfer_encoding = response
        .headers
        .iter()
        .find(|header| header.name.eq_ignore_ascii_case("transfer-encoding"))
        .and_then(|header| std::str::from_utf8(header.value).ok());
    let content_length = response
        .headers
        .iter()
        .find(|header| header.name.eq_ignore_ascii_case("content-length"))
        .map(|header| {
            std::str::from_utf8(header.value)
                .ok()
                .and_then(|value| value.parse::<usize>().ok())
                .ok_or_else(|| JevError::Transport("invalid Jev Content-Length".to_owned()))
        })
        .transpose()?;
    let body = if transfer_encoding.is_some_and(|value| {
        value
            .split(',')
            .any(|value| value.trim().eq_ignore_ascii_case("chunked"))
    }) {
        decode_chunked_body(body)?
    } else if let Some(expected) = content_length {
        if body.len() != expected {
            return Err(JevError::Transport(format!(
                "Jev Content-Length was {expected}, received {} bytes",
                body.len()
            )));
        }
        body.to_vec()
    } else {
        body.to_vec()
    };
    let body = String::from_utf8(body)
        .map_err(|error| JevError::Transport(format!("Jev response is not UTF-8: {error}")))?;
    Ok((status, body))
}

fn decode_chunked_body(encoded: &[u8]) -> Result<Vec<u8>, JevError> {
    let mut cursor = 0;
    let mut decoded = Vec::new();
    loop {
        let line_end = encoded[cursor..]
            .windows(2)
            .position(|window| window == b"\r\n")
            .map(|position| cursor + position)
            .ok_or_else(|| JevError::Transport("invalid Jev chunk size line".to_owned()))?;
        let size_text = std::str::from_utf8(&encoded[cursor..line_end])
            .map_err(|error| JevError::Transport(format!("invalid Jev chunk size: {error}")))?;
        let size_text = size_text.split(';').next().unwrap_or_default().trim();
        let size = usize::from_str_radix(size_text, 16)
            .map_err(|error| JevError::Transport(format!("invalid Jev chunk size: {error}")))?;
        cursor = line_end + 2;
        if size == 0 {
            return Ok(decoded);
        }
        let data_end = cursor
            .checked_add(size)
            .filter(|end| encoded.get(*end..*end + 2) == Some(b"\r\n"))
            .ok_or_else(|| JevError::Transport("truncated Jev chunk".to_owned()))?;
        decoded.extend_from_slice(&encoded[cursor..data_end]);
        cursor = data_end + 2;
    }
}

impl JevJudge for DirectHttpJev {
    fn select_file(&self, input: &FileSelectionInput) -> Result<Option<FileSelection>, JevError> {
        if input.files.is_empty() {
            return Ok(None);
        }
        let files = input
            .files
            .iter()
            .map(|(name, duration)| serde_json::json!({"name": name, "duration_secs": duration}))
            .collect::<Vec<_>>();
        let state = serde_json::json!({
            "target": {
                "game_id": input.target.game_id,
                "teams": [input.target.away_team, input.target.home_team],
                "date": input.target.date,
            },
            "item": {"title": input.item_title, "files": files}
        });
        if input.files.len() == 1 {
            return Ok(Some(FileSelection::Fallback {
                name: input.files[0].0.clone(),
            }));
        }
        let options = input
            .files
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>();
        let question = JevQuestion::Choice(JevChoice {
            instructions: "Which listed file corresponds to the target Game, rather than another Game in the same item? Return only one supplied filename.".to_owned(),
            criteria: Value::Object(
                options.iter().map(|name| ((*name).to_owned(), Value::Null)).collect(),
            ),
        });
        let Some((answer, confidence)) = self.ask_one(state, question)? else {
            return Ok(None);
        };
        let Some(name) = answer.choice else {
            return Ok(None);
        };
        if !self.decisive(confidence) {
            return Ok(Some(FileSelection::Review {
                name: Some(name),
                confidence,
            }));
        }
        Ok(Some(FileSelection::Selected { name, confidence }))
    }

    fn match_candidate(
        &self,
        game: &GameContext,
        candidate: &ProbeCandidate,
    ) -> Result<Option<CandidateVerdict>, JevError> {
        let state = serde_json::json!({
            "target": {"game_id": game.game_id, "teams": [game.away_team, game.home_team], "date": game.date},
            "candidate": {"title": candidate.title, "description": candidate.description, "duration_secs": candidate.duration_secs}
        });
        let question = JevQuestion::Score(JevScore {
            instructions: "How well does this candidate describe the same full Game? Use the rubric; do not infer missing data.".to_owned(),
            criteria: vec!["unrelated".into(), "weak".into(), "plausible".into(), "strong".into()],
        });
        let Some((answer, confidence)) = self.ask_one(state, question)? else {
            return Ok(None);
        };
        Ok(Some(CandidateVerdict {
            same_game: answer.score.map(|value| value >= 2.0),
            both_teams: None,
            date_or_round: None,
            full_game: None,
            quality: answer.score,
            decisive: self.decisive(confidence),
        }))
    }

    fn classify_game_type(&self, input: &GameTypeChoice) -> Result<Option<String>, JevError> {
        let state = serde_json::json!({"page_text": input.page_text, "season": input.season});
        let options = input.choices.iter().map(String::as_str).collect::<Vec<_>>();
        self.accepted_choice(
            state,
            "Classify the Game type using only the supplied labels. Choose UNCERTAIN when evidence is insufficient.",
            &options,
        )
    }

    fn align_team(&self, input: &TeamChoice) -> Result<Option<String>, JevError> {
        let teams = input
            .known_teams
            .iter()
            .map(|(slug, name)| serde_json::json!({"slug": slug, "name": name}))
            .collect::<Vec<_>>();
        let state =
            serde_json::json!({"label": input.label, "season": input.season, "known_teams": teams});
        let options = input
            .known_teams
            .iter()
            .map(|(slug, _)| slug.as_str())
            .collect::<Vec<_>>();
        self.accepted_choice(
            state,
            "Which existing stored team does this historical label refer to? Choose only a supplied slug; otherwise choose an uncertain outcome if one is supplied.",
            &options,
        )
    }

    fn match_collector_note(&self, input: &CollectorNoteInput) -> Result<Option<String>, JevError> {
        let state =
            serde_json::json!({"note": input.note, "candidate_games": input.candidate_games});
        let options = input
            .candidate_games
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        self.accepted_choice(
            state,
            "Which candidate game does the collector note describe? Choose only a supplied game_id.",
            &options,
        )
    }

    fn choose_search_template(
        &self,
        game: &GameContext,
        source_class: &str,
    ) -> Result<Option<SearchTemplateSelection>, JevError> {
        let state = serde_json::json!({"game": {"game_id": game.game_id, "teams": [game.away_team, game.home_team], "date": game.date}, "source_class": source_class});
        let options = [
            "teams_and_date",
            "finals_game_number",
            "era_round_label",
            "team_archive",
            "source_specific",
        ];
        let Some(choice) = self.choose(
            state,
            "Which predefined search-query template is most appropriate? Do not generate a query.",
            &options,
        )?
        else {
            return Ok(None);
        };
        if !self.decisive(choice.confidence) {
            return Ok(None);
        }
        let template = match choice.choice.as_str() {
            "teams_and_date" => SearchTemplate::TeamsAndDate,
            "finals_game_number" => SearchTemplate::FinalsGameNumber,
            "era_round_label" => SearchTemplate::EraRoundLabel,
            "team_archive" => SearchTemplate::TeamArchive,
            "source_specific" => SearchTemplate::SourceSpecific,
            _ => return Ok(None),
        };
        Ok(Some(SearchTemplateSelection {
            template,
            confidence: choice.confidence,
        }))
    }

    fn classify_crawl_failure(
        &self,
        response: &str,
    ) -> Result<Option<CrawlFailureChoice>, JevError> {
        let options = [
            "not_found",
            "rate_limited",
            "access_blocked",
            "transient_server",
            "parser_change",
            "unknown",
        ];
        let Some(choice) = self.choose(
            serde_json::json!({"response": response}),
            "Classify the ambiguous crawl response using the supplied labels.",
            &options,
        )?
        else {
            return Ok(None);
        };
        if !self.decisive(choice.confidence) {
            return Ok(None);
        }
        Ok(Some(match choice.choice.as_str() {
            "not_found" => CrawlFailureChoice::NotFound,
            "rate_limited" => CrawlFailureChoice::RateLimited,
            "access_blocked" => CrawlFailureChoice::AccessBlocked,
            "transient_server" => CrawlFailureChoice::TransientServer,
            "parser_change" => CrawlFailureChoice::ParserChange,
            _ => CrawlFailureChoice::Unknown,
        }))
    }

    fn classify_html_row(
        &self,
        fragment: &str,
        heading: &str,
    ) -> Result<Option<HtmlRowChoice>, JevError> {
        let options = [
            "header_or_footer",
            "team_total",
            "player_total",
            "dnp_row",
            "separator",
            "unknown",
        ];
        let Some(choice) = self.choose(
            serde_json::json!({"fragment": fragment, "heading": heading}),
            "Classify the quarantined HTML row fragment using the supplied labels.",
            &options,
        )?
        else {
            return Ok(None);
        };
        if !self.decisive(choice.confidence) {
            return Ok(None);
        }
        Ok(Some(match choice.choice.as_str() {
            "header_or_footer" => HtmlRowChoice::HeaderOrFooter,
            "team_total" => HtmlRowChoice::TeamTotal,
            "player_total" => HtmlRowChoice::PlayerTotal,
            "dnp_row" => HtmlRowChoice::DnpRow,
            "separator" => HtmlRowChoice::Separator,
            _ => HtmlRowChoice::Unknown,
        }))
    }

    fn route_shell_command(&self, command: &str) -> Result<Option<String>, JevError> {
        let options = [
            "find_game",
            "open_season",
            "open_team",
            "play_game",
            "show_availability",
            "show_tape_sources",
            "open_review",
            "other",
        ];
        self.accepted_choice(
            serde_json::json!({"command": command}),
            "Route the natural-language command to one supplied existing Shell intent. Do not perform the action.",
            &options,
        )
    }

    fn prioritize_review(&self, items: &[String], query: &str) -> Result<Option<u8>, JevError> {
        let question = JevQuestion::Score(JevScore {
            instructions: "Rate how useful this review item is for a human reviewer; the result is only a queue priority.".to_owned(),
            criteria: vec!["unrelated".into(), "weak".into(), "plausible".into(), "strong".into()],
        });
        let state = serde_json::json!({"items": items, "query": query});
        let Some((answer, confidence)) = self.ask_one(state, question)? else {
            return Ok(None);
        };
        let score = answer.score.unwrap_or(0.0).clamp(0.0, 3.0).round() as u8;
        Ok(self.decisive(confidence).then_some(score))
    }
}

/// Pick the existing file selected by Jev, or the deterministic longest
/// original when the sidecar is disabled/uncertain. This is the narrow
/// policy for multi-file Internet Archive items.
pub fn select_ia_file(
    judge: &dyn JevJudge,
    input: &FileSelectionInput,
) -> Result<Option<(String, Option<u64>)>, JevError> {
    let fallback = || {
        input
            .files
            .iter()
            .max_by_key(|(_, duration)| duration.unwrap_or(0))
            .cloned()
    };
    match judge.select_file(input).ok().flatten() {
        Some(FileSelection::Selected { name, .. } | FileSelection::Fallback { name }) => Ok(input
            .files
            .iter()
            .find(|(candidate, _)| *candidate == name)
            .cloned()
            .or_else(fallback)),
        Some(FileSelection::Review { .. }) | None => Ok(fallback()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_ten_workflows_are_available_behind_disabled_judge() {
        let judge = DisabledJevJudge;
        let game = GameContext {
            game_id: "g".into(),
            home_team: "h".into(),
            away_team: "a".into(),
            date: "2026-01-01".into(),
        };
        let files = FileSelectionInput {
            target: game.clone(),
            item_title: "x".into(),
            files: vec![("x.mp4".into(), None)],
        };
        let candidate = ProbeCandidate {
            url_or_pointer: "pointer".into(),
            title: "x".into(),
            description: String::new(),
            duration_secs: None,
        };
        let note = CollectorNoteInput {
            note: "x".into(),
            candidate_games: vec!["194611010TRH".into()],
        };
        let team = TeamChoice {
            label: "x".into(),
            season: "1946-47".into(),
            known_teams: vec![("TRH".into(), "Huskies".into())],
        };
        assert!(judge.select_file(&files).unwrap().is_none());
        assert!(judge.match_candidate(&game, &candidate).unwrap().is_none());
        assert!(judge
            .classify_game_type(&GameTypeChoice::new("x", "x", &["UNCERTAIN"]))
            .unwrap()
            .is_none());
        assert!(judge.align_team(&team).unwrap().is_none());
        assert!(judge.match_collector_note(&note).unwrap().is_none());
        assert!(judge
            .choose_search_template(&game, "youtube")
            .unwrap()
            .is_none());
        assert!(judge.classify_crawl_failure("x").unwrap().is_none());
        assert!(judge.classify_html_row("x", "x").unwrap().is_none());
        assert!(judge.route_shell_command("x").unwrap().is_none());
        assert!(judge.prioritize_review(&[], "x").unwrap().is_none());
    }

    #[test]
    fn https_endpoint_and_request_are_rendered_without_a_transport() {
        let endpoint = parse_https_endpoint("https://api.typesafe.ai/v1/systemone").unwrap();
        assert_eq!(endpoint.host, "api.typesafe.ai");
        assert_eq!(endpoint.port, 443);
        assert_eq!(endpoint.target, "/v1/systemone");

        let request = render_jev_request(&endpoint, "test-key", b"{}");
        let request = String::from_utf8(request).unwrap();
        assert!(request.starts_with("POST /v1/systemone HTTP/1.1\r\n"));
        assert!(request.contains("Host: api.typesafe.ai\r\n"));
        assert!(request.contains("Authorization: Bearer test-key\r\n"));
        assert!(request.ends_with("Content-Length: 2\r\nConnection: close\r\n\r\n{}"));
    }

    #[test]
    fn response_wire_parser_preserves_http_error_body() {
        let response = parse_http_response(
            b"HTTP/1.1 403 Forbidden\r\nContent-Length: 14\r\n\r\nblocked by pol",
        )
        .unwrap();
        assert_eq!(response.0, 403);
        assert_eq!(response.1, "blocked by pol");
    }

    #[test]
    fn response_wire_parser_decodes_chunked_json() {
        let response = parse_http_response(
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\n{\"m\r\n3\r\nv\":\r\n1\r\n1\r\n1\r\n}\r\n0\r\n\r\n",
        )
        .unwrap();
        assert_eq!(response.0, 200);
        assert_eq!(response.1, "{\"mv\":1}");
    }
    #[test]
    fn openrouter_request_uses_configured_model_and_chat_shape() {
        let question = JevQuestion::Choice(JevChoice {
            instructions: "Choose one.".to_owned(),
            criteria: serde_json::json!({"a": null}),
        });
        let mut request = JevRequest::new(serde_json::json!({"game": "x"}), question);
        request.model = "vendor/model".to_owned();

        let body: Value = serde_json::from_slice(&openrouter_request_body(&request).unwrap())
            .expect("OpenRouter request JSON");
        assert_eq!(body["model"], "vendor/model");
        assert_eq!(body["response_format"]["type"], "json_object");
        let messages = body["messages"].as_array().expect("chat messages");
        assert_eq!(messages[0]["role"], "system");
        let embedded: Value = serde_json::from_str(
            messages[1]["content"]
                .as_str()
                .expect("serialized typed request"),
        )
        .expect("embedded typed request");
        assert_eq!(embedded["model"], "vendor/model");
        assert_eq!(embedded["questions"]["decision"]["type"], "choice");
    }

    #[test]
    fn openrouter_response_parses_typed_content_from_message_choices() {
        let question = JevQuestion::Choice(JevChoice {
            instructions: "Choose one.".to_owned(),
            criteria: serde_json::json!({"a": null}),
        });
        let body = serde_json::json!({
            "model": "vendor/model",
            "choices": [{
                "message": {
                    "content": "```json\n{\"answers\":{\"decision\":{\"type\":\"choice\",\"choice\":\"a\",\"confidence\":0.9}}}\n```"
                }
            }]
        });
        let response = parse_openrouter_response(&body.to_string(), "fallback/model")
            .expect("OpenRouter response");
        assert_eq!(response.model, "vendor/model");
        let answer = response.answer("decision").expect("decision answer");
        JevResponse::validate_answer(&question, answer).expect("typed answer");
        assert_eq!(answer["choice"], "a");
    }

    #[test]
    fn openrouter_configuration_requires_key_and_model() {
        assert!(matches!(
            DirectHttpJev::openrouter_from_env_or("", "vendor/model"),
            Err(JevError::MissingApiKey)
        ));
        assert!(matches!(
            DirectHttpJev::openrouter_from_env_or("test-key", ""),
            Err(JevError::MissingModel)
        ));
        let client = DirectHttpJev::openrouter_from_env_or("test-key", "vendor/model")
            .expect("offline OpenRouter configuration");
        assert_eq!(client.endpoint.target, "/api/v1/chat/completions");
        assert_eq!(client.wire_format, JevWireFormat::OpenRouter);
        assert!(!format!("{client:?}").contains("test-key"));
    }
}
