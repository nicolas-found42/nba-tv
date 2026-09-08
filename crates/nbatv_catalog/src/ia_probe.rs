//! Internet Archive SourceProbe (rung 1, issue #21).
//!
//! [`IaProbe`] answers rung 1 through the public, keyless IA APIs quoted in
//! research note 15 §1: an `advancedsearch.php` inventory query, then one
//! `metadata/<identifier>` fetch per promising item to enumerate files. It
//! returns evidence only — one [`ProbeCandidate`] per IA item pointing at the
//! item's longest original video file as a direct
//! `https://archive.org/download/<identifier>/<filename>` URL (the
//! progressive-bytes shape the Player Backend's Lane A plays). Scoring stays
//! catalog-side ([`crate::score_candidate`]).
//!
//! Query shapes (note 15 verbatim):
//!
//! - inventory: `identifier:(*nba*finals*game*) AND mediatype:(movies)`
//! - fallback (only when the inventory query returns zero docs):
//!   `title:(NBA Finals) AND mediatype:(movies)`
//!
//! Year narrowing is client-side: when any identifier mentions the game's
//! season year (4-digit, else last-2-digit for slugs like
//! `93-nbafinals-game-6`), only those items are fetched — undated noise
//! never costs a metadata request. At most [`MAX_METADATA_FETCHES`] items
//! are fetched, so one probe call costs at most 3 outbound requests.
//! Regular-season games mostly record honest misses here: note 15 finds IA
//! full-game density high for Finals, low for the regular season, and this
//! probe invents no team-name query the note does not verify.
//!
//! Pacing: one [`PolitenessConfig::ia_request_min_interval`] sleep precedes
//! every outbound request (the sleeper is injectable so tests assert the
//! pacing without waiting). Rung 1 is quota-free: the YouTube budget is
//! never touched.
//!
//! Failure honesty: a failed search defers the query (records nothing,
//! retries next sweep). A failed item fetch is skipped, never fatal — only
//! when NO item could be evaluated does the probe defer, so one unreadable
//! item cannot discard a sibling's good evidence. Items with no playable
//! original video file simply yield no candidate.
//!
//! Series-item caveat: one IA item can hold a whole series (e.g.
//! `1996-nba-finals-game-3` carries all six games' files). The probe reports
//! the longest original video file — full-game bytes from the right series,
//! not necessarily the probed game of it. The per-game crosswalk on the tape
//! row is the BR-slug `game_id` the sweep was asked about.
//!
//! Transport is a `curl` subprocess (mirroring the repo's polite BR shell
//! fetches): `curl` must be on `PATH`. JSON is parsed with the dependency-
//! free reader below — this ticket adds zero new dependencies.
//!
//! Live verification (polite: 2-3 outbound requests, ≥5s apart, identifying
//! UA, metadata only — never media bytes). Re-run monthly against the
//! stamp below:
//!
//! ```sh
//! UA="nba-tv-catalog-smoke/1.0 (personal archive research)"
//! curl -sS -m 30 -A "$UA" \
//!   'https://archive.org/advancedsearch.php?q=identifier%3A%28%2Anba%2Afinals%2Agame%2A%29+AND+mediatype%3A%28movies%29&fl%5B%5D=identifier&fl%5B%5D=title&rows=8&output=json'
//! # wait ≥5s, then pick an item from the docs list:
//! curl -sS -m 30 -A "$UA" 'https://archive.org/metadata/1996-nba-finals-game-3'
//! ```
//!
//! Smoke log 2026-09-08 (3 requests, identifying UA, ≥5s apart, no media):
//! the inventory query returned game-shaped items (`1996-nba-finals-game-3`,
//! `1990-nba-finals-game-3`, `1975-nba-finals-game-1`, …); the metadata
//! fetch for `1996-nba-finals-game-3` enumerated `1996 NBA Finals Game 3.mp4`
//! (MPEG4, 714 MB, 138 min) — the longest-original-file candidate shape this
//! probe emits. A live sweep of `199006140POR` (1990 Finals G5, POR/DET)
//! through [`IaProbe`] resolved Playable at rank 1, confidence 1.0, with a
//! genuine `archive.org/download/…` URL.
//!
use crate::politeness::PolitenessConfig;
use crate::probe::{GameContext, ProbeCandidate, ProbeOutcome, SourceProbe};
use crate::scorer::score_candidate;
use nbatv_ladder::YoutubeQuota;
use std::process::Command;
use std::time::Duration;

