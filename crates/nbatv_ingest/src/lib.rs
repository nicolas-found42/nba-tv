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

pub mod crawl;

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
// BR HTML parsers (dependency-free string scanning)
// ---------------------------------------------------------------------------
//
// Three small parsers over Basketball-Reference static HTML: the league-year
// `_games.html` schedule index, per-game `/boxscores/*.html` pages, and
// league-year `_totals.html` season totals. Scanning is plain substring work
// on ASCII delimiters (`<`, `>`, quotes, `data-stat` attributes — BR's
// `orb`/`drb`/`trb` stat keys are mapped to the db's `oreb`/`dreb`/`reb`
// names at decode time). Human text between delimiters is only touched
// through `str::get` ranges anchored at `find` results or processed
// char-by-char, so multibyte names (Ginóbili, Jokić, …) can never trigger a
// recomputed-length slicing panic. Missing/blank stat cells decode to
// `None`, never 0; stats the era did not record decode to `None` even when
// a cell carries a value (see [`era_recorded`]).

/// One row of a BR `_games.html` schedule table.
#[derive(Debug, Clone, PartialEq)]
pub struct GameIndexRow {
    pub game_id: String,
    pub date: String,
    pub home_br: String,
    pub away_br: String,
}

/// Result of [`parse_games_page`]: the valid rows plus how many game rows
/// were dropped for a missing or invalid box-score slug. Every slug is
/// checked with [`validate_game_id`]; future/unplayed games have no slug
/// yet and land in the same skip count.
#[derive(Debug, Clone, PartialEq)]
pub struct GamesPage {
    pub rows: Vec<GameIndexRow>,
    pub skipped_bad_slugs: usize,
}

/// Team box-score row parsed from a BR box page. Field-for-field onto
/// `nbatv_db::BoxTeamRow` (this crate stays dependency-free, so the shape is
/// mirrored, not imported); the caller attaches `game_id` from the snapshot
/// file name at insert time.
#[derive(Debug, Clone, PartialEq)]
pub struct BoxTeamInput {
    pub team_br: String,
    pub mp: Option<String>,
    pub fg: Option<i32>,
    pub fga: Option<i32>,
    pub fg3: Option<i32>,
    pub fg3a: Option<i32>,
    pub ft: Option<i32>,
    pub fta: Option<i32>,
    pub oreb: Option<i32>,
    pub dreb: Option<i32>,
    pub reb: Option<i32>,
    pub ast: Option<i32>,
    pub stl: Option<i32>,
    pub blk: Option<i32>,
    pub tov: Option<i32>,
    pub pf: Option<i32>,
    pub pts: Option<i32>,
    pub plus_minus: Option<f64>,
}

/// Player box-score row parsed from a BR box page. Field-for-field onto
/// `nbatv_db::BoxPlayerRow` (mirrored; caller attaches `game_id`). DNP rows
/// carry `dnp_reason` with every stat `None`. `position` is always `None`:
/// the basic box table carries no position column.
#[derive(Debug, Clone, PartialEq)]
pub struct BoxPlayerInput {
    pub team_br: String,
    pub player_br: String,
    pub starter: Option<bool>,
    pub position: Option<String>,
    pub mp: Option<String>,
    pub fg: Option<i32>,
    pub fga: Option<i32>,
    pub fg3: Option<i32>,
    pub fg3a: Option<i32>,
    pub ft: Option<i32>,
    pub fta: Option<i32>,
    pub oreb: Option<i32>,
    pub dreb: Option<i32>,
    pub reb: Option<i32>,
    pub ast: Option<i32>,
    pub stl: Option<i32>,
    pub blk: Option<i32>,
    pub tov: Option<i32>,
    pub pf: Option<i32>,
    pub pts: Option<i32>,
    pub plus_minus: Option<f64>,
    pub dnp_reason: Option<String>,
}

/// One (player, season, team) totals row parsed from a BR `_totals.html`
/// page. Field-for-field onto `nbatv_db::SeasonTotalRow` (mirrored).
/// Multi-team seasons keep every stint row AND the combined `TOT`/`2TM`/
/// `3TM` row as separate entries — splits are never merged.
#[derive(Debug, Clone, PartialEq)]
pub struct SeasonTotalInput {
    pub player_br: String,
    pub season: i32,
    pub team_br: String,
    pub g: i32,
    pub mp: Option<i32>,
    pub fg: Option<i32>,
    pub fga: Option<i32>,
    pub fg3: Option<i32>,
    pub fg3a: Option<i32>,
    pub ft: Option<i32>,
    pub fta: Option<i32>,
    pub oreb: Option<i32>,
    pub dreb: Option<i32>,
    pub reb: Option<i32>,
    pub ast: Option<i32>,
    pub stl: Option<i32>,
    pub blk: Option<i32>,
    pub tov: Option<i32>,
    pub pf: Option<i32>,
    pub pts: Option<i32>,
}

/// End index (one past `>`) of the tag opening at byte `lt`, ignoring `>`
/// inside single/double quotes. `None` when the tag never closes.
fn tag_end(html: &str, lt: usize) -> Option<usize> {
    let bytes = html.as_bytes();
    let mut i = lt;
    let mut quote: Option<u8> = None;
    while let Some(&b) = bytes.get(i) {
        if let Some(q) = quote {
            if b == q {
                quote = None;
            }
        } else if b == b'"' || b == b'\'' {
            quote = Some(b);
        } else if b == b'>' {
            return Some(i + 1);
        }
        i += 1;
    }
    None
}

fn is_space_byte(b: Option<&u8>) -> bool {
    matches!(b, Some(b' ') | Some(b'\t') | Some(b'\n') | Some(b'\r'))
}

/// Value of attribute `name` in a tag fragment such as
/// `<td data-stat="mp" csk="1946-11-01">`. Matches whole attribute names
/// only (so `id` never matches `hidden`), tolerates spaces around `=`, and
/// accepts single or double quotes.
fn attr_value(tag: &str, name: &str) -> Option<String> {
    let bytes = tag.as_bytes();
    let mut start = 0usize;
    while start < tag.len() {
        let rel = tag.get(start..)?.find(name)?;
        let i = start + rel;
        if i > 0 {
            let prev = bytes[i - 1];
            if prev.is_ascii_alphanumeric() || matches!(prev, b'-' | b'_' | b':' | b'.') {
                start = i + name.len();
                continue;
            }
        }
        let mut j = i + name.len();
        while is_space_byte(bytes.get(j)) {
            j += 1;
        }
        if bytes.get(j) != Some(&b'=') {
            start = i + name.len();
            continue;
        }
        j += 1;
        while is_space_byte(bytes.get(j)) {
            j += 1;
        }
        let q = *bytes.get(j)?;
        if q != b'"' && q != b'\'' {
            start = i + name.len();
            continue;
        }
        let vstart = j + 1;
        let rel_end = tag.get(vstart..)?.find(q as char)?;
        return tag.get(vstart..vstart + rel_end).map(str::to_owned);
    }
    None
}

/// Whether `html[lt..]` opens a tag named `name` (`<td …>`, `<tr>`, …).
/// The byte after the name must be a tag boundary so `<th` never matches
/// `<thead` and `<tr` never matches `<track>`.
fn is_tag_open(html: &str, lt: usize, name: &[u8]) -> bool {
    let bytes = html.as_bytes();
    if bytes.get(lt) != Some(&b'<') {
        return false;
    }
    for (k, &b) in name.iter().enumerate() {
        if bytes.get(lt + 1 + k) != Some(&b) {
            return false;
        }
    }
    matches!(
        bytes.get(lt + 1 + name.len()),
        Some(b' ') | Some(b'>') | Some(b'/') | Some(b'\t') | Some(b'\n') | Some(b'\r')
    )
}

/// First `href` value inside `inner` (the `<a href="…">` of a linked cell).
fn first_href(inner: &str) -> Option<String> {
    let mut pos = 0usize;
    while pos < inner.len() {
        let rel = inner.get(pos..)?.find("<a")?;
        let lt = pos + rel;
        if !is_tag_open(inner, lt, b"a") {
            pos = lt + 1;
            continue;
        }
        let end = tag_end(inner, lt)?;
        let tag = inner.get(lt..end)?;
        if let Some(h) = attr_value(tag, "href") {
            return Some(h);
        }
        pos = end;
    }
    None
}

/// The few entities BR emits. `&amp;` decodes last so `&amp;lt;` becomes
/// `&lt;` (single decode), not `<`.
fn decode_entities(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&#x27;", "'")
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
}

