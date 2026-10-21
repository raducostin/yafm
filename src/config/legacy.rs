//! One-time import of lfm 3.1 config files (`lfm.ini`, `lfm.keys`, `lfm.theme`)
//! into yafm's TOML files. Legacy files are only read, never modified.

use std::path::PathBuf;

use toml::{Table, Value};

use crate::config::ini::parse_ini;
use crate::config::paths::ConfigPaths;
use crate::config::settings::Settings;

#[derive(Debug, Default)]
pub struct ImportReport {
    pub written: Vec<PathBuf>,
    pub skipped: Vec<PathBuf>,
    pub warnings: Vec<String>,
}

const KEYS_HEADER: &str = "# yafm key bindings (imported from lfm.keys)\n\n";
const THEME_HEADER: &str = "# yafm color theme (imported from lfm.theme)\n# Format is:   item = \"foreground background\"  or  item = \"=other_item\"\n\n";

/// Converts a legacy file's contents into TOML plus warnings.
type Converter = fn(&str) -> (String, Vec<String>);

/// For each legacy file present in `paths.legacy_dir`, writes the TOML
/// equivalent in `paths.dir`, unless that target exists and `force` is false.
pub fn import_legacy(paths: &ConfigPaths, force: bool) -> std::io::Result<ImportReport> {
    let mut report = ImportReport::default();
    let jobs: [(PathBuf, PathBuf, Converter); 3] = [
        (paths.legacy_ini(), paths.config(), |src| {
            let (s, w) = ini_to_settings(src);
            (s.to_toml(), w)
        }),
        (paths.legacy_keys(), paths.keys(), |src| {
            let (t, w) = ini_to_keys_toml(src);
            (format!("{KEYS_HEADER}{t}"), w)
        }),
        (paths.legacy_theme(), paths.theme(), |src| {
            let (t, w) = ini_to_theme_toml(src);
            (format!("{THEME_HEADER}{t}"), w)
        }),
    ];
    for (legacy, target, convert) in jobs {
        if !legacy.exists() {
            continue;
        }
        if target.exists() && !force {
            report.skipped.push(target);
            continue;
        }
        let (contents, warnings) = convert(&std::fs::read_to_string(&legacy)?);
        report.warnings.extend(
            warnings
                .into_iter()
                .map(|w| format!("{}: {w}", legacy.display())),
        );
        std::fs::create_dir_all(&paths.dir)?;
        std::fs::write(&target, contents)?;
        report.written.push(target);
    }
    Ok(report)
}

/// Python wrote booleans as 0/1 and `sort_type` as `SortType.byName`.
fn legacy_scalar(section: &str, key: &str, value: &str) -> Value {
    match (section, key) {
        ("Options", "sort_type") => {
            Value::String(value.strip_prefix("SortType.").unwrap_or(value).to_string())
        }
        ("Options", "automatic_file_encoding_conversion") => match value.parse::<i64>() {
            Ok(n) => Value::Integer(n),
            Err(_) => Value::String(value.to_string()),
        },
        ("Options" | "Confirmations", _) => match value.parse::<i64>() {
            Ok(n) => Value::Boolean(n != 0),
            Err(_) => Value::String(value.to_string()),
        },
        _ => Value::String(value.to_string()),
    }
}

pub fn ini_to_settings(src: &str) -> (Settings, Vec<String>) {
    let mut warnings = Vec::new();
    let mut root = Table::new();
    for (section, entries) in parse_ini(src) {
        let (name, table) = match section.as_str() {
            "Files" => (
                "files",
                entries
                    .iter()
                    .map(|(k, v)| {
                        let exts = v
                            .split(',')
                            .map(|e| Value::String(e.trim().to_string()))
                            .filter(|e| e.as_str() != Some(""));
                        (k.clone(), Value::Array(exts.collect()))
                    })
                    .collect::<Table>(),
            ),
            "PowerCLI Favs" => {
                let favs = entries
                    .iter()
                    .map(|(_, v)| Value::String(v.clone()))
                    .collect();
                (
                    "powercli",
                    Table::from_iter([("favs".to_string(), Value::Array(favs))]),
                )
            }
            "Options" | "Confirmations" | "Misc" | "Programs" | "Bookmarks" => (
                match section.as_str() {
                    "Options" => "options",
                    "Confirmations" => "confirmations",
                    "Misc" => "misc",
                    "Programs" => "programs",
                    _ => "bookmarks",
                },
                entries
                    .iter()
                    .map(|(k, v)| (k.clone(), legacy_scalar(&section, k, v)))
                    .collect::<Table>(),
            ),
            _ => {
                warnings.push(format!("unknown section [{section}], ignoring"));
                continue;
            }
        };
        root.insert(name.to_string(), Value::Table(table));
    }
    let (settings, more) = Settings::from_toml(&toml::to_string(&root).expect("serialize"));
    warnings.extend(more);
    (settings, warnings)
}