/// Longest-original wins, but never more than two items per probe call:
/// one search plus up to two metadata fetches bounds a call at 3 requests.
const MAX_METADATA_FETCHES: usize = 2;
/// Inventory page size for the advancedsearch query.
const SEARCH_ROWS: u32 = 25;
/// Request timeout for the live transport.
const CURL_MAX_TIME_SECS: u64 = 30;
/// Contact UA for live IA requests (private-research posture).
const IA_USER_AGENT: &str = "nba-tv-personal-archive/private-research-contact-local";

/// What went wrong behind an [`IaHttp`] fetch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IaError {
    /// The bytes never arrived (curl failure, HTTP error, stub miss).
    Transport(String),
    /// The bytes arrived but are not the expected JSON shape.
    Parse(String),
}

impl std::fmt::Display for IaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Transport(message) => write!(f, "ia transport: {message}"),
            Self::Parse(message) => write!(f, "ia parse: {message}"),
        }
    }
}

impl std::error::Error for IaError {}

/// The one seam the probe reads bytes through: production shells out to
/// `curl`, tests serve committed JSON snapshots. Never touches media —
/// `advancedsearch` + `metadata` JSON only.
pub trait IaHttp: Send + Sync {
    /// GET `url`, returning the response body as text.
    fn get(&self, url: &str) -> Result<String, IaError>;
}

/// Live [`IaHttp`]: `curl -sS --fail` with the archive contact UA.
#[derive(Debug, Clone)]
pub struct CurlHttp {
    user_agent: String,
    max_time_secs: u64,
}

impl CurlHttp {
    /// Live transport with the contact UA and the default timeout.
    pub fn new() -> Self {
        Self {
            user_agent: IA_USER_AGENT.to_owned(),
            max_time_secs: CURL_MAX_TIME_SECS,
        }
    }
}

impl Default for CurlHttp {
    fn default() -> Self {
        Self::new()
    }
}

impl IaHttp for CurlHttp {
    fn get(&self, url: &str) -> Result<String, IaError> {
        let output = Command::new("curl")
            .arg("-sS")
            .arg("--fail")
            .arg("--max-time")
            .arg(self.max_time_secs.to_string())
            .arg("-A")
            .arg(&self.user_agent)
            .arg("--")
            .arg(url)
            .output()
            .map_err(|err| IaError::Transport(format!("curl for {url}: {err}")))?;
        if !output.status.success() {
            let detail = String::from_utf8_lossy(&output.stderr);
            return Err(IaError::Transport(format!(
                "curl for {url} exited {}: {}",
                output.status,
                detail.trim()
            )));
        }
        String::from_utf8(output.stdout)
            .map_err(|err| IaError::Parse(format!("ia body for {url} is not UTF-8: {err}")))
    }
}

/// The rung-1 probe. Owns its transport and its pacing sleeper; per-game
/// state never escapes one [`SourceProbe::probe`] call.
pub struct IaProbe {
    http: Box<dyn IaHttp>,
    sleep: Box<dyn Fn(Duration) + Send + Sync>,
}

impl IaProbe {
    /// Live probe: `curl` transport, real pacing sleeps.
    pub fn live() -> Self {
        Self {
            http: Box::new(CurlHttp::new()),
            sleep: Box::new(std::thread::sleep),
        }
    }