/// Visible text of a cell: strip `<…>` tags char-by-char (multibyte-safe —
/// never slices human text by byte length), decode entities, trim.
fn inner_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut chars = html.chars();
    while let Some(c) = chars.next() {
        if c == '<' {
            for c2 in chars.by_ref() {
                if c2 == '>' {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    decode_entities(out.trim())
}

/// One `<td>`/`<th>` cell: its `data-stat` key, visible text, first link,
/// and `csk` sort key when present (dates carry ISO `csk`).
struct Cell {
    stat: String,
    text: String,
    href: Option<String>,
    csk: Option<String>,
}

/// All cells of one `<tr>…</tr>` slice, in document order. Unterminated
/// cells end the scan; junk between cells is ignored.
fn row_cells(row: &str) -> Vec<Cell> {
    let mut cells = Vec::new();
    let mut pos = 0usize;
    while pos < row.len() {
        let rest = match row.get(pos..) {
            Some(r) => r,
            None => break,
        };
        let td = rest.find("<td").map(|r| (pos + r, "td"));
        let th = rest.find("<th").map(|r| (pos + r, "th"));
        let (lt, kind) = match (td, th) {
            (Some(a), Some(b)) => {
                if a.0 <= b.0 {
                    a
                } else {
                    b
                }
            }
            (Some(a), None) => a,
            (None, Some(b)) => b,
            (None, None) => break,
        };
        if !is_tag_open(row, lt, kind.as_bytes()) {
            pos = lt + 1;
            continue;
        }
        let open_end = match tag_end(row, lt) {
            Some(e) => e,
            None => break,
        };
        let tag = match row.get(lt..open_end) {
            Some(t) => t,
            None => break,
        };
        let close_needle = if kind == "td" { "</td" } else { "</th" };
        let search = match row.get(open_end..) {
            Some(s) => s,
            None => break,
        };
        let close_rel = match search.find(close_needle) {
            Some(r) => r,
            None => break,
        };
        let inner = match row.get(open_end..open_end + close_rel) {
            Some(s) => s,
            None => break,
        };
        let close_end = match row.get(open_end + close_rel..).and_then(|s| s.find('>')) {
            Some(r) => open_end + close_rel + r + 1,
            None => break,
        };
        cells.push(Cell {
            stat: attr_value(tag, "data-stat").unwrap_or_default(),
            text: inner_text(inner),
            href: first_href(inner),
            csk: attr_value(tag, "csk"),
        });
        pos = close_end;
    }
    cells
}

/// `(id, inner HTML)` of every `<table…>…</table>` in document order.
fn tables(html: &str) -> Vec<(String, &str)> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    while pos < html.len() {
        let rest = match html.get(pos..) {
            Some(r) => r,
            None => break,
        };
        let rel = match rest.find("<table") {
            Some(r) => r,
            None => break,
        };
        let lt = pos + rel;
        if !is_tag_open(html, lt, b"table") {
            pos = lt + 1;
            continue;
        }
        let open_end = match tag_end(html, lt) {
            Some(e) => e,
            None => break,
        };
        let tag = match html.get(lt..open_end) {
            Some(t) => t,
            None => break,
        };
        let id = attr_value(tag, "id").unwrap_or_default();
        let search = match html.get(open_end..) {
            Some(s) => s,
            None => break,
        };
        let close_rel = match search.find("</table") {
            Some(r) => r,
            None => break,
        };
        let inner = match html.get(open_end..open_end + close_rel) {
            Some(s) => s,
            None => break,
        };
        let close_end = match html.get(open_end + close_rel..).and_then(|s| s.find('>')) {
            Some(r) => open_end + close_rel + r + 1,
            None => break,
        };
        out.push((id, inner));
        pos = close_end;
    }
    out
}

/// Inner HTML of every `<tr…>…</tr>` of a table, in order. Rows without a
/// closing tag are ignored.
fn table_rows(table: &str) -> Vec<&str> {
    let mut rows = Vec::new();
    let mut pos = 0usize;
    while pos < table.len() {
        let rest = match table.get(pos..) {
            Some(r) => r,
            None => break,
        };
        let rel = match rest.find("<tr") {
            Some(r) => r,
            None => break,
        };
        let lt = pos + rel;
        if !is_tag_open(table, lt, b"tr") {
            pos = lt + 1;
            continue;
        }
        let open_end = match tag_end(table, lt) {
            Some(e) => e,
            None => break,
        };
        let search = match table.get(open_end..) {
            Some(s) => s,
            None => break,
        };
        let close_rel = match search.find("</tr") {
            Some(r) => r,
            None => break,
        };
        let row = match table.get(open_end..open_end + close_rel) {
            Some(s) => s,
            None => break,
        };
        let close_end = match table.get(open_end + close_rel..).and_then(|s| s.find('>')) {
            Some(r) => open_end + close_rel + r + 1,
            None => break,
        };
        rows.push(row);
        pos = close_end;
    }
    rows
}

/// `194611010TRH` from `/boxscores/194611010TRH.html` (query strings and
/// fragments tolerated); `None` when the shape is absent.
fn slug_from_box_href(href: &str) -> Option<String> {
    let path = href.split(['?', '#']).next().unwrap_or(href);
    let after = path.rsplit('/').next().unwrap_or(path);
    let slug = after.strip_suffix(".html").unwrap_or(after);
    if slug.is_empty() {
        None
    } else {
        Some(slug.to_owned())
    }
}

/// `YYYY-MM-DD` shape check for schedule `csk` values (live pages carry the
/// game slug there instead — see `parse_games_page`).
fn is_ymd(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b[..4].iter().all(|c| c.is_ascii_digit())
        && b[5..7].iter().all(|c| c.is_ascii_digit())
        && b[8..10].iter().all(|c| c.is_ascii_digit())
}

/// `NYK` from `/teams/NYK/1947.html`.
fn slug_from_team_href(href: &str) -> Option<String> {
    let path = href.split(['?', '#']).next().unwrap_or(href);
    let mut parts = path.split('/').filter(|p| !p.is_empty());
    if parts.next()? != "teams" {
        return None;
    }
    let slug = parts.next()?;
    if slug.is_empty() {
        None
    } else {
        Some(slug.to_owned())
    }
}

/// `curryst01` from `/players/c/curryst01.html`.
fn slug_from_player_href(href: &str) -> Option<String> {
    let path = href.split(['?', '#']).next().unwrap_or(href);
    let after = path.rsplit('/').next().unwrap_or(path);
    let slug = after.strip_suffix(".html").unwrap_or(after);
    if slug.is_empty() || slug == "players" {
        None
    } else {
        Some(slug.to_owned())
    }
}

/// Nullable integer stat: blank or unparseable → `None`, never 0.
/// Thousands separators (`1,234`) are tolerated.
fn parse_opt_int(text: &str) -> Option<i32> {
    let t = text.trim();
    if t.is_empty() {
        return None;
    }
    if t.contains(',') {
        t.split(',').collect::<String>().parse::<i32>().ok()
    } else {
        t.parse::<i32>().ok()
    }
}

/// Nullable `+/-` (`+7`, `-3`): blank or unparseable → `None`.
fn parse_opt_plus_minus(text: &str) -> Option<f64> {
    let t = text.trim();
    if t.is_empty() {
        return None;
    }
    t.parse::<f64>().ok()
}

fn opt_text(t: &str) -> Option<String> {
    let s = t.trim();
    if s.is_empty() {
        None
    } else {
        Some(s.to_owned())
    }
}

/// `true` for minute totals (`"240"`, `"44:32"`): digits with at most one
/// colon. Any other non-blank MP text (Did Not Play, Did Not Dress,
/// Not With Team, …) marks a DNP row.
fn is_minutes_shape(t: &str) -> bool {
    if t.is_empty() {
        return false;
    }
    let mut colon = false;
    let mut digits = 0u32;
    for c in t.chars() {
        if c == ':' {
            if colon {
                return false;
            }
            colon = true;
        } else if c.is_ascii_digit() {
            digits += 1;
        } else {
            return false;
        }
    }
    digits > 0
}

/// Era rule: stats the era did not record decode to `None` even when a cell
/// carries a value. Cutoffs are season-ending years, verified against BR
/// league-year pages: rebounds enter boxes in 1950-51; steals/blocks and
/// the offensive/defensive rebound split in 1973-74; turnovers in 1977-78;
/// threes in 1979-80. Per-game assists are blank before 1950-51, while
/// season totals carry assists since 1946-47 — hence `is_box`. Stat names
/// are canonical (`oreb`/`dreb`/`reb`; the HTML keys `orb`/`drb`/`trb` are
/// mapped by the caller). `None` season (undated page) disables clamping;
/// missing columns are `None` regardless.
fn era_recorded(stat: &str, season: Option<i32>, is_box: bool) -> bool {
    let Some(y) = season else {
        return true;
    };
    match stat {
        "fg3" | "fg3a" => y >= 1980,
        "stl" | "blk" | "oreb" | "dreb" => y >= 1974,
        "tov" => y >= 1978,
        "reb" => y >= 1951,
        "ast" => !is_box || y >= 1951,
        _ => true,
    }
}

const MONTHS: [(&str, i32); 12] = [
    ("january", 1),
    ("february", 2),
    ("march", 3),
    ("april", 4),
    ("may", 5),
    ("june", 6),
    ("july", 7),
    ("august", 8),
    ("september", 9),
    ("october", 10),
    ("november", 11),
    ("december", 12),
];

/// Season-ending year from a BR box page's `scorebox_meta` date
/// (`"November 1, 1946"` → `Some(1947)`): October–December games belong to
/// the season ending the next year. `None` when the page carries no date.
fn page_season(html: &str) -> Option<i32> {
    let meta_rel = html.find("scorebox_meta")?;
    let div_end = html
        .get(meta_rel..)
        .and_then(|s| s.find("</div>"))
        .map(|r| meta_rel + r)
        .unwrap_or(html.len());
    let region = html.get(meta_rel..div_end)?;
    let text = inner_text(region).to_lowercase();
    let b = text.as_bytes();
    let mut year: Option<i32> = None;
    let mut i = 0usize;
    while i + 4 <= b.len() {
        let digit_run = b[i].is_ascii_digit()
            && b[i + 1].is_ascii_digit()
            && b[i + 2].is_ascii_digit()
            && b[i + 3].is_ascii_digit();
        let bounded_before = i == 0 || !b[i - 1].is_ascii_digit();
        let bounded_after = b.get(i + 4).is_none_or(|c| !c.is_ascii_digit());
        if digit_run && bounded_before && bounded_after {
            if let Some(y) = text.get(i..i + 4).and_then(|s| s.parse::<i32>().ok()) {
                if (1800..=2100).contains(&y) {
                    year = Some(y);
                    break;
                }
            }
            i += 4;
        } else {
            i += 1;
        }
    }
    let y = year?;
    let month = MONTHS
        .iter()
        .find(|(name, _)| text.contains(*name))
        .map(|(_, n)| *n);
    match month {
        Some(m) if m >= 10 => Some(y + 1),
        _ => Some(y),
    }
}

/// Schedule index from a BR `_games.html` page. Scans every `games` table
/// (the page repeats `id="games"` once per month) when present, otherwise
/// the whole document. Header rows (no team links) are ignored silently;
/// game rows without a valid slug are skipped and counted. Dates prefer
/// the machine `csk` (`1946-11-01`) over display text.
pub fn parse_games_page(html: &str) -> GamesPage {
    let all = tables(html);
    let bodies: Vec<&str> = {
        let monthly: Vec<&str> = all
            .iter()
            .filter(|(id, _)| id == "games")
            .map(|(_, inner)| *inner)
            .collect();
        if monthly.is_empty() {
            vec![html]
        } else {
            monthly
        }
    };
    let rows_in: Vec<&str> = bodies.iter().flat_map(|b| table_rows(b)).collect();
    parse_game_rows(&rows_in)
}

/// Row scan behind [`parse_games_page`]: header rows (no team links)
/// ignored silently, slugless rows skipped and counted.
fn parse_game_rows(rows_in: &[&str]) -> GamesPage {
    let mut rows = Vec::new();
    let mut skipped_bad_slugs = 0usize;
    for row in rows_in {
        let cells = row_cells(row);
        let date = cells.iter().find(|c| c.stat == "date_game");
        let visitor = cells.iter().find(|c| c.stat == "visitor_team_name");
        let home = cells.iter().find(|c| c.stat == "home_team_name");
        let (Some(visitor), Some(home)) = (visitor, home) else {
            continue;
        };
        let away = visitor.href.as_deref().and_then(slug_from_team_href);
        let home_br = home.href.as_deref().and_then(slug_from_team_href);
        let (Some(away_br), Some(home_br)) = (away, home_br) else {
            continue;
        };
        let slug = cells
            .iter()
            .find(|c| c.stat == "box_score_text")
            .and_then(|c| c.href.as_deref())
            .and_then(slug_from_box_href);
        let Some(slug) = slug else {
            skipped_bad_slugs += 1;
            continue;
        };
        if !validate_game_id(&slug) {
            skipped_bad_slugs += 1;
            continue;
        }
        // Live-data correction: on real schedule pages `date_game`'s `csk`
        // is the game slug (e.g. `194611010TRH`), not a machine date — only
        // a `YYYY-MM-DD`-shaped `csk` wins over the display text.
        let date_text = date
            .and_then(|c| c.csk.clone().filter(|s| is_ymd(s)))
            .or_else(|| date.map(|c| c.text.clone()))
            .unwrap_or_default();
        rows.push(GameIndexRow {
            game_id: slug,
            date: date_text,
            home_br,
            away_br,
        });
    }
    GamesPage {
        rows,
        skipped_bad_slugs,
    }
}

/// `Some("NYK")` for `box-NYK-game-basic`, else `None`.
fn box_table_team(id: &str) -> Option<String> {
    let mid = id.strip_prefix("box-")?.strip_suffix("-game-basic")?;
    if mid.is_empty() || mid.contains('/') || mid.contains('.') {
        None
    } else {
        Some(mid.to_owned())
    }
}

fn cell_text<'a>(cells: &'a [Cell], stat: &str) -> &'a str {
    cells
        .iter()
        .find(|c| c.stat == stat)
        .map(|c| c.text.as_str())
        .unwrap_or("")
}

/// Nullable int stat with the era clamp applied. `raw` is the BR HTML key
/// (`orb`/`drb`/`trb`), `canon` the era-rule name (`oreb`/`dreb`/`reb`).
fn gated_int(
    cells: &[Cell],
    raw: &str,
    canon: &str,
    season: Option<i32>,
    is_box: bool,
) -> Option<i32> {
    if !era_recorded(canon, season, is_box) {
        return None;
    }
    parse_opt_int(cell_text(cells, raw))
}

/// Team totals from one box table's `Team Totals` row. The core six
/// (`fg`/`fga`/`ft`/`fta`/`pf`/`pts`) are required — a totals row without
/// them is malformed and skipped; everything else is nullable.
fn parse_box_team_row(team_br: &str, cells: &[Cell], season: Option<i32>) -> Option<BoxTeamInput> {
    let req = |stat: &str| parse_opt_int(cell_text(cells, stat));
    // Live-data correction (1946-47 crawl): the earliest seasons blank even
    // core cells (November 1946 team totals carry no `fga`, variously no
    // `fta`/`pf`), so every column is optional and blank means
    // era-did-not-record, never zero. Only `pts` is required — a totals row
    // without points records no result and is skipped.
    let pts = req("pts")?;
    Some(BoxTeamInput {
        team_br: team_br.to_owned(),
        // Team MP (`240`) is a bookkeeping total, recorded in every era.
        mp: opt_text(cell_text(cells, "mp")),
        fg: req("fg"),
        fga: req("fga"),
        fg3: gated_int(cells, "fg3", "fg3", season, true),
        fg3a: gated_int(cells, "fg3a", "fg3a", season, true),
        ft: req("ft"),
        fta: req("fta"),
        oreb: gated_int(cells, "orb", "oreb", season, true),
        dreb: gated_int(cells, "drb", "dreb", season, true),
        reb: gated_int(cells, "trb", "reb", season, true),
        ast: gated_int(cells, "ast", "ast", season, true),
        stl: gated_int(cells, "stl", "stl", season, true),
        blk: gated_int(cells, "blk", "blk", season, true),
        tov: gated_int(cells, "tov", "tov", season, true),
        pf: req("pf"),
        pts: Some(pts),
        plus_minus: parse_opt_plus_minus(cell_text(cells, "plus_minus")),
    })
}

/// One player row. Non-blank, non-minutes MP text (Did Not Play, Did Not
/// Dress, Not With Team, …) yields a DNP row: every stat `None` with
/// `dnp_reason` set. Blank MP is just unrecorded, not DNP.
fn parse_box_player_row(
    team_br: &str,
    player_br: &str,
    starter: Option<bool>,
    cells: &[Cell],
    season: Option<i32>,
) -> BoxPlayerInput {
    let mp_raw = cell_text(cells, "mp");
    if !mp_raw.trim().is_empty() && !is_minutes_shape(mp_raw.trim()) {
        return BoxPlayerInput {
            team_br: team_br.to_owned(),
            player_br: player_br.to_owned(),
            starter,
            position: None,
            mp: None,
            fg: None,
            fga: None,
            fg3: None,
            fg3a: None,
            ft: None,
            fta: None,
            oreb: None,
            dreb: None,
            reb: None,
            ast: None,
            stl: None,
            blk: None,
            tov: None,
            pf: None,
            pts: None,
            plus_minus: None,
            dnp_reason: Some(mp_raw.trim().to_owned()),
        };
    }
    BoxPlayerInput {
        team_br: team_br.to_owned(),
        player_br: player_br.to_owned(),
        starter,
        position: None,
        mp: opt_text(mp_raw),
        fg: gated_int(cells, "fg", "fg", season, true),
        fga: gated_int(cells, "fga", "fga", season, true),
        fg3: gated_int(cells, "fg3", "fg3", season, true),
        fg3a: gated_int(cells, "fg3a", "fg3a", season, true),
        ft: gated_int(cells, "ft", "ft", season, true),
        fta: gated_int(cells, "fta", "fta", season, true),
        oreb: gated_int(cells, "orb", "oreb", season, true),
        dreb: gated_int(cells, "drb", "dreb", season, true),
        reb: gated_int(cells, "trb", "reb", season, true),
        ast: gated_int(cells, "ast", "ast", season, true),
        stl: gated_int(cells, "stl", "stl", season, true),
        blk: gated_int(cells, "blk", "blk", season, true),
        tov: gated_int(cells, "tov", "tov", season, true),
        pf: gated_int(cells, "pf", "pf", season, true),
        pts: gated_int(cells, "pts", "pts", season, true),
        plus_minus: parse_opt_plus_minus(cell_text(cells, "plus_minus")),
        dnp_reason: None,
    }
}

