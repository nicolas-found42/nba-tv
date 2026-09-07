//! Native Shell entry point. Headless CI never launches this —
//! all navigation logic is unit-tested through the library.

use nbatv_shell::ShellApp;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions::default();
    eframe::run_native(
        "NBA TV Archive",
        options,
        Box::new(|_cc| Ok(Box::new(ShellApp::new()))),
    )
}
