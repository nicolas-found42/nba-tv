//! Rung-0 probe: the NBA free-tier classics catalog as a build-time-checked
//! static table (issue #24).
//!
//! Scope (research 04 §S0, re-verified in 15 §2.1): fans can access hundreds
//! of full Classic Games free with an NBA ID, including **all NBA Finals
//! series since 1990** ([NBA Help Center, "Classic Games and Original NBA
//! Content](NBA_HELP_URL), page stamp 2026-02-03). There is no published
//! public JSON API for the NBA App catalog itself, so this probe does no HTTP
//! at all: it matches Finals games against the [`FINALS_CATALOG`] table and
//! emits watch-elsewhere pointers. The NBA App stays an External Surface —
//! rows are pointers, never decoded bytes (CONTEXT.md vocabulary).
//!
//! URL shape: a per-game deep link rarely exists, so every entry points at
//! the sanctioned Watch surface ([`NBA_WATCH_URL`]) with a series note naming
//! the Finals matchup. Verified live 2026-09-08: `GET nba.com/watch` → 301 to
//! `/watch/featured`; `GET nba.com/watch/featured` → 200. The logged-out page
//! carries nav only (no per-game addressing), which is exactly why the
//! series-landing-plus-note shape is the honest one.
//!
//! Monthly re-enumeration (ladder v2 §3: "rung 0 re-enumerated monthly"): the
//! table carries a [`NBA_CATALOG_VERIFIED`] stamp; [`nba_catalog_due`] reports
//! when a stored rung-0 row is older than [`NBA_CATALOG_REFRESH_DAYS`], and
//! [`nba_rescan_hint`] encodes the same cadence in the shared [`RescanHint`]
//! shape. Refresh procedure: re-run the verification command below, update the
//! table, bump the stamp. The sweep's 90-day rescan window stays the outer
//! bound; this monthly cadence governs the catalog data itself.
//!
//! Verification command (a human re-runs this monthly; each run spends smoke
//! budget — at most 4 outbound requests, ≥5s apart, identifying UA, no media):
//!
//! ```sh
//! UA="nba-tv-catalog-smoke/1.0 (personal archive research)"
//! curl -sS -m 25 -A "$UA" -o /dev/null -w "%{http_code}\n" \
//!   https://support.watch.nba.com/hc/en-us/articles/28006218859415-Classic-Games-and-Original-NBA-Content
//! # wait ≥5s, then:
//! curl -sS -m 25 -A "$UA" -o /dev/null -w "%{http_code} -> %{redirect_url}\n" \
//!   https://www.nba.com/watch
//! # wait ≥5s, then:
//! curl -sS -m 25 -A "$UA" -o /dev/null -w "%{http_code}\n" \
//!   https://www.nba.com/watch/featured
//! ```
//!
//! Smoke log 2026-09-08 (3 requests, identifying UA, ≥5s apart, no media):
//!
//! 1. `GET` the Help Center article → **403** with `cf-mitigated: challenge`
//!    (Cloudflare browser challenge; non-browser clients cannot read the
//!    article body). Article scope therefore re-verified via research docs
//!    04/15, which read it 2026-09-08 ("hundreds of full Classic Games ...
//!    all NBA Finals series since 1990", stamp February 03, 2026).
//! 2. `GET https://www.nba.com/watch` → **301** to `/watch/featured`.
//! 3. `GET https://www.nba.com/watch/featured` → **200** (228 KB). Logged-out
//!    HTML shows nav only — no per-game deep links exist to point at.
//!
//! [`RescanHint`]: nbatv_ladder::RescanHint

use crate::politeness::PolitenessConfig;
use crate::probe::{GameContext, ProbeCandidate, ProbeOutcome, SourceProbe};
use nbatv_ladder::{RescanHint, YoutubeQuota};