/// `(team rows, player rows)` from one BR box-score page. Team tables are
/// located by `id="box-{TEAM}-game-basic"`; each table's `Team Totals` row
/// becomes the team input and every other linked row a player input.
/// `Starters`/`Reserves` section headers set `starter`; tables without
/// section headers leave it `None`. The page's `scorebox_meta` date drives
/// the era clamp; undated pages skip clamping (missing cells are still
/// `None`).
pub fn parse_box_page(html: &str) -> (Vec<BoxTeamInput>, Vec<BoxPlayerInput>) {
    let season = page_season(html);
    let mut teams = Vec::new();
    let mut players = Vec::new();
    for (id, body) in tables(html) {
        let team_br = match box_table_team(&id) {
            Some(t) => t,
            None => continue,
        };
        let mut section: Option<bool> = None;
        for row in table_rows(body) {
            let cells = row_cells(row);
            let player_cell = match cells.iter().find(|c| c.stat == "player") {
                Some(c) => c,
                None => continue,
            };
            let label = player_cell.text.trim();
            if label.eq_ignore_ascii_case("starters") {
                section = Some(true);
                continue;
            }
            if label.eq_ignore_ascii_case("reserves") {
                section = Some(false);
                continue;
            }
            if label.eq_ignore_ascii_case("team totals") {
                if let Some(t) = parse_box_team_row(&team_br, &cells, season) {
                    teams.push(t);
                }
                continue;
            }
            let player_br = match player_cell.href.as_deref().and_then(slug_from_player_href) {
                Some(s) => s,
                None => continue,
            };
            players.push(parse_box_player_row(
                &team_br, &player_br, section, &cells, season,
            ));
        }
    }
    (teams, players)
}

/// Season totals from a BR `_totals.html` page for `season` (ending year,
/// e.g. `2015` for 2014-15 — the batch key, which is authoritative where
/// the page header's display label is ambiguous). Uses the `totals_stats`
/// (else `totals`) table, falling back to the whole document. Every linked
/// player row becomes its own input, so traded-player stints and their
/// combined `TOT`/`2TM`/`3TM` row are kept as separate rows. Rows without a
/// player link or without games played are skipped.
pub fn parse_totals_page(html: &str, season: i32) -> Vec<SeasonTotalInput> {
    let all = tables(html);
    let body: &str = all
        .iter()
        .find(|(id, _)| id == "totals_stats" || id == "totals")
        .map(|(_, inner)| *inner)
        .unwrap_or(html);
    let mut out = Vec::new();
    for row in table_rows(body) {
        let cells = row_cells(row);
        // Live pages renamed the column keys (`name_display` for the player,
        // `team_name_abbr` for the team, `games` for games played); accept
        // both the documented and the live shapes.
        let player_cell = match cells
            .iter()
            .find(|c| c.stat == "player" || c.stat == "name_display")
        {
            Some(c) => c,
            None => continue,
        };
        let player_br = match player_cell.href.as_deref().and_then(slug_from_player_href) {
            Some(s) => s,
            None => continue,
        };
        let team_br = match cells
            .iter()
            .find(|c| c.stat == "team" || c.stat == "team_id" || c.stat == "team_name_abbr")
            .map(|c| c.text.trim().to_owned())
        {
            Some(t) if !t.is_empty() => t,
            _ => continue,
        };
        let g = match parse_opt_int(cell_text(&cells, "g"))
            .or_else(|| parse_opt_int(cell_text(&cells, "games")))
        {
            Some(g) => g,
            None => continue,
        };
        out.push(SeasonTotalInput {
            player_br,
            season,
            team_br,
            g,
            mp: gated_int(&cells, "mp", "mp", Some(season), false),
            fg: gated_int(&cells, "fg", "fg", Some(season), false),
            fga: gated_int(&cells, "fga", "fga", Some(season), false),
            fg3: gated_int(&cells, "fg3", "fg3", Some(season), false),
            fg3a: gated_int(&cells, "fg3a", "fg3a", Some(season), false),
            ft: gated_int(&cells, "ft", "ft", Some(season), false),
            fta: gated_int(&cells, "fta", "fta", Some(season), false),
            oreb: gated_int(&cells, "orb", "oreb", Some(season), false),
            dreb: gated_int(&cells, "drb", "dreb", Some(season), false),
            reb: gated_int(&cells, "trb", "reb", Some(season), false),
            ast: gated_int(&cells, "ast", "ast", Some(season), false),
            stl: gated_int(&cells, "stl", "stl", Some(season), false),
            blk: gated_int(&cells, "blk", "blk", Some(season), false),
            tov: gated_int(&cells, "tov", "tov", Some(season), false),
            pf: gated_int(&cells, "pf", "pf", Some(season), false),
            pts: gated_int(&cells, "pts", "pts", Some(season), false),
        });
    }
    out
}

// ---------------------------------------------------------------------------
// Fetch pipeline: one polite, resumable season batch at a time
// ---------------------------------------------------------------------------
//
// `fetch_season` pulls every page of a single season through a [`FetchClient`]
// (real HTTP in production, canned HTML in tests — no network here), spaces
// requests by [`FETCH_MIN_INTERVAL`], and stores each body as gzip at
// `out_dir.join(raw_snapshot_path(source, season, file_name))`. Files already
// on disk are skipped without touching the network, so a killed run resumes
// where it stopped; per-page `meta-revised` stamps come back in the report
// for the caller's freshness store (see [`recrawl_hint`]).

use std::time::{Duration, Instant};

/// Minimum spacing between BR requests. robots.txt sets `Crawl-delay: 3`;
/// the extra half second is margin. One season batch runs at a time, so
/// this single constant paces the whole archive build (~75k box pages at
/// this rate ≈ 3 days, resumable per season).
pub const FETCH_MIN_INTERVAL: Duration = Duration::from_millis(3_500);

/// How long the pipeline must still wait given `elapsed` since the previous
/// request. Pure (never sleeps) so etiquette stays unit-testable.
pub fn etiquette_delay(elapsed: Duration) -> Duration {
    FETCH_MIN_INTERVAL.saturating_sub(elapsed)
}

/// Fetch-pipeline failure: bad path components, snapshot I/O, the client's
/// own fetch error, or undecodable snapshot bytes.
#[derive(Debug)]
pub enum FetchError {
    UnsafePath(String),
    Io(std::io::Error),
    Client(String),
    /// The page does not exist at the source (HTTP 404/410 from the live
    /// client). Distinct from [`FetchError::Client`] so a crawl driver can
    /// dead-pool a permanently missing page instead of retrying it.
    NotFound(String),
    Decode(String),
}

impl fmt::Display for FetchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FetchError::UnsafePath(s) => write!(f, "unsafe snapshot path component: {s}"),
            FetchError::Io(e) => write!(f, "snapshot I/O: {e}"),
            FetchError::Client(s) => write!(f, "fetch failed: {s}"),
            FetchError::NotFound(s) => write!(f, "page not found: {s}"),
            FetchError::Decode(s) => write!(f, "bad snapshot bytes: {s}"),
        }
    }
}

impl std::error::Error for FetchError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            FetchError::Io(e) => Some(e),
            _ => None,
        }
    }
}

/// Page source for [`fetch_season`]. Production use is a thin HTTP client;
/// tests substitute canned HTML. Returns the raw page body; the pipeline
/// derives freshness stamps and snapshot bytes itself.
///
/// The body MUST be transfer-decoded (identity) text: the client must not
/// hand back compressed bytes. In practice that means sending no
/// `Accept-Encoding: gzip` (BR then serves identity), or inflating the
/// response before returning — the `String` return type already forces
/// this, since compressed bytes are not valid UTF-8. Snapshot storage
/// re-compresses with [`gzip_encode`] itself, so the pipeline never meets
/// server-side dynamic-Huffman gzip: [`gzip_decode`] only ever reads back
/// what [`write_snapshot_gz`] wrote (plus `gzip -0`-shaped files).
pub trait FetchClient {
    fn fetch(&self, url: &str) -> Result<String, FetchError>;
}

/// One page of a season batch: destination file name under
/// `data/raw/{source}/{season}/`, its URL, the last `meta-revised` stamp
/// seen for it (freshness bookkeeping), and `force` to re-fetch even when
/// the snapshot is already on disk.
#[derive(Debug, Clone, PartialEq)]
pub struct FetchJob {
    pub file_name: String,
    pub url: String,
    pub last_seen_revision: Option<String>,
    pub force: bool,
}

impl FetchJob {
    pub fn new(file_name: &str, url: &str) -> Self {
        FetchJob {
            file_name: file_name.to_owned(),
            url: url.to_owned(),
            last_seen_revision: None,
            force: false,
        }
    }

    pub fn with_revision(mut self, rev: &str) -> Self {
        self.last_seen_revision = Some(rev.to_owned());
        self
    }

    pub fn force(mut self) -> Self {
        self.force = true;
        self
    }
}

/// Outcome of [`fetch_season`]: fetched vs resume-skipped file names plus
/// the `meta-revised` stamps observed (feed them back as
/// `last_seen_revision`, via [`recrawl_hint`], on the next pass).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FetchReport {
    pub fetched: Vec<String>,
    pub skipped: Vec<String>,
    pub revisions: Vec<PageRevision>,
}

/// Reject path components that could escape the caller-supplied directory
/// (`..`, separators, empty). `raw_snapshot_path` joins literals, so
/// validated components keep every write under `out_dir`.
fn check_component(what: &str, s: &str) -> Result<(), FetchError> {
    if s.is_empty()
        || s == "."
        || s == ".."
        || s.contains('/')
        || s.contains('\\')
        || s.contains('\0')
    {
        return Err(FetchError::UnsafePath(format!("{what}={s:?}")));
    }
    Ok(())
}

/// BR per-page freshness stamp (e.g. `16:31:52 03-Sep-2026`) from
/// `<meta name="revised" content="…">` (`lastmod`/`last-modified` accepted
/// too). Opaque string; `None` when the page carries no stamp — then
/// [`recrawl_hint`] never fires for it.
pub fn parse_meta_revised(html: &str) -> Option<String> {
    let mut pos = 0usize;
    while pos < html.len() {
        let rest = html.get(pos..)?;
        let rel = rest.find("<meta")?;
        let lt = pos + rel;
        if !is_tag_open(html, lt, b"meta") {
            pos = lt + 1;
            continue;
        }
        let end = tag_end(html, lt)?;
        let tag = html.get(lt..end)?;
        pos = end;
        let name = attr_value(tag, "name").unwrap_or_default().to_lowercase();
        match name.as_str() {
            "revised" | "lastmod" | "last-modified" | "modified" => {
                if let Some(c) = attr_value(tag, "content") {
                    let c = c.trim();
                    if !c.is_empty() {
                        return Some(c.to_owned());
                    }
                }
            }
            _ => {}
        }
    }
    None
}

/// CRC-32 (ISO 3309) table, built at compile time for the snapshot codec.
const CRC32_TABLE: [u32; 256] = {
    let mut table = [0u32; 256];
    let mut i = 0usize;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 == 1 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
            k += 1;
        }
        table[i] = c;
        i += 1;
    }
    table
};

fn crc32(data: &[u8]) -> u32 {
    let mut c = !0u32;
    for &b in data {
        c = CRC32_TABLE[((c ^ u32::from(b)) & 0xFF) as usize] ^ (c >> 8);
    }
    !c
}

/// Valid gzip bytes (magic `1f 8b` included) for `raw`, using stored
/// (uncompressed) deflate blocks — one per 64 KiB. Readable by any gzip
/// tool; needs no compression dependency.
pub fn gzip_encode(raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(raw.len() + 32);
    out.extend_from_slice(&[0x1F, 0x8B, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x03]);
    if raw.is_empty() {
        out.extend_from_slice(&[0x01, 0x00, 0x00, 0xFF, 0xFF]);
    } else {
        let chunks = raw.chunks(65_535);
        let n = chunks.len();
        for (idx, chunk) in chunks.enumerate() {
            out.push(if idx + 1 == n { 0x01 } else { 0x00 });
            let len = chunk.len() as u16;
            out.extend_from_slice(&len.to_le_bytes());
            out.extend_from_slice(&(!len).to_le_bytes());
            out.extend_from_slice(chunk);
        }
    }
    out.extend_from_slice(&crc32(raw).to_le_bytes());
    out.extend_from_slice(&(raw.len() as u32).to_le_bytes());
    out
}

/// Inverse of [`gzip_encode`]: stored-block gzip streams only (what this
/// pipeline writes, plus `gzip -0`-style files). Anything else — bad magic,
/// flags, Huffman blocks, truncated or CRC-mismatched bytes — is `Err`,
/// never a panic.
pub fn gzip_decode(gz: &[u8]) -> Result<Vec<u8>, FetchError> {
    let bad = |msg: &str| FetchError::Decode(msg.to_owned());
    let header = gz
        .get(..10)
        .ok_or_else(|| bad("too short for gzip header"))?;
    if header[0] != 0x1F || header[1] != 0x8B {
        return Err(bad("bad gzip magic"));
    }
    if header[2] != 0x08 {
        return Err(bad("unsupported compression method"));
    }
    if header[3] != 0x00 {
        return Err(bad("unsupported gzip flags"));
    }
    let mut pos = 10usize;
    let mut out = Vec::new();
    loop {
        let b = *gz.get(pos).ok_or_else(|| bad("truncated deflate block"))?;
        pos += 1;
        if b & 0xF8 != 0 {
            return Err(bad("reserved deflate bits set"));
        }
        if b >> 1 & 0x03 != 0 {
            return Err(bad("only stored deflate blocks are supported"));
        }
        let last = b & 0x01 == 1;
        let at = |o: usize| {
            gz.get(pos + o)
                .copied()
                .ok_or_else(|| bad("truncated block length"))
        };
        let len = u16::from_le_bytes([at(0)?, at(1)?]) as usize;
        let nlen = u16::from_le_bytes([at(2)?, at(3)?]);
        if len as u16 != !nlen {
            return Err(bad("block length mismatch"));
        }
        pos += 4;
        let end = pos
            .checked_add(len)
            .ok_or_else(|| bad("block length overflow"))?;
        let data = gz
            .get(pos..end)
            .ok_or_else(|| bad("truncated block data"))?;
        out.extend_from_slice(data);
        pos = end;
        if last {
            break;
        }
    }
    let trailer = |o: usize| -> Result<u32, FetchError> {
        Ok(u32::from_le_bytes([
            *gz.get(pos + o)
                .ok_or_else(|| bad("truncated gzip trailer"))?,
            *gz.get(pos + o + 1)
                .ok_or_else(|| bad("truncated gzip trailer"))?,
            *gz.get(pos + o + 2)
                .ok_or_else(|| bad("truncated gzip trailer"))?,
            *gz.get(pos + o + 3)
                .ok_or_else(|| bad("truncated gzip trailer"))?,
        ]))
    };
    let want_crc = trailer(0)?;
    let want_len = trailer(4)?;
    if out.len() as u32 != want_len {
        return Err(bad("length mismatch"));
    }
    if crc32(&out) != want_crc {
        return Err(bad("crc32 mismatch"));
    }
    Ok(out)
}

