//! Rung-2 yt-dlp sidecar probe: YouTube-class full-game search (issue #22).
//!
//! [`YtdlpProbe`] answers rung 2 (`youtube`, embed-class) by shelling out to
//! a user-provisioned `yt-dlp` binary — the same sidecar precedent as ffmpeg
//! (ADR-0001): the probe never links yt-dlp, it runs one bounded search
//! invocation per game and parses the JSON lines back into evidence.
//!
//! Per-game query (research 15 §3.2 templates): `<AWAY> vs <HOME> Full Game
//! <DATE>` issued as `ytsearch<N>:…` with `N = SEARCH_RESULT_COUNT`. The
//! query text is recorded verbatim on the sweep row as evidence.
//!
//! Invocation (flags verified against yt-dlp 2026.8.19, the version installed
//! here — `yt-dlp --help` plus its own sources under
//! `/opt/homebrew/Cellar/yt-dlp`):
//!
//! - `--flat-playlist --dump-json`: one JSON object per line per search hit,
//!   no per-video extraction requests.
//! - `--match-filters "duration > 4200"`: the full-game floor (70 min,
//!   research 15 §3.2). Two source-grounded details: the flag is plural in
//!   this yt-dlp (`--match-filter` no longer exists), and playlist entries
//!   are filtered with `incomplete=True`, so a *missing* duration passes the
//!   gate while a *known-short* one is dropped — highlights/partials filter
//!   out, unknown-length tapes survive for the scorer to judge by title.
//! - `--sleep-requests <s>` from [`PolitenessConfig::ytdlp_request_sleep`]:
//!   seconds between extraction requests. yt-dlp applies it between *every*
//!   request, which subsumes the config's every-N count (sleeping every
//!   request is at least as polite as every Nth) — there is no every-N flag
//!   upstream, so the count rides along unused rather than via a hand-rolled
//!   sleep. The probe never calls `thread::sleep` itself.
//! - `--socket-timeout 30`: bounds each request so one stalled game cannot
//!   hang the sequential sweep.
//!
//! Evidence-only: whatever the sidecar returns is recorded with honest
//! durations — the probe never drops candidates client-side; scoring stays
//! in [`crate::score_candidate`]. `url_or_pointer` carries the canonical
//! `https://www.youtube.com/watch?v=<id>` watch URL; the shell converts it
//! to the sanctioned `/embed/` player through the existing
//! `dispatch_external` path, so this probe stays shell-agnostic.
//!
//! Failures that yield no evidence spend one quota unit (the query was
//! issued) and defer: a missing/crashing sidecar or a nonzero exit records
//! nothing, so the rung retries next sweep instead of burning the 90-day
//! rescan window on a broken environment. An exit-0 run with no hits records
//! an empty query (Reject), which does consume the rung. Quota gates first:
//! `quota.schedule(1)` per query, zero grant → deferred without invoking the
//! sidecar at all.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::jev::{
    DirectHttpJev, JevConfidence, JevError, JevJudge, SearchTemplate, DEFAULT_THRESHOLD,
};
use crate::politeness::PolitenessConfig;
use crate::probe::{GameContext, ProbeCandidate, ProbeOutcome, ProbeRegistry, SourceProbe};
use nbatv_ladder::YoutubeQuota;
use std::sync::Arc;

/// Full-game floor in seconds (70 min): the `--match-filters` gate and the
/// duration research 15 §3.2 requires alongside exact date + both teams.
pub const FULL_GAME_FILTER_SECS: u64 = 4_200;
/// YouTube search results requested per game (`ytsearch<N>`).
pub const SEARCH_RESULT_COUNT: u32 = 10;
/// Per-request socket timeout handed to the sidecar.
pub const SOCKET_TIMEOUT_SECS: u64 = 30;

/// Rung-2 probe shelling out to a `yt-dlp` sidecar binary.
#[derive(Clone)]
pub struct YtdlpProbe {
    binary: PathBuf,
    judge: Option<Arc<dyn JevJudge>>,
}

impl std::fmt::Debug for YtdlpProbe {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("YtdlpProbe")
            .field("binary", &self.binary)
            .field("jev_enabled", &self.judge.is_some())
            .finish()
    }
}

impl YtdlpProbe {
    /// Probe using `yt-dlp` resolved from `PATH`.
    pub fn new() -> Self {
        Self {
            binary: PathBuf::from("yt-dlp"),
            judge: None,
        }
    }

    /// Probe with the optional Jev sidecar enabled from the environment.
    /// Missing or malformed configuration keeps the deterministic query.
    pub fn new_with_env_jev() -> Self {
        match DirectHttpJev::from_env() {
            Ok(judge) => Self::new().with_judge(Box::new(judge)),
            Err(JevError::MissingApiKey) => Self::new(),
            Err(error) => {
                eprintln!("nbatv-catalog: Jev search templates disabled: {error}");
                Self::new()
            }
        }
    }