/// Help Center page documenting the free-tier scope (all Finals series since
/// 1990). Behind a browser challenge for non-browser clients (see smoke log).
pub const NBA_HELP_URL: &str = "https://support.watch.nba.com/hc/en-us/articles/28006218859415-Classic-Games-and-Original-NBA-Content";

/// Sanctioned Watch surface every catalog entry points at (verified 200 on
/// 2026-09-08). Per-game deep links rarely exist; the series note in the
/// candidate carries the identity.
pub const NBA_WATCH_URL: &str = "https://www.nba.com/watch/featured";

/// Date the static table was last verified against the live surface
/// (`YYYY-MM-DD`). Bump on each monthly re-enumeration.
pub const NBA_CATALOG_VERIFIED: &str = "2026-09-08";

/// Rung-0 catalog re-enumeration cadence in days (ladder v2 §3: monthly).
pub const NBA_CATALOG_REFRESH_DAYS: u64 = 30;

/// One Finals series in the free tier: season (ending year) plus the two
/// finalists as Basketball-Reference slugs with display names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FinalsEntry {
    /// Season ending year, e.g. `1998` for the 1997-98 season.
    pub season: u16,
    /// First finalist (BR slug, e.g. `"CHI"`).
    pub team_a: &'static str,
    /// Second finalist (BR slug, e.g. `"UTA"`).
    pub team_b: &'static str,
    /// Display name for `team_a` (e.g. `"Chicago Bulls"`).
    pub team_a_name: &'static str,
    /// Display name for `team_b` (e.g. `"Utah Jazz"`).
    pub team_b_name: &'static str,
}

