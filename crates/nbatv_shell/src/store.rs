//! Fixture store for tests and offline development (no network, no media).
//!
//! 2 seasons, 3 teams (1 defunct), 4 games covering all four tape states.
//! The live Shell renders from the archive database through
//! [`crate::db_store::DbStore`] instead — this store seeds nothing in the
//! live path; [`FixtureStore::fixture`] exists so view logic stays
//! unit-testable without a database.

use crate::model::{
    BoxPlayer, BoxScore, BoxTeam, Game, GameType, Season, TapeSource, TapeState, Team,
};
use crate::route::Route;
use nbatv_db::{GameId, SourceClass};

/// Seeded/playable counts for a Season dashboard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SeasonCounts {
    /// Games on the Schedule (Box Score is always seeded).
    pub seeded: usize,
    /// Games whose tape is playable right now.
    pub playable: usize,
}

/// One ⌘K palette hit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaletteItem {
    pub kind: PaletteKind,
    pub label: String,
    pub detail: String,
    pub route: Route,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaletteKind {
    Season,
    Team,
    Game,
}

/// Fixed fixtures for tests and offline development. The live path reads
/// the archive database instead (see [`crate::db_store::DbStore`]).
#[derive(Clone, Debug)]
pub struct FixtureStore {
    seasons: Vec<Season>,
    teams: Vec<Team>,
    games: Vec<Game>,
    boxes: Vec<BoxScore>,
}