    /// Probe over an explicit transport (offline stubs in tests), real
    /// pacing sleeps.
    pub fn with_http(http: Box<dyn IaHttp>) -> Self {
        Self {
            http,
            sleep: Box::new(std::thread::sleep),
        }
    }

    /// Probe over an explicit transport and sleeper. Tests record the sleep
    /// durations to prove pacing without waiting out the interval.
    pub fn with_http_and_sleep(
        http: Box<dyn IaHttp>,
        sleep: Box<dyn Fn(Duration) + Send + Sync>,
    ) -> Self {
        Self { http, sleep }
    }

    /// Note-15 verbatim inventory query (identifier wildcards, movies only).
    pub fn inventory_query() -> &'static str {
        "identifier:(*nba*finals*game*) AND mediatype:(movies)"
    }

    /// Note-15 fielded-title fallback (broader, noisier; scorer judges).
    pub fn title_query() -> &'static str {
        "title:(NBA Finals) AND mediatype:(movies)"
    }

    fn paced_get(&self, url: &str, politeness: &PolitenessConfig) -> Result<String, IaError> {
        (self.sleep)(politeness.ia_request_min_interval);
        self.http.get(url)
    }
}

impl SourceProbe for IaProbe {
    fn rung(&self) -> u8 {
        1
    }

    fn name(&self) -> &'static str {
        // Matches Rung::InternetArchive.name() ("internet-archive").
        "internet-archive"
    }

    fn probe(
        &self,
        game: &GameContext,
        politeness: &PolitenessConfig,
        _quota: &mut YoutubeQuota,
    ) -> ProbeOutcome {
        // Rung 1 is quota-free: the budget passes through untouched.
        let inventory = Self::inventory_query().to_owned();
        let mut queries = vec![inventory.clone()];
        let docs = match self.search(&inventory, politeness) {
            Ok(docs) => docs,
            Err(_) => return ProbeOutcome::deferred(inventory),
        };
        let mut docs = docs;
        if docs.is_empty() {
            let fallback = Self::title_query().to_owned();
            queries.push(fallback.clone());
            docs = match self.search(&fallback, politeness) {
                Ok(docs) => docs,
                Err(_) => return ProbeOutcome::deferred(queries.join("\n")),
            };
        }
        let (year4, year2) = season_years(&game.date);
        let shortlist = prefer_year(docs, &year4, &year2);
        // An empty shortlist means both searches answered with zero docs:
        // a recorded miss, not a failure.
        let had_docs = !shortlist.is_empty();
        let mut candidates = Vec::new();
        let mut fetched: Vec<String> = Vec::new();
        for doc in shortlist.into_iter().take(MAX_METADATA_FETCHES) {
            let url = metadata_url(&doc.identifier);
            // One unreadable item is skipped, never fatal: a sibling item
            // may still hold the game's tape. Only when NO item could be
            // evaluated does the query defer (retry next sweep).
            let body = match self.paced_get(&url, politeness) {
                Ok(body) => body,
                Err(_) => continue,
            };
            let item = match parse_metadata(&body) {
                Ok(item) => item,
                Err(_) => continue,
            };
            fetched.push(doc.identifier.clone());
            if let Some((file, duration_secs)) = pick_video_file(&item.files) {
                candidates.push(ProbeCandidate {
                    url_or_pointer: download_url(&item.identifier, &file.name),
                    title: item.title,
                    description: describe(&item.description, &file.name),
                    duration_secs,
                });
            }
        }
        if fetched.is_empty() && had_docs {
            return ProbeOutcome::deferred(queries.join("\n"));
        }
        queries.push(format!(
            "metadata: {}",
            if fetched.is_empty() {
                "none".to_owned()
            } else {
                fetched.join(" ")
            }
        ));
        ProbeOutcome::found(queries.join("\n"), candidates)
    }
}

impl IaProbe {
    fn search(
        &self,
        query: &str,
        politeness: &PolitenessConfig,
    ) -> Result<Vec<SearchDoc>, IaError> {
        let body = self.paced_get(&search_url(query), politeness)?;
        parse_search(&body)
    }
}

