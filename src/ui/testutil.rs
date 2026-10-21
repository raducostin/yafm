//! Shared helpers for UI tests.

use std::path::Path;

use crate::config::paths::ConfigPaths;
use crate::ui::state::App;

/// An `App` on `d` (both panes) with a few files, config under `d/cfg`.
pub fn test_app(d: &Path, w: u16, h: u16) -> App {
    for n in ["a.txt", "b.rs", ".dot"] {
        std::fs::write(d.join(n), n).unwrap();
    }
    std::fs::create_dir(d.join("sub")).unwrap();
    std::fs::write(d.join("sub/inner"), b"").unwrap();
    let paths = ConfigPaths::from_base(&d.join("cfg"), &d.join("home"));
    let loaded = crate::startup::load_all(&paths);
    App::new(loaded, paths, d, d, w, h).unwrap()
}