/// Gzip `html` into `path`, creating parent dirs. `path` must already be
/// resolved under the caller dir (see [`fetch_season`]); this helper only
/// writes where told.
pub fn write_snapshot_gz(path: &Path, html: &str) -> Result<(), FetchError> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(FetchError::Io)?;
        }
    }
    std::fs::write(path, gzip_encode(html.as_bytes())).map_err(FetchError::Io)?;
    Ok(())
}

/// Inverse of [`write_snapshot_gz`].
pub fn read_snapshot_gz(path: &Path) -> Result<String, FetchError> {
    let bytes = std::fs::read(path).map_err(FetchError::Io)?;
    let raw = gzip_decode(&bytes)?;
    String::from_utf8(raw).map_err(|e| FetchError::Decode(format!("snapshot is not UTF-8: {e}")))
}

/// Fetch one season batch: every job in order, `>= FETCH_MIN_INTERVAL`
/// apart, each body stored as gzip at
/// `out_dir.join(raw_snapshot_path(source, season, file_name))`. Files
/// already on disk are skipped without touching the network (resume); pass
/// [`FetchJob::force`] to re-fetch one. Nothing is ever written outside
/// `out_dir`: path components are validated before joining. Returns which
/// files were fetched versus resume-skipped, plus the observed
/// `meta-revised` stamps (feed each stamp to [`recrawl_hint`]).
pub fn fetch_season<C: FetchClient>(
    client: &C,
    source: &str,
    season: &str,
    jobs: &[FetchJob],
    out_dir: &Path,
) -> Result<FetchReport, FetchError> {
    fetch_season_with_sleeper(client, source, season, jobs, out_dir, &mut |d| {
        std::thread::sleep(d);
    })
}

/// [`fetch_season`] with injectable sleep, so tests can observe etiquette
/// waits without waiting them. Paces at [`FETCH_MIN_INTERVAL`]; see
/// [`fetch_season_with_sleeper_at`] for a caller-chosen interval.
pub fn fetch_season_with_sleeper<C, S>(
    client: &C,
    source: &str,
    season: &str,
    jobs: &[FetchJob],
    out_dir: &Path,
    sleep: &mut S,
) -> Result<FetchReport, FetchError>
where
    C: FetchClient,
    S: FnMut(Duration),
{
    fetch_season_with_sleeper_at(
        client,
        source,
        season,
        jobs,
        out_dir,
        FETCH_MIN_INTERVAL,
        sleep,
    )
}