impl FixtureStore {
    /// The canonical fixture set.
    pub fn fixture() -> Self {
        Self {
            seasons: vec![
                Season {
                    slug: "1946-47".to_string(),
                    label: "1946–47".to_string(),
                    league: "BAA".to_string(),
                },
                Season {
                    slug: "1947-48".to_string(),
                    label: "1947–48".to_string(),
                    league: "BAA".to_string(),
                },
            ],
            teams: vec![
                Team {
                    br_slug: "BOS".to_string(),
                    city: "Boston".to_string(),
                    name: "Celtics".to_string(),
                    abbrev: "BOS".to_string(),
                    active_from: 1946,
                    active_to: None,
                },
                Team {
                    br_slug: "NYK".to_string(),
                    city: "New York".to_string(),
                    name: "Knicks".to_string(),
                    abbrev: "NYK".to_string(),
                    active_from: 1946,
                    active_to: None,
                },
                Team {
                    br_slug: "TRH".to_string(),
                    city: "Toronto".to_string(),
                    name: "Huskies".to_string(),
                    abbrev: "TRH".to_string(),
                    active_from: 1946,
                    active_to: Some(1946),
                },
            ],
            games: vec![
                Game {
                    game_id: "194611010TRH".to_string(),
                    season: "1946-47".to_string(),
                    date: "1946-11-01".to_string(),
                    game_type: GameType::Regular,
                    home_team: "TRH".to_string(),
                    away_team: "NYK".to_string(),
                    home_pts: 66,
                    away_pts: 68,
                    tape: TapeState::Playable,
                    sources: vec![TapeSource {
                        game_id: GameId("194611010TRH".to_string()),
                        rank: 1,
                        source_class: SourceClass::InternetArchive,
                        url_or_pointer: "https://archive.org/details/194611010TRH".to_string(),
                        match_confidence: 0.9,
                        verified_at: "2026-01-01".to_string(),
                    }],
                },
                Game {
                    game_id: "194612070BOS".to_string(),
                    season: "1946-47".to_string(),
                    date: "1946-12-07".to_string(),
                    game_type: GameType::Regular,
                    home_team: "BOS".to_string(),
                    away_team: "NYK".to_string(),
                    home_pts: 55,
                    away_pts: 49,
                    tape: TapeState::Sweeping,
                    sources: vec![TapeSource {
                        game_id: GameId("194612070BOS".to_string()),
                        rank: 2,
                        source_class: SourceClass::YouTube,
                        url_or_pointer: "https://www.youtube.com/watch?v=fixture-sweep".to_string(),
                        match_confidence: 0.4,
                        verified_at: String::new(),
                    }],
                },
                Game {
                    game_id: "194704160BOS".to_string(),
                    season: "1946-47".to_string(),
                    date: "1947-04-16".to_string(),
                    game_type: GameType::Playoffs,
                    home_team: "BOS".to_string(),
                    away_team: "NYK".to_string(),
                    home_pts: 60,
                    away_pts: 58,
                    tape: TapeState::Unavailable,
                    sources: vec![],
                },
                Game {
                    game_id: "194711150BOS".to_string(),
                    season: "1947-48".to_string(),
                    date: "1947-11-15".to_string(),
                    game_type: GameType::Regular,
                    home_team: "BOS".to_string(),
                    away_team: "NYK".to_string(),
                    home_pts: 70,
                    away_pts: 65,
                    tape: TapeState::Pointer,
                    sources: vec![TapeSource {
                        game_id: GameId("194711150BOS".to_string()),
                        rank: 6,
                        source_class: SourceClass::Purchase,
                        url_or_pointer: "Catalog ref FTE-194711150BOS (pointer only)".to_string(),
                        match_confidence: 1.0,
                        verified_at: "2026-02-01".to_string(),
                    }],
                },
            ],
            boxes: vec![
                box_score(
                    "194611010TRH",
                    box_team(BoxTeamInput {
                        game_id: "194611010TRH",
                        team_br: "NYK",
                        fg: Some(24),
                        fga: Some(28),
                        ft: Some(20),
                        fta: Some(25),
                        pf: Some(14),
                        pts: Some(68),
                    }),
                    box_team(BoxTeamInput {
                        game_id: "194611010TRH",
                        team_br: "TRH",
                        fg: Some(22),
                        fga: None,
                        ft: Some(22),
                        fta: Some(27),
                        pf: Some(16),
                        pts: Some(66),
                    }),
                    vec![
                        box_player(BoxPlayerInput {
                            game_id: "194611010TRH",
                            team_br: "NYK",
                            player_br: "ostrank01",
                            player_name: "Leo Ostransky",
                            starter: true,
                            position: "F",
                            mp: Some(40),
                            fg: Some(6),
                            fga: Some(14),
                            ft: Some(4),
                            fta: Some(6),
                            pts: Some(16),
                            plus_minus: None,
                        }),
                        box_player(BoxPlayerInput {
                            game_id: "194611010TRH",
                            team_br: "NYK",
                            player_br: "sadowsk01",
                            player_name: "Ed Sadowski",
                            starter: true,
                            position: "C",
                            mp: Some(38),
                            fg: Some(7),
                            fga: Some(15),
                            ft: Some(5),
                            fta: Some(7),
                            pts: Some(19),
                            plus_minus: None,
                        }),
                        box_player(BoxPlayerInput {
                            game_id: "194611010TRH",
                            team_br: "TRH",
                            player_br: "shewch01",
                            player_name: "Mike Shewchuk",
                            starter: true,
                            position: "G",
                            mp: Some(40),
                            fg: Some(8),
                            fga: Some(18),
                            ft: Some(6),
                            fta: Some(8),
                            pts: Some(22),
                            plus_minus: None,
                        }),
                        box_player_dnp(
                            "194611010TRH",
                            "TRH",
                            "fit team01",
                            "Sam Teammate",
                            "Coach's decision",
                        ),
                    ],
                ),
                box_score(
                    "194612070BOS",
                    box_team(BoxTeamInput {
                        game_id: "194612070BOS",
                        team_br: "NYK",
                        fg: Some(20),
                        fga: Some(26),
                        ft: Some(9),
                        fta: Some(12),
                        pf: Some(12),
                        pts: Some(49),
                    }),
                    box_team(BoxTeamInput {
                        game_id: "194612070BOS",
                        team_br: "BOS",
                        fg: Some(23),
                        fga: Some(29),
                        ft: Some(9),
                        fta: Some(14),
                        pf: Some(15),
                        pts: Some(55),
                    }),
                    vec![
                        box_player(BoxPlayerInput {
                            game_id: "194612070BOS",
                            team_br: "NYK",
                            player_br: "ostrank01",
                            player_name: "Leo Ostransky",
                            starter: true,
                            position: "F",
                            mp: Some(40),
                            fg: Some(5),
                            fga: Some(12),
                            ft: Some(2),
                            fta: Some(4),
                            pts: Some(12),
                            plus_minus: None,
                        }),
                        box_player(BoxPlayerInput {
                            game_id: "194612070BOS",
                            team_br: "BOS",
                            player_br: "sadowsk01",
                            player_name: "Ed Sadowski",
                            starter: true,
                            position: "C",
                            mp: Some(39),
                            fg: Some(8),
                            fga: Some(16),
                            ft: Some(4),
                            fta: Some(6),
                            pts: Some(20),
                            plus_minus: None,
                        }),
                    ],
                ),
                box_score(
                    "194704160BOS",
                    box_team(BoxTeamInput {
                        game_id: "194704160BOS",
                        team_br: "NYK",
                        fg: Some(21),
                        fga: Some(30),
                        ft: Some(16),
                        fta: Some(20),
                        pf: Some(18),
                        pts: Some(58),
                    }),
                    box_team(BoxTeamInput {
                        game_id: "194704160BOS",
                        team_br: "BOS",
                        fg: Some(24),
                        fga: Some(32),
                        ft: Some(12),
                        fta: Some(16),
                        pf: Some(14),
                        pts: Some(60),
                    }),
                    vec![
                        box_player(BoxPlayerInput {
                            game_id: "194704160BOS",
                            team_br: "NYK",
                            player_br: "ostrank01",
                            player_name: "Leo Ostransky",
                            starter: true,
                            position: "F",
                            mp: None,
                            fg: Some(6),
                            fga: Some(14),
                            ft: Some(3),
                            fta: Some(5),
                            pts: Some(15),
                            plus_minus: None,
                        }),
                        box_player(BoxPlayerInput {
                            game_id: "194704160BOS",
                            team_br: "BOS",
                            player_br: "sadowsk01",
                            player_name: "Ed Sadowski",
                            starter: true,
                            position: "C",
                            mp: None,
                            fg: Some(7),
                            fga: Some(15),
                            ft: Some(4),
                            fta: Some(6),
                            pts: Some(18),
                            plus_minus: None,
                        }),
                    ],
                ),
                box_score(
                    "194711150BOS",
                    box_team(BoxTeamInput {
                        game_id: "194711150BOS",
                        team_br: "NYK",
                        fg: Some(25),
                        fga: Some(34),
                        ft: Some(15),
                        fta: Some(19),
                        pf: Some(17),
                        pts: Some(65),
                    }),
                    box_team(BoxTeamInput {
                        game_id: "194711150BOS",
                        team_br: "BOS",
                        fg: Some(27),
                        fga: Some(35),
                        ft: Some(16),
                        fta: Some(20),
                        pf: Some(13),
                        pts: Some(70),
                    }),
                    vec![
                        box_player(BoxPlayerInput {
                            game_id: "194711150BOS",
                            team_br: "NYK",
                            player_br: "ostrank01",
                            player_name: "Leo Ostransky",
                            starter: true,
                            position: "F",
                            mp: Some(40),
                            fg: Some(7),
                            fga: Some(16),
                            ft: Some(4),
                            fta: Some(5),
                            pts: Some(18),
                            plus_minus: None,
                        }),
                        box_player(BoxPlayerInput {
                            game_id: "194711150BOS",
                            team_br: "BOS",
                            player_br: "sadowsk01",
                            player_name: "Ed Sadowski",
                            starter: true,
                            position: "C",
                            mp: Some(41),
                            fg: Some(9),
                            fga: Some(18),
                            ft: Some(5),
                            fta: Some(7),
                            pts: Some(23),
                            plus_minus: None,
                        }),
                    ],
                ),
            ],
        }
    }

