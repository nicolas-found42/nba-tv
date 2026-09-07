//! Facade guard: the crate root must keep re-exporting the public surface
//! (`store`, `model`, `handover`, `embed`, `route`) so external consumers
//! (and the driver binary) name one path. Regression pin for the T5 review,
//! which caught the `store` re-export being dropped.

use nbatv_shell::{dispatch_for, is_sanctioned_embed, FixtureStore, PlayDispatch, Route, ShellApp};

#[test]
fn facade_re_exports_the_public_surface() {
    // Store facade: fixtures load through the re-export.
    let store = FixtureStore::fixture();
    assert!(!store.seasons().is_empty());

    // App + route + handover facades compose headlessly.
    let mut app = ShellApp::new();
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