/// [`fetch_season_with_sleeper`] with a caller-chosen minimum spacing
/// between requests. `interval` below [`FETCH_MIN_INTERVAL`] exceeds BR's
/// robots.txt Crawl-delay 3 — the caller owns that decision (the
/// `nbatv-crawl` bin exposes it as `--interval`).
pub fn fetch_season_with_sleeper_at<C, S>(
    client: &C,
    source: &str,
    season: &str,
    jobs: &[FetchJob],
    out_dir: &Path,
    interval: Duration,
    sleep: &mut S,
) -> Result<FetchReport, FetchError>
where
    C: FetchClient,
    S: FnMut(Duration),
{
    check_component("source", source)?;
    check_component("season", season)?;
    let mut report = FetchReport::default();
    let mut last_fetch: Option<Instant> = None;
    for job in jobs {
        check_component("file_name", &job.file_name)?;
        let target = out_dir.join(raw_snapshot_path(source, season, &job.file_name));
        if !job.force && target.is_file() {
            report.skipped.push(job.file_name.clone());
            continue;
        }
        if let Some(t0) = last_fetch {
            let wait = interval.saturating_sub(t0.elapsed());
            if !wait.is_zero() {
                sleep(wait);
            }
        }
        let html = client.fetch(&job.url)?;
        last_fetch = Some(Instant::now());
        let revision = PageRevision {
            page: job.file_name.clone(),
            meta_revised: parse_meta_revised(&html),
        };
        write_snapshot_gz(&target, &html)?;
        report.fetched.push(job.file_name.clone());
        report.revisions.push(revision);
    }
    Ok(report)
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

#[cfg(test)]
mod br_parse_tests {
    use super::*;

    /// Tiny `_games.html` shape: header row (no links, ignored), two valid
    /// games (one `csk` date, one display-text fallback + multibyte arena
    /// the parser skips over), one bad-slug row, one slugless future game.
    const GAMES_HTML: &str = "\
<table class=\"stats_table\" id=\"games\">\
<thead><tr>\
<th data-stat=\"date_game\">Date</th>\
<th data-stat=\"visitor_team_name\">Visitor/Neutral</th>\
<th data-stat=\"visitor_pts\">PTS</th>\
<th data-stat=\"home_team_name\">Home/Neutral</th>\
<th data-stat=\"home_pts\">PTS</th>\
<th data-stat=\"box_score_text\">Box Score</th>\
</tr></thead><tbody>\
<tr>\
<th data-stat=\"date_game\" csk=\"1946-11-01\"><a href=\"/boxscores/\">Fri, Nov 1, 1946</a></th>\
<td data-stat=\"visitor_team_name\"><a href=\"/teams/NYK/1947.html\">New York Knicks</a></td>\
<td data-stat=\"visitor_pts\">68</td>\
<td data-stat=\"home_team_name\"><a href=\"/teams/TRH/1947.html\">Toronto Huskies</a></td>\
<td data-stat=\"home_pts\">66</td>\
<td data-stat=\"box_score_text\"><a href=\"/boxscores/194611010TRH.html\">Box Score</a></td>\
<td data-stat=\"arena\">Maple Leaf Gardens</td>\
</tr>\
<tr>\
<th data-stat=\"date_game\">Sat, Nov 2, 1946</th>\
<td data-stat=\"visitor_team_name\"><a href=\"/teams/PIT/1947.html\">Pittsburgh Ironmen</a></td>\
<td data-stat=\"visitor_pts\">55</td>\
<td data-stat=\"home_team_name\"><a href=\"/teams/CHS/1947.html\">Chicago Stags</a></td>\
<td data-stat=\"home_pts\">63</td>\
<td data-stat=\"box_score_text\"><a href=\"/boxscores/194611020CHS.html\">Box Score</a></td>\
<td data-stat=\"arena\">Salle M\u{e4}ller</td>\
</tr>\
<tr>\
<th data-stat=\"date_game\" csk=\"1946-11-03\">Sun, Nov 3, 1946</th>\
<td data-stat=\"visitor_team_name\"><a href=\"/teams/NYK/1947.html\">New York Knicks</a></td>\
<td data-stat=\"visitor_pts\">80</td>\
<td data-stat=\"home_team_name\"><a href=\"/teams/CHS/1947.html\">Chicago Stags</a></td>\
<td data-stat=\"home_pts\">79</td>\
<td data-stat=\"box_score_text\"><a href=\"/boxscores/not-a-game.html\">Box Score</a></td>\
</tr>\
<tr>\
<th data-stat=\"date_game\" csk=\"1946-11-04\">Mon, Nov 4, 1946</th>\
<td data-stat=\"visitor_team_name\"><a href=\"/teams/TRH/1947.html\">Toronto Huskies</a></td>\
<td data-stat=\"visitor_pts\"></td>\
<td data-stat=\"home_team_name\"><a href=\"/teams/PIT/1947.html\">Pittsburgh Ironmen</a></td>\
<td data-stat=\"home_pts\"></td>\
<td data-stat=\"box_score_text\"></td>\
</tr>\
</tbody></table>";

    #[test]
    fn games_page_happy_path_reads_slugs_and_dates() {
        let page = parse_games_page(GAMES_HTML);
        assert_eq!(page.rows.len(), 2);
        assert_eq!(
            page.rows[0],
            GameIndexRow {
                game_id: "194611010TRH".to_owned(),
                date: "1946-11-01".to_owned(),
                home_br: "TRH".to_owned(),
                away_br: "NYK".to_owned(),
            }
        );
        // No `csk` on the second row: display text is the fallback, and the
        // multibyte arena cell must not disturb the scan.
        assert_eq!(page.rows[1].game_id, "194611020CHS");
        assert_eq!(page.rows[1].date, "Sat, Nov 2, 1946");
        assert_eq!(page.rows[1].home_br, "CHS");
        assert_eq!(page.rows[1].away_br, "PIT");
        for row in &page.rows {
            assert!(validate_game_id(&row.game_id));
        }
    }

    #[test]
    fn games_page_skips_bad_slugs_and_counts_them() {
        let page = parse_games_page(GAMES_HTML);
        // `not-a-game.html` plus the slugless future game.
        assert_eq!(page.skipped_bad_slugs, 2);
        assert!(page.rows.iter().all(|r| r.game_id != "not-a-game"));
    }

    #[test]
    fn games_page_slug_csk_falls_back_to_display_date() {
        // Live schedule pages carry the game slug in `date_game`'s `csk`,
        // not a machine date — only a `YYYY-MM-DD` csk wins; otherwise the
        // display text is the date (observed 1946-47 crawl).
        let html = "\
<table class=\"stats_table\" id=\"schedule\"><tbody>\
<tr>\
<th data-stat=\"date_game\" csk=\"194611010TRH\">Fri, Nov 1, 1946</th>\
<td data-stat=\"visitor_team_name\"><a href=\"/teams/NYK/1947.html\">New York Knicks</a></td>\
<td data-stat=\"home_team_name\"><a href=\"/teams/TRH/1947.html\">Toronto Huskies</a></td>\
<td data-stat=\"box_score_text\"><a href=\"/boxscores/194611010TRH.html\">Box Score</a></td>\
</tr>\
</tbody></table>";
        let page = parse_games_page(html);
        assert_eq!(page.rows.len(), 1);
        assert_eq!(page.rows[0].game_id, "194611010TRH");
        assert_eq!(page.rows[0].date, "Fri, Nov 1, 1946");
    }
    #[test]
    fn games_page_scans_every_monthly_table() {
        // Live `_games.html` pages repeat `id="games"` once per month.
        // Taking only the first table truncates the season to October
        // (observed on the 1964-65 crawl: 31 rows of ~600).
        let html = "\
<table class=\"stats_table\" id=\"games\"><tbody>\
<tr>\
<th data-stat=\"date_game\" csk=\"1964-10-16\">Fri, Oct 16, 1964</th>\
<td data-stat=\"visitor_team_name\"><a href=\"/teams/DET/1965.html\">Detroit Pistons</a></td>\
<td data-stat=\"home_team_name\"><a href=\"/teams/PHI/1965.html\">Philadelphia 76ers</a></td>\
<td data-stat=\"box_score_text\"><a href=\"/boxscores/196410160PHI.html\">Box Score</a></td>\
</tr>\
</tbody></table>\
<table class=\"stats_table\" id=\"games\"><tbody>\
<tr>\
<th data-stat=\"date_game\" csk=\"1965-04-25\">Sun, Apr 25, 1965</th>\
<td data-stat=\"visitor_team_name\"><a href=\"/teams/BOS/1965.html\">Boston Celtics</a></td>\
<td data-stat=\"home_team_name\"><a href=\"/teams/LAL/1965.html\">Los Angeles Lakers</a></td>\
<td data-stat=\"box_score_text\"><a href=\"/boxscores/196504250LAL.html\">Box Score</a></td>\
</tr>\
</tbody></table>";
        let page = parse_games_page(html);
        let ids: Vec<&str> = page.rows.iter().map(|r| r.game_id.as_str()).collect();
        assert_eq!(ids, vec!["196410160PHI", "196504250LAL"]);
    }

    /// Modern box shape (2015 Finals): full stat columns, Starters/Reserves
    /// sections, a DNP row, blank team `+/-`, and a multibyte name.
    const BOX_MODERN_HTML: &str = "\
<div class=\"scorebox_meta\"><div><strong>June 16, 2015</strong>, Oracle Arena</div></div>\
<table class=\"stats_table\" id=\"box-GSW-game-basic\">\
<thead><tr><th data-stat=\"player\">Golden State Warriors</th>\
<th data-stat=\"mp\">MP</th><th data-stat=\"fg\">FG</th><th data-stat=\"fga\">FGA</th>\
<th data-stat=\"fg3\">3P</th><th data-stat=\"fg3a\">3PA</th>\
<th data-stat=\"ft\">FT</th><th data-stat=\"fta\">FTA</th>\
<th data-stat=\"orb\">ORB</th><th data-stat=\"drb\">DRB</th><th data-stat=\"trb\">TRB</th>\
<th data-stat=\"ast\">AST</th><th data-stat=\"stl\">STL</th><th data-stat=\"blk\">BLK</th>\
<th data-stat=\"tov\">TOV</th><th data-stat=\"pf\">PF</th><th data-stat=\"pts\">PTS</th>\
<th data-stat=\"plus_minus\">+/-</th></tr></thead><tbody>\
<tr><th data-stat=\"player\">Starters</th></tr>\
<tr>\
<th data-stat=\"player\"><a href=\"/players/c/curryst01.html\">Stephen Curry</a></th>\
<td data-stat=\"mp\">44:00</td><td data-stat=\"fg\">10</td><td data-stat=\"fga\">23</td>\
<td data-stat=\"fg3\">5</td><td data-stat=\"fg3a\">12</td>\
<td data-stat=\"ft\">8</td><td data-stat=\"fta\">9</td>\
<td data-stat=\"orb\">1</td><td data-stat=\"drb\">4</td><td data-stat=\"trb\">5</td>\
<td data-stat=\"ast\">6</td><td data-stat=\"stl\">2</td><td data-stat=\"blk\">0</td>\
<td data-stat=\"tov\">3</td><td data-stat=\"pf\">4</td><td data-stat=\"pts\">33</td>\
<td data-stat=\"plus_minus\">+7</td>\
</tr>\
<tr><th data-stat=\"player\">Reserves</th></tr>\
<tr>\
<th data-stat=\"player\"><a href=\"/players/g/ginobma01.html\">Manu Gin\u{f3}bili</a></th>\
<td data-stat=\"mp\">18:24</td><td data-stat=\"fg\">3</td><td data-stat=\"fga\">7</td>\
<td data-stat=\"fg3\">1</td><td data-stat=\"fg3a\">3</td>\
<td data-stat=\"ft\">2</td><td data-stat=\"fta\">2</td>\
<td data-stat=\"orb\">0</td><td data-stat=\"drb\">2</td><td data-stat=\"trb\">2</td>\
<td data-stat=\"ast\">4</td><td data-stat=\"stl\">1</td><td data-stat=\"blk\">0</td>\
<td data-stat=\"tov\">1</td><td data-stat=\"pf\">2</td><td data-stat=\"pts\">9</td>\
<td data-stat=\"plus_minus\">-2</td>\
</tr>\
<tr>\
<th data-stat=\"player\"><a href=\"/players/r/rushbr01.html\">Brandon Rush</a></th>\
<td data-stat=\"mp\">Did Not Play</td>\
<td data-stat=\"fg\"></td><td data-stat=\"fga\"></td><td data-stat=\"fg3\"></td>\
<td data-stat=\"fg3a\"></td><td data-stat=\"ft\"></td><td data-stat=\"fta\"></td>\
<td data-stat=\"orb\"></td><td data-stat=\"drb\"></td><td data-stat=\"trb\"></td>\
<td data-stat=\"ast\"></td><td data-stat=\"stl\"></td><td data-stat=\"blk\"></td>\
<td data-stat=\"tov\"></td><td data-stat=\"pf\"></td><td data-stat=\"pts\"></td>\
<td data-stat=\"plus_minus\"></td>\
</tr>\
<tr>\
<th data-stat=\"player\">Team Totals</th>\
<td data-stat=\"mp\">240</td><td data-stat=\"fg\">38</td><td data-stat=\"fga\">85</td>\
<td data-stat=\"fg3\">10</td><td data-stat=\"fg3a\">30</td>\
<td data-stat=\"ft\">19</td><td data-stat=\"fta\">24</td>\
<td data-stat=\"orb\">9</td><td data-stat=\"drb\">33</td><td data-stat=\"trb\">42</td>\
<td data-stat=\"ast\">22</td><td data-stat=\"stl\">8</td><td data-stat=\"blk\">4</td>\
<td data-stat=\"tov\">12</td><td data-stat=\"pf\">22</td><td data-stat=\"pts\">105</td>\
<td data-stat=\"plus_minus\"></td>\
</tr>\
</tbody></table>\
<table class=\"stats_table\" id=\"box-CLE-game-basic\">\
<tbody>\
<tr><th data-stat=\"player\">Starters</th></tr>\
<tr>\
<th data-stat=\"player\"><a href=\"/players/j/jamesle01.html\">LeBron James</a></th>\
<td data-stat=\"mp\">46:00</td><td data-stat=\"fg\">13</td><td data-stat=\"fga\">30</td>\
<td data-stat=\"fg3\">2</td><td data-stat=\"fg3a\">8</td>\
<td data-stat=\"ft\">4</td><td data-stat=\"fta\">6</td>\
<td data-stat=\"orb\">2</td><td data-stat=\"drb\">10</td><td data-stat=\"trb\">12</td>\
<td data-stat=\"ast\">8</td><td data-stat=\"stl\">4</td><td data-stat=\"blk\">2</td>\
<td data-stat=\"tov\">6</td><td data-stat=\"pf\">1</td><td data-stat=\"pts\">32</td>\
<td data-stat=\"plus_minus\">-4</td>\
</tr>\
<tr><th data-stat=\"player\">Reserves</th></tr>\
<tr>\
<th data-stat=\"player\"><a href=\"/players/d/dellama01.html\">Matthew Dellavedova</a></th>\
<td data-stat=\"mp\">20:00</td><td data-stat=\"fg\">1</td><td data-stat=\"fga\">6</td>\
<td data-stat=\"fg3\">0</td><td data-stat=\"fg3a\">3</td>\
<td data-stat=\"ft\">0</td><td data-stat=\"fta\">0</td>\
<td data-stat=\"orb\">0</td><td data-stat=\"drb\">1</td><td data-stat=\"trb\">1</td>\
<td data-stat=\"ast\">1</td><td data-stat=\"stl\">0</td><td data-stat=\"blk\">0</td>\
<td data-stat=\"tov\">2</td><td data-stat=\"pf\">3</td><td data-stat=\"pts\">2</td>\
<td data-stat=\"plus_minus\">-9</td>\
</tr>\
<tr>\
<th data-stat=\"player\">Team Totals</th>\
<td data-stat=\"mp\">240</td><td data-stat=\"fg\">35</td><td data-stat=\"fga\">84</td>\
<td data-stat=\"fg3\">5</td><td data-stat=\"fg3a\">25</td>\
<td data-stat=\"ft\">22</td><td data-stat=\"fta\">28</td>\
<td data-stat=\"orb\">11</td><td data-stat=\"drb\">30</td><td data-stat=\"trb\">41</td>\
<td data-stat=\"ast\">18</td><td data-stat=\"stl\">7</td><td data-stat=\"blk\">3</td>\
<td data-stat=\"tov\">15</td><td data-stat=\"pf\">20</td><td data-stat=\"pts\">97</td>\
<td data-stat=\"plus_minus\"></td>\
</tr>\
</tbody></table>";

    #[test]
    fn box_page_modern_season_carries_full_stats() {
        let (teams, players) = parse_box_page(BOX_MODERN_HTML);
        assert_eq!(teams.len(), 2);
        let gsw = teams.iter().find(|t| t.team_br == "GSW").unwrap();
        assert_eq!(
            (gsw.fg, gsw.fga, gsw.ft, gsw.fta, gsw.pf, gsw.pts),
            (Some(38), Some(85), Some(19), Some(24), Some(22), Some(105))
        );
        assert_eq!((gsw.fg3, gsw.fg3a), (Some(10), Some(30)));
        assert_eq!((gsw.oreb, gsw.dreb, gsw.reb), (Some(9), Some(33), Some(42)));
        assert_eq!(
            (gsw.ast, gsw.stl, gsw.blk, gsw.tov),
            (Some(22), Some(8), Some(4), Some(12))
        );
        assert_eq!(gsw.mp.as_deref(), Some("240"));
        // Blank team +/- decodes to None, never 0.
        assert_eq!(gsw.plus_minus, None);

        let curry = players.iter().find(|p| p.player_br == "curryst01").unwrap();
        assert_eq!(curry.team_br, "GSW");
        assert_eq!(curry.starter, Some(true));
        assert_eq!(curry.position, None);
        assert_eq!(curry.mp.as_deref(), Some("44:00"));
        assert_eq!(
            (curry.fg, curry.fga, curry.fg3, curry.fg3a),
            (Some(10), Some(23), Some(5), Some(12))
        );
        assert_eq!(
            (curry.stl, curry.blk, curry.tov),
            (Some(2), Some(0), Some(3))
        );
        assert_eq!(curry.pts, Some(33));
        assert_eq!(curry.plus_minus, Some(7.0));
        assert_eq!(curry.dnp_reason, None);

        // Multibyte link text decodes without disturbing the slug.
        let manu = players.iter().find(|p| p.player_br == "ginobma01").unwrap();
        assert_eq!(manu.starter, Some(false));
        assert_eq!(manu.pts, Some(9));

        let delly = players.iter().find(|p| p.player_br == "dellama01").unwrap();
        assert_eq!(
            (delly.team_br.as_str(), delly.starter),
            ("CLE", Some(false))
        );
    }

    #[test]
    fn box_page_dnp_rows_carry_reason_and_no_stats() {
        let (_, players) = parse_box_page(BOX_MODERN_HTML);
        let dnp = players.iter().find(|p| p.player_br == "rushbr01").unwrap();
        assert_eq!(dnp.dnp_reason.as_deref(), Some("Did Not Play"));
        assert_eq!(dnp.mp, None);
        assert_eq!(dnp.fg, None);
        assert_eq!(dnp.fga, None);
        assert_eq!(dnp.fg3, None);
        assert_eq!(dnp.fg3a, None);
        assert_eq!(dnp.ft, None);
        assert_eq!(dnp.fta, None);
        assert_eq!(dnp.oreb, None);
        assert_eq!(dnp.dreb, None);
        assert_eq!(dnp.reb, None);
        assert_eq!(dnp.ast, None);
        assert_eq!(dnp.stl, None);
        assert_eq!(dnp.blk, None);
        assert_eq!(dnp.tov, None);
        assert_eq!(dnp.pf, None);
        assert_eq!(dnp.pts, None);
        assert_eq!(dnp.plus_minus, None);
    }

    /// 1946-47 box shape: only FG/FT/PF/PTS columns exist; no MP column, no
    /// section headers, November date (season 1947).
    const BOX_1946_HTML: &str = "\
<div class=\"scorebox_meta\"><div><strong>November 1, 1946</strong>, Maple Leaf Gardens</div></div>\
<table class=\"stats_table\" id=\"box-NYK-game-basic\">\
<tbody>\
<tr>\
<th data-stat=\"player\"><a href=\"/players/s/sadowed01.html\">Ed Sadowski</a></th>\
<td data-stat=\"fg\">7</td><td data-stat=\"fga\">15</td>\
<td data-stat=\"ft\">6</td><td data-stat=\"fta\">8</td>\
<td data-stat=\"pf\">4</td><td data-stat=\"pts\">20</td>\
</tr>\
<tr>\
<th data-stat=\"player\"><a href=\"/players/k/kaploso01.html\">Sonny Kaplow</a></th>\
<td data-stat=\"fg\">2</td><td data-stat=\"fga\">9</td>\
<td data-stat=\"ft\">1</td><td data-stat=\"fta\">2</td>\
<td data-stat=\"pf\">5</td><td data-stat=\"pts\">5</td>\
</tr>\
<tr>\
<th data-stat=\"player\">Team Totals</th>\
<td data-stat=\"mp\">240</td>\
<td data-stat=\"fg\">22</td><td data-stat=\"fga\">60</td>\
<td data-stat=\"ft\">24</td><td data-stat=\"fta\">30</td>\
<td data-stat=\"pf\">22</td><td data-stat=\"pts\">68</td>\
</tr>\
</tbody></table>\
<table class=\"stats_table\" id=\"box-TRH-game-basic\">\
<tbody>\
<tr>\
<th data-stat=\"player\"><a href=\"/players/m/mogusle01.html\">Leo Mogus</a></th>\
<td data-stat=\"fg\">6</td><td data-stat=\"fga\">14</td>\
<td data-stat=\"ft\">5</td><td data-stat=\"fta\">7</td>\
<td data-stat=\"pf\">3</td><td data-stat=\"pts\">17</td>\
</tr>\
<tr>\
<th data-stat=\"player\">Team Totals</th>\
<td data-stat=\"mp\">240</td>\
<td data-stat=\"fg\">20</td><td data-stat=\"fga\">55</td>\
<td data-stat=\"ft\">26</td><td data-stat=\"fta\">34</td>\
<td data-stat=\"pf\">20</td><td data-stat=\"pts\">66</td>\
</tr>\
</tbody></table>";

    #[test]
    fn box_page_1946_era_columns_are_null() {
        assert_eq!(page_season(BOX_1946_HTML), Some(1947));
        let (teams, players) = parse_box_page(BOX_1946_HTML);
        assert_eq!(teams.len(), 2);
        let nyk = teams.iter().find(|t| t.team_br == "NYK").unwrap();
        assert_eq!(
            (nyk.fg, nyk.fga, nyk.ft, nyk.fta, nyk.pf, nyk.pts),
            (Some(22), Some(60), Some(24), Some(30), Some(22), Some(68))
        );
        assert_eq!(nyk.mp.as_deref(), Some("240"));
        // Era-absent stats are None, never 0.
        assert_eq!(nyk.fg3, None);
        assert_eq!(nyk.fg3a, None);
        assert_eq!(nyk.oreb, None);
        assert_eq!(nyk.dreb, None);
        assert_eq!(nyk.reb, None);
        assert_eq!(nyk.ast, None);
        assert_eq!(nyk.stl, None);
        assert_eq!(nyk.blk, None);
        assert_eq!(nyk.tov, None);
        assert_eq!(nyk.plus_minus, None);

        let ed = players.iter().find(|p| p.player_br == "sadowed01").unwrap();
        assert_eq!(
            (ed.fg, ed.fga, ed.ft, ed.fta, ed.pf, ed.pts),
            (Some(7), Some(15), Some(6), Some(8), Some(4), Some(20))
        );
        assert_eq!(ed.mp, None);
        assert_eq!(ed.fg3, None);
        assert_eq!(ed.stl, None);
        assert_eq!(ed.blk, None);
        assert_eq!(ed.tov, None);
        assert_eq!(ed.reb, None);
        assert_eq!(ed.ast, None);
        assert_eq!(ed.dnp_reason, None);
        // No section headers on the era page: starter stays unknown.
        assert_eq!(ed.starter, None);
    }

    /// Live 1946 shape: team totals blank even core cells (`fga` always in
    /// November 1946, variously `fta`/`pf`). The row is kept with `None`s —
    /// blank means era-did-not-record, never zero — while a totals row with
    /// no `pts` at all records no result and is skipped.
    const BOX_LIVE_BLANKS_HTML: &str = "\
<table class=\"stats_table\" id=\"box-TRH-game-basic\">\
<tbody>\
<tr><th data-stat=\"player\">Team Totals</th>\
<td data-stat=\"mp\">240</td><td data-stat=\"fg\">25</td><td data-stat=\"fga\"></td>\
<td data-stat=\"ft\">16</td><td data-stat=\"fta\"></td>\
<td data-stat=\"pf\"></td><td data-stat=\"pts\">66</td>\
</tr>\
</tbody></table>\
<table class=\"stats_table\" id=\"box-XXX-game-basic\">\
<tbody>\
<tr><th data-stat=\"player\">Team Totals</th>\
<td data-stat=\"mp\">240</td><td data-stat=\"fg\">25</td><td data-stat=\"fga\"></td>\
<td data-stat=\"ft\">16</td><td data-stat=\"fta\"></td>\
<td data-stat=\"pf\"></td><td data-stat=\"pts\"></td>\
</tr>\
</tbody></table>";

    #[test]
    fn box_page_live_blanks_keep_row_with_nones() {
        let (teams, _) = parse_box_page(BOX_LIVE_BLANKS_HTML);
        assert_eq!(teams.len(), 1);
        let trh = &teams[0];
        assert_eq!(trh.team_br, "TRH");
        assert_eq!(
            (trh.fg, trh.fga, trh.ft, trh.fta, trh.pf, trh.pts),
            (Some(25), None, Some(16), None, None, Some(66))
        );
    }

    /// Totals shape (2014-15): a full row, a traded player with stint rows
    /// plus the combined `2TM` row, a multibyte name with a blank cell, a
    /// header row without a player link, and a gameless row — both skipped.
    const TOTALS_HTML: &str = "\
<table class=\"stats_table\" id=\"totals_stats\">\
<thead><tr><th data-stat=\"player\">Player</th><th data-stat=\"team\">Tm</th>\
<th data-stat=\"g\">G</th></tr></thead><tbody>\
<tr>\
<th data-stat=\"player\"><a href=\"/players/j/jamesle01.html\">LeBron James</a></th>\
<td data-stat=\"age\">30</td><td data-stat=\"team\">CLE</td>\
<td data-stat=\"g\">69</td><td data-stat=\"mp\">2456</td>\
<td data-stat=\"fg\">624</td><td data-stat=\"fga\">1279</td>\
<td data-stat=\"fg3\">120</td><td data-stat=\"fg3a\">339</td>\
<td data-stat=\"ft\">375</td><td data-stat=\"fta\">528</td>\
<td data-stat=\"orb\">48</td><td data-stat=\"drb\">441</td><td data-stat=\"trb\">489</td>\
<td data-stat=\"ast\">511</td><td data-stat=\"stl\">109</td><td data-stat=\"blk\">49</td>\
<td data-stat=\"tov\">272</td><td data-stat=\"pf\">164</td><td data-stat=\"pts\">1743</td>\
</tr>\
<tr>\
<th data-stat=\"player\"><a href=\"/players/s/smithjr01.html\">J.R. Smith</a></th>\
<td data-stat=\"team\">NYK</td>\
<td data-stat=\"g\">32</td><td data-stat=\"mp\">890</td>\
<td data-stat=\"fg\">130</td><td data-stat=\"fga\">330</td>\
<td data-stat=\"fg3\">60</td><td data-stat=\"fg3a\">170</td>\
<td data-stat=\"ft\">40</td><td data-stat=\"fta\">55</td>\
<td data-stat=\"orb\">10</td><td data-stat=\"drb\">80</td><td data-stat=\"trb\">90</td>\
<td data-stat=\"ast\">100</td><td data-stat=\"stl\">25</td><td data-stat=\"blk\">5</td>\
<td data-stat=\"tov\">50</td><td data-stat=\"pf\">70</td><td data-stat=\"pts\">360</td>\
</tr>\
<tr>\
<th data-stat=\"player\"><a href=\"/players/s/smithjr01.html\">J.R. Smith</a></th>\
<td data-stat=\"team\">CLE</td>\
<td data-stat=\"g\">46</td><td data-stat=\"mp\">1400</td>\
<td data-stat=\"fg\">200</td><td data-stat=\"fga\">480</td>\
<td data-stat=\"fg3\">107</td><td data-stat=\"fg3a\">273</td>\
<td data-stat=\"ft\">60</td><td data-stat=\"fta\">80</td>\
<td data-stat=\"orb\">15</td><td data-stat=\"drb\">120</td><td data-stat=\"trb\">135</td>\
<td data-stat=\"ast\">110</td><td data-stat=\"stl\">60</td><td data-stat=\"blk\">12</td>\
<td data-stat=\"tov\">55</td><td data-stat=\"pf\">90</td><td data-stat=\"pts\">567</td>\
</tr>\
<tr>\
<th data-stat=\"player\"><a href=\"/players/s/smithjr01.html\">J.R. Smith</a></th>\
<td data-stat=\"team\">2TM</td>\
<td data-stat=\"g\">78</td><td data-stat=\"mp\">2290</td>\
<td data-stat=\"fg\">330</td><td data-stat=\"fga\">810</td>\
<td data-stat=\"fg3\">167</td><td data-stat=\"fg3a\">443</td>\
<td data-stat=\"ft\">100</td><td data-stat=\"fta\">135</td>\
<td data-stat=\"orb\">25</td><td data-stat=\"drb\">200</td><td data-stat=\"trb\">225</td>\
<td data-stat=\"ast\">210</td><td data-stat=\"stl\">85</td><td data-stat=\"blk\">17</td>\
<td data-stat=\"tov\">105</td><td data-stat=\"pf\">160</td><td data-stat=\"pts\">927</td>\
</tr>\
<tr>\
<th data-stat=\"player\"><a href=\"/players/j/jokicni01.html\">Nikola Joki\u{107}</a></th>\
<td data-stat=\"team\">DEN</td>\
<td data-stat=\"g\">80</td><td data-stat=\"mp\">1700</td>\
<td data-stat=\"fg\">300</td><td data-stat=\"fga\">600</td>\
<td data-stat=\"fg3\">30</td><td data-stat=\"fg3a\">100</td>\
<td data-stat=\"ft\">150</td><td data-stat=\"fta\">190</td>\
<td data-stat=\"orb\">120</td><td data-stat=\"drb\">400</td><td data-stat=\"trb\">520</td>\
<td data-stat=\"ast\">180</td><td data-stat=\"stl\">70</td><td data-stat=\"blk\"></td>\
<td data-stat=\"tov\">110</td><td data-stat=\"pf\">200</td><td data-stat=\"pts\">780</td>\
</tr>\
<tr><th data-stat=\"player\">Player</th><td data-stat=\"team\">Tm</td><td data-stat=\"g\">G</td></tr>\
<tr>\
<th data-stat=\"player\"><a href=\"/players/x/ghostxx01.html\">Ghost Player</a></th>\
<td data-stat=\"team\">CLE</td><td data-stat=\"g\"></td>\
</tr>\
</tbody></table>";

    #[test]
    fn totals_page_keeps_stint_and_combined_splits() {
        let rows = parse_totals_page(TOTALS_HTML, 2015);
        // LeBron + 3 Smith rows (NYK, CLE, 2TM) + Jokić; header and
        // gameless rows skipped.
        assert_eq!(rows.len(), 5);
        assert!(rows.iter().all(|r| r.season == 2015));

        let lebron = rows.iter().find(|r| r.player_br == "jamesle01").unwrap();
        assert_eq!(lebron.team_br, "CLE");
        assert_eq!(lebron.g, 69);
        assert_eq!(
            (lebron.fg, lebron.fga, lebron.pts),
            (Some(624), Some(1279), Some(1743))
        );
        assert_eq!(
            (lebron.oreb, lebron.dreb, lebron.reb),
            (Some(48), Some(441), Some(489))
        );
        assert_eq!(
            (lebron.stl, lebron.blk, lebron.tov),
            (Some(109), Some(49), Some(272))
        );

        let smith: Vec<&SeasonTotalInput> =
            rows.iter().filter(|r| r.player_br == "smithjr01").collect();
        assert_eq!(smith.len(), 3);
        let mut teams: Vec<&str> = smith.iter().map(|r| r.team_br.as_str()).collect();
        teams.sort_unstable();
        assert_eq!(teams, vec!["2TM", "CLE", "NYK"]);

        // Multibyte name parses; the blank block cell is None, never 0.
        let jokic = rows.iter().find(|r| r.player_br == "jokicni01").unwrap();
        assert_eq!(jokic.team_br, "DEN");
        assert_eq!(jokic.blk, None);
        assert_eq!(jokic.stl, Some(70));
    }

    #[test]
    fn totals_page_accepts_live_key_aliases() {
        // Live totals tables renamed the column keys (`name_display`,
        // `team_name_abbr`, `games`); the parser accepts both shapes
        // (observed 1946-47 crawl, Fulks row).
        let html = "\
<table class=\"stats_table\" id=\"totals_stats\"><tbody>\
<tr>\
<th data-stat=\"name_display\"><a href=\"/players/f/fulksjo01.html\">Joe Fulks</a></th>\
<td data-stat=\"team_name_abbr\">PHW</td>\
<td data-stat=\"games\">60</td>\
<td data-stat=\"fg\">389</td><td data-stat=\"fga\">1400</td>\
<td data-stat=\"pts\">1389</td>\
</tr>\
</tbody></table>";
        let rows = parse_totals_page(html, 1947);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].player_br, "fulksjo01");
        assert_eq!(rows[0].team_br, "PHW");
        assert_eq!(rows[0].g, 60);
        assert_eq!(rows[0].pts, Some(1389));
    }

    #[test]
    fn era_cutoffs_match_recorded_history() {
        // Threes from 1979-80, steals/blocks/split rebounds from 1973-74,
        // turnovers from 1977-78, total rebounds from 1950-51.
        assert!(!era_recorded("fg3", Some(1979), true));
        assert!(era_recorded("fg3", Some(1980), true));
        assert!(!era_recorded("stl", Some(1973), true));
        assert!(era_recorded("stl", Some(1974), true));
        assert!(!era_recorded("blk", Some(1973), true));
        assert!(!era_recorded("oreb", Some(1973), true));
        assert!(era_recorded("dreb", Some(1974), true));
        assert!(!era_recorded("tov", Some(1977), true));
        assert!(era_recorded("tov", Some(1978), true));
        assert!(!era_recorded("reb", Some(1950), true));
        assert!(era_recorded("reb", Some(1951), true));
        // Per-game assists blank before 1950-51, but season totals carry
        // assists since 1946-47.
        assert!(!era_recorded("ast", Some(1947), true));
        assert!(era_recorded("ast", Some(1947), false));
        // Undated pages never clamp; always-recorded stats pass through.
        assert!(era_recorded("stl", None, true));
        assert!(era_recorded("fg", Some(1947), true));
        assert!(era_recorded("pts", Some(1947), false));
    }

    #[test]
    fn page_season_rolls_october_into_next_year() {
        let nov = "<div class=\"scorebox_meta\">November 1, 1946</div>";
        assert_eq!(page_season(nov), Some(1947));
        let jun = "<div class=\"scorebox_meta\">June 16, 2015</div>";
        assert_eq!(page_season(jun), Some(2015));
        let jan = "<div class=\"scorebox_meta\">January 5, 1980</div>";
        assert_eq!(page_season(jan), Some(1980));
        assert_eq!(page_season("<html><body>no meta</body></html>"), None);
    }

    #[test]
    fn scanner_helpers_never_slice_human_text() {
        // The ä-panic regression pin: multibyte text inside tags, entities,
        // and quoted attributes must decode without panicking.
        assert_eq!(
            inner_text("<b>J\u{e4}rf\u{e4}lla</b> &amp; S\u{f6}dra"),
            "J\u{e4}rf\u{e4}lla & S\u{f6}dra"
        );
        assert_eq!(
            inner_text("Gheorghe Mure\u{219}an"),
            "Gheorghe Mure\u{219}an"
        );
        assert_eq!(inner_text("<a href=\"/x.html\">A&lt;B&gt;</a>"), "A<B>");
        assert_eq!(
            attr_value("<td data-stat=\"mp\" csk=\"1946-11-01\">", "csk").as_deref(),
            Some("1946-11-01")
        );
        // `id` must not match inside `hidden`.
        assert_eq!(attr_value("<div hidden=\"x\">", "id"), None);
        assert_eq!(
            slug_from_box_href("/boxscores/194611010TRH.html"),
            Some("194611010TRH".to_owned())
        );
        assert_eq!(
            slug_from_box_href("/boxscores/194611010TRH.html#q1"),
            Some("194611010TRH".to_owned())
        );
        assert_eq!(
            slug_from_team_href("/teams/TRH/1947.html"),
            Some("TRH".to_owned())
        );
        assert_eq!(slug_from_team_href("/boxscores/194611010TRH.html"), None);
        assert_eq!(
            slug_from_player_href("/players/c/curryst01.html"),
            Some("curryst01".to_owned())
        );
        assert!(is_minutes_shape("44:00"));
        assert!(is_minutes_shape("240"));
        assert!(!is_minutes_shape("Did Not Play"));
        assert!(!is_minutes_shape(""));
        assert!(!is_minutes_shape("12:3:4"));
        assert_eq!(parse_opt_int("1,234"), Some(1234));
        assert_eq!(parse_opt_int(""), None);
        assert_eq!(parse_opt_int("  "), None);
        assert_eq!(parse_opt_plus_minus("+7"), Some(7.0));
        assert_eq!(parse_opt_plus_minus(""), None);
    }
}