    /// Probe using an explicit binary path. Tests point this at a fake
    /// shell script; production leaves [`YtdlpProbe::new`] on `PATH`.
    pub fn with_binary(path: impl Into<PathBuf>) -> Self {
        Self {
            binary: path.into(),
            judge: None,
        }
    }

    /// Attach the optional search-template sidecar. The default remains
    /// disabled, and every fallback keeps the current teams-and-date query.
    pub fn with_judge(mut self, judge: Box<dyn JevJudge>) -> Self {
        self.judge = Some(judge.into());
        self
    }
    /// The sidecar binary this probe invokes.
    pub fn binary(&self) -> &Path {
        &self.binary
    }

    /// The human search pattern for a game: teams plus ISO date.
    pub fn search_pattern(game: &GameContext) -> String {
        format!(
            "{} vs {} Full Game {}",
            game.away_team, game.home_team, game.date
        )
    }

    /// The exact outbound query argument: the pattern as a bounded
    /// `ytsearch<N>` query. This string is recorded as sweep evidence.
    pub fn search_query(game: &GameContext) -> String {
        format!(
            "ytsearch{}:{}",
            SEARCH_RESULT_COUNT,
            Self::search_pattern(game)
        )
    }

    /// Render only templates supported by the current deterministic Game
    /// context. Unsupported templates fall back instead of inventing a round
    /// label or Finals game number that the Archive does not contain.
    fn search_pattern_for(game: &GameContext, template: SearchTemplate) -> Option<String> {
        match template {
            SearchTemplate::TeamsAndDate => Some(Self::search_pattern(game)),
            SearchTemplate::TeamArchive => Some(format!(
                "{} vs {} Complete Game Archive {}",
                game.away_team, game.home_team, game.date
            )),
            SearchTemplate::FinalsGameNumber
            | SearchTemplate::EraRoundLabel
            | SearchTemplate::SourceSpecific => None,
        }
    }

    fn selected_search_query(&self, game: &GameContext) -> String {
        let selected = self
            .judge
            .as_ref()
            .and_then(|judge| {
                judge
                    .choose_search_template(game, self.name())
                    .ok()
                    .flatten()
            })
            .filter(|selection| JevConfidence(selection.confidence).is_decisive(DEFAULT_THRESHOLD))
            .and_then(|selection| Self::search_pattern_for(game, selection.template));
        selected.map_or_else(
            || Self::search_query(game),
            |pattern| format!("ytsearch{SEARCH_RESULT_COUNT}:{pattern}"),
        )
    }

    #[cfg(test)]
    fn argv(game: &GameContext, politeness: &PolitenessConfig) -> Vec<String> {
        Self::argv_for_query(politeness, &Self::search_query(game))
    }

    fn argv_for_query(politeness: &PolitenessConfig, query: &str) -> Vec<String> {
        vec![
            "--flat-playlist".to_owned(),
            "--dump-json".to_owned(),
            "--match-filters".to_owned(),
            format!("duration > {FULL_GAME_FILTER_SECS}"),
            "--sleep-requests".to_owned(),
            format!("{}", politeness.ytdlp_request_sleep.as_secs_f64()),
            "--socket-timeout".to_owned(),
            SOCKET_TIMEOUT_SECS.to_string(),
            query.to_owned(),
        ]
    }

    /// Parse `--dump-json` output: one JSON object per line. Blank lines are
    /// skipped; a line that is not a top-level object with a usable `id` is
    /// skipped, never fatal — yt-dlp diagnostics go to stderr, but a stray
    /// stdout line must not sink the whole query.
    fn parse_entries(stdout: &str) -> Vec<ProbeCandidate> {
        stdout.lines().filter_map(parse_entry_line).collect()
    }
}

impl Default for YtdlpProbe {
    fn default() -> Self {
        Self::new()
    }
}

impl SourceProbe for YtdlpProbe {
    fn rung(&self) -> u8 {
        2
    }

    fn name(&self) -> &'static str {
        "youtube"
    }

    fn probe(
        &self,
        game: &GameContext,
        politeness: &PolitenessConfig,
        quota: &mut YoutubeQuota,
    ) -> ProbeOutcome {
        let fallback_query = Self::search_query(game);
        if quota.schedule(1) == 0 {
            return ProbeOutcome::deferred(fallback_query);
        }
        let query = self.selected_search_query(game);
        let output = Command::new(&self.binary)
            .args(Self::argv_for_query(politeness, &query))
            .stdin(Stdio::null())
            .output();
        let output = match output {
            Ok(output) => output,
            Err(_) => return ProbeOutcome::deferred(query),
        };
        if !output.status.success() {
            return ProbeOutcome::deferred(query);
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        ProbeOutcome::found(query, Self::parse_entries(&stdout))
    }
}

