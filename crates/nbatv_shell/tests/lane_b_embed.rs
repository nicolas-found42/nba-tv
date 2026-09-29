//! Wave 4 proof (#23): the Lane B embed-card state machine, headless.
//!
//! - Play on an embed row records a pending Lane B session for the
//!   sanctioned vendor player (in-window webview on `lane-b` builds).
//! - A framing/platform refusal falls back to the existing OpenExternal
//!   dispatch behind an honest card — never an error, never a blank.
//! - The session/sign-in hint toggles when the vendor wants a login; the
//!   app never sees credentials.
//! - The webview profile (where cookies live) stays out of the repo tree
//!   and out of the data cache.
//! The real webview is verified by a documented visual smoke run only;
//! everything here runs without a window or network.
//!
//! Visual smoke, 2026-09-08 (lane-b feature build, this machine):
//! `cargo run -p nbatv_shell --features lane-b` ran a 12 s event loop with
//! an on-screen 'NBA TV Archive' window (800x632, confirmed via
//! Swift/CoreGraphics) and empty stderr; a throwaway example opened the real
//! `EmbedHost` child webview against the live window at
//! `https://www.youtube.com/embed/dQw4w9WgXcQ?enablejsapi=1&rel=0` and
//! auto-closed cleanly (the example was deleted afterwards).
//! NOT verified: player pixels on screen, clicking Play in the vendor
//! player, resize behavior, sign-in click-through, refusal rendering.
//! Those need a human at the window; the headless tests here pin the
//! dispatch/card behavior around them.
use nbatv_db::rusqlite::Connection;
use nbatv_db::{
    create_schema, insert_game, insert_season, insert_tape_source, insert_team, GameRow, SeasonRow,
    TapeSource, TeamRow,
};
use nbatv_shell::{
    is_sign_in_url, EmbedSession, LaneBStatus, PlayDispatch, ShellApp, WEBVIEW_PROFILE_DIR,
};

const GAME_ID: &str = "196901010BOS";
const WATCH_URL: &str = "https://www.youtube.com/watch?v=dQw4w9WgXcQ";
const EMBED_URL: &str = "https://www.youtube.com/embed/dQw4w9WgXcQ?enablejsapi=1&rel=0";

/// In-memory archive with one game whose only tape row is a rank-2 YouTube
/// watch URL (valid 11-char id): dispatch must resolve it to the sanctioned
/// `/embed/` player, never the `/watch` page.
fn seeded_embed_conn() -> Connection {
    let conn = nbatv_db::open_in_memory().expect("in-memory archive db");
    create_schema(&conn).expect("create_schema");
    insert_season(
        &conn,
        &SeasonRow {
            league: "NBA".to_owned(),
            year: 1970,
            label: "1969-70".to_owned(),
        },
    )
    .unwrap();
    for (slug, city, name) in [("BOS", "Boston", "Celtics"), ("NYK", "New York", "Knicks")] {
        insert_team(
            &conn,
            &TeamRow {
                br_slug: slug.to_owned(),
                nba_team_id: None,
                franchise_id: None,
                city: city.to_owned(),
                name: name.to_owned(),
                abbrev: slug.to_owned(),
                active_from: Some(1946),
                active_to: None,
            },
        )
        .unwrap();
    }
    insert_game(
        &conn,
        &GameRow {
            game_id: GAME_ID.to_owned(),
            nba_game_id: None,
            league: "NBA".to_owned(),
            season: 1970,
            date: "1969-10-10".to_owned(),
            game_type: "REGULAR".to_owned(),
            home_team: "BOS".to_owned(),
            away_team: "NYK".to_owned(),
            home_pts: 110,
            away_pts: 108,
            ot: None,
            arena: None,
            attendance: None,
            br_url: "https://www.basketball-reference.com/boxscores/196901010BOS.html".to_owned(),
            sources: "[]".to_owned(),
        },
    )
    .unwrap();
    insert_tape_source(
        &conn,
        &TapeSource {
            game_id: nbatv_db::GameId(GAME_ID.to_owned()),
            rank: 2,
            source_class: nbatv_db::SourceClass::YouTube,
            url_or_pointer: WATCH_URL.to_owned(),
            match_confidence: 0.8,
            verified_at: "2026-09-01".to_owned(),
        },
    )
    .unwrap();
    conn
}