#[cfg(test)]
mod fetch_tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// Canned-HTML client: no network, records call order.
    struct MapClient {
        pages: HashMap<String, String>,
        calls: RefCell<Vec<String>>,
    }

    impl MapClient {
        fn with(pages: &[(&str, &str)]) -> Self {
            MapClient {
                pages: pages
                    .iter()
                    .map(|(u, h)| ((*u).to_owned(), (*h).to_owned()))
                    .collect(),
                calls: RefCell::new(Vec::new()),
            }
        }
    }

    impl FetchClient for MapClient {
        fn fetch(&self, url: &str) -> Result<String, FetchError> {
            self.calls.borrow_mut().push(url.to_owned());
            self.pages
                .get(url)
                .cloned()
                .ok_or_else(|| FetchError::Client(format!("no fixture for {url}")))
        }
    }

    static DIR_SEQ: AtomicU64 = AtomicU64::new(0);

    /// Unique caller-supplied dir under the system temp dir (std only, no
    /// `tempdir` crate). Removed best-effort when the test finishes.
    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn fresh(tag: &str) -> Self {
            let n = DIR_SEQ.fetch_add(1, Ordering::SeqCst);
            let mut path = std::env::temp_dir();
            path.push(format!("nbatv-ingest-{tag}-{}-{n}", std::process::id()));
            std::fs::create_dir_all(&path).expect("create temp dir");
            TempDir { path }
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    const GAME_HTML: &str = "\
<html><head><meta name=\"revised\" content=\"09:00:00 04-Sep-2026\"></head>\
<body><div class=\"scorebox_meta\">November 1, 1946</div></body></html>";

    #[test]
    fn etiquette_constant_covers_crawl_delay_with_margin() {
        assert!(FETCH_MIN_INTERVAL >= Duration::from_millis(3_500));
        assert_eq!(
            etiquette_delay(Duration::from_millis(0)),
            FETCH_MIN_INTERVAL
        );
        assert_eq!(etiquette_delay(Duration::from_secs(10)), Duration::ZERO);
        assert_eq!(
            etiquette_delay(Duration::from_millis(1_000)),
            Duration::from_millis(2_500)
        );
    }

    #[test]
    fn gzip_round_trip_covers_empty_unicode_and_multiblock() {
        for raw in [
            Vec::new(),
            "<html>first game: NYK 68, TRH 66 — M\u{e4}ller</html>"
                .as_bytes()
                .to_vec(),
            vec![b'a'; 100_000],
        ] {
            let gz = gzip_encode(&raw);
            assert_eq!(&gz[0..2], &[0x1F, 0x8B]);
            assert_eq!(gzip_decode(&gz).unwrap(), raw);
        }
        // 100_000 bytes need two stored blocks (64 KiB cap each).
        assert!(gzip_encode(&vec![b'a'; 100_000]).len() > 65_535);
    }

    #[test]
    fn gzip_decode_rejects_garbage_without_panicking() {
        assert!(gzip_decode(b"").is_err());
        assert!(gzip_decode(b"not gzip at all................").is_err());
        let mut gz = gzip_encode(b"hello");
        assert!(gzip_decode(&gz[..gz.len() - 3]).is_err());
        gz[1] = 0x00;
        assert!(gzip_decode(&gz).is_err());
        let mut bad_crc = gzip_encode(b"hello");
        let n = bad_crc.len();
        bad_crc[n - 8] ^= 0xFF;
        assert!(gzip_decode(&bad_crc).is_err());
    }

    #[test]
    fn fetch_writes_gz_snapshot_and_records_revision() {
        let dir = TempDir::fresh("write");
        let client =
            MapClient::with(&[("https://br.example/boxscores/194611010TRH.html", GAME_HTML)]);
        let jobs = [FetchJob::new(
            "194611010TRH.html.gz",
            "https://br.example/boxscores/194611010TRH.html",
        )];
        let mut sleeps = Vec::new();
        let report = fetch_season_with_sleeper(
            &client,
            "br-box",
            "BAA_1947",
            &jobs,
            &dir.path,
            &mut |d: Duration| sleeps.push(d),
        )
        .unwrap();
        assert_eq!(report.fetched, vec!["194611010TRH.html.gz".to_owned()]);
        assert!(report.skipped.is_empty());
        // Single request: no etiquette wait needed.
        assert!(sleeps.is_empty());
        assert_eq!(client.calls.borrow().len(), 1);

        let target = dir.path.join(raw_snapshot_path(
            "br-box",
            "BAA_1947",
            "194611010TRH.html.gz",
        ));
        assert!(target.is_file());
        assert!(target.starts_with(&dir.path));
        assert_eq!(read_snapshot_gz(&target).unwrap(), GAME_HTML);
        assert_eq!(report.revisions.len(), 1);
        assert_eq!(report.revisions[0].page, "194611010TRH.html.gz");
        assert_eq!(
            report.revisions[0].meta_revised.as_deref(),
            Some("09:00:00 04-Sep-2026")
        );
        // The observed stamp feeds the existing freshness helper.
        assert!(recrawl_hint(None, &report.revisions[0]));
        assert!(!recrawl_hint(
            Some("09:00:00 04-Sep-2026"),
            &report.revisions[0]
        ));
    }

    #[test]
    fn fetch_skips_files_already_on_disk_without_network() {
        let dir = TempDir::fresh("resume");
        let target = dir.path.join(raw_snapshot_path(
            "br-box",
            "BAA_1947",
            "194611010TRH.html.gz",
        ));
        write_snapshot_gz(&target, GAME_HTML).unwrap();
        let before = std::fs::read(&target).unwrap();

        // Empty fixture map: any fetch attempt errors, so a network touch
        // would fail the run.
        let client = MapClient::with(&[]);
        let jobs = [FetchJob::new(
            "194611010TRH.html.gz",
            "https://br.example/boxscores/194611010TRH.html",
        )];
        let report = fetch_season(&client, "br-box", "BAA_1947", &jobs, &dir.path).unwrap();
        assert!(report.fetched.is_empty());
        assert_eq!(report.skipped, vec!["194611010TRH.html.gz".to_owned()]);
        assert!(client.calls.borrow().is_empty());
        assert_eq!(std::fs::read(&target).unwrap(), before);
    }

    #[test]
    fn fetch_paces_sequential_requests_with_etiquette_waits() {
        let dir = TempDir::fresh("pace");
        let client = MapClient::with(&[
            ("https://br.example/a.html", "<html>a</html>"),
            ("https://br.example/b.html", "<html>b</html>"),
        ]);
        let jobs = [
            FetchJob::new("a.html.gz", "https://br.example/a.html"),
            FetchJob::new("b.html.gz", "https://br.example/b.html"),
        ];
        let mut sleeps = Vec::new();
        let report = fetch_season_with_sleeper(
            &client,
            "br-box",
            "BAA_1947",
            &jobs,
            &dir.path,
            &mut |d: Duration| sleeps.push(d),
        )
        .unwrap();
        assert_eq!(report.fetched.len(), 2);
        // One season batch, in order, with one full etiquette wait between
        // the two requests (back-to-back fixture fetches take ~no time).
        assert_eq!(
            *client.calls.borrow(),
            vec![
                "https://br.example/a.html".to_owned(),
                "https://br.example/b.html".to_owned()
            ]
        );
        assert_eq!(sleeps.len(), 1);
        assert!(sleeps[0] >= Duration::from_secs(3));
        assert!(sleeps[0] <= FETCH_MIN_INTERVAL);
    }

    #[test]
    fn fetch_force_refetches_an_on_disk_snapshot() {
        let dir = TempDir::fresh("force");
        let target = dir
            .path
            .join(raw_snapshot_path("br-box", "BAA_1947", "a.html.gz"));
        write_snapshot_gz(&target, "<html>stale</html>").unwrap();
        let client = MapClient::with(&[("https://br.example/a.html", "<html>fresh</html>")]);
        let jobs = [FetchJob::new("a.html.gz", "https://br.example/a.html").force()];
        let report = fetch_season(&client, "br-box", "BAA_1947", &jobs, &dir.path).unwrap();
        assert_eq!(report.fetched, vec!["a.html.gz".to_owned()]);
        assert_eq!(read_snapshot_gz(&target).unwrap(), "<html>fresh</html>");
    }

    #[test]
    fn fetch_rejects_path_components_that_escape_the_caller_dir() {
        let dir = TempDir::fresh("unsafe");
        let client = MapClient::with(&[("https://br.example/a.html", "<html>a</html>")]);
        for (source, season, file) in [
            ("../outside", "BAA_1947", "a.html.gz"),
            ("br-box", "..", "a.html.gz"),
            ("br-box", "BAA_1947", "../evil.html.gz"),
            ("br-box", "BAA_1947", "sub/dir.html.gz"),
            ("br-box", "BAA_1947", ""),
        ] {
            let jobs = [FetchJob::new(file, "https://br.example/a.html")];
            assert!(
                matches!(
                    fetch_season(&client, source, season, &jobs, &dir.path),
                    Err(FetchError::UnsafePath(_))
                ),
                "must reject {source:?}/{season:?}/{file:?}"
            );
        }
        assert!(client.calls.borrow().is_empty());
    }

    #[test]
    fn meta_revised_parses_stamp_and_absent_means_no_signal() {
        assert_eq!(
            parse_meta_revised(GAME_HTML).as_deref(),
            Some("09:00:00 04-Sep-2026")
        );
        assert_eq!(
            parse_meta_revised("<html><body>no meta</body></html>"),
            None
        );
        let rev = PageRevision {
            page: "a.html.gz".to_owned(),
            meta_revised: None,
        };
        assert!(!recrawl_hint(None, &rev));
    }
}