/// Every NBA Finals series since 1990 (free tier scope per the Help Center).
/// One entry per season ending year; re-enumerated monthly (see
/// [`NBA_CATALOG_VERIFIED`]).
pub const FINALS_CATALOG: &[FinalsEntry] = &[
    FinalsEntry {
        season: 1990,
        team_a: "DET",
        team_b: "POR",
        team_a_name: "Detroit Pistons",
        team_b_name: "Portland Trail Blazers",
    },
    FinalsEntry {
        season: 1991,
        team_a: "CHI",
        team_b: "LAL",
        team_a_name: "Chicago Bulls",
        team_b_name: "Los Angeles Lakers",
    },
    FinalsEntry {
        season: 1992,
        team_a: "CHI",
        team_b: "POR",
        team_a_name: "Chicago Bulls",
        team_b_name: "Portland Trail Blazers",
    },
    FinalsEntry {
        season: 1993,
        team_a: "CHI",
        team_b: "PHO",
        team_a_name: "Chicago Bulls",
        team_b_name: "Phoenix Suns",
    },
    FinalsEntry {
        season: 1994,
        team_a: "HOU",
        team_b: "NYK",
        team_a_name: "Houston Rockets",
        team_b_name: "New York Knicks",
    },
    FinalsEntry {
        season: 1995,
        team_a: "HOU",
        team_b: "ORL",
        team_a_name: "Houston Rockets",
        team_b_name: "Orlando Magic",
    },
    FinalsEntry {
        season: 1996,
        team_a: "CHI",
        team_b: "SEA",
        team_a_name: "Chicago Bulls",
        team_b_name: "Seattle SuperSonics",
    },
    FinalsEntry {
        season: 1997,
        team_a: "CHI",
        team_b: "UTA",
        team_a_name: "Chicago Bulls",
        team_b_name: "Utah Jazz",
    },
    FinalsEntry {
        season: 1998,
        team_a: "CHI",
        team_b: "UTA",
        team_a_name: "Chicago Bulls",
        team_b_name: "Utah Jazz",
    },
    FinalsEntry {
        season: 1999,
        team_a: "SAS",
        team_b: "NYK",
        team_a_name: "San Antonio Spurs",
        team_b_name: "New York Knicks",
    },
    FinalsEntry {
        season: 2000,
        team_a: "LAL",
        team_b: "IND",
        team_a_name: "Los Angeles Lakers",
        team_b_name: "Indiana Pacers",
    },
    FinalsEntry {
        season: 2001,
        team_a: "LAL",
        team_b: "PHI",
        team_a_name: "Los Angeles Lakers",
        team_b_name: "Philadelphia 76ers",
    },
    FinalsEntry {
        season: 2002,
        team_a: "LAL",
        team_b: "NJN",
        team_a_name: "Los Angeles Lakers",
        team_b_name: "New Jersey Nets",
    },
    FinalsEntry {
        season: 2003,
        team_a: "SAS",
        team_b: "NJN",
        team_a_name: "San Antonio Spurs",
        team_b_name: "New Jersey Nets",
    },
    FinalsEntry {
        season: 2004,
        team_a: "DET",
        team_b: "LAL",
        team_a_name: "Detroit Pistons",
        team_b_name: "Los Angeles Lakers",
    },
    FinalsEntry {
        season: 2005,
        team_a: "SAS",
        team_b: "DET",
        team_a_name: "San Antonio Spurs",
        team_b_name: "Detroit Pistons",
    },
    FinalsEntry {
        season: 2006,
        team_a: "MIA",
        team_b: "DAL",
        team_a_name: "Miami Heat",
        team_b_name: "Dallas Mavericks",
    },
    FinalsEntry {
        season: 2007,
        team_a: "SAS",
        team_b: "CLE",
        team_a_name: "San Antonio Spurs",
        team_b_name: "Cleveland Cavaliers",
    },
    FinalsEntry {
        season: 2008,
        team_a: "BOS",
        team_b: "LAL",
        team_a_name: "Boston Celtics",
        team_b_name: "Los Angeles Lakers",
    },
    FinalsEntry {
        season: 2009,
        team_a: "LAL",
        team_b: "ORL",
        team_a_name: "Los Angeles Lakers",
        team_b_name: "Orlando Magic",
    },
    FinalsEntry {
        season: 2010,
        team_a: "LAL",
        team_b: "BOS",
        team_a_name: "Los Angeles Lakers",
        team_b_name: "Boston Celtics",
    },
    FinalsEntry {
        season: 2011,
        team_a: "DAL",
        team_b: "MIA",
        team_a_name: "Dallas Mavericks",
        team_b_name: "Miami Heat",
    },
    FinalsEntry {
        season: 2012,
        team_a: "MIA",
        team_b: "OKC",
        team_a_name: "Miami Heat",
        team_b_name: "Oklahoma City Thunder",
    },
    FinalsEntry {
        season: 2013,
        team_a: "MIA",
        team_b: "SAS",
        team_a_name: "Miami Heat",
        team_b_name: "San Antonio Spurs",
    },
    FinalsEntry {
        season: 2014,
        team_a: "SAS",
        team_b: "MIA",
        team_a_name: "San Antonio Spurs",
        team_b_name: "Miami Heat",
    },
    FinalsEntry {
        season: 2015,
        team_a: "GSW",
        team_b: "CLE",
        team_a_name: "Golden State Warriors",
        team_b_name: "Cleveland Cavaliers",
    },
    FinalsEntry {
        season: 2016,
        team_a: "CLE",
        team_b: "GSW",
        team_a_name: "Cleveland Cavaliers",
        team_b_name: "Golden State Warriors",
    },
    FinalsEntry {
        season: 2017,
        team_a: "GSW",
        team_b: "CLE",
        team_a_name: "Golden State Warriors",
        team_b_name: "Cleveland Cavaliers",
    },
    FinalsEntry {
        season: 2018,
        team_a: "GSW",
        team_b: "CLE",
        team_a_name: "Golden State Warriors",
        team_b_name: "Cleveland Cavaliers",
    },
    FinalsEntry {
        season: 2019,
        team_a: "TOR",
        team_b: "GSW",
        team_a_name: "Toronto Raptors",
        team_b_name: "Golden State Warriors",
    },
    FinalsEntry {
        season: 2020,
        team_a: "LAL",
        team_b: "MIA",
        team_a_name: "Los Angeles Lakers",
        team_b_name: "Miami Heat",
    },
    FinalsEntry {
        season: 2021,
        team_a: "MIL",
        team_b: "PHO",
        team_a_name: "Milwaukee Bucks",
        team_b_name: "Phoenix Suns",
    },
    FinalsEntry {
        season: 2022,
        team_a: "GSW",
        team_b: "BOS",
        team_a_name: "Golden State Warriors",
        team_b_name: "Boston Celtics",
    },
    FinalsEntry {
        season: 2023,
        team_a: "DEN",
        team_b: "MIA",
        team_a_name: "Denver Nuggets",
        team_b_name: "Miami Heat",
    },
    FinalsEntry {
        season: 2024,
        team_a: "BOS",
        team_b: "DAL",
        team_a_name: "Boston Celtics",
        team_b_name: "Dallas Mavericks",
    },
    FinalsEntry {
        season: 2025,
        team_a: "OKC",
        team_b: "IND",
        team_a_name: "Oklahoma City Thunder",
        team_b_name: "Indiana Pacers",
    },
];