/// A one-probe registry for the rung-2 sweep: the single registration point
/// for this ticket. Callers needing several rungs register onto their own
/// [`ProbeRegistry`] per rung instead.
pub fn registry_with_ytdlp<'a>(probe: &'a YtdlpProbe) -> ProbeRegistry<'a> {
    let mut registry = ProbeRegistry::new();
    registry.register(probe);
    registry
}

/// One candidate from one `--dump-json` line, or `None` to skip the line.
fn parse_entry_line(line: &str) -> Option<ProbeCandidate> {
    if line.trim().is_empty() {
        return None;
    }
    let fields = top_level_fields(line)?;
    let id = match fields.iter().find(|(k, _)| k == "id") {
        Some((_, FieldValue::Str(id))) if !id.is_empty() => id,
        _ => return None,
    };
    let title = fields
        .iter()
        .find(|(k, _)| k == "title")
        .and_then(|(_, v)| v.as_str())
        .unwrap_or_default();
    let description = fields
        .iter()
        .find(|(k, _)| k == "description")
        .and_then(|(_, v)| v.as_str())
        .unwrap_or_default();
    let duration_secs = fields
        .iter()
        .find(|(k, _)| k == "duration")
        .and_then(|(_, v)| v.as_duration());
    Some(ProbeCandidate {
        url_or_pointer: format!("https://www.youtube.com/watch?v={id}"),
        title,
        description,
        duration_secs,
    })
}

/// A top-level JSON value, simplified to what flat search entries carry.
#[derive(Debug, PartialEq)]
enum FieldValue {
    Str(String),
    Num(f64),
    Null,
    Other,
}

impl FieldValue {
    fn as_str(&self) -> Option<String> {
        match self {
            Self::Str(s) => Some(s.clone()),
            _ => None,
        }
    }

    fn as_duration(&self) -> Option<u64> {
        match self {
            Self::Num(f) if *f >= 0.0 => Some(*f as u64),
            _ => None,
        }
    }
}

/// Depth-1 key/value pairs of a flat JSON object. Returns `None` when the
/// line is not an object at all; nested objects/arrays become
/// [`FieldValue::Other`] so decoy keys inside them never shadow top-level
/// fields (flat entries carry e.g. thumbnail lists).
fn top_level_fields(line: &str) -> Option<Vec<(String, FieldValue)>> {
    let bytes = line.as_bytes();
    let mut cursor = Cursor { bytes, pos: 0 };
    cursor.skip_ws();
    if cursor.bump() != Some(b'{') {
        return None;
    }
    let mut fields = Vec::new();
    loop {
        cursor.skip_ws();
        match cursor.peek() {
            None => return None,
            Some(b'}') => {
                cursor.bump();
                return Some(fields);
            }
            _ => {}
        }
        let key = cursor.parse_string()?;
        cursor.skip_ws();
        if cursor.bump() != Some(b':') {
            return None;
        }
        cursor.skip_ws();
        fields.push((key, cursor.parse_value()?));
        cursor.skip_ws();
        match cursor.bump() {
            Some(b',') => {}
            Some(b'}') => return Some(fields),
            _ => return None,
        }
    }
}