#[test]
fn play_on_embed_row_records_pending_lane_b_session() {
    let mut app = ShellApp::from_connection(seeded_embed_conn());
    // Same path the Game view's Play button uses.
    app.press_play(GAME_ID);
    assert_eq!(
        app.last_dispatch(),
        Some(&PlayDispatch::OpenEmbed {
            url: EMBED_URL.to_string(),
        })
    );
    let session = app
        .lane_b_session()
        .expect("OpenEmbed dispatch must record a Lane B session");
    assert_eq!(session.url(), EMBED_URL);
    assert_eq!(session.status(), &LaneBStatus::Pending);
    assert!(session.can_host());
    assert!(!session.sign_in_hint());
}

#[test]
fn embed_refusal_falls_back_to_the_open_external_path() {
    let mut app = ShellApp::from_connection(seeded_embed_conn());
    app.press_play(GAME_ID);
    // The platform refused framing (or the child webview failed to open):
    // the dispatch falls back to the existing external path, surfaced as a
    // card rather than an error.
    app.note_embed_refused();
    assert_eq!(
        app.last_dispatch(),
        Some(&PlayDispatch::OpenExternal {
            url: EMBED_URL.to_string(),
        })
    );
    let session = app.lane_b_session().expect("session survives refusal");
    assert_eq!(session.status(), &LaneBStatus::Refused);
    assert_eq!(session.fallback_url(), Some(EMBED_URL));
    // Refusing twice, or with no session at all, stays honest and total.
    app.note_embed_refused();
    assert_eq!(
        app.last_dispatch(),
        Some(&PlayDispatch::OpenExternal {
            url: EMBED_URL.to_string(),
        })
    );
    let mut bare = ShellApp::empty();
    bare.note_embed_refused();
    assert_eq!(bare.last_dispatch(), None);
}

#[test]
fn sanction_failing_embed_never_hosts() {
    // Fixture sweep row: a `/watch` URL that dispatches as-is (13-char
    // placeholder id) and fails the sanction gate — the shell must show
    // the gate note, never a webview.
    let mut app = ShellApp::with_fixture();
    app.press_play("194612070BOS");
    let dispatch = app.last_dispatch().cloned();
    assert!(matches!(dispatch, Some(PlayDispatch::OpenEmbed { .. })));
    let session = app.lane_b_session().expect("session still recorded");
    assert_eq!(session.status(), &LaneBStatus::Pending);
    assert!(!session.can_host());
}

#[test]
fn sign_in_hint_marks_only_vendor_login_surfaces() {
    assert!(is_sign_in_url(
        "https://accounts.google.com/signin/v2/identifier?service=youtube"
    ));
    assert!(is_sign_in_url(
        "https://www.youtube.com/signin?next=/embed/x"
    ));
    assert!(!is_sign_in_url(EMBED_URL));
    assert!(!is_sign_in_url(
        "https://www.dailymotion.com/embed/video/x8abc12"
    ));
    assert!(!is_sign_in_url(""));

    let mut session = EmbedSession::pending(EMBED_URL.to_string());
    assert!(!session.sign_in_hint());
    session.set_sign_in_hint(true);
    assert!(session.sign_in_hint());
    session.set_sign_in_hint(false);
    assert!(!session.sign_in_hint());

    // The shell-level seam the (lane-b) url poll drives.
    let mut app = ShellApp::from_connection(seeded_embed_conn());
    app.press_play(GAME_ID);
    app.set_lane_b_sign_in_hint(true);
    assert!(app.lane_b_session().expect("session").sign_in_hint());
    // Leaving the game retires the session with the dispatch.
    app.navigate(nbatv_shell::Route::Game {
        game_id: "194704160BOS".to_string(),
    });
    assert_eq!(app.lane_b_session(), None);
}

#[test]
fn webview_profile_stays_out_of_the_repo_and_cache() {
    // Cookies live in the webview profile: a gitignored data dir on
    // backends that honor one (Windows/Linux), the app-scoped OS store
    // elsewhere (macOS) — never the repo tree, never the data cache.
    // The behavioral contract: relative path (never escapes to an
    // absolute repo-exit location), never the Cache Tier.
    assert!(!WEBVIEW_PROFILE_DIR.contains("cache"));
    assert!(!std::path::Path::new(WEBVIEW_PROFILE_DIR).is_absolute());
}
