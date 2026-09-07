//! Shell routes (owned here): Home → Season dashboard → Team → Game.
//!
//! Canonical path shapes, built from `/{season}/{team}/{game_id}` segments:
//!
//! | Route | Canonical path |
//! |-------|----------------|
//! | `Home` | `/` |
//! | `Season` | `/{season}` |
//! | `Team` | `/{season}/{team}` |
//! | `Game` | `/game/{game_id}` |
//!
//! `parse` additionally accepts the deep-link form
//! `/{season}/{team}/{game_id}` (e.g. pasted archival URLs) and resolves it
//! to `Game`, keyed on the trailing segment — the `game_id` is the identity,
//! the leading segments are context.

use crate::store::FixtureStore;

/// Shell-owned navigation target.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Route {
    Home,
    Season { season: String },
    Team { season: String, team: String },
    Game { game_id: String },
}

/// One breadcrumb segment: `League › Season › Team › Game` (variant A).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Crumb {
    pub label: String,
    pub route: Route,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError(pub String);

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unrecognized shell path: {}", self.0)
    }
}

impl std::error::Error for ParseError {}

impl Route {
    /// Canonical path for this route. Round-trips through [`Route::parse`].
    pub fn to_path(&self) -> String {
        match self {
            Route::Home => "/".to_string(),
            Route::Season { season } => format!("/{season}"),
            Route::Team { season, team } => format!("/{season}/{team}"),
            Route::Game { game_id } => format!("/game/{game_id}"),
        }
    }

    /// Parse a path into a route. Tolerates query strings, fragments, and
    /// a trailing slash. Accepts both the canonical `/game/{game_id}` form
    /// and the deep-link `/{season}/{team}/{game_id}` form.
    pub fn parse(path: &str) -> Result<Route, ParseError> {
        let bare = path.split(['?', '#']).next().unwrap_or("").trim();
        let segments: Vec<&str> = bare.split('/').filter(|s| !s.is_empty()).collect();
        match segments.as_slice() {
            [] => Ok(Route::Home),
            ["game", game_id] => Ok(Route::Game {
                game_id: game_id.to_string(),
            }),
            [season] => Ok(Route::Season {
                season: season.to_string(),
            }),
            [season, team] => Ok(Route::Team {
                season: season.to_string(),
                team: team.to_string(),
            }),
            [_, _, game_id] => Ok(Route::Game {
                game_id: game_id.to_string(),
            }),
            _ => Err(ParseError(path.to_string())),
        }
    }

