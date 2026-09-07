//! Shell app skeleton (eframe): Home season picker → Season dashboard
//! (club grid with defunct tags + filter box + seeded/playable counts) →
//! Team view (Regular-season + Playoffs ledgers with tape glyphs) → Game
//! view (Box Score always rendered, tape-state banner). ⌘K palette as a
//! modal popup over teams, seasons, and games.
//!
//! All data comes from [`FixtureStore`]; every query the views use is a
//! pure function on the store, so all navigation logic is unit-testable
//! without a display. Never construct this in tests with a display —
//! drive [`ShellApp::navigate`] and the store queries instead.

use crate::model::{cell, TapeState};
use crate::route::Route;
use crate::store::{FixtureStore, PaletteKind};
use eframe::egui;

/// The Shell window.
pub struct ShellApp {
    store: FixtureStore,
    route: Route,
    /// Season dashboard club filter.
    filter: String,
    palette_open: bool,
    palette_query: String,
}

impl ShellApp {
    pub fn new() -> Self {
        Self {
            store: FixtureStore::fixture(),
            route: Route::Home,
            filter: String::new(),
            palette_open: false,
            palette_query: String::new(),
        }
    }

    pub fn route(&self) -> &Route {
        &self.route
    }

    pub fn navigate(&mut self, route: Route) {
        self.palette_open = false;
        self.route = route;
    }

    pub fn set_palette_open(&mut self, open: bool) {
        self.palette_open = open;
        if open {
            self.palette_query.clear();
        }
    }

    pub fn is_palette_open(&self) -> bool {
        self.palette_open
    }
}

impl Default for ShellApp {
    fn default() -> Self {
        Self::new()
    }
}

impl eframe::App for ShellApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // ⌘K / Ctrl+K toggles the palette from anywhere.
        if ctx.input_mut(|i| {
            i.consume_key(egui::Modifiers::COMMAND, egui::Key::K)
                || i.consume_key(egui::Modifiers::CTRL, egui::Key::K)
        }) {
            let open = !self.palette_open;
            self.set_palette_open(open);
        }

        egui::TopBottomPanel::top("shell-top").show(ctx, |ui| {
            ui.horizontal(|ui| {
                let crumbs = self.route.breadcrumbs(&self.store);
                for (i, crumb) in crumbs.iter().enumerate() {
                    if i > 0 {
                        ui.label("›");
                    }
                    let at_leaf = i + 1 == crumbs.len();
                    if at_leaf {
                        ui.strong(&crumb.label);
                    } else if ui.button(&crumb.label).clicked() {
                        let target = crumb.route.clone();
                        self.navigate(target);
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("⌘K palette").clicked() {
                        self.set_palette_open(true);
                    }
                });
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            // Clone the route so view builders can navigate freely.
            match self.route.clone() {
                Route::Home => self.show_home(ui),
                Route::Season { season } => self.show_season(ui, &season),
                Route::Team { season, team } => self.show_team(ui, &season, &team),
                Route::Game { game_id } => self.show_game(ui, &game_id),
            }
        });

        if self.palette_open {
            self.show_palette(ctx);
        }
    }
}

// ---- Views ---------------------------------------------------------------

impl ShellApp {
    fn show_home(&mut self, ui: &mut egui::Ui) {
        ui.heading("NBA TV Archive");
        ui.label("Pick a Season to open its dashboard.");
        ui.separator();
        let seasons: Vec<_> = self.store.seasons().to_vec();
        for season in seasons {
            let counts = self.store.season_counts(&season.slug);
            let label = format!(
                "{}  ·  {} ({} seeded, {} playable)",
                season.label, season.league, counts.seeded, counts.playable
            );
            if ui.button(label).clicked() {
                self.navigate(Route::Season {
                    season: season.slug.clone(),
                });
            }
        }
    }

    fn show_season(&mut self, ui: &mut egui::Ui, season: &str) {
        let title = self
            .store
            .season_label(season)
            .unwrap_or_else(|| season.to_string());
        ui.heading(format!("Season {title}"));
        let counts = self.store.season_counts(season);
        ui.label(format!(
            "{} clubs  ·  {} seeded  ·  {} playable",
            self.store.teams_for_season(season).len(),
            counts.seeded,
            counts.playable
        ));
        ui.separator();
        ui.horizontal(|ui| {
            ui.label("Filter clubs:");
            ui.text_edit_singleline(&mut self.filter);
        });
        let teams: Vec<_> = self
            .store
            .filter_teams(season, &self.filter)
            .into_iter()
            .cloned()
            .collect();
        if teams.is_empty() {
            ui.weak("No clubs match this filter.");
            return;
        }
        egui::Grid::new("club-grid")
            .spacing([12.0, 6.0])
            .show(ui, |ui| {
                for team in teams {
                    let tag = team.status_tag();
                    let label = format!("{} ({}) [{}]", team.display_name(), team.br_slug, tag);
                    if ui.button(label).clicked() {
                        self.navigate(Route::Team {
                            season: season.to_string(),
                            team: team.br_slug.clone(),
                        });
                    }
                    ui.end_row();
                }
            });
    }

    fn show_team(&mut self, ui: &mut egui::Ui, season: &str, team: &str) {
        let name = self
            .store
            .team_name(team)
            .unwrap_or_else(|| team.to_string());
        ui.heading(name);
        ui.weak(format!("Season {}", season));
        ui.separator();
        let (regular, playoffs) = self.store.team_games(season, team);
        let regular: Vec<crate::model::Game> = regular.into_iter().cloned().collect();
        let playoffs: Vec<crate::model::Game> = playoffs.into_iter().cloned().collect();
        self.show_ledger(ui, "Regular season", &regular.iter().collect::<Vec<_>>());
        ui.separator();
        self.show_ledger(ui, "Playoffs", &playoffs.iter().collect::<Vec<_>>());
    }