    pub fn seasons(&self) -> &[Season] {
        &self.seasons
    }

    pub fn season_label(&self, slug: &str) -> Option<String> {
        self.seasons
            .iter()
            .find(|s| s.slug == slug)
            .map(|s| s.label.clone())
    }

    pub fn team(&self, slug: &str) -> Option<&Team> {
        self.teams.iter().find(|t| t.br_slug == slug)
    }

    pub fn team_name(&self, slug: &str) -> Option<String> {
        self.team(slug).map(|t| t.display_name())
    }

    pub fn game(&self, game_id: &str) -> Option<&Game> {
        self.games.iter().find(|g| g.game_id == game_id)
    }

    pub fn games(&self) -> &[Game] {
        &self.games
    }

    /// Clubs on a Season dashboard: every club whose span covers the
    /// season's start year, in slug order.
    pub fn teams_for_season(&self, season: &str) -> Vec<&Team> {
        let year: u16 = season
            .split('-')
            .next()
            .and_then(|y| y.parse().ok())
            .unwrap_or(0);
        let mut teams: Vec<&Team> = self.teams.iter().filter(|t| t.active_in(year)).collect();
        teams.sort_by(|a, b| a.br_slug.cmp(&b.br_slug));
        teams
    }

    /// Dashboard filter box: substring over city, name, slug, abbrev
    /// (case-insensitive). Empty query returns the whole club grid.
    pub fn filter_teams(&self, season: &str, query: &str) -> Vec<&Team> {
        let q = query.trim().to_lowercase();
        self.teams_for_season(season)
            .into_iter()
            .filter(|t| {
                q.is_empty()
                    || t.city.to_lowercase().contains(&q)
                    || t.name.to_lowercase().contains(&q)
                    || t.br_slug.to_lowercase().contains(&q)
                    || t.abbrev.to_lowercase().contains(&q)
            })
            .collect()
    }