/// The catalog entry for a season ending year, if the free tier covers it.
pub fn finals_for_season(season: u16) -> Option<&'static FinalsEntry> {
    FINALS_CATALOG.iter().find(|entry| entry.season == season)
}

/// Season ending year for an ISO date: October+ tips into the next season's
/// campaign (e.g. `1997-11-01` → `1998`). `None` for unparseable dates.
pub fn season_for_date(date: &str) -> Option<u16> {
    let (year, month, _) = parse_ymd(date)?;
    if month >= 10 {
        u16::try_from(year + 1).ok()
    } else {
        u16::try_from(year).ok()
    }
}

/// Whether a calendar month can hold that season's Finals games: May–July
/// always (Game 1 has tipped into late May), plus the 2020 bubble anomaly
/// (Sept–Oct 2020). Regular season (Oct–Apr) and preseason never qualify, so
/// a regular-season game between two finalists cannot false-match.
pub fn finals_month_ok(season: u16, month: u32) -> bool {
    matches!(month, 5..=7) || (season == 2020 && matches!(month, 9..=10))
}

/// Monthly re-enumeration check: true when `queried_at` is older than
/// [`NBA_CATALOG_REFRESH_DAYS`] before `now` (`YYYY-MM-DD`, prefixes also
/// work). Unparseable stamps read as due (re-verify rather than trust);
/// future stamps read as fresh (clock skew never forces a re-probe).
pub fn nba_catalog_due(queried_at: &str, now: &str) -> bool {
    let (queried_days, now_days) = match (ymd_to_days(queried_at), ymd_to_days(now)) {
        (Some(q), Some(n)) => (q, n),
        _ => return true,
    };
    now_days >= queried_days && now_days - queried_days > NBA_CATALOG_REFRESH_DAYS as i64
}

/// Monthly rung-0 rescan hint in the shared [`RescanHint`] shape (30 days,
/// not the 90-day game default).
pub fn nba_rescan_hint(game_id: impl Into<String>) -> RescanHint {
    RescanHint {
        game_id: game_id.into(),
        not_before_days: NBA_CATALOG_REFRESH_DAYS,
    }
}

/// Find the Finals entry for one game: the season's finalists (falling back
/// one season so the October 2020 bubble Finals still resolve), gated on a
/// Finals-plausible month and on both teams being the two finalists.
fn find_finals(game: &GameContext) -> Option<&'static FinalsEntry> {
    let (_, month, _) = parse_ymd(&game.date)?;
    let season = season_for_date(&game.date)?;
    [season, season.saturating_sub(1)]
        .into_iter()
        .filter(|s| finals_month_ok(*s, month))
        .filter_map(finals_for_season)
        .find(|entry| teams_match(entry, &game.home_team, &game.away_team))
}