    fn show_ledger(&mut self, ui: &mut egui::Ui, title: &str, games: &[&crate::model::Game]) {
        ui.strong(title);
        if games.is_empty() {
            ui.weak("No games.");
            return;
        }
        egui::Grid::new(title).spacing([8.0, 4.0]).show(ui, |ui| {
            for game in games {
                ui.monospace(game.tape.glyph().to_string());
                ui.monospace(game.label());
                ui.monospace(game.scoreline());
                if ui.button("Open").clicked() {
                    self.navigate(Route::Game {
                        game_id: game.game_id.clone(),
                    });
                }
                ui.end_row();
            }
        });
    }

    fn show_game(&mut self, ui: &mut egui::Ui, game_id: &str) {
        let Some(game) = self.store.game(game_id).cloned() else {
            ui.heading("Game not found");
            ui.label(format!("No Game with id {game_id}."));
            return;
        };
        ui.heading(game.label());
        ui.weak(game.scoreline());
        ui.separator();
        // Tape banner derives ONLY from tape state — never from box presence.
        ui.colored_label(banner_color(game.tape), game.tape.banner());
        if game.tape == TapeState::Playable {
            ui.horizontal(|ui| {
                if ui.button("▶ Play in Player Backend").clicked() {
                    // Follow-up slice: resolve via nbatv_player and hand the
                    // TapeSource to the Player Backend (Lane A pump / Lane B
                    // embed). Until then this is a placeholder by design —
                    // clicking does nothing. No media here.
                }
            });
        }
        ui.separator();
        // Box Score always renders, even when tape is unavailable.
        let Some(bx) = self.store.box_for(game_id).cloned() else {
            ui.weak("Box Score pending.");
            return;
        };
        ui.strong("Box Score — team");
        egui::Grid::new("box-team")
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                for head in [
                    "Team", "FG", "FGA", "FT", "FTA", "STL", "BLK", "PF", "PTS", "+/-",
                ] {
                    ui.strong(head);
                }
                ui.end_row();
                for row in &bx.teams {
                    ui.monospace(&row.team_br);
                    ui.monospace(cell(Some(row.fg)));
                    ui.monospace(cell(Some(row.fga)));
                    ui.monospace(cell(Some(row.ft)));
                    ui.monospace(cell(Some(row.fta)));
                    ui.monospace(cell(row.stl));
                    ui.monospace(cell(row.blk));
                    ui.monospace(cell(Some(row.pf)));
                    ui.monospace(cell(Some(row.pts)));
                    ui.monospace(cell(row.plus_minus));
                    ui.end_row();
                }
            });
        ui.separator();
        ui.strong("Box Score — players");
        egui::Grid::new("box-player")
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                for head in ["Player", "Team", "MP", "FG", "FGA", "PTS", "Note"] {
                    ui.strong(head);
                }
                ui.end_row();
                for row in &bx.players {
                    ui.monospace(&row.player_name);
                    ui.monospace(&row.team_br);
                    ui.monospace(cell(row.mp));
                    ui.monospace(cell(row.fg));
                    ui.monospace(cell(row.fga));
                    ui.monospace(cell(row.pts));
                    ui.monospace(row.dnp_reason.as_deref().unwrap_or_default());
                    ui.end_row();
                }
            });
    }

    fn show_palette(&mut self, ctx: &egui::Context) {
        let mut target: Option<Route> = None;
        let mut close = false;
        egui::Window::new("⌘K — jump to teams, Seasons, games")
            .open(&mut self.palette_open)
            .show(ctx, |ui| {
                ui.text_edit_singleline(&mut self.palette_query);
                ui.separator();
                let hits = self.store.palette_search(&self.palette_query);
                if hits.is_empty() {
                    ui.weak("No matches.");
                    return;
                }
                egui::ScrollArea::vertical()
                    .max_height(320.0)
                    .show(ui, |ui| {
                        for hit in hits {
                            let kind = match hit.kind {
                                PaletteKind::Season => "SEASON",
                                PaletteKind::Team => "TEAM",
                                PaletteKind::Game => "GAME",
                            };
                            let label = format!("{kind} · {} — {}", hit.label, hit.detail);
                            if ui.button(label).clicked() {
                                target = Some(hit.route);
                                close = true;
                            }
                        }
                    });
            });
        if let Some(route) = target {
            self.navigate(route);
        } else if close {
            self.set_palette_open(false);
        }
    }
}

fn banner_color(tape: TapeState) -> egui::Color32 {
    match tape {
        TapeState::Playable => egui::Color32::DARK_GREEN,
        TapeState::Sweeping => egui::Color32::from_rgb(146, 64, 14),
        TapeState::Unavailable => egui::Color32::DARK_RED,
        TapeState::Pointer => egui::Color32::from_rgb(120, 90, 10),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigate_sets_route_and_closes_palette() {
        let mut app = ShellApp::new();
        assert_eq!(*app.route(), Route::Home);
        app.set_palette_open(true);
        app.navigate(Route::Team {
            season: "1946-47".into(),
            team: "TRH".into(),
        });
        assert_eq!(
            *app.route(),
            Route::Team {
                season: "1946-47".into(),
                team: "TRH".into()
            }
        );
        assert!(!app.is_palette_open());
    }

    #[test]
    fn palette_toggle_clears_query() {
        let mut app = ShellApp::new();
        app.palette_query = "bos".into();
        app.set_palette_open(true);
        assert!(app.palette_query.is_empty());
        assert!(app.is_palette_open());
    }
}
