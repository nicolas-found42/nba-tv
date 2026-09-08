//! Shell app skeleton (eframe): Home season picker → Season dashboard
//! (club grid with defunct tags + filter box + seeded/playable counts) →
//! Team view (Regular-season + Playoffs ledgers with tape glyphs) → Game
//! view (Box Score always rendered, tape-state banner). ⌘K palette as a
//! modal popup over teams, seasons, and games.
//!
//! All data comes from the [`Store`] catalog (the archive database in
//! production, fixtures in tests/offline dev); every query the views use is
//! exposed on the store, so all navigation logic is unit-testable without
//! a display. Never construct this in tests with a display — drive
//! [`ShellApp::navigate`] and the store queries instead.

use crate::db_store::{DbStore, Store, ARCHIVE_DB_PATH};
use crate::handover::{dispatch_for, PlayDispatch};
use crate::model::{cell, TapeState};
use crate::route::Route;
use crate::store::PaletteKind;
use eframe::egui;
use nbatv_player::{EguiTextureStage, FrameToTexture, Pump};
/// Lane A playback state for the viewed game. Headless-readable via
/// [`ShellApp::lane_a_status`]; the Game view renders it every frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LaneAStatus {
    /// Decoding (or ready to start on the next frame pull).
    Playing,
    /// Held by the pause control; the sidecar and last texture are kept.
    Paused,
    /// Stream drained after at least one converted frame, or stopped by
    /// the Stop control after frames showed.
    Ended,
    /// No session could start, or the stream ended before any frame
    /// converted. Carries the honest reason shown in the Game view.
    Error(String),
}

/// The Shell window.
pub struct ShellApp {
    store: Store,
    route: Route,
    /// Season dashboard club filter.
    filter: String,
    palette_open: bool,
    palette_query: String,
    /// Outcome of the last Play press (pure resolve-dispatch result).
    last_dispatch: Option<PlayDispatch>,
    /// Lane A decode session for the viewed game. `press_play` on a
    /// progressive tape starts it (spawn is lazy: the sidecar starts on the
    /// first frame pull, so `press_play` itself stays free of process and
    /// network effects and headless tests never spawn). Leaving the route
    /// or pressing Play on a non-progressive game retires it, which drops
    /// the sidecar child.
    lane_a_pump: Option<Pump>,
    lane_a_src: Option<String>,
    lane_a_stage: EguiTextureStage,
    lane_a_texture: Option<egui::TextureHandle>,
    lane_a_status: Option<LaneAStatus>,
    lane_a_paused: bool,
    /// Lane B child webview, once opened (only with the `lane-b` feature).
    /// The webview lives exactly as long as this host: dropping it
    /// destroys the native child, which is why navigation clears it.
    #[cfg(feature = "lane-b")]
    embed_host: Option<crate::embed::EmbedHost>,
    /// URL currently hosted or attempted-and-failed, so a failing open is
    /// tried once per dispatch instead of once per frame.
    #[cfg(feature = "lane-b")]
    embed_open_url: Option<String>,
}

impl ShellApp {
    /// Live default: open the archive database at [`ARCHIVE_DB_PATH`].
    /// A missing or unreadable database degrades to an empty in-memory
    /// archive (empty season list, no crash) — never fixtures.
    pub fn new() -> Self {
        Self::open_archive(ARCHIVE_DB_PATH)
    }

    /// Live path used by the binary: open the archive database at `path`,
    /// creating the schema so an empty file is valid. Any open failure
    /// logs once and degrades to an empty in-memory archive, never a panic
    /// and never fixtures.
    pub fn open_archive(path: impl AsRef<std::path::Path>) -> Self {
        match DbStore::open(path.as_ref()) {
            Ok(store) => Self::with_store(Store::Db(store)),
            Err(err) => {
                eprintln!(
                    "archive db: cannot open {} ({err}); running with an empty season list",
                    path.as_ref().display()
                );
                Self::empty()
            }
        }
    }

    /// Hermetic empty archive (no filesystem, no fixtures): pure-logic
    /// headless tests use this.
    pub fn empty() -> Self {
        Self::with_store(Store::Db(DbStore::in_memory()))
    }

    /// Offline/test path: fixed fixtures, no database. The live path never
    /// uses this — it seeds nothing outside tests and offline development.
    pub fn with_fixture() -> Self {
        Self::with_store(Store::Fixture(crate::store::FixtureStore::fixture()))
    }

