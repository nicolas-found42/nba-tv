//! Shell domain model: Season, Team, Game, Schedule views, Box Score,
//! Game Tape state, and the local Tape Source / Source Ladder shapes.
//!
//! Vocabulary follows the repo glossary (CONTEXT.md). Box Score is never
//! unavailable — only Game Tape is — so every stat field that an era did
//! not record is `None` and renders as `—`, while tape availability lives
//! exclusively in [`TapeState`].
//!
//! The [`TapeSource`] / [`PlaybackClass`] shapes mirror the workspace
//! contract (ladder rungs 0–7); integration dedups onto `nbatv_db` later.

use std::fmt::Display;

/// Format one Box Score cell: a recorded value, or `—` when the era did
/// not record it (`None`). Never invent a zero for unrecorded data.
pub fn cell<T: Display>(value: Option<T>) -> String {
    match value {
        Some(v) => v.to_string(),
        None => "—".to_string(),
    }
}

/// Regular season or Playoffs. A Team's Schedule is a view over the Season
/// Schedule, split into these two ledgers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameType {
    Regular,
    Playoffs,
}

/// Availability of the Game Tape for one Game.
///
/// This is the ONLY signal that may render a Game as tape-unavailable.
/// Box Score presence must never influence it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TapeState {
    /// Tape resolves to something watchable right now.
    Playable,
    /// Ladder sweep still in flight; may become playable.
    Sweeping,
    /// Sweep exhausted; no known tape. Box Score still renders.
    Unavailable,
    /// Tape exists but is pointer-only (catalog / purchase / institutional).
    Pointer,
}

impl TapeState {
    /// Agate-ledger glyph (matches the browse-to-watch prototype, variant A).
    pub fn glyph(self) -> char {
        match self {
            TapeState::Playable => '▶',
            TapeState::Sweeping => '◌',
            TapeState::Unavailable => '∅',
            TapeState::Pointer => '⧉',
        }
    }

    /// Banner line for the Game view.
    pub fn banner(self) -> &'static str {
        match self {
            TapeState::Playable => "Game Tape: playable — watch in the Player Backend.",
            TapeState::Sweeping => "Game Tape: sweeping sources — check back later.",
            TapeState::Unavailable => {
                "Game Tape: unavailable — no known tape. Box Score below is complete."
            }
            TapeState::Pointer => {
                "Game Tape: pointer only — this entry names where tape lives; nothing plays in-window."
            }
        }
    }
}

/// How a resolved tape plays. Determined by Source Ladder rung.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlaybackClass {
    ProgressiveFile,
    ExternalSurface,
    Pointer,
}

/// Map a Source Ladder rung (0–7) to its [`PlaybackClass`]:
/// rungs 1+4 play as progressive files, 0+2+3 on an External Surface,
/// 5+6+7 are pointer-only. Unknown rungs return `None`.
pub fn playback_class_for_rank(rank: u8) -> Option<PlaybackClass> {
    match rank {
        1 | 4 => Some(PlaybackClass::ProgressiveFile),
        0 | 2 | 3 => Some(PlaybackClass::ExternalSurface),
        5 | 6 | 7 => Some(PlaybackClass::Pointer),
        _ => None,
    }
}

/// One place that holds Game Tape. Grey sources are pointers only;
/// this struct never triggers a download.
#[derive(Clone, Debug, PartialEq)]
pub struct TapeSource {
    pub game_id: String,
    pub rank: u8,
    pub source_class: String,
    pub url_or_pointer: String,
    pub match_confidence: f32,
    pub verified_at: String,
}

impl TapeSource {
    pub fn playback_class(&self) -> Option<PlaybackClass> {
        playback_class_for_rank(self.rank)
    }
}

/// One year of games, from first game to Finals.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Season {
    /// URL slug, e.g. `1946-47`.
    pub slug: String,
    /// Display label, e.g. `1946–47`.
    pub label: String,
    pub league: String,
}

/// One club, active or defunct.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Team {
    /// Basketball-Reference slug, e.g. `TRH`.
    pub br_slug: String,
    pub city: String,
    pub name: String,
    pub abbrev: String,
    /// First season-start year (e.g. 1946 for 1946–47).
    pub active_from: u16,
    /// Last season-start year, or `None` while still active.
    pub active_to: Option<u16>,
}

impl Team {
    pub fn display_name(&self) -> String {
        format!("{} {}", self.city, self.name)
    }

    /// True when the club no longer plays.
    pub fn is_defunct(&self) -> bool {
        self.active_to.is_some()
    }

    /// True when the club's span covers the season starting in `year`.
    pub fn active_in(&self, year: u16) -> bool {
        self.active_from <= year && self.active_to.map_or(true, |end| year <= end)
    }

    /// Dashboard tag: `"defunct"` or `"active"`.
    pub fn status_tag(&self) -> &'static str {
        if self.is_defunct() {
            "defunct"
        } else {
            "active"
        }
    }
}

/// One contest between two teams on one date (NBA/BAA only).
#[derive(Clone, Debug, PartialEq)]
pub struct Game {
    /// Basketball-Reference box-score slug, e.g. `194611010TRH`.
    pub game_id: String,
    /// Season slug, e.g. `1946-47`.
    pub season: String,
    pub date: String,
    pub game_type: GameType,
    pub home_team: String,
    pub away_team: String,
    pub home_pts: u32,
    pub away_pts: u32,
    pub tape: TapeState,
    pub sources: Vec<TapeSource>,
}