/// Converts the one `[section]` of a legacy keys/theme file with `convert`
/// applied to each value.
fn convert_single_section(
    src: &str,
    from: &str,
    to: &str,
    convert: fn(&str) -> Value,
) -> (String, Vec<String>) {
    let mut warnings = Vec::new();
    let mut table = Table::new();
    for (section, entries) in parse_ini(src) {
        if section != from {
            warnings.push(format!("unknown section [{section}], ignoring"));
            continue;
        }
        for (k, v) in entries {
            table.insert(k, convert(&v));
        }
    }
    if table.is_empty() {
        warnings.push(format!("no [{from}] section found"));
    }
    let root = Table::from_iter([(to.to_string(), Value::Table(table))]);
    (toml::to_string(&root).expect("serialize"), warnings)
}

pub fn ini_to_keys_toml(src: &str) -> (String, Vec<String>) {
    convert_single_section(src, "Main", "main", |v| {
        Value::Array(
            v.split_whitespace()
                .map(|k| Value::String(k.to_string()))
                .collect(),
        )
    })
}

pub fn ini_to_theme_toml(src: &str) -> (String, Vec<String>) {
    convert_single_section(src, "Colors", "colors", |v| {
        Value::String(v.split_whitespace().collect::<Vec<_>>().join(" "))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::Action;
    use crate::config::keymap::KeyMap;
    use crate::config::settings::SortType;
    use crate::config::theme::{ColorItem, Theme};
    use strum::IntoEnumIterator;

    #[test]
    fn settings_from_python_ini() {
        let (s, w) = ini_to_settings(include_str!("../../tests/fixtures/legacy/lfm.ini"));
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(s.options.sort_type, SortType::BySize);
        assert_eq!(s.pane_sort[1].sort_type, SortType::BySize);
        assert!(!s.options.show_dotfiles);
        assert_eq!(s.files["audio"], vec!["mp3", "flac"]);
        assert_eq!(s.bookmarks[&'3'], "/tmp");
        assert_eq!(s.powercli_favs[0], "less \"$f\" $");
        assert_eq!(s.powercli_favs[9], "");
    }

    #[test]
    fn sort_type_without_prefix_is_accepted() {
        let (s, w) = ini_to_settings("[Options]\nsort_type: byExt\n");
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(s.options.sort_type, SortType::ByExt);
    }

    #[test]
    fn default_keys_import_equals_builtin_defaults() {
        let (toml, w) = ini_to_keys_toml(include_str!("../../tests/fixtures/legacy/lfm.keys"));
        assert!(w.is_empty(), "{w:?}");
        let (m, _) = KeyMap::from_toml(&toml);
        for a in Action::iter() {
            let mut imported = m.keys_for(a);
            // yafm extras that lfm's key file cannot contain
            if a == Action::HelpMenu {
                // F1 also opens help (lfm only showed "F1=Help")
                imported.insert(0, crate::keys::parse_key("F1").unwrap());
            }
            // Space sizes the directory under the cursor instead of paging down
            let spc = crate::keys::parse_key("spc").unwrap();
            if a == Action::CursorPagedown {
                imported.retain(|k| *k != spc);
            }
            if a == Action::ShowCurrentDirSize {
                imported.push(spc);
            }
            if a == Action::ToggleHumanSizes {
                imported.push(crate::keys::parse_key("H").unwrap());
            }
            assert_eq!(imported, KeyMap::default_map().keys_for(a), "{a:?}");
        }
    }

    #[test]
    fn default_theme_import_equals_builtin() {
        let (toml, _) = ini_to_theme_toml(include_str!("../../tests/fixtures/legacy/lfm.theme"));
        let (t, w) = Theme::from_toml(&toml);
        assert!(w.is_empty(), "{w:?}");
        for i in ColorItem::iter() {
            assert_eq!(t.get(i), Theme::default_theme().get(i));
        }
    }
}