struct SearchDoc {
    identifier: String,
    #[allow(dead_code)]
    title: String,
}

/// One `metadata` item: identity text plus its file list.
#[derive(Debug, Clone, PartialEq, Eq)]
struct MetadataItem {
    identifier: String,
    title: String,
    description: String,
    files: Vec<MetadataFile>,
}

/// One `metadata` file entry: the fields file picking needs.
#[derive(Debug, Clone, PartialEq, Eq)]
struct MetadataFile {
    name: String,
    source: String,
    length_secs: Option<u64>,
}

fn search_url(query: &str) -> String {
    format!(
        "https://archive.org/advancedsearch.php?q={}&fl%5B%5D=identifier&fl%5B%5D=title&fl%5B%5D=date&rows={}&output=json",
        encode_query(query),
        SEARCH_ROWS
    )
}

fn metadata_url(identifier: &str) -> String {
    format!(
        "https://archive.org/metadata/{}",
        encode_segment(identifier)
    )
}

fn download_url(identifier: &str, filename: &str) -> String {
    format!(
        "https://archive.org/download/{}/{}",
        encode_segment(identifier),
        encode_segment(filename)
    )
}

/// Human evidence for the candidate: the source description plus the exact
/// file the URL points at (a highlights-named file then scores honestly
/// as a clip, and reviewers see which file won a multi-file item).
fn describe(description: &str, filename: &str) -> String {
    if description.is_empty() {
        format!("File: {filename}")
    } else {
        format!("{description}\nFile: {filename}")
    }
}

/// `(YYYY, YY)` season years from an ISO date; empty when the date carries
/// no leading year.
fn season_years(date: &str) -> (String, String) {
    let year: String = date.chars().take(4).collect();
    if year.len() == 4 && year.chars().all(|c| c.is_ascii_digit()) {
        let short: String = year.chars().skip(2).collect();
        (year, short)
    } else {
        (String::new(), String::new())
    }
}

/// Year preference: when any identifier mentions the season year (4-digit,
/// else last-2-digit for slugs like `93-nbafinals-game-6`), only those are
/// fetched — undated noise never costs a metadata request. Otherwise the
/// source order stands. Either way the scorer judges what comes back.
fn prefer_year(docs: Vec<SearchDoc>, year4: &str, year2: &str) -> Vec<SearchDoc> {
    fn mentions(doc: &SearchDoc, year4: &str, year2: &str) -> bool {
        !year4.is_empty() && doc.identifier.contains(year4)
            || !year2.is_empty() && doc.identifier.contains(year2)
    }
    if docs.iter().any(|doc| mentions(doc, year4, year2)) {
        docs.into_iter()
            .filter(|doc| mentions(doc, year4, year2))
            .collect()
    } else {
        docs
    }
}

/// Longest original video file: skips derivatives (thumbnails, `_files.xml`,
/// sqlite sidecars) by source marker, playable tape by extension. First
/// wins ties, so single-file items are order-independent.
fn pick_video_file(files: &[MetadataFile]) -> Option<(&MetadataFile, Option<u64>)> {
    const VIDEO_EXTS: [&str; 9] = [
        ".mp4", ".mkv", ".avi", ".ogv", ".webm", ".mov", ".m4v", ".mpg", ".mpeg",
    ];
    let mut best: Option<(&MetadataFile, u64, bool)> = None;
    for file in files {
        if file.source.to_lowercase() == "derivative" {
            continue;
        }
        let lower = file.name.to_lowercase();
        if !VIDEO_EXTS.iter().any(|ext| lower.ends_with(ext)) {
            continue;
        }
        let length = file.length_secs.unwrap_or(0);
        let has_length = file.length_secs.is_some();
        let wins = match &best {
            None => true,
            Some((_, best_length, best_has)) => {
                (has_length && !best_has) || (has_length == *best_has && length > *best_length)
            }
        };
        if wins {
            best = Some((file, length, has_length));
        }
    }
    best.map(|(file, _, _)| (file, file.length_secs))
}