impl Game {
    /// One-line ledger label, e.g. `NYK @ TRH · 1946-11-01`.
    pub fn label(&self) -> String {
        format!("{} @ {} · {}", self.away_team, self.home_team, self.date)
    }

    /// Short scoreline, e.g. `NYK 68 — TRH 66`.
    pub fn scoreline(&self) -> String {
        format!(
            "{} {} — {} {}",
            self.away_team, self.away_pts, self.home_team, self.home_pts
        )
    }
}

/// Team totals for one Game. Non-`Option` columns were always recorded;
/// `Option` columns are `None` where the era did not record them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoxTeam {
    pub game_id: String,
    pub team_br: String,
    pub mp: Option<u32>,
    pub fg: u32,
    pub fga: u32,
    pub ft: u32,
    pub fta: u32,
    pub oreb: Option<u32>,
    pub dreb: Option<u32>,
    pub reb: Option<u32>,
    pub ast: Option<u32>,
    pub stl: Option<u32>,
    pub blk: Option<u32>,
    pub pf: u32,
    pub pts: u32,
    pub plus_minus: Option<i32>,
}

/// Player totals for one Game. A DNP row carries `dnp_reason` and `None`
/// stats, which render as `—`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoxPlayer {
    pub game_id: String,
    pub team_br: String,
    pub player_br: String,
    pub player_name: String,
    pub starter: Option<bool>,
    pub position: Option<String>,
    pub mp: Option<u32>,
    pub fg: Option<u32>,
    pub fga: Option<u32>,
    pub ft: Option<u32>,
    pub fta: Option<u32>,
    pub pts: Option<u32>,
    pub plus_minus: Option<i32>,
    pub dnp_reason: Option<String>,
}

/// The recorded result of one Game: team totals plus player totals.
/// Always present, even when the Game Tape is unavailable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoxScore {
    pub game_id: String,
    pub teams: Vec<BoxTeam>,
    pub players: Vec<BoxPlayer>,
}

impl BoxScore {
    pub fn for_team(&self, team_br: &str) -> Vec<&BoxPlayer> {
        self.players
            .iter()
            .filter(|p| p.team_br == team_br)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_cells_render_em_dash() {
        assert_eq!(cell(None::<u32>), "—");
        assert_eq!(cell(Option::<u32>::None), "—");
        assert_eq!(cell(None::<String>), "—");
    }

    #[test]
    fn recorded_cells_render_value() {
        assert_eq!(cell(Some(68u32)), "68");
        assert_eq!(cell(Some(-4i32)), "-4");
        assert_eq!(cell(Some("TRH".to_string())), "TRH");
    }

    #[test]
    fn zero_is_recorded_not_missing() {
        // A recorded zero must render as `0`, never as `—`.
        assert_eq!(cell(Some(0u32)), "0");
    }

    #[test]
    fn tape_glyphs_are_distinct() {
        let glyphs = [
            TapeState::Playable.glyph(),
            TapeState::Sweeping.glyph(),
            TapeState::Unavailable.glyph(),
            TapeState::Pointer.glyph(),
        ];
        assert_eq!(glyphs, ['▶', '◌', '∅', '⧉']);
    }

    #[test]
    fn tape_banners_mention_state() {
        assert!(TapeState::Playable.banner().contains("playable"));
        assert!(TapeState::Sweeping.banner().contains("sweeping"));
        assert!(TapeState::Unavailable.banner().contains("unavailable"));
        assert!(TapeState::Pointer.banner().contains("pointer"));
        // The unavailable banner must reassure that the Box Score survives.
        assert!(TapeState::Unavailable.banner().contains("Box Score"));
    }

    #[test]
    fn ladder_rungs_map_to_playback_class() {
        assert_eq!(
            playback_class_for_rank(0),
            Some(PlaybackClass::ExternalSurface)
        );
        assert_eq!(
            playback_class_for_rank(1),
            Some(PlaybackClass::ProgressiveFile)
        );
        assert_eq!(
            playback_class_for_rank(2),
            Some(PlaybackClass::ExternalSurface)
        );
        assert_eq!(
            playback_class_for_rank(3),
            Some(PlaybackClass::ExternalSurface)
        );
        assert_eq!(
            playback_class_for_rank(4),
            Some(PlaybackClass::ProgressiveFile)
        );
        assert_eq!(playback_class_for_rank(5), Some(PlaybackClass::Pointer));
        assert_eq!(playback_class_for_rank(6), Some(PlaybackClass::Pointer));
        assert_eq!(playback_class_for_rank(7), Some(PlaybackClass::Pointer));
        assert_eq!(playback_class_for_rank(8), None);
        assert_eq!(playback_class_for_rank(255), None);
    }

    #[test]
    fn defunct_span_covers_only_its_seasons() {
        let huskies = Team {
            br_slug: "TRH".into(),
            city: "Toronto".into(),
            name: "Huskies".into(),
            abbrev: "TRH".into(),
            active_from: 1946,
            active_to: Some(1946),
        };
        assert!(huskies.is_defunct());
        assert_eq!(huskies.status_tag(), "defunct");
        assert!(huskies.active_in(1946));
        assert!(!huskies.active_in(1947));
    }
}
