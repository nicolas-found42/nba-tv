//! Facade guard: the crate root must keep re-exporting the public surface
//! (`store`, `model`, `handover`, `embed`, `route`, `db_store`) so external
//! consumers (and the driver binary) name one path. Regression pin for the
//! T5 review, which caught the `store` re-export being dropped.

use nbatv_shell::{
    dispatch_for, is_sanctioned_embed, FixtureStore, PlayDispatch, Route, ShellApp, ARCHIVE_DB_PATH,
};

#[test]
fn facade_re_exports_the_public_surface() {
    // Store facade: fixtures load through the re-export.
    let store = FixtureStore::fixture();
    assert!(!store.seasons().is_empty());

    // App + route + handover facades compose headlessly on the fixture path.
    let mut app = ShellApp::with_fixture();
    app.navigate(Route::Home);
    app.press_play("194611010TRH");
    assert!(matches!(
        app.last_dispatch(),
        Some(PlayDispatch::PlayProgressive { .. })
    ));

    // Pure helper facades resolve through the root too.
    assert!(dispatch_for("000000000AAA", None, &[]) == PlayDispatch::Unavailable);
    assert!(!is_sanctioned_embed(
        "https://www.youtube.com/watch?v=fixture-sweep"
    ));
}

#[test]
fn facade_exposes_the_live_db_path() {
    // The binary opens this path at startup; the contract lives at the root.
    assert_eq!(ARCHIVE_DB_PATH, "data/archive.db");
    // The hermetic empty archive (no filesystem, no fixtures) renders the
    // honest empty state the missing-db path degrades to.
    let mut app = ShellApp::empty();
    assert!(app.store().seasons().is_empty());
    app.press_play("194611010TRH");
    assert_eq!(app.last_dispatch(), Some(&PlayDispatch::Unavailable));
}