/// Form-encode a query value (`+` for spaces, as in note 15's pasted URL).
fn encode_query(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// Path-encode an identifier or filename (`%20` for spaces, `/` kept: IA
/// download URLs address `<identifier>/<filename>` verbatim).
fn encode_segment(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn parse_search(body: &str) -> Result<Vec<SearchDoc>, IaError> {
    let root = parse_json(body)?;
    let mut docs = Vec::new();
    if let Some(response) = root.get("response") {
        if let Some(list) = response.get("docs").and_then(Json::array) {
            for doc in list {
                if let Some(identifier) = doc.get("identifier").and_then(Json::as_str) {
                    docs.push(SearchDoc {
                        identifier: identifier.to_owned(),
                        title: doc.get("title").and_then(Json::text).unwrap_or_default(),
                    });
                }
            }
        }
    }
    Ok(docs)
}

fn parse_metadata(body: &str) -> Result<MetadataItem, IaError> {
    let root = parse_json(body)?;
    let metadata = root
        .get("metadata")
        .ok_or_else(|| IaError::Parse("metadata response has no metadata object".to_owned()))?;
    let identifier = metadata
        .get("identifier")
        .and_then(Json::text)
        .ok_or_else(|| IaError::Parse("metadata item has no identifier".to_owned()))?;
    let title = metadata
        .get("title")
        .and_then(Json::text)
        .unwrap_or_else(|| identifier.clone());
    let description = metadata
        .get("description")
        .and_then(Json::text)
        .unwrap_or_default();
    let mut files = Vec::new();
    if let Some(list) = root.get("files").and_then(Json::array) {
        for file in list {
            if let Some(name) = file.get("name").and_then(Json::as_str) {
                files.push(MetadataFile {
                    name: name.to_owned(),
                    source: file
                        .get("source")
                        .and_then(Json::as_str)
                        .unwrap_or("original")
                        .to_owned(),
                    length_secs: file.get("length").and_then(Json::seconds),
                });
            }
        }
    }
    Ok(MetadataItem {
        identifier,
        title,
        description,
        files,
    })
}

// ---------------------------------------------------------------------------
// Dependency-free JSON reader: the IA responses need objects, arrays,
// strings (with escapes), numbers, and literals — nothing else.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    fn get(&self, key: &str) -> Option<&Json> {
        if let Self::Obj(entries) = self {
            entries.iter().find(|(k, _)| k == key).map(|(_, v)| v)
        } else {
            None
        }
    }

    fn array(&self) -> Option<&[Json]> {
        if let Self::Arr(items) = self {
            Some(items)
        } else {
            None
        }
    }

    fn as_str(&self) -> Option<&str> {
        if let Self::Str(text) = self {
            Some(text)
        } else {
            None
        }
    }

    /// Source text that may arrive as one string or several (IA repeats
    /// multi-value fields as arrays).
    fn text(&self) -> Option<String> {
        match self {
            Self::Str(text) => Some(text.clone()),
            Self::Arr(items) => {
                let parts: Vec<&str> = items.iter().filter_map(Self::as_str).collect();
                if parts.is_empty() {
                    None
                } else {
                    Some(parts.join("\n"))
                }
            }
            _ => None,
        }
    }

    /// A duration that may arrive as seconds (`8494.65`) or text
    /// (`"8494.65"`); unparseable stays unknown, never zero-filled.
    fn seconds(&self) -> Option<u64> {
        match self {
            Self::Num(value) if value.is_finite() && *value >= 0.0 => Some(*value as u64),
            Self::Str(text) => text
                .trim()
                .parse::<f64>()
                .ok()
                .filter(|value| value.is_finite() && *value >= 0.0)
                .map(|value| value as u64),
            _ => None,
        }
    }
}