/// Both teams are the two finalists, order-insensitive (BR slugs compare
/// ASCII-case-insensitively; the archive stores them uppercase).
fn teams_match(entry: &FinalsEntry, home: &str, away: &str) -> bool {
    (entry.team_a.eq_ignore_ascii_case(home) && entry.team_b.eq_ignore_ascii_case(away))
        || (entry.team_a.eq_ignore_ascii_case(away) && entry.team_b.eq_ignore_ascii_case(home))
}

/// Recorded query text for a catalog hit.
fn query_text_hit(game: &GameContext, entry: &FinalsEntry) -> String {
    format!(
        "nba classics catalog: {} NBA Finals ({} vs {}) for {} @ {} {} \
         [static catalog verified {}]",
        entry.season,
        entry.team_a,
        entry.team_b,
        game.away_team,
        game.home_team,
        game.date,
        NBA_CATALOG_VERIFIED,
    )
}

/// Recorded query text for a miss: the rung was still swept, honestly empty.
fn query_text_miss(game: &GameContext) -> String {
    format!(
        "nba classics catalog: no Finals entry for {} @ {} {} \
         [static catalog verified {}]",
        game.away_team, game.home_team, game.date, NBA_CATALOG_VERIFIED,
    )
}

/// Watch-elsewhere candidate for a catalog hit: both finalists named (with
/// box-score slugs for the scorer's identity match), series landing URL plus
/// the find-it note. Deliberately no full-tape marker and no duration: a
/// series page is LIKELY evidence, never CONFIRMED.
fn candidate_for(entry: &FinalsEntry) -> ProbeCandidate {
    ProbeCandidate {
        url_or_pointer: NBA_WATCH_URL.to_owned(),
        title: format!(
            "{} NBA Finals: {} vs {} — NBA Classic Games (watch in NBA App)",
            entry.season, entry.team_a_name, entry.team_b_name,
        ),
        description: format!(
            "The {} NBA Finals series ({} vs {}, box-score slugs {} / {}) is \
             in the NBA Classic Games free tier with an NBA ID: NBA App or \
             site (Watch → Featured → Classic Games). Watch-elsewhere pointer: \
             per-game deep links rarely exist, so start at {} and find the \
             {} Finals series. Static catalog verified {}; re-enumerated monthly.",
            entry.season,
            entry.team_a_name,
            entry.team_b_name,
            entry.team_a,
            entry.team_b,
            NBA_WATCH_URL,
            entry.season,
            NBA_CATALOG_VERIFIED,
        ),
        duration_secs: None,
    }
}

/// `YYYY-MM-DD` (prefix) to days since the Unix epoch (Howard Hinnant's
/// days-from-civil; std-only, no date dependency).
fn ymd_to_days(s: &str) -> Option<i64> {
    let (year, month, day) = parse_ymd(s)?;
    Some(days_from_civil(
        i64::from(year),
        i64::from(month),
        i64::from(day),
    ))
}

/// Split the `YYYY-MM-DD` prefix; `None` when the shape or ranges are wrong.
fn parse_ymd(s: &str) -> Option<(i32, u32, u32)> {
    let bytes = s.as_bytes();
    if bytes.len() < 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    let year: i32 = s[0..4].parse().ok()?;
    let month: u32 = s[5..7].parse().ok()?;
    let day: u32 = s[8..10].parse().ok()?;
    if year <= 0 || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    Some((year, month, day))
}

fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// The rung-0 probe: static Finals catalog, no network, no quota.
#[derive(Debug, Clone, Copy, Default)]
pub struct NbaProbe;

impl NbaProbe {
    /// Stateless probe over the static [`FINALS_CATALOG`].
    pub fn new() -> Self {
        Self
    }
}