struct Cursor<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let b = self.bytes.get(self.pos).copied()?;
        self.pos += 1;
        Some(b)
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.pos += 1;
        }
    }

    /// Parse a `"…"` string with full escape handling (`\"`, `\\`, `\uXXXX`
    /// with surrogate pairs). Operates byte-wise, which is safe: UTF-8
    /// continuation bytes never collide with ASCII `"` or `\`.
    fn parse_string(&mut self) -> Option<String> {
        if self.bump() != Some(b'"') {
            return None;
        }
        let mut out: Vec<u8> = Vec::new();
        loop {
            match self.bump()? {
                b'"' => return Some(String::from_utf8_lossy(&out).into_owned()),
                b'\\' => match self.bump()? {
                    b'"' => out.push(b'"'),
                    b'\\' => out.push(b'\\'),
                    b'/' => out.push(b'/'),
                    b'b' => out.push(0x08),
                    b'f' => out.push(0x0C),
                    b'n' => out.push(b'\n'),
                    b'r' => out.push(b'\r'),
                    b't' => out.push(b'\t'),
                    b'u' => {
                        let high = self.parse_hex4()?;
                        let code = if (0xD800..0xDC00).contains(&high) {
                            if self.bump() == Some(b'\\') && self.bump() == Some(b'u') {
                                let low = self.parse_hex4()?;
                                if (0xDC00..0xE000).contains(&low) {
                                    0x10000 + ((high - 0xD800) << 10) + (low - 0xDC00)
                                } else {
                                    return None;
                                }
                            } else {
                                return None;
                            }
                        } else {
                            high
                        };
                        let ch = char::from_u32(code).unwrap_or('\u{FFFD}');
                        let mut buf = [0u8; 4];
                        out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                    }
                    _ => return None,
                },
                b => out.push(b),
            }
        }
    }
    fn parse_hex4(&mut self) -> Option<u32> {
        let mut value = 0u32;
        for _ in 0..4 {
            let digit = self.bump()?;
            let hex = (digit as char).to_digit(16)?;
            value = value * 16 + hex;
        }
        Some(value)
    }

    fn parse_literal(&mut self, word: &[u8], value: FieldValue) -> Option<FieldValue> {
        for expected in word {
            if self.bump() != Some(*expected) {
                return None;
            }
        }
        Some(value)
    }

    fn parse_value(&mut self) -> Option<FieldValue> {
        match self.peek()? {
            b'"' => Some(FieldValue::Str(self.parse_string()?)),
            b'{' | b'[' => {
                self.skip_nested()?;
                Some(FieldValue::Other)
            }
            b't' => self.parse_literal(b"true", FieldValue::Other),
            b'f' => self.parse_literal(b"false", FieldValue::Other),
            b'n' => self.parse_literal(b"null", FieldValue::Null),
            b'-' | b'0'..=b'9' => {
                let start = self.pos;
                while matches!(
                    self.peek(),
                    Some(b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9')
                ) {
                    self.pos += 1;
                }
                let num: f64 = std::str::from_utf8(&self.bytes[start..self.pos])
                    .ok()?
                    .parse()
                    .ok()?;
                Some(FieldValue::Num(num))
            }
            _ => None,
        }
    }

    /// Skip one nested object/array wholesale, string-aware so braces inside
    /// strings do not unbalance the depth count.
    fn skip_nested(&mut self) -> Option<()> {
        let mut depth = 0usize;
        loop {
            match self.bump()? {
                b'"' => {
                    self.pos -= 1;
                    self.parse_string()?;
                }
                b'{' | b'[' => depth += 1,
                b'}' | b']' => {
                    depth = depth.checked_sub(1)?;
                    if depth == 0 {
                        return Some(());
                    }
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game() -> GameContext {
        GameContext {
            game_id: "194611010TRH".to_owned(),
            home_team: "TRH".to_owned(),
            away_team: "NYK".to_owned(),
            date: "1946-11-01".to_owned(),
        }
    }

    #[test]
    fn argv_lists_flags_before_the_query() {
        let argv = YtdlpProbe::argv(&game(), &PolitenessConfig::default());
        assert_eq!(
            argv.last().unwrap(),
            "ytsearch10:NYK vs TRH Full Game 1946-11-01"
        );
        assert!(argv.contains(&"--flat-playlist".to_owned()));
        assert!(argv.contains(&"--dump-json".to_owned()));
    }

    #[test]
    fn escaped_and_unicode_titles_survive() {
        let line = r#"{"id":"ABCDEFGHIJK","title":"\"Full\" Game \u0026 Remaster \ud83c\udfc0","duration":7200}"#;
        let candidate = parse_entry_line(line).expect("parses");
        assert_eq!(candidate.title, "\"Full\" Game & Remaster 🏀");
        assert_eq!(candidate.duration_secs, Some(7200));
    }

    #[test]
    fn nested_decoy_keys_do_not_shadow_top_level_fields() {
        let line = r#"{"id":"ABCDEFGHIJK","title":"Real Title","thumbnails":[{"id":"0","title":"decoy"}],"duration":7200}"#;
        let candidate = parse_entry_line(line).expect("parses");
        assert_eq!(candidate.title, "Real Title");
    }

    #[test]
    fn odd_durations_and_missing_fields_degrade_gracefully() {
        let negative = r#"{"id":"ABCDEFGHIJK","title":"t","duration":-5}"#;
        assert_eq!(parse_entry_line(negative).unwrap().duration_secs, None);
        let missing = r#"{"id":"ABCDEFGHIJK","title":"t"}"#;
        let candidate = parse_entry_line(missing).expect("parses");
        assert_eq!(candidate.duration_secs, None);
        assert_eq!(candidate.description, "");
        assert!(parse_entry_line(r#"{"title":"no id"}"#).is_none());
        assert!(parse_entry_line("").is_none());
        assert!(parse_entry_line("[1,2,3]").is_none());
    }
}
