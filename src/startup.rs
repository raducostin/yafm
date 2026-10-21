//! Loading of every config file at startup.

use std::path::Path;

use crate::config::history::History;
use crate::config::keymap::{KeyMap, DEFAULT_KEYS_TOML};
use crate::config::paths::ConfigPaths;
use crate::config::settings::Settings;
use crate::config::theme::{Theme, DEFAULT_THEME_TOML};

pub struct Loaded {
    pub settings: Settings,
    pub keymap: KeyMap,
    pub theme: Theme,
    pub history: History,
    /// Problems worth the user's attention, each prefixed with its file.
    pub warnings: Vec<String>,
    /// Informational notes (e.g. actions reachable only from menus).
    pub notes: Vec<String>,
}

/// Reads `path`, writing `default` to it first when it does not exist.
fn read_or_create(path: &Path, default: &str) -> Result<String, String> {
    if !path.exists() {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(path, default).map_err(|e| format!("cannot write: {e}"))?;
        return Ok(default.to_string());
    }
    std::fs::read_to_string(path).map_err(|e| format!("cannot read: {e}"))
}

fn prefixed(path: &Path, warnings: Vec<String>) -> impl Iterator<Item = String> + '_ {
    warnings
        .into_iter()
        .map(move |w| format!("{}: {w}", path.display()))
}

/// Creates missing config/keys/theme files from the defaults, then loads everything.
pub fn load_all(paths: &ConfigPaths) -> Loaded {
    let mut warnings = Vec::new();
    let mut notes = Vec::new();

    let (settings, w) = Settings::load_or_create(&paths.config());
    warnings.extend(prefixed(&paths.config(), w));

    let keymap = match read_or_create(&paths.keys(), DEFAULT_KEYS_TOML) {
        Ok(src) => {
            let (km, w) = KeyMap::from_toml(&src);
            // lfm binds several actions only through menus; that is not a problem.
            let (missing, w): (Vec<_>, Vec<_>) =
                w.into_iter().partition(|w| w.starts_with("missing action"));
            notes.extend(prefixed(&paths.keys(), missing));
            warnings.extend(prefixed(&paths.keys(), w));
            km
        }
        Err(e) => {
            warnings.push(format!("{}: {e}, using defaults", paths.keys().display()));
            KeyMap::default_map()
        }
    };

    let theme = match read_or_create(&paths.theme(), DEFAULT_THEME_TOML) {
        Ok(src) => {
            let (t, w) = Theme::from_toml(&src);
            warnings.extend(prefixed(&paths.theme(), w));
            t
        }
        Err(e) => {
            warnings.push(format!(
                "{}: {e}, using default theme",
                paths.theme().display()
            ));
            Theme::default_theme()
        }
    };

    let (history, w) = History::load(&paths.history());
    warnings.extend(prefixed(&paths.history(), w));

    Loaded {
        settings,
        keymap,
        theme,
        history,
        warnings,
        notes,
    }
}