    /// All games of one Season's Schedule, in date order.
    pub fn games_for_season(&self, season: &str) -> Vec<&Game> {
        let mut games: Vec<&Game> = self.games.iter().filter(|g| g.season == season).collect();
        games.sort_by(|a, b| a.date.cmp(&b.date).then(a.game_id.cmp(&b.game_id)));
        games
    }

    /// One Team's games as a view over the Season Schedule, split into
    /// the Regular-season ledger and the Playoffs ledger.
    pub fn team_games(&self, season: &str, team: &str) -> (Vec<&Game>, Vec<&Game>) {
        let mut regular = Vec::new();
        let mut playoffs = Vec::new();
        for game in self.games_for_season(season) {
            if game.home_team != team && game.away_team != team {
                continue;
            }
            match game.game_type {
                GameType::Regular => regular.push(game),
                GameType::Playoffs => playoffs.push(game),
            }
        }
        (regular, playoffs)
    }

    /// Seeded + playable counts for a Season dashboard.
    pub fn season_counts(&self, season: &str) -> SeasonCounts {
        let games = self.games_for_season(season);
        SeasonCounts {
            seeded: games.len(),
            playable: games
                .iter()
                .filter(|g| g.tape == TapeState::Playable)
                .count(),
        }
    }

    /// Box Score for a Game. Always present — even when tape is unavailable.
    pub fn box_for(&self, game_id: &str) -> Option<&BoxScore> {
        self.boxes.iter().find(|b| b.game_id == game_id)
    }

    /// ⌘K palette: substring over seasons, teams, and games.
    /// Empty query lists everything (fixtures are tiny).
    pub fn palette_search(&self, query: &str) -> Vec<PaletteItem> {
        let q = query.trim().to_lowercase();
        let mut out = Vec::new();
        let hit = |hay: &str| q.is_empty() || hay.to_lowercase().contains(&q);
        for season in &self.seasons {
            if hit(&format!(
                "{} {} {}",
                season.slug, season.label, season.league
            )) {
                out.push(PaletteItem {
                    kind: PaletteKind::Season,
                    label: format!("Season {}", season.label),
                    detail: season.league.clone(),
                    route: Route::Season {
                        season: season.slug.clone(),
                    },
                });
            }
        }
        for team in &self.teams {
            if hit(&format!(
                "{} {} {} {}",
                team.city, team.name, team.br_slug, team.abbrev
            )) {
                out.push(PaletteItem {
                    kind: PaletteKind::Team,
                    label: team.display_name(),
                    detail: team.status_tag().to_string(),
                    route: Route::Team {
                        // Jump to the club's first season.
                        season: self.first_season_for(team),
                        team: team.br_slug.clone(),
                    },
                });
            }
        }
        for game in &self.games {
            if hit(&format!(
                "{} {} {} {}",
                game.label(),
                game.game_id,
                game.season,
                game.scoreline()
            )) {
                out.push(PaletteItem {
                    kind: PaletteKind::Game,
                    label: format!("{} — {}", game.date, game.label()),
                    detail: format!("{} {}", game.scoreline(), game.tape.glyph()),
                    route: Route::Game {
                        game_id: game.game_id.clone(),
                    },
                });
            }
        }
        out
    }