    /// Breadcrumb trail `League › Season › Team › Game` per variant A.
    ///
    /// Depth is fixed per route: Home 1, Season 2, Team 3, Game 4. Labels
    /// resolve through `store`; unknown slugs fall back to the raw slug so
    /// a bad link still renders a trail. For a Game, the Team crumb is the
    /// home club (navigation context, not a claim about the matchup).
    pub fn breadcrumbs(&self, store: &FixtureStore) -> Vec<Crumb> {
        let league = Crumb {
            label: "League".to_string(),
            route: Route::Home,
        };
        match self {
            Route::Home => vec![league],
            Route::Season { season } => vec![
                league,
                Crumb {
                    label: store.season_label(season).unwrap_or_else(|| season.clone()),
                    route: self.clone(),
                },
            ],
            Route::Team { season, team } => vec![
                league,
                Crumb {
                    label: store.season_label(season).unwrap_or_else(|| season.clone()),
                    route: Route::Season {
                        season: season.clone(),
                    },
                },
                Crumb {
                    label: store.team_name(team).unwrap_or_else(|| team.clone()),
                    route: self.clone(),
                },
            ],
            Route::Game { game_id } => {
                let Some(game) = store.game(game_id) else {
                    return vec![
                        league,
                        Crumb {
                            label: game_id.clone(),
                            route: self.clone(),
                        },
                    ];
                };
                vec![
                    league,
                    Crumb {
                        label: store
                            .season_label(&game.season)
                            .unwrap_or_else(|| game.season.clone()),
                        route: Route::Season {
                            season: game.season.clone(),
                        },
                    },
                    Crumb {
                        label: store
                            .team_name(&game.home_team)
                            .unwrap_or_else(|| game.home_team.clone()),
                        route: Route::Team {
                            season: game.season.clone(),
                            team: game.home_team.clone(),
                        },
                    },
                    Crumb {
                        label: game.label(),
                        route: self.clone(),
                    },
                ]
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::FixtureStore;

    fn store() -> FixtureStore {
        FixtureStore::fixture()
    }

    #[test]
    fn home_round_trip() {
        assert_eq!(Route::parse("/"), Ok(Route::Home));
        assert_eq!(Route::parse(""), Ok(Route::Home));
        assert_eq!(Route::Home.to_path(), "/");
        assert_eq!(Route::parse(&Route::Home.to_path()), Ok(Route::Home));
    }

    #[test]
    fn season_round_trip() {
        let route = Route::Season {
            season: "1946-47".to_string(),
        };
        assert_eq!(route.to_path(), "/1946-47");
        assert_eq!(Route::parse("/1946-47"), Ok(route.clone()));
        assert_eq!(Route::parse(&route.to_path()), Ok(route));
    }

    #[test]
    fn team_round_trip() {
        let route = Route::Team {
            season: "1946-47".to_string(),
            team: "BOS".to_string(),
        };
        assert_eq!(route.to_path(), "/1946-47/BOS");
        assert_eq!(Route::parse("/1946-47/BOS"), Ok(route.clone()));
        assert_eq!(Route::parse(&route.to_path()), Ok(route));
    }

    #[test]
    fn defunct_team_slug_round_trip() {
        // Defunct clubs keep plain slugs; nothing about the path shape
        // may exclude them.
        let route = Route::Team {
            season: "1946-47".to_string(),
            team: "TRH".to_string(),
        };
        assert_eq!(route.to_path(), "/1946-47/TRH");
        assert_eq!(Route::parse("/1946-47/TRH"), Ok(route.clone()));
        assert_eq!(Route::parse(&route.to_path()), Ok(route.clone()));
        let crumbs = route.breadcrumbs(&store());
        assert_eq!(crumbs.len(), 3);
        assert!(crumbs[2].label.contains("Huskies"));
    }

    #[test]
    fn game_round_trip() {
        let route = Route::Game {
            game_id: "194611010TRH".to_string(),
        };
        assert_eq!(route.to_path(), "/game/194611010TRH");
        assert_eq!(Route::parse("/game/194611010TRH"), Ok(route.clone()));
        assert_eq!(Route::parse(&route.to_path()), Ok(route));
    }

    #[test]
    fn deep_link_form_resolves_to_game() {
        // `/{season}/{team}/{game_id}` deep links resolve on the trailing id.
        assert_eq!(
            Route::parse("/1946-47/TRH/194611010TRH"),
            Ok(Route::Game {
                game_id: "194611010TRH".to_string()
            })
        );
    }

    #[test]
    fn parse_tolerates_trailing_slash_query_and_fragment() {
        let team = Route::Team {
            season: "1946-47".to_string(),
            team: "BOS".to_string(),
        };
        assert_eq!(Route::parse("/1946-47/BOS/"), Ok(team.clone()));
        assert_eq!(Route::parse("/1946-47/BOS?variant=A"), Ok(team.clone()));
        assert_eq!(Route::parse("/1946-47/BOS#ledger"), Ok(team));
    }

    #[test]
    fn breadcrumb_depth_per_route() {
        let s = store();
        assert_eq!(Route::Home.breadcrumbs(&s).len(), 1);
        assert_eq!(
            Route::Season {
                season: "1946-47".into()
            }
            .breadcrumbs(&s)
            .len(),
            2
        );
        assert_eq!(
            Route::Team {
                season: "1946-47".into(),
                team: "BOS".into()
            }
            .breadcrumbs(&s)
            .len(),
            3
        );
        assert_eq!(
            Route::Game {
                game_id: "194611010TRH".into()
            }
            .breadcrumbs(&s)
            .len(),
            4
        );
    }

    #[test]
    fn breadcrumb_labels_follow_league_season_team_game() {
        let s = store();
        let crumbs = Route::Game {
            game_id: "194611010TRH".into(),
        }
        .breadcrumbs(&s);
        assert_eq!(crumbs[0].label, "League");
        assert_eq!(crumbs[1].label, "1946–47");
        assert!(crumbs[2].label.contains("Huskies"));
        assert!(crumbs[3].label.contains("NYK @ TRH"));
        // Each crumb navigates somewhere real.
        assert_eq!(crumbs[0].route, Route::Home);
        assert_eq!(
            crumbs[1].route,
            Route::Season {
                season: "1946-47".into()
            }
        );
    }
}
