//! Key bindings: `keys.toml` `[main]` table mapping action names to key strings.
//! Ported from `load_keys` in lfm's `preferences.py`.

use std::collections::{HashMap, HashSet};
use std::str::FromStr;

use strum::IntoEnumIterator;

use crate::actions::Action;
use crate::keys::{parse_key, Key};

pub const DEFAULT_KEYS_TOML: &str = include_str!("../../assets/default-keys.toml");

#[derive(Debug, Clone, Default)]
pub struct KeyMap {
    map: HashMap<Key, Action>,
}

impl KeyMap {
    pub fn from_toml(src: &str) -> (KeyMap, Vec<String>) {
        let mut warnings = Vec::new();
        let table: toml::Table = match src.parse() {
            Ok(t) => t,
            Err(e) => {
                warnings.push(format!("keys file has a syntax error, using defaults: {e}"));
                return (KeyMap::default_map(), warnings);
            }
        };
        let main = match table.get("main").and_then(|v| v.as_table()) {
            Some(m) => m.clone(),
            None => {
                warnings.push("keys file has no [main] table, using defaults".to_string());
                return (KeyMap::default_map(), warnings);
            }
        };
        let mut km = KeyMap::default();
        let mut bound = HashSet::new();
        for (name, value) in &main {
            let Ok(action) = Action::from_str(name) else {
                warnings.push(format!("unknown action \"{name}\" in [main], ignoring"));
                continue;
            };
            let keys: Result<Vec<Key>, String> = match value.as_array() {
                Some(items) => items
                    .iter()
                    .map(|it| {
                        let s = it.as_str().ok_or_else(|| it.to_string())?;
                        parse_key(s).map_err(|_| s.to_string())
                    })
                    .collect(),
                None => Err(value.to_string()),
            };
            let keys = match keys {
                Ok(k) => k,
                Err(bad) => {
                    warnings.push(format!(
                        "action \"{name}\" contains invalid key definition \"{bad}\", ignoring"
                    ));
                    continue;
                }
            };
            for k in keys {
                if let Some(prev) = km.map.get(&k) {
                    let prev: &str = (*prev).into();
                    warnings.push(format!(
                        "key \"{k}\" in action \"{name}\" already used in \"{prev}\", ignoring"
                    ));
                    continue;
                }
                km.map.insert(k, action);
                bound.insert(action);
            }
        }
        for a in Action::iter().filter(|a| !bound.contains(a)) {
            let a: &str = a.into();
            warnings.push(format!("missing action {a}: no key bound"));
        }
        (km, warnings)
    }

    pub fn default_map() -> KeyMap {
        let (km, warnings) = KeyMap::from_toml_strict(DEFAULT_KEYS_TOML);
        debug_assert!(
            warnings.iter().all(|w| w.starts_with("missing action")),
            "{warnings:?}"
        );
        km
    }

    // Parses without falling back to defaults, so default_map cannot recurse.
    fn from_toml_strict(src: &str) -> (KeyMap, Vec<String>) {
        assert!(
            src.parse::<toml::Table>().is_ok(),
            "built-in default keys must be valid TOML"
        );
        KeyMap::from_toml(src)
    }

    pub fn get(&self, k: &Key) -> Option<Action> {
        self.map.get(k).copied()
    }

    pub fn keys_for(&self, a: Action) -> Vec<Key> {
        let mut keys: Vec<Key> = self
            .map
            .iter()
            .filter(|(_, v)| **v == a)
            .map(|(k, _)| *k)
            .collect();
        keys.sort_by_key(|k| k.to_string());
        keys
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::parse_key;

    #[test]
    fn default_map_has_lfm_default_hotkeys() {
        let m = KeyMap::default_map();
        for (k, a) in [
            ("F5", Action::CopyFile),
            ("F6", Action::MoveFile2),
            ("tab", Action::PaneChangeFocus),
            ("C-h", Action::DotfilesToggle),
            ("backspace", Action::CursorPageup),
            ("B", Action::BookmarkSet),
            ("b", Action::BookmarkGoto),
            ("=", Action::PaneOtherTabEqual),
            ("q", Action::QuitChdir),
            ("F10", Action::QuitChdir),
            ("C-q", Action::QuitNochdir),
            ("F1", Action::HelpMenu),
            ("h", Action::HelpMenu),
            ("F12", Action::FileMenu),
            ("spc", Action::ShowCurrentDirSize),
            ("pagedown", Action::CursorPagedown),
            ("C-n", Action::CursorPagedown),
        ] {
            assert_eq!(m.get(&parse_key(k).unwrap()), Some(a), "{k}");
        }
    }

    #[test]
    fn default_toml_has_no_duplicate_or_invalid_warnings() {
        let (_, w) = KeyMap::from_toml(DEFAULT_KEYS_TOML);
        // menu-only actions have no key
        assert!(w.iter().all(|s| s.starts_with("missing action")), "{w:?}");
    }

    #[test]
    fn duplicate_key_first_wins() {
        let (m, w) = KeyMap::from_toml("[main]\ncursor_up = [\"k\"]\ncursor_down = [\"k\"]\n");
        assert_eq!(m.get(&parse_key("k").unwrap()), Some(Action::CursorUp));
        assert!(w.iter().any(|s| s.contains("cursor_down")));
    }

    #[test]
    fn unknown_action_and_bad_key_warn() {
        let (m, w) = KeyMap::from_toml("[main]\nfly = [\"x\"]\ncursor_up = [\"nope\"]\n");
        assert!(m.get(&parse_key("x").unwrap()).is_none());
        assert!(w.iter().any(|s| s.contains("fly")) && w.iter().any(|s| s.contains("nope")));
    }

    #[test]
    fn syntax_error_falls_back_to_defaults() {
        let (m, w) = KeyMap::from_toml("[main\n");
        assert_eq!(m.get(&parse_key("F5").unwrap()), Some(Action::CopyFile));
        assert_eq!(w.len(), 1);
    }

    #[test]
    fn ctrl_h_and_backspace_are_distinct_keys() {
        use crate::keys::from_crossterm;
        use crossterm::event::{KeyCode as C, KeyEvent, KeyModifiers as M};
        let m = KeyMap::default_map();
        let ctrl_h = from_crossterm(&KeyEvent::new(C::Char('h'), M::CONTROL));
        let bs = from_crossterm(&KeyEvent::new(C::Backspace, M::NONE));
        assert_eq!(m.get(&ctrl_h), Some(Action::DotfilesToggle));
        assert_eq!(m.get(&bs), Some(Action::CursorPageup));
    }
}