    fn first_season_for(&self, team: &Team) -> String {
        self.seasons
            .iter()
            .find(|s| {
                s.slug
                    .split('-')
                    .next()
                    .and_then(|y| y.parse::<u16>().ok())
                    .is_some_and(|y| team.active_in(y))
            })
            .map(|s| s.slug.clone())
            .unwrap_or_else(|| "1946-47".to_string())
    }
}

/// Fixture team-totals input: the columns the early eras actually recorded.
/// Minutes, rebound/assist splits, steals, blocks, and plus/minus predate
/// the archive's earliest rows and stay `None` (rendered as `—`).
struct BoxTeamInput<'a> {
    game_id: &'a str,
    team_br: &'a str,
    fg: Option<u32>,
    fga: Option<u32>,
    ft: Option<u32>,
    fta: Option<u32>,
    pf: Option<u32>,
    pts: Option<u32>,
}

fn box_team(input: BoxTeamInput<'_>) -> BoxTeam {
    // Early eras did not record minutes, offensive/defensive splits,
    // steals, blocks, or plus/minus: all `None`, rendered as `—`.
    BoxTeam {
        game_id: input.game_id.to_string(),
        team_br: input.team_br.to_string(),
        mp: None,
        fg: input.fg,
        fga: input.fga,
        ft: input.ft,
        fta: input.fta,
        oreb: None,
        dreb: None,
        reb: None,
        ast: None,
        stl: None,
        blk: None,
        pf: input.pf,
        pts: input.pts,
        plus_minus: None,
    }
}

/// Fixture player-totals input: one recorded player line of a Box Score.
struct BoxPlayerInput<'a> {
    game_id: &'a str,
    team_br: &'a str,
    player_br: &'a str,
    player_name: &'a str,
    starter: bool,
    position: &'a str,
    mp: Option<u32>,
    fg: Option<u32>,
    fga: Option<u32>,
    ft: Option<u32>,
    fta: Option<u32>,
    pts: Option<u32>,
    plus_minus: Option<i32>,
}

fn box_player(input: BoxPlayerInput<'_>) -> BoxPlayer {
    BoxPlayer {
        game_id: input.game_id.to_string(),
        team_br: input.team_br.to_string(),
        player_br: input.player_br.to_string(),
        player_name: input.player_name.to_string(),
        starter: Some(input.starter),
        position: Some(input.position.to_string()),
        mp: input.mp,
        fg: input.fg,
        fga: input.fga,
        ft: input.ft,
        fta: input.fta,
        pts: input.pts,
        plus_minus: input.plus_minus,
        dnp_reason: None,
    }
}

fn box_player_dnp(
    game_id: &str,
    team_br: &str,
    player_br: &str,
    player_name: &str,
    reason: &str,
) -> BoxPlayer {
    BoxPlayer {
        game_id: game_id.to_string(),
        team_br: team_br.to_string(),
        player_br: player_br.to_string(),
        player_name: player_name.to_string(),
        starter: Some(false),
        position: None,
        mp: None,
        fg: None,
        fga: None,
        ft: None,
        fta: None,
        pts: None,
        plus_minus: None,
        dnp_reason: Some(reason.to_string()),
    }
}