    /// Headless db tests: take ownership of an already-seeded connection
    /// (callers run `create_schema` plus inserts first; the schema is
    /// ensured again idempotently here).
    pub fn from_connection(conn: rusqlite::Connection) -> Self {
        Self::with_store(Store::Db(DbStore::from_connection(conn)))
    }

    fn with_store(store: Store) -> Self {
        Self {
            store,
            route: Route::Home,
            filter: String::new(),
            palette_open: false,
            palette_query: String::new(),
            last_dispatch: None,
            lane_a_pump: None,
            lane_a_src: None,
            lane_a_stage: EguiTextureStage::default(),
            lane_a_texture: None,
            lane_a_status: None,
            lane_a_paused: false,
            #[cfg(feature = "lane-b")]
            embed_host: None,
            #[cfg(feature = "lane-b")]
            embed_open_url: None,
        }
    }

    /// The catalog the views render through (archive db or fixtures).
    /// Headless tests drive the same queries the views use via this seam.
    pub fn store(&self) -> &Store {
        &self.store
    }

    pub fn route(&self) -> &Route {
        &self.route
    }

    pub fn navigate(&mut self, route: Route) {
        self.palette_open = false;
        if self.route != route {
            // A Play outcome belongs to the game it was pressed on: leaving
            // the route retires it, so the next Game view can neither
            // auto-open nor display the previous game's embed.
            // `maybe_open_embed` then serves only the viewed game.
            self.last_dispatch = None;
            // The sidecar child dies with the session: leaving the game
            // stops its decode, same as the webview teardown below.
            self.retire_lane_a();
        }
        // The child webview is a native overlay, not an egui widget: it
        // would float above the next view, so leaving a route destroys it.
        // The next Game view re-opens for its own dispatch (see
        // `maybe_open_embed`).
        #[cfg(feature = "lane-b")]
        {
            self.embed_host = None;
            self.embed_open_url = None;
        }
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

    /// Outcome of the last Play press, if the button has been pressed.
    pub fn last_dispatch(&self) -> Option<&PlayDispatch> {
        self.last_dispatch.as_ref()
    }

    /// Resolve what pressing Play means for `game_id` and record it.
    ///
    /// This is the exact path the Game view's Play button uses, exposed so
    /// tests can drive it headlessly. It gathers the game's real tape
    /// source rows from the store (no invented URLs: a game with no rows
    /// resolves honestly to [`PlayDispatch::Unavailable`]) and dispatches
    /// through [`dispatch_for`].
    ///
    /// Cache Tier lookup is a later slice, so the cache is always `None`
    /// for now. Recording the outcome starts (or retires) the Lane A
    /// session to match; the sidecar itself spawns lazily on the first
    /// frame pull, so this stays free of process and network effects.
    /// Box Score is never consulted (tape-only signal).
    pub fn press_play(&mut self, game_id: &str) {
        // Cache Tier lookup is a later slice: no cache yet, always None.
        // This records the pure outcome and starts/retires the Lane A
        // session to match: a progressive dispatch starts Lane A (sidecar
        // spawn is lazy on the first frame pull, so this stays free of
        // process and network effects); any other dispatch retires it.
        // Lane B hosts the OpenEmbed outcome via `maybe_open_embed`
        // (lane-b builds). Box Score is never consulted (tape-only signal).
        let sources = self
            .store
            .game(game_id)
            .map(|game| game.sources)
            .unwrap_or_default();
        let dispatch = dispatch_for(game_id, None, &sources);
        match &dispatch {
            PlayDispatch::PlayProgressive { src } => self.begin_lane_a(src.clone()),
            _ => self.retire_lane_a(),
        }
        self.last_dispatch = Some(dispatch);
    }

    /// Lane A session status for the viewed game, if Play resolved to a
    /// progressive tape. Headless-readable so tests can drive playback
    /// without a display.
    pub fn lane_a_status(&self) -> Option<&LaneAStatus> {
        self.lane_a_status.as_ref()
    }

    /// Source the Lane A session is (or was) decoding, if any.
    pub fn lane_a_src(&self) -> Option<&str> {
        self.lane_a_src.as_deref()
    }

    /// Frames the Lane A stage has converted so far this session.
    pub fn lane_a_frames_converted(&self) -> u64 {
        self.lane_a_stage.frames_converted
    }

    /// Whether Lane A playback is held paused.
    pub fn lane_a_paused(&self) -> bool {
        self.lane_a_paused
    }

    /// Start (or restart) the Lane A session for `src` at the normalized
    /// decode extent. Records `Playing`; the sidecar itself spawns lazily
    /// on the first [`ShellApp::advance_lane_a`] pull so recording intent
    /// never spawns a process.
    pub fn begin_lane_a(&mut self, src: String) {
        self.retire_lane_a();
        self.lane_a_src = Some(src);
        self.lane_a_status = Some(LaneAStatus::Playing);
    }

    /// Pull one decoded frame through the Lane A stage.
    ///
    /// Spawns the sidecar on the first call for the session, then reads one
    /// frame per call so the Game view can pace decode to its repaint
    /// cadence. Returns the converted image for texture upload, or `None`
    /// when paused, when no session exists, or at end of stream (which
    /// settles the status to `Ended` on a clean run or `Error` when no
    /// bytes ever decoded — e.g. a details page instead of a file).
    /// Headless-safe: touches no window or GPU context.
    pub fn advance_lane_a(&mut self) -> Option<nbatv_player::TextureImage> {
        if self.lane_a_paused || self.lane_a_status.is_none() {
            return None;
        }
        if self.lane_a_pump.is_none() && matches!(self.lane_a_status, Some(LaneAStatus::Playing)) {
            let src = self.lane_a_src.clone().unwrap_or_default();
            match Pump::open_lane_a(&src) {
                Ok(pump) => self.lane_a_pump = Some(pump),
                Err(err) => {
                    self.lane_a_status = Some(LaneAStatus::Error(format!(
                        "could not start the decoder ({err}); is ffmpeg installed?"
                    )));
                    return None;
                }
            }
        }
        let pump = self.lane_a_pump.as_mut()?;
        match pump.next_frame() {
            Some(frame) => self.lane_a_stage.convert(&frame),
            None => {
                self.lane_a_pump = None;
                if self.lane_a_stage.frames_converted > 0 {
                    self.lane_a_status = Some(LaneAStatus::Ended);
                } else {
                    let src = self.lane_a_src.clone().unwrap_or_default();
                    self.lane_a_status = Some(LaneAStatus::Error(format!(
                        "no decodable bytes from {src}: expected a progressive file URL, not a details or share page"
                    )));
                }
                None
            }
        }
    }

    /// Hold (`true`) or resume (`false`) Lane A playback. Pausing keeps the
    /// sidecar and the last texture; resuming continues the same stream.
    /// No session, or a finished/failed one, ignores the call.
    pub fn set_lane_a_paused(&mut self, paused: bool) {
        match self.lane_a_status {
            Some(LaneAStatus::Playing) | Some(LaneAStatus::Paused) => {
                self.lane_a_paused = paused;
                self.lane_a_status = Some(if paused {
                    LaneAStatus::Paused
                } else {
                    LaneAStatus::Playing
                });
            }
            _ => {}
        }
    }

    /// Restart Lane A playback from the beginning of the session source.
    /// No session source: does nothing.
    pub fn restart_lane_a(&mut self) {
        if let Some(src) = self.lane_a_src.clone() {
            self.begin_lane_a(src);
        }
    }

    /// Stop Lane A playback and show its outcome instead. used by the Game
    /// view's Stop control; navigation and non-progressive dispatches use
    /// [`ShellApp::retire_lane_a`].
    pub fn stop_lane_a(&mut self) {
        let frames = self.lane_a_stage.frames_converted;
        self.retire_lane_a();
        if frames > 0 {
            self.lane_a_status = Some(LaneAStatus::Ended);
        }
    }

    /// Drop the Lane A session entirely: sidecar child, texture, source,
    /// status, and pause flag. Dropping the pump kills the sidecar.
    fn retire_lane_a(&mut self) {
        self.lane_a_pump = None;
        self.lane_a_src = None;
        self.lane_a_stage = EguiTextureStage::default();
        self.lane_a_texture = None;
        self.lane_a_status = None;
        self.lane_a_paused = false;
    }

    /// Parent the Lane B child webview for the current Game view, once per
    /// dispatch. Only with the `lane-b` feature; default builds never call
    /// this (no `wry` linked) and the Game view shows the URL instead.
    ///
    /// Opens only when the route is a Game view whose last dispatch maps
    /// to a sanctioned embed URL (see [`crate::embed::embed_url_for`]) and
    /// that URL was not already hosted or attempted: reopening every frame
    /// would stack native child windows above the egui UI with no z-order
    /// control (map research #5). A failed open logs once and falls back
    /// to the Game view's URL + external-fallback note — never a blank lie.
    #[cfg(feature = "lane-b")]
    fn maybe_open_embed(&mut self, frame: &eframe::Frame) {
        let url = match (&self.route, &self.last_dispatch) {
            (Route::Game { .. }, Some(dispatch)) => crate::embed::embed_url_for(dispatch),
            _ => None,
        };
        let Some(url) = url else {
            return;
        };
        if self.embed_open_url.as_deref() == Some(url.as_str()) {
            return;
        }
        // Drop a stale host before opening so at most one child webview is
        // ever parented: native overlays ignore egui z-order.
        self.embed_host = None;
        match crate::embed::EmbedHost::open(frame, crate::embed::EmbedBounds::PLACEHOLDER, &url) {
            Ok(host) => self.embed_host = Some(host),
            Err(err) => eprintln!("lane-b: embed host failed for {url}: {err}"),
        }
        self.embed_open_url = Some(url);
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

        // Lane B only: parent the child webview for the Game view's
        // OpenEmbed dispatch (once per dispatch; default builds never
        // create a WebView — the Game view shows the URL instead).
        #[cfg(feature = "lane-b")]
        self.maybe_open_embed(_frame);

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
        let seasons = self.store.seasons();
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
        let teams = self.store.filter_teams(season, &self.filter);
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
        let Some(game) = self.store.game(game_id) else {
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
                    // Resolve via the Player Backend and start/retire the
                    // Lane A session to match (sidecar spawn is lazy on the
                    // first frame pull, so pressing never blocks the UI).
                    // Lane B hosts OpenEmbed via `maybe_open_embed`
                    // (lane-b builds).
                    let game_id = game.game_id.clone();
                    self.press_play(&game_id);
                }
            });
        }
        // Every Play outcome renders visibly: pressing Play must never look
        // like nothing happened, whatever lane the tape resolved to.
        match self.last_dispatch.clone() {
            Some(PlayDispatch::PlayProgressive { .. }) => self.show_lane_a(ui, &game.game_id),
            Some(PlayDispatch::OpenEmbed { url }) => {
                ui.separator();
                ui.strong("Vendor embed — last Play outcome (Lane B)");
                ui.monospace(&url);
                if crate::embed::is_sanctioned_embed(&url) {
                    // The wording must match what this build actually hosts:
                    // default builds link no webview, so they must never claim
                    // one opens.
                    #[cfg(feature = "lane-b")]
                    {
                        let bounds = crate::embed::EmbedBounds::PLACEHOLDER;
                        let attempted = self.embed_open_url.as_deref() == Some(url.as_str());
                        if self.embed_host.is_some() {
                            ui.weak(format!(
                                "Embed player hosted in a child webview at {}×{} @ {},{} physical px (placeholder rect).",
                                bounds.w, bounds.h, bounds.x, bounds.y
                            ));
                        } else if attempted {
                            ui.weak("The child webview failed to open — open the URL above in your browser.");
                        } else {
                            ui.weak("Opening the child webview…");
                        }
                    }
                    #[cfg(not(feature = "lane-b"))]
                    {
                        ui.weak("This build hosts no webview (rebuild with `--features lane-b`) — open the URL above in your browser.");
                    }
                } else {
                    ui.weak("This URL fails the embed sanction gate (embeds load only from /embed/ player URLs) — open it in your browser instead.");
                }
            }
            Some(PlayDispatch::OpenExternal { url }) => {
                ui.separator();
                ui.strong("External Surface — last Play outcome");
                ui.monospace(&url);
                ui.weak("Vendor pages (NBA App / watch.nba.com) are Widevine-encrypted: open the URL above in your browser. Nothing decodes in-window by design.");
            }
            Some(PlayDispatch::ShowPointer { pointer }) => {
                ui.separator();
                ui.strong("Pointer only — last Play outcome");
                ui.monospace(&pointer);
                ui.weak("This entry names where tape lives; nothing plays in-window.");
            }
            Some(PlayDispatch::Unavailable) => {
                ui.separator();
                ui.weak("Still no known tape after Play — the Box Score below is complete.");
            }
            None => {}
        }
        // REVIEW candidates surface here and only here: they never become
        // tape rows and dispatch never sees them.
        let reviews = self.store.review_list(game_id);
        if !reviews.is_empty() {
            ui.separator();
            ui.strong(format!(
                "Needs review — {} candidate{} (never auto-plays)",
                reviews.len(),
                if reviews.len() == 1 { "" } else { "s" }
            ));
            for item in &reviews {
                ui.monospace(format!(
                    "rung {} {} — {}",
                    item.rung, item.rung_name, item.title
                ));
                ui.monospace(&item.url_or_pointer);
            }
            ui.weak("These candidates need a human look before they can play.");
        }
        ui.separator();
        // Box Score always renders, even when tape is unavailable.
        let Some(bx) = self.store.box_for(game_id) else {
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
                    ui.monospace(cell(row.fg));
                    ui.monospace(cell(row.fga));
                    ui.monospace(cell(row.ft));
                    ui.monospace(cell(row.fta));
                    ui.monospace(cell(row.stl));
                    ui.monospace(cell(row.blk));
                    ui.monospace(cell(row.pf));
                    ui.monospace(cell(row.pts));
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

    /// Lane A player section: decoded video (or the honest reason there is
    /// none) plus transport controls. Runs every frame the Game view shows
    /// a progressive dispatch; pulls one frame per tick paced to ~30fps.
    fn show_lane_a(&mut self, ui: &mut egui::Ui, game_id: &str) {
        ui.separator();
        ui.strong("Player Backend — Lane A (ffmpeg sidecar)");
        if matches!(self.lane_a_status, Some(LaneAStatus::Playing)) {
            if let Some(image) = self.advance_lane_a() {
                let (w, h) = (image.width as usize, image.height as usize);
                let color = egui::ColorImage::from_rgba_unmultiplied([w, h], &image.rgba);
                match &mut self.lane_a_texture {
                    Some(handle) => {
                        if handle.size() != [w, h] {
                            *handle = ui.ctx().load_texture("lane-a", color, Default::default());
                        } else {
                            handle.set(color, Default::default());
                        }
                    }
                    None => {
                        self.lane_a_texture =
                            Some(ui.ctx().load_texture("lane-a", color, Default::default()));
                    }
                }
            }
            // Keep ticking while the stream runs. One frame per tick at
            // ~30fps: close enough for archive footage without wall-clock
            // sync (a driver slice can pace to the source rate later).
            if matches!(self.lane_a_status, Some(LaneAStatus::Playing)) {
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(33));
            }
        }
        match self.lane_a_status.clone() {
            Some(LaneAStatus::Playing) => {
                if let Some(tex) = &self.lane_a_texture {
                    let size = tex.size_vec2();
                    let scale = (ui.available_width() / size.x).min(1.0);
                    ui.image(egui::load::SizedTexture::new(tex.id(), size * scale));
                    ui.weak(format!(
                        "Playing — {} frames decoded.",
                        self.lane_a_frames_converted()
                    ));
                } else {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.weak("Starting the decoder…");
                    });
                }
                ui.horizontal(|ui| {
                    if ui.button("⏸ Pause").clicked() {
                        self.set_lane_a_paused(true);
                    }
                    if ui.button("↺ Restart").clicked() {
                        self.restart_lane_a();
                    }
                    if ui.button("⏹ Stop").clicked() {
                        self.stop_lane_a();
                    }
                });
            }
            Some(LaneAStatus::Paused) => {
                if let Some(tex) = &self.lane_a_texture {
                    let size = tex.size_vec2();
                    let scale = (ui.available_width() / size.x).min(1.0);
                    ui.image(egui::load::SizedTexture::new(tex.id(), size * scale));
                }
                ui.weak(format!(
                    "Paused — {} frames decoded.",
                    self.lane_a_frames_converted()
                ));
                ui.horizontal(|ui| {
                    if ui.button("▶ Resume").clicked() {
                        self.set_lane_a_paused(false);
                    }
                    if ui.button("↺ Restart").clicked() {
                        self.restart_lane_a();
                    }
                    if ui.button("⏹ Stop").clicked() {
                        self.stop_lane_a();
                    }
                });
            }
            Some(LaneAStatus::Ended) => {
                if let Some(tex) = &self.lane_a_texture {
                    let size = tex.size_vec2();
                    let scale = (ui.available_width() / size.x).min(1.0);
                    ui.image(egui::load::SizedTexture::new(tex.id(), size * scale));
                }
                ui.weak(format!(
                    "Tape ended — {} frames played.",
                    self.lane_a_frames_converted()
                ));
                if ui.button("↺ Watch again").clicked() {
                    self.restart_lane_a();
                }
            }
            Some(LaneAStatus::Error(message)) => {
                ui.colored_label(
                    egui::Color32::DARK_RED,
                    format!("Lane A cannot play this tape: {message}"),
                );
                if let Some(src) = self.lane_a_src.clone() {
                    ui.monospace(src);
                }
                ui.weak("The Box Score below is unaffected — only tape is unavailable.");
                ui.horizontal(|ui| {
                    if ui.button("↻ Try again").clicked() {
                        self.restart_lane_a();
                    }
                    if ui.button("Dismiss").clicked() {
                        self.retire_lane_a();
                        self.last_dispatch = None;
                    }
                });
            }
            None => {
                ui.weak("Playback stopped.");
                if ui.button("▶ Play").clicked() {
                    self.press_play(game_id);
                }
            }
        }
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
        let mut app = ShellApp::empty();
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
        let mut app = ShellApp::empty();
        app.palette_query = "bos".into();
        app.set_palette_open(true);
        assert!(app.palette_query.is_empty());
        assert!(app.is_palette_open());
    }

    #[test]
    fn fresh_app_has_no_dispatch() {
        let app = ShellApp::empty();
        assert_eq!(app.last_dispatch(), None);
    }

    #[test]
    fn press_play_on_fixture_game_dispatches_progressive() {
        let mut app = ShellApp::with_fixture();
        app.navigate(Route::Game {
            game_id: "194611010TRH".into(),
        });
        // Same path the Play button uses.
        app.press_play("194611010TRH");
        assert_eq!(
            app.last_dispatch(),
            Some(&PlayDispatch::PlayProgressive {
                src: "https://archive.org/details/194611010TRH".to_string(),
            })
        );
    }

    #[test]
    fn press_play_with_no_sources_is_honestly_unavailable() {
        let mut app = ShellApp::with_fixture();
        app.navigate(Route::Game {
            game_id: "194704160BOS".into(),
        });
        // Fixture game with no tape source rows: no fake playable, exactly
        // Unavailable.
        app.press_play("194704160BOS");
        assert_eq!(app.last_dispatch(), Some(&PlayDispatch::Unavailable));
    }

    #[test]
    fn press_play_on_pointer_game_shows_pointer() {
        let mut app = ShellApp::with_fixture();
        app.navigate(Route::Game {
            game_id: "194711150BOS".into(),
        });
        app.press_play("194711150BOS");
        assert_eq!(
            app.last_dispatch(),
            Some(&PlayDispatch::ShowPointer {
                pointer: "Catalog ref FTE-194711150BOS (pointer only)".to_string(),
            })
        );
    }

    #[test]
    fn press_play_on_unknown_game_is_unavailable() {
        let mut app = ShellApp::with_fixture();
        app.press_play("000000000AAA");
        assert_eq!(app.last_dispatch(), Some(&PlayDispatch::Unavailable));
    }
    #[test]
    fn navigate_to_another_game_retires_last_dispatch() {
        // Pressing Play on game A then viewing game B must not serve A's
        // embed on B's view: leaving the route retires the outcome.
        let mut app = ShellApp::with_fixture();
        app.press_play("194611010TRH");
        assert!(app.last_dispatch().is_some());
        app.navigate(Route::Game {
            game_id: "194704160BOS".into(),
        });
        assert_eq!(app.last_dispatch(), None);
    }

    #[test]
    fn same_route_navigation_keeps_last_dispatch() {
        // Re-rendering the same Game view (e.g. ledger Open while already
        // there) must not wipe the just-pressed outcome.
        let mut app = ShellApp::with_fixture();
        app.navigate(Route::Game {
            game_id: "194611010TRH".into(),
        });
        app.press_play("194611010TRH");
        app.navigate(Route::Game {
            game_id: "194611010TRH".into(),
        });
        assert!(app.last_dispatch().is_some());
    }
}
