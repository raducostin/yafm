//! Starting and stopping the terminal UI.

use std::path::{Path, PathBuf};

use crate::config::paths::ConfigPaths;
use crate::startup::Loaded;
use crate::ui::actions::event_loop;
use crate::ui::screen::{install_panic_hook, CrosstermScreen, Screen};
use crate::ui::state::App;

pub const MIN_COLUMNS: u16 = 66;

/// Runs the UI until the user quits; returns the directory to change to (quit
/// with chdir). The terminal is restored before settings and history are saved.
pub fn run_ui(
    loaded: Loaded,
    paths: ConfigPaths,
    path1: &Path,
    path2: &Path,
) -> std::io::Result<Option<PathBuf>> {
    install_panic_hook();
    let (result, app) = {
        let mut screen = CrosstermScreen::new()?;
        let (w, h) = screen.size();
        if w < MIN_COLUMNS {
            return Err(std::io::Error::other(format!(
                "Terminal too narrow to show contents.\nIt should have {MIN_COLUMNS} columns at minimum."
            )));
        }
        let mut app = App::new(loaded, paths, path1, path2, w, h)?;
        (event_loop(&mut screen, &mut app), app)
    };
    if app.settings.options.save_configuration_at_exit {
        if let Err(e) = app.settings.save(&app.paths.config()) {
            println!("yafm - ERROR: Cannot save configuration file\n{e}");
        }
    }
    if app.settings.options.save_history_at_exit {
        if let Err(e) = app.history.save(&app.paths.history()) {
            println!("yafm - ERROR: Cannot save history file\n{e}");
        }
    }
    result
}