impl SourceProbe for NbaProbe {
    fn rung(&self) -> u8 {
        0
    }

    fn name(&self) -> &'static str {
        "official-nba-free-tier"
    }

    fn probe(
        &self,
        game: &GameContext,
        _politeness: &PolitenessConfig,
        _quota: &mut YoutubeQuota,
    ) -> ProbeOutcome {
        match find_finals(game) {
            Some(entry) => {
                ProbeOutcome::found(query_text_hit(game, entry), vec![candidate_for(entry)])
            }
            None => ProbeOutcome::empty(query_text_miss(game)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scorer::score_candidate;
    use nbatv_ladder::exhaustion::MatchLevel;

    fn game(home: &str, away: &str, date: &str) -> GameContext {
        GameContext {
            game_id: format!("{}{home}", date.replace('-', "")),
            home_team: home.to_owned(),
            away_team: away.to_owned(),
            date: date.to_owned(),
        }
    }

    fn probe(game: &GameContext) -> ProbeOutcome {
        NbaProbe::new().probe(game, &PolitenessConfig::default(), &mut YoutubeQuota::new())
    }

    #[test]
    fn catalog_covers_every_finals_since_1990_without_gaps() {
        let seasons: Vec<u16> = FINALS_CATALOG.iter().map(|e| e.season).collect();
        let expected: Vec<u16> = (1990u16..=2025).collect();
        assert_eq!(seasons, expected, "one entry per Finals series 1990–2025");
        for entry in FINALS_CATALOG {
            for slug in [entry.team_a, entry.team_b] {
                assert_eq!(slug.len(), 3, "BR slug shape for {slug}");
                assert!(slug.bytes().all(|b| b.is_ascii_uppercase()));
            }
            assert_ne!(entry.team_a, entry.team_b);
        }
    }

    #[test]
    fn finals_game_surfaces_rung0_watch_elsewhere_candidate() {
        // 1998 Finals G6, Bulls at Jazz.
        let outcome = probe(&game("UTA", "CHI", "1998-06-14"));
        assert!(!outcome.deferred);
        assert_eq!(outcome.candidates.len(), 1);
        let candidate = &outcome.candidates[0];
        assert_eq!(candidate.url_or_pointer, NBA_WATCH_URL);
        assert!(
            score_candidate(&game("UTA", "CHI", "1998-06-14"), candidate) == MatchLevel::Likely,
            "series page names both teams, claims no full-tape evidence: LIKELY, not CONFIRMED"
        );
    }

    #[test]
    fn regular_season_game_between_finalists_does_not_match() {
        let outcome = probe(&game("CHI", "UTA", "1997-11-01"));
        assert!(!outcome.deferred);
        assert!(
            outcome.candidates.is_empty(),
            "November is never the Finals"
        );
        assert!(
            !outcome.query_text.is_empty(),
            "misses still record the query"
        );
    }

    #[test]
    fn non_finalist_playoff_game_does_not_match() {
        // 1998 ECF: Pacers at Bulls — only one 1998 finalist.
        let outcome = probe(&game("CHI", "IND", "1998-05-25"));
        assert!(outcome.candidates.is_empty());
    }

    #[test]
    fn pre_1990_finals_are_out_of_scope() {
        // 1989 Finals rematch date, but the free tier starts at 1990.
        let outcome = probe(&game("DET", "LAL", "1989-06-13"));
        assert!(outcome.candidates.is_empty());
    }

    #[test]
    fn bubble_finals_in_october_2020_match() {
        // 2020 Finals G6, Lakers vs Heat in the bubble.
        let outcome = probe(&game("MIA", "LAL", "2020-10-11"));
        assert_eq!(outcome.candidates.len(), 1);
    }

    #[test]
    fn october_preseason_between_finalists_does_not_match() {
        let outcome = probe(&game("CHI", "UTA", "1997-10-15"));
        assert!(outcome.candidates.is_empty());
    }

    #[test]
    fn malformed_date_is_an_empty_miss_never_a_panic() {
        let outcome = probe(&game("CHI", "UTA", "not-a-date"));
        assert!(!outcome.deferred);
        assert!(outcome.candidates.is_empty());
    }

    #[test]
    fn rung_and_name_match_the_ladder() {
        let probe = NbaProbe::new();
        assert_eq!(probe.rung(), 0);
        assert_eq!(probe.name(), "official-nba-free-tier");
    }

    #[test]
    fn probe_spends_no_youtube_quota() {
        let g = game("UTA", "CHI", "1998-06-14");
        let mut quota = YoutubeQuota::new();
        let before = quota.remaining();
        NbaProbe::new().probe(&g, &PolitenessConfig::default(), &mut quota);
        assert_eq!(quota.remaining(), before);
    }

    #[test]
    fn query_text_names_the_method_and_catalog_stamp() {
        let hit = probe(&game("UTA", "CHI", "1998-06-14"));
        assert!(hit.query_text.contains("1998"), "{hit:?}");
        assert!(hit.query_text.contains(NBA_CATALOG_VERIFIED), "{hit:?}");
        let miss = probe(&game("TRH", "NYK", "1946-11-01"));
        assert!(miss.query_text.contains(NBA_CATALOG_VERIFIED), "{miss:?}");
    }

    #[test]
    fn monthly_due_honors_the_refresh_cadence() {
        assert!(!nba_catalog_due("2026-01-01", "2026-01-01"), "same day");
        assert!(
            !nba_catalog_due("2026-01-01", "2026-01-31"),
            "30 days is fresh"
        );
        assert!(
            nba_catalog_due("2026-01-01", "2026-02-01"),
            "31 days is due"
        );
        assert!(nba_catalog_due("2025-06-01", "2026-09-08"), "stale catalog");
        assert!(
            !nba_catalog_due("2026-09-08", "2026-01-01"),
            "future stamp stays fresh"
        );
        assert!(
            nba_catalog_due("unparseable", "2026-09-08"),
            "unparseable reads as due"
        );
    }

    #[test]
    fn rescan_hint_is_monthly_not_the_90_day_default() {
        let hint = nba_rescan_hint("19980614UTA");
        assert_eq!(hint.game_id, "19980614UTA");
        assert_eq!(hint.not_before_days, NBA_CATALOG_REFRESH_DAYS);
        assert_eq!(hint.not_before_days, 30);
    }

    #[test]
    fn sweep_of_a_finals_game_writes_a_rung0_tape_row() {
        let conn = nbatv_db::open_in_memory().unwrap();
        nbatv_db::create_schema(&conn).unwrap();
        let g = game("UTA", "CHI", "1998-06-14");
        let probe = NbaProbe::new();
        let mut registry = crate::probe::ProbeRegistry::new();
        registry.register(&probe);
        let report = crate::sweep::sweep_game(
            &conn,
            &g,
            &registry,
            &PolitenessConfig::default(),
            &mut YoutubeQuota::new(),
            "2026-09-08",
        )
        .unwrap();
        assert_eq!(report.probed, vec![0], "LIKELY at rung 0 stops the ascent");
        assert_eq!(
            report.status,
            nbatv_ladder::SweepStatus::Playable { rank: 0 }
        );
        let tapes = nbatv_db::tape_sources_for(&conn, &g.game_id).unwrap();
        assert_eq!(tapes.len(), 1);
        assert_eq!(tapes[0].rank, 0);
        assert_eq!(tapes[0].source_class, nbatv_db::SourceClass::Official);
        assert_eq!(tapes[0].url_or_pointer, NBA_WATCH_URL);
        let queries = nbatv_db::game_queries_for(&conn, &g.game_id).unwrap();
        assert_eq!(queries.len(), 1);
        assert_eq!(queries[0].best_match_level, "likely");
    }
}