// ---------------------------------------------------------------------------
// Snapshot ingest: `data/raw/br` -> the archive db
// ---------------------------------------------------------------------------

use nbatv_db::{
    insert_box_player, insert_box_team, insert_season, insert_season_total, upsert_game,
    upsert_team, BoxPlayerRow, BoxTeamRow, GameRow, SeasonRow, SeasonTotalRow, TeamRow,
};
use rusqlite::Connection;

/// One run of the snapshot builder. State counters (`seasons`..`games_*`)
/// describe what the archive holds after the run and are stable across
/// re-ingests; the `inserted_*`/`upgraded_*` counters describe the writes
/// this run actually performed and drop back to zero on a resume pass.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct IngestReport {
    pub seasons: usize,
    pub teams: usize,
    pub games: usize,
    pub games_with_box: usize,
    pub games_without_box: usize,
    /// Box snapshot present but its team sides disagree with the schedule:
    /// the game is stored as a bare schedule row and the box is skipped.
    pub games_mismatched: usize,
    /// Schedule rows dropped for a missing/invalid box slug (summed over
    /// every page parsed this run).
    pub skipped_bad_slugs: usize,
    /// Box snapshots whose file name passes validation but has no schedule
    /// row: the schedule is the authority, so the page is ignored.
    pub skipped_orphan_box_pages: usize,
    /// Schedule rows whose game-id date disagrees with the season dir they
    /// sit in (a mislabeled page in the crawl): dropped, never stored under
    /// a wrong season.
    pub skipped_season_mismatch: usize,
    /// Team slugs not in [`TEAM_CITY_NAME`]: stored with slug-shaped names
    /// so games stay browsable, and reported for the crosswalk to grow.
    pub unknown_team_slugs: Vec<String>,
    pub inserted_box_teams: usize,
    pub inserted_box_players: usize,
    pub inserted_season_total_rows: usize,
    /// Games whose scores went from bare 0-0 to box-derived this run.
    pub upgraded_games: usize,
}

#[derive(Debug)]
pub enum IngestError {
    Io(std::io::Error),
    Db(rusqlite::Error),
    /// A snapshot failed the gzip/UTF-8 decode the crawl format guarantees.
    Snapshot(String),
}

impl fmt::Display for IngestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IngestError::Io(e) => write!(f, "snapshot read failed: {e}"),
            IngestError::Db(e) => write!(f, "archive db write failed: {e}"),
            IngestError::Snapshot(e) => write!(f, "snapshot decode failed: {e}"),
        }
    }
}

impl std::error::Error for IngestError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            IngestError::Io(e) => Some(e),
            IngestError::Db(e) => Some(e),
            IngestError::Snapshot(_) => None,
        }
    }
}

impl From<std::io::Error> for IngestError {
    fn from(e: std::io::Error) -> Self {
        IngestError::Io(e)
    }
}

impl From<rusqlite::Error> for IngestError {
    fn from(e: rusqlite::Error) -> Self {
        IngestError::Db(e)
    }
}