fn box_score(game_id: &str, away: BoxTeam, home: BoxTeam, players: Vec<BoxPlayer>) -> BoxScore {
    // Team rows: away first (box-score convention), then home.
    BoxScore {
        game_id: game_id.to_string(),
        teams: vec![away, home],
        players,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::cell;

    fn store() -> FixtureStore {
        FixtureStore::fixture()
    }

    #[test]
    fn fixture_shape_two_seasons_three_teams_four_games() {
        let s = store();
        assert_eq!(s.seasons().len(), 2);
        assert_eq!(s.games().len(), 4);
        assert_eq!(s.teams_for_season("1946-47").len(), 3);
        // Defunct Huskies sit out 1947–48.
        assert_eq!(s.teams_for_season("1947-48").len(), 2);
    }

    #[test]
    fn ledger_splits_regular_vs_playoffs() {
        let s = store();
        let (regular, playoffs) = s.team_games("1946-47", "BOS");
        assert_eq!(regular.len(), 1);
        assert_eq!(playoffs.len(), 1);
        assert_eq!(regular[0].game_id, "194612070BOS");
        assert_eq!(playoffs[0].game_id, "194704160BOS");
        // No game appears in both ledgers.
        assert!(regular.iter().all(|g| g.game_type == GameType::Regular));
        assert!(playoffs.iter().all(|g| g.game_type == GameType::Playoffs));
    }

    #[test]
    fn ledger_excludes_other_teams_and_seasons() {
        let s = store();
        let (regular, playoffs) = s.team_games("1946-47", "TRH");
        assert_eq!(regular.len(), 1);
        assert!(playoffs.is_empty());
        let (regular, _) = s.team_games("1947-48", "TRH");
        assert!(regular.is_empty());
    }

    #[test]
    fn season_counts_seed_and_playable() {
        let s = store();
        assert_eq!(
            s.season_counts("1946-47"),
            SeasonCounts {
                seeded: 3,
                playable: 1
            }
        );
        assert_eq!(
            s.season_counts("1947-48"),
            SeasonCounts {
                seeded: 1,
                playable: 0
            }
        );
    }

    #[test]
    fn filter_box_matches_city_name_and_slug() {
        let s = store();
        assert_eq!(s.filter_teams("1946-47", "").len(), 3);
        assert_eq!(s.filter_teams("1946-47", "toronto").len(), 1);
        assert_eq!(s.filter_teams("1946-47", "HUSKIES").len(), 1);
        assert_eq!(s.filter_teams("1946-47", "trh").len(), 1);
        assert_eq!(s.filter_teams("1946-47", "celtics").len(), 1);
        assert!(s.filter_teams("1946-47", "lakers").is_empty());
    }

    #[test]
    fn palette_lists_all_on_empty_query() {
        let s = store();
        let items = s.palette_search("");
        // 2 seasons + 3 teams + 4 games.
        assert_eq!(items.len(), 9);
    }

    #[test]
    fn palette_finds_teams_seasons_games() {
        let s = store();
        let teams: Vec<_> = s
            .palette_search("celtics")
            .into_iter()
            .filter(|i| i.kind == PaletteKind::Team)
            .collect();
        assert_eq!(teams.len(), 1);
        assert!(matches!(teams[0].route, Route::Team { .. }));
        assert!(!s.palette_search("1947").is_empty());
        let games: Vec<_> = s
            .palette_search("194611010TRH")
            .into_iter()
            .filter(|i| i.kind == PaletteKind::Game)
            .collect();
        assert_eq!(games.len(), 1);
    }

    #[test]
    fn box_always_present_even_when_tape_unavailable() {
        // Tape-unavailable must render only from tape state, never from box:
        // the unavailable-tape game still has a full Box Score.
        let s = store();
        let game = s.game("194704160BOS").unwrap();
        assert_eq!(game.tape, TapeState::Unavailable);
        let bx = s.box_for("194704160BOS").expect("box must exist");
        assert_eq!(bx.teams.len(), 2);
        assert!(!bx.players.is_empty());
    }

    #[test]
    fn every_game_has_a_box_score() {
        let s = store();
        for game in s.games() {
            assert!(
                s.box_for(&game.game_id).is_some(),
                "missing box for {}",
                game.game_id
            );
        }
    }

    #[test]
    fn era_nulls_render_as_em_dash() {
        let s = store();
        let bx = s.box_for("194611010TRH").unwrap();
        // Early era: no steals/blocks recorded anywhere.
        for row in &bx.teams {
            assert_eq!(cell(row.stl), "—");
            assert_eq!(cell(row.blk), "—");
            assert_eq!(cell(row.plus_minus), "—");
        }
        // Recorded totals still render; the TRH fixture row carries the
        // live-November-1946 shape (team `fga` unrecorded) and renders `—`.
        assert_eq!(cell(bx.teams[0].pts), "68");
        assert_eq!(cell(bx.teams[1].fga), "—");
        // DNP row: every stat cell is `—`, reason survives.
        let dnp = bx.players.iter().find(|p| p.dnp_reason.is_some()).unwrap();
        assert_eq!(cell(dnp.pts), "—");
        assert_eq!(cell(dnp.mp), "—");
        assert_eq!(dnp.dnp_reason.as_deref(), Some("Coach's decision"));
    }
}