fn parse_json(body: &str) -> Result<Json, IaError> {
    let mut parser = JsonParser {
        bytes: body.as_bytes(),
        pos: 0,
    };
    let value = parser.value()?;
    parser.whitespace();
    if parser.pos != parser.bytes.len() {
        return Err(parser.error("trailing characters"));
    }
    Ok(value)
}

struct JsonParser<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl JsonParser<'_> {
    fn error(&self, what: &str) -> IaError {
        IaError::Parse(format!("{what} at byte {}", self.pos))
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn whitespace(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.pos += 1;
        }
    }

    fn value(&mut self) -> Result<Json, IaError> {
        self.whitespace();
        match self.peek() {
            Some(b'{') => self.object(),
            Some(b'[') => self.array(),
            Some(b'"') => Ok(Json::Str(self.string()?)),
            Some(b't') => self.literal("true", Json::Bool(true)),
            Some(b'f') => self.literal("false", Json::Bool(false)),
            Some(b'n') => self.literal("null", Json::Null),
            Some(byte) if byte == b'-' || byte.is_ascii_digit() => self.number(),
            _ => Err(self.error("expected a value")),
        }
    }

    fn literal(&mut self, word: &str, value: Json) -> Result<Json, IaError> {
        if self.bytes[self.pos..].starts_with(word.as_bytes()) {
            self.pos += word.len();
            Ok(value)
        } else {
            Err(self.error("bad literal"))
        }
    }

    fn number(&mut self) -> Result<Json, IaError> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        while let Some(byte) = self.peek() {
            if byte.is_ascii_digit() || matches!(byte, b'.' | b'e' | b'E' | b'+' | b'-') {
                self.pos += 1;
            } else {
                break;
            }
        }
        let text = std::str::from_utf8(&self.bytes[start..self.pos])
            .map_err(|_| self.error("bad number"))?;
        text.parse::<f64>()
            .map(Json::Num)
            .map_err(|_| self.error("bad number"))
    }

    fn object(&mut self) -> Result<Json, IaError> {
        self.pos += 1; // `{`
        let mut entries = Vec::new();
        self.whitespace();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Ok(Json::Obj(entries));
        }
        loop {
            self.whitespace();
            if self.peek() != Some(b'"') {
                return Err(self.error("expected a key"));
            }
            let key = self.string()?;
            self.whitespace();
            if self.peek() != Some(b':') {
                return Err(self.error("expected ':'"));
            }
            self.pos += 1;
            entries.push((key, self.value()?));
            self.whitespace();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(Json::Obj(entries));
                }
                _ => return Err(self.error("expected ',' or '}'")),
            }
        }
    }

    fn array(&mut self) -> Result<Json, IaError> {
        self.pos += 1; // `[`
        let mut items = Vec::new();
        self.whitespace();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Ok(Json::Arr(items));
        }
        loop {
            items.push(self.value()?);
            self.whitespace();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b']') => {
                    self.pos += 1;
                    return Ok(Json::Arr(items));
                }
                _ => return Err(self.error("expected ',' or ']'")),
            }
        }
    }

    fn string(&mut self) -> Result<String, IaError> {
        self.pos += 1; // `"`
        let mut out = String::new();
        loop {
            let byte = self
                .peek()
                .ok_or_else(|| self.error("unterminated string"))?;
            self.pos += 1;
            match byte {
                b'"' => return Ok(out),
                b'\\' => {
                    let escaped = self.peek().ok_or_else(|| self.error("bad escape"))?;
                    self.pos += 1;
                    match escaped {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{0008}'),
                        b'f' => out.push('\u{000C}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => out.push(self.unicode()?),
                        _ => return Err(self.error("bad escape")),
                    }
                }
                0x00..=0x7F => {
                    out.push(byte as char);
                }
                _ => {
                    // Multi-byte UTF-8: the body was validated as UTF-8 on
                    // entry, so the lead byte determines the sequence
                    // length. Byte-wise `as char` here would corrupt é
                    // (C3 A9) into Ã©.
                    let len = match byte {
                        0xC2..=0xDF => 2,
                        0xE0..=0xEF => 3,
                        0xF0..=0xF4 => 4,
                        _ => return Err(self.error("invalid UTF-8 lead byte")),
                    };
                    // pos already passed the lead byte; slice from it.
                    let start = self.pos - 1;
                    let end = start + len;
                    let slice = self
                        .bytes
                        .get(start..end)
                        .ok_or_else(|| self.error("truncated UTF-8 sequence"))?;
                    let text = std::str::from_utf8(slice)
                        .map_err(|_| self.error("invalid UTF-8 sequence"))?;
                    out.push_str(text);
                    self.pos = end;
                }
            }
        }
    }

    fn unicode(&mut self) -> Result<char, IaError> {
        let high = self.hex4()?;
        let code = if (0xD800..0xDC00).contains(&high) {
            if self.bytes.get(self.pos..self.pos + 2) == Some(b"\\u".as_slice()) {
                self.pos += 2;
                let low = self.hex4()?;
                if !(0xDC00..0xE000).contains(&low) {
                    return Err(self.error("bad low surrogate"));
                }
                0x10000 + ((high - 0xD800) << 10) + (low - 0xDC00)
            } else {
                return Err(self.error("lone high surrogate"));
            }
        } else {
            high
        };
        char::from_u32(code).ok_or_else(|| self.error("bad code point"))
    }

    fn hex4(&mut self) -> Result<u32, IaError> {
        if self.pos + 4 > self.bytes.len() {
            return Err(self.error("short \\u escape"));
        }
        let text = std::str::from_utf8(&self.bytes[self.pos..self.pos + 4])
            .map_err(|_| self.error("bad \\u escape"))?;
        let value = u32::from_str_radix(text, 16).map_err(|_| self.error("bad \\u escape"))?;
        self.pos += 4;
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_shapes_stay_note15_verbatim() {
        assert_eq!(
            IaProbe::inventory_query(),
            "identifier:(*nba*finals*game*) AND mediatype:(movies)"
        );
        assert_eq!(
            IaProbe::title_query(),
            "title:(NBA Finals) AND mediatype:(movies)"
        );
    }

    #[test]
    fn query_encoding_matches_the_pasted_url_form() {
        assert_eq!(
            search_url(IaProbe::inventory_query()),
            "https://archive.org/advancedsearch.php?q=identifier%3A%28%2Anba%2Afinals%2Agame%2A%29+AND+mediatype%3A%28movies%29&fl%5B%5D=identifier&fl%5B%5D=title&fl%5B%5D=date&rows=25&output=json"
        );
    }

    #[test]
    fn filenames_encode_spaces_but_keep_dots() {
        assert_eq!(
            download_url("some-id", "1996 NBA Finals Game 3.mp4"),
            "https://archive.org/download/some-id/1996%20NBA%20Finals%20Game%203.mp4"
        );
    }

    #[test]
    fn json_reader_handles_the_shapes_we_consume() {
        let value = parse_json(
            r#"{"response": {"numFound": 2, "docs": [{"identifier": "a\"b", "n": null, "ok": true}]}}"#,
        )
        .expect("parse");
        let docs = value
            .get("response")
            .and_then(|r| r.get("docs"))
            .and_then(Json::array)
            .expect("docs");
        assert_eq!(docs.len(), 1);
        assert_eq!(
            docs[0].get("identifier").and_then(Json::as_str),
            Some("a\"b")
        );

        let value = parse_json(r#"{"length": "8494.65", "n": 7621.5, "bad": "soon", "neg": -3.0}"#)
            .expect("parse");
        assert_eq!(value.get("length").and_then(Json::seconds), Some(8_494));
        assert_eq!(value.get("n").and_then(Json::seconds), Some(7_621));
        assert_eq!(value.get("bad").and_then(Json::seconds), None);
        assert_eq!(value.get("neg").and_then(Json::seconds), None);

        assert!(parse_json("{oops").is_err());
        assert!(parse_json(r#"{"a": tru}"#).is_err());
        assert!(parse_json(r#"{"a": [1,]}"#).is_err());
    }

    #[test]
    fn multi_value_text_fields_join() {
        let value =
            parse_json(r#"{"title": ["Line one", "Line two"], "empty": []}"#).expect("parse");
        assert_eq!(
            value.get("title").and_then(Json::text),
            Some("Line one\nLine two".to_owned())
        );
        assert_eq!(value.get("empty").and_then(Json::text), None);
    }

    #[test]
    fn json_strings_decode_raw_utf8_not_latin1_mojibake() {
        // Raw multi-byte UTF-8 in IA titles/descriptions (é = C3 A9) must
        // decode as one char, not two Latin-1 chars.
        let body = String::from_utf8(b"\"Caf\xC3\xA9 R\xC3\xA9gime \xE2\xAD\x90 1996\"".to_vec())
            .expect("valid utf-8 body");
        let value = parse_json(&body).expect("parse");
        assert_eq!(
            value.as_str(),
            Some("Café Régime ⭐ 1996"),
            "byte-wise `as char` would yield CafÃ© RÃ©gime â­ 1996"
        );
    }

    #[test]
    fn video_pick_skips_derivatives_and_prefers_measured_length() {
        let files = vec![
            MetadataFile {
                name: "thumb.jpg".to_owned(),
                source: "original".to_owned(),
                length_secs: None,
            },
            MetadataFile {
                name: "game.MP4".to_owned(),
                source: "original".to_owned(),
                length_secs: None,
            },
            MetadataFile {
                name: "game-long.mp4".to_owned(),
                source: "original".to_owned(),
                length_secs: Some(7_000),
            },
            MetadataFile {
                name: "game-long.mp4".to_owned(),
                source: "derivative".to_owned(),
                length_secs: Some(9_999),
            },
        ];
        let (picked, duration) = pick_video_file(&files).expect("a pick");
        assert_eq!(picked.name, "game-long.mp4");
        assert_eq!(picked.source, "original");
        assert_eq!(duration, Some(7_000));
    }

    #[test]
    fn year_preference_keeps_only_season_mentions() {
        fn doc(identifier: &str) -> SearchDoc {
            SearchDoc {
                identifier: identifier.to_owned(),
                title: String::new(),
            }
        }
        let kept = prefer_year(
            vec![
                doc("nba-finals-promo"),
                doc("1990-nba-finals-game-3"),
                doc("93-nbafinals-game-6"),
            ],
            "1990",
            "90",
        );
        assert_eq!(
            kept.iter()
                .map(|d| d.identifier.as_str())
                .collect::<Vec<_>>(),
            vec!["1990-nba-finals-game-3"]
        );
        // Last-2-digit slugs match their own season ("93" for 1993).
        let kept = prefer_year(
            vec![doc("nba-finals-promo"), doc("93-nbafinals-game-6")],
            "1993",
            "93",
        );
        assert_eq!(
            kept.iter()
                .map(|d| d.identifier.as_str())
                .collect::<Vec<_>>(),
            vec!["93-nbafinals-game-6"]
        );
        // No mention anywhere: source order stands, the scorer judges.
        let kept = prefer_year(vec![doc("b"), doc("a")], "1990", "90");
        assert_eq!(
            kept.iter()
                .map(|d| d.identifier.as_str())
                .collect::<Vec<_>>(),
            vec!["b", "a"]
        );
    }

    #[test]
    fn season_years_read_the_iso_prefix() {
        assert_eq!(
            season_years("1990-06-14"),
            ("1990".to_owned(), "90".to_owned())
        );
        assert_eq!(season_years("unknown"), (String::new(), String::new()));
    }
}