/// Slug -> (city, name) for every franchise the crawl can surface, BAA
/// 1946-47 through the present. Same shape as [`franchise_crosswalk`]: a
/// checked-in domain constant, not a scrape. BR's league schedule pages
/// label most modern clubs by city alone, so the split is curated here
/// rather than guessed from link text; an unknown slug still gets a team
/// row (slug-shaped name) and is reported via
/// `IngestReport::unknown_team_slugs`.
const TEAM_CITY_NAME: &[(&str, &str, &str)] = &[
    ("ATL", "Atlanta", "Hawks"),
    ("BAL", "Baltimore", "Bullets"),
    ("BOS", "Boston", "Celtics"),
    ("BRK", "Brooklyn", "Nets"),
    ("BUF", "Buffalo", "Braves"),
    ("CHA", "Charlotte", "Bobcats"),
    ("CHH", "Charlotte", "Hornets"),
    ("CHO", "Charlotte", "Hornets"),
    ("CHI", "Chicago", "Bulls"),
    ("CHS", "Chicago", "Stags"),
    ("CIN", "Cincinnati", "Royals"),
    ("CLE", "Cleveland", "Cavaliers"),
    ("CLR", "Cleveland", "Rebels"),
    ("DAL", "Dallas", "Mavericks"),
    ("DEN", "Denver", "Nuggets"),
    ("DET", "Detroit", "Pistons"),
    ("DTF", "Detroit", "Falcons"),
    ("GSW", "Golden State", "Warriors"),
    ("HOU", "Houston", "Rockets"),
    ("IND", "Indiana", "Pacers"),
    ("KCK", "Kansas City", "Kings"),
    ("KCO", "Kansas City-Omaha", "Kings"),
    ("LAC", "Los Angeles", "Clippers"),
    ("LAL", "Los Angeles", "Lakers"),
    ("MEM", "Memphis", "Grizzlies"),
    ("MIA", "Miami", "Heat"),
    ("MIL", "Milwaukee", "Bucks"),
    ("MIN", "Minnesota", "Timberwolves"),
    ("NJN", "New Jersey", "Nets"),
    ("NOH", "New Orleans", "Hornets"),
    ("NOJ", "New Orleans", "Jazz"),
    ("NOP", "New Orleans", "Pelicans"),
    ("NYK", "New York", "Knicks"),
    ("OKC", "Oklahoma City", "Thunder"),
    ("ORL", "Orlando", "Magic"),
    ("PHI", "Philadelphia", "76ers"),
    ("PHO", "Phoenix", "Suns"),
    ("PHW", "Philadelphia", "Warriors"),
    ("PIT", "Pittsburgh", "Ironmen"),
    ("POR", "Portland", "Trail Blazers"),
    ("PRO", "Providence", "Steamrollers"),
    ("SAC", "Sacramento", "Kings"),
    ("SAS", "San Antonio", "Spurs"),
    ("SEA", "Seattle", "SuperSonics"),
    ("SFW", "San Francisco", "Warriors"),
    ("STB", "St. Louis", "Bombers"),
    ("STL", "St. Louis", "Hawks"),
    ("TOR", "Toronto", "Raptors"),
    ("TRH", "Toronto", "Huskies"),
    ("UTA", "Utah", "Jazz"),
    ("WAS", "Washington", "Wizards"),
    ("WSB", "Washington", "Bullets"),
    ("WSC", "Washington", "Capitols"),
];

/// `1946-47` -> 1947, with the century rollover (`1999-00` -> 2000). NBA
/// endings run `47`..`99` then `00`..: a short year below `46` belongs to
/// the 2000s.
fn season_slug_to_ending_year(slug: &str) -> Option<i32> {
    let bytes = slug.as_bytes();
    if bytes.len() != 7 || bytes[4] != b'-' {
        return None;
    }
    let numeric = |slice: &[u8]| slice.iter().all(|b| b.is_ascii_digit());
    if !numeric(&bytes[..4]) || !numeric(&bytes[5..]) {
        return None;
    }
    let short: i32 = slug[5..].parse().ok()?;
    let century = if short <= 45 { 2000 } else { 1900 };
    Some(century + short)
}

/// BAA through 1948-49, NBA from 1949-50 on.
fn league_for_ending_year(year: i32) -> &'static str {
    if year <= 1949 {
        "BAA"
    } else {
        "NBA"
    }
}

/// Snapshots are stored gzip (see [`write_snapshot_gz`]); the earliest
/// crawl waves predate that and stored plain UTF-8, so the magic bytes
/// decide which decoder runs. Anything else is a hard decode error.
fn read_snapshot_page(path: &Path) -> Result<String, IngestError> {
    let bytes = std::fs::read(path)?;
    let decode = |r: Result<String, IngestError>| {
        r.map_err(|e| match e {
            IngestError::Snapshot(detail) => {
                IngestError::Snapshot(format!("{}: {detail}", path.display()))
            }
            other => other,
        })
    };
    if bytes.starts_with(&[0x1f, 0x8b]) {
        decode(read_snapshot_gz(path).map_err(|e| IngestError::Snapshot(e.to_string())))
    } else {
        decode(
            String::from_utf8(bytes)
                .map_err(|e| IngestError::Snapshot(format!("snapshot is not UTF-8: {e}"))),
        )
    }
}

fn sorted_entries(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .collect();
    entries.sort();
    Ok(entries)
}

/// Ingest a snapshot crawl (`data/raw/br`, one directory per season slug)
/// into the archive db. Idempotent: safe to re-run after the crawl grows.
///
/// Honesty notes, all visible in [`IngestReport`]:
/// - `game_type` is `REGULAR` for every row: the league schedule pages
///   carry no round marker, so playoff/Finals games read as regular until
///   the crawl gains round data.
/// - `ot`/`arena`/`attendance` are NULL: the schedule parser does not
///   extract them yet.
/// - Games whose box snapshot is absent stay as bare schedule rows (0-0)
///   and upgrade automatically when a later crawl adds the snapshot.
///
/// The whole run commits atomically; teams are re-upserted from the full
/// crawl's observed span so a grown crawl widens the spans wholesale.
pub fn ingest_snapshot_dir(conn: &Connection, root: &Path) -> Result<IngestReport, IngestError> {
    nbatv_db::create_schema(conn)?;
    let tx = conn.unchecked_transaction()?;
    let mut report = IngestReport::default();
    let mut team_spans: BTreeMap<String, (i32, i32)> = BTreeMap::new();

    for entry in sorted_entries(root)? {
        let Some(slug) = entry.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let Some(year) = season_slug_to_ending_year(slug) else {
            continue;
        };
        if !entry.is_dir() {
            continue;
        }
        ingest_season_dir(&tx, &mut report, &mut team_spans, &entry, slug, year)?;
    }

    // A club observed in the crawl's latest season has no recorded end:
    // its `active_to` stays NULL (the Shell reads that as "active"), so
    // only clubs whose slug stops before the frontier get the defunct tag.
    let crawl_frontier = team_spans.values().map(|(_, to)| *to).max();
    for (slug, (from, to)) in &team_spans {
        let active_to = if Some(*to) == crawl_frontier {
            None
        } else {
            Some(*to)
        };
        let (city, name) = match TEAM_CITY_NAME.iter().find(|(s, _, _)| s == slug) {
            Some((_, city, name)) => ((*city).to_owned(), (*name).to_owned()),
            None => {
                report.unknown_team_slugs.push(slug.clone());
                (slug.clone(), slug.clone())
            }
        };
        upsert_team(
            &tx,
            &TeamRow {
                br_slug: slug.clone(),
                nba_team_id: None,
                franchise_id: None,
                city,
                name,
                abbrev: slug.clone(),
                active_from: Some(*from),
                active_to,
            },
        )?;
    }
    report.teams = team_spans.len();

    tx.commit()?;
    Ok(report)
}

fn ingest_season_dir(
    tx: &Connection,
    report: &mut IngestReport,
    team_spans: &mut BTreeMap<String, (i32, i32)>,
    dir: &Path,
    slug: &str,
    year: i32,
) -> Result<(), IngestError> {
    let league = league_for_ending_year(year);

    // Schedule first: the league index pages are the authority for which
    // games exist; full-season and monthly pages dedup by game id.
    let mut schedule: BTreeMap<String, GameIndexRow> = BTreeMap::new();
    let mut schedule_pages = 0;
    for path in sorted_entries(dir)? {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !name.starts_with("_games") || !name.ends_with(".html") {
            continue;
        }
        let html = read_snapshot_page(&path)?;
        let page = parse_games_page(&html);
        report.skipped_bad_slugs += page.skipped_bad_slugs;
        for row in page.rows {
            let id_year: i32 = row.game_id[..4].parse().unwrap_or(0);
            if id_year != year - 1 && id_year != year {
                report.skipped_season_mismatch += 1;
                continue;
            }
            schedule.entry(row.game_id.clone()).or_insert(row);
        }
        schedule_pages += 1;
    }
    if schedule_pages == 0 {
        return Ok(());
    }

    let season_known: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM seasons WHERE league = ?1 AND year = ?2)",
        rusqlite::params![league, year],
        |row| row.get(0),
    )?;
    if !season_known {
        insert_season(
            tx,
            &SeasonRow {
                league: league.to_owned(),
                year,
                label: slug.to_owned(),
            },
        )?;
    }
    report.seasons += 1;

    for row in schedule.values() {
        for team in [row.home_br.as_str(), row.away_br.as_str()] {
            let span = team_spans
                .entry(team.to_owned())
                .or_insert((year - 1, year - 1));
            span.0 = span.0.min(year - 1);
            span.1 = span.1.max(year - 1);
        }
    }

    for (game_id, row) in &schedule {
        let box_path = dir.join(format!("{game_id}.html"));
        if !box_path.exists() {
            upsert_game(tx, &bare_game_row(game_id, row, year, league))?;
            report.games_without_box += 1;
            continue;
        }
        let html = read_snapshot_page(&box_path)?;
        let (teams, players) = parse_box_page(&html);
        let side_pts = |team: &str| -> Option<i32> {
            teams.iter().find(|t| t.team_br == team).and_then(|t| t.pts)
        };
        match (side_pts(&row.home_br), side_pts(&row.away_br)) {
            (Some(home_pts), Some(away_pts)) => {
                let prior: Option<(i32, i32)> = tx
                    .query_row(
                        "SELECT home_pts, away_pts FROM games WHERE game_id = ?1",
                        rusqlite::params![game_id],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                    .map(Some)
                    .or_else(|e| {
                        if e == rusqlite::Error::QueryReturnedNoRows {
                            Ok(None)
                        } else {
                            Err(e)
                        }
                    })?;
                let mut game = bare_game_row(game_id, row, year, league);
                game.home_pts = home_pts;
                game.away_pts = away_pts;
                upsert_game(tx, &game)?;
                if prior == Some((0, 0)) {
                    report.upgraded_games += 1;
                }
                let has_box: bool = tx.query_row(
                    "SELECT EXISTS(SELECT 1 FROM box_team WHERE game_id = ?1)",
                    rusqlite::params![game_id],
                    |r| r.get(0),
                )?;
                if !has_box {
                    for team in teams {
                        insert_box_team(tx, &box_team_row(game_id, team))?;
                        report.inserted_box_teams += 1;
                    }
                    for player in players {
                        insert_box_player(tx, &box_player_row(game_id, player))?;
                        report.inserted_box_players += 1;
                    }
                }
                report.games_with_box += 1;
            }
            _ => {
                upsert_game(tx, &bare_game_row(game_id, row, year, league))?;
                report.games_mismatched += 1;
            }
        }
    }
    report.games += schedule.len();

    for path in sorted_entries(dir)? {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !name.ends_with(".html") || name.starts_with('_') {
            continue;
        }
        let stem = name.strip_suffix(".html").unwrap_or(name);
        if validate_game_id(stem) && !schedule.contains_key(stem) {
            report.skipped_orphan_box_pages += 1;
        }
    }

    let totals_path = dir.join("_totals.html");
    if totals_path.exists() {
        let html = read_snapshot_page(&totals_path)?;
        for total in parse_totals_page(&html, year) {
            let known: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM player_season_totals \
                 WHERE player_br = ?1 AND season = ?2 AND team_br = ?3)",
                rusqlite::params![total.player_br, total.season, total.team_br],
                |row| row.get(0),
            )?;
            if known {
                continue;
            }
            insert_season_total(tx, &season_total_row(total))?;
            report.inserted_season_total_rows += 1;
        }
    }
    Ok(())
}

/// The schedule-authority row: full identity, scores unknown (0-0) until a
/// box snapshot upgrades them.
fn bare_game_row(game_id: &str, row: &GameIndexRow, year: i32, league: &str) -> GameRow {
    GameRow {
        game_id: game_id.to_owned(),
        nba_game_id: None,
        league: league.to_owned(),
        season: year,
        date: row.date.clone(),
        game_type: "REGULAR".to_owned(),
        home_team: row.home_br.clone(),
        away_team: row.away_br.clone(),
        home_pts: 0,
        away_pts: 0,
        ot: None,
        arena: None,
        attendance: None,
        br_url: format!("https://www.basketball-reference.com/boxscores/{game_id}.html"),
        sources: "[]".to_owned(),
    }
}

fn box_team_row(game_id: &str, input: BoxTeamInput) -> BoxTeamRow {
    let BoxTeamInput {
        team_br,
        mp,
        fg,
        fga,
        fg3,
        fg3a,
        ft,
        fta,
        oreb,
        dreb,
        reb,
        ast,
        stl,
        blk,
        tov,
        pf,
        pts,
        plus_minus,
    } = input;
    BoxTeamRow {
        game_id: game_id.to_owned(),
        team_br,
        mp,
        fg,
        fga,
        fg3,
        fg3a,
        ft,
        fta,
        oreb,
        dreb,
        reb,
        ast,
        stl,
        blk,
        tov,
        pf,
        pts,
        plus_minus,
    }
}

fn box_player_row(game_id: &str, input: BoxPlayerInput) -> BoxPlayerRow {
    let BoxPlayerInput {
        team_br,
        player_br,
        starter,
        position,
        mp,
        fg,
        fga,
        fg3,
        fg3a,
        ft,
        fta,
        oreb,
        dreb,
        reb,
        ast,
        stl,
        blk,
        tov,
        pf,
        pts,
        plus_minus,
        dnp_reason,
    } = input;
    BoxPlayerRow {
        game_id: game_id.to_owned(),
        team_br,
        player_br,
        starter,
        position,
        mp,
        fg,
        fga,
        fg3,
        fg3a,
        ft,
        fta,
        oreb,
        dreb,
        reb,
        ast,
        stl,
        blk,
        tov,
        pf,
        pts,
        plus_minus,
        dnp_reason,
    }
}

fn season_total_row(input: SeasonTotalInput) -> SeasonTotalRow {
    let SeasonTotalInput {
        player_br,
        season,
        team_br,
        g,
        mp,
        fg,
        fga,
        fg3,
        fg3a,
        ft,
        fta,
        oreb,
        dreb,
        reb,
        ast,
        stl,
        blk,
        tov,
        pf,
        pts,
    } = input;
    SeasonTotalRow {
        player_br,
        season,
        team_br,
        g,
        mp,
        fg,
        fga,
        fg3,
        fg3a,
        ft,
        fta,
        oreb,
        dreb,
        reb,
        ast,
        stl,
        blk,
        tov,
        pf,
        pts,
    }
}
