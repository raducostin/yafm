//! Color theme: `theme.toml` `[colors]` table, values `"fg bg"` or `"=other_item"`.
//! Ported from `load_colortheme` in lfm's `preferences.py`.

use std::collections::HashMap;

use ratatui::style::Color;
use std::str::FromStr;

use strum::{EnumIter, EnumString, IntoEnumIterator, IntoStaticStr};

pub const DEFAULT_THEME_TOML: &str = include_str!("../../assets/default-theme.toml");

#[derive(EnumString, EnumIter, IntoStaticStr, Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[strum(serialize_all = "snake_case")]
pub enum ColorItem {
    Header,
    TabActive,
    TabInactive,
    PaneActive,
    PaneInactive,
    PaneHeaderPath,
    PaneHeaderTitles,
    Statusbar,
    PowercliPrompt,
    PowercliText,
    SelectedFiles,
    Cursor,
    CursorSelected,
    FilesDir,
    FilesExe,
    FilesReg,
    FilesArchive,
    FilesAudio,
    FilesData,
    FilesDevel,
    FilesDocument,
    FilesEbook,
    FilesGraphics,
    FilesPdf,
    FilesTemp,
    FilesVideo,
    FilesWeb,
    Dialog,
    DialogTitle,
    ButtonActive,
    ButtonInactive,
    DialogError,
    DialogErrorTitle,
    DialogErrorText,
    DialogPerms,
    Selectitem,
    SelectitemTitle,
    SelectitemCursor,
    Entryline,
    ProgressbarFg,
    ProgressbarBg,
    ViewWhiteOnBlack,
    ViewRedOnBlack,
    ViewBlueOnBlack,
    ViewGreenOnBlack,
}

#[derive(Debug, Clone)]
pub struct Theme {
    colors: HashMap<ColorItem, (Color, Color)>,
}

impl Theme {
    pub fn from_toml(src: &str) -> (Theme, Vec<String>) {
        let mut warnings = Vec::new();
        let colors_table = match src.parse::<toml::Table>() {
            Ok(t) => t.get("colors").and_then(|v| v.as_table()).cloned(),
            Err(e) => {
                warnings.push(format!(
                    "theme file has a syntax error, using default theme: {e}"
                ));
                return (Theme::default_theme(), warnings);
            }
        };
        let Some(colors_table) = colors_table else {
            warnings.push("theme file has no [colors] table, using default theme".to_string());
            return (Theme::default_theme(), warnings);
        };
        let mut colors = HashMap::new();
        let mut refs = Vec::new();
        for (name, value) in &colors_table {
            let Ok(item) = ColorItem::from_str(&name.trim().to_lowercase()) else {
                warnings.push(format!("color item invalid: {name}"));
                continue;
            };
            let Some(desc) = value.as_str() else {
                warnings.push(format!("color item {name} must be a string: {value}"));
                continue;
            };
            if let Some(target) = desc.strip_prefix('=') {
                match ColorItem::from_str(target.trim()) {
                    Ok(t) => refs.push((item, t)),
                    Err(_) => warnings.push(format!(
                        "color item pointed to an invalid item: {name}: {target}"
                    )),
                }
                continue;
            }
            let parts: Vec<&str> = desc.split_whitespace().collect();
            match (
                parts.as_slice(),
                parts.first().and_then(|f| parse_color(f)),
                parts.get(1).and_then(|b| parse_color(b)),
            ) {
                ([_, _], Some(fg), Some(bg)) => {
                    colors.insert(item, (fg, bg));
                }
                _ => warnings.push(format!(
                    "color item contains invalid colors: {name}: {desc}"
                )),
            }
        }
        for (item, target) in refs {
            match colors.get(&target).copied() {
                Some(c) => {
                    colors.insert(item, c);
                }
                None => {
                    let (i, t): (&str, &str) = (item.into(), target.into());
                    warnings.push(format!("color item pointed to undefined item: {i}: {t}"));
                }
            }
        }
        for item in ColorItem::iter() {
            colors.entry(item).or_insert_with(|| {
                let name: &str = item.into();
                warnings.push(format!(
                    "missing color item {name}, added as white on black"
                ));
                (Color::Gray, Color::Black)
            });
        }
        (Theme { colors }, warnings)
    }

    pub fn default_theme() -> Theme {
        let (theme, warnings) = Theme::from_toml(DEFAULT_THEME_TOML);
        debug_assert!(warnings.is_empty(), "{warnings:?}");
        theme
    }

    pub fn get(&self, item: ColorItem) -> (Color, Color) {
        self.colors[&item]
    }
}

/// lfm color names; a trailing `*` selects the intense variant.
pub fn parse_color(s: &str) -> Option<Color> {
    let s = s.to_lowercase();
    let (name, bright) = match s.strip_suffix('*') {
        Some(n) => (n, true),
        None => (s.as_str(), false),
    };
    let c = match (name, bright) {
        ("black", false) => Color::Black,
        ("red", false) => Color::Red,
        ("green", false) => Color::Green,
        ("yellow", false) => Color::Yellow,
        ("blue", false) => Color::Blue,
        ("magenta", false) => Color::Magenta,
        ("cyan", false) => Color::Cyan,
        ("white", false) => Color::Gray,
        ("black", true) => Color::DarkGray,
        ("red", true) => Color::LightRed,
        ("green", true) => Color::LightGreen,
        ("yellow", true) => Color::LightYellow,
        ("blue", true) => Color::LightBlue,
        ("magenta", true) => Color::LightMagenta,
        ("cyan", true) => Color::LightCyan,
        ("white", true) => Color::White,
        _ => return None,
    };
    Some(c)
}

#[cfg(test)]
mod tests {
    use super::*;
    use strum::IntoEnumIterator;

    #[test]
    fn has_45_items() {
        assert_eq!(ColorItem::iter().count(), 45);
    }

    #[test]
    fn default_theme_is_clean() {
        let (_, w) = Theme::from_toml(DEFAULT_THEME_TOML);
        assert!(w.is_empty(), "{w:?}");
    }

    #[test]
    fn colors_and_intensity() {
        assert_eq!(parse_color("red*"), Some(Color::LightRed));
        assert_eq!(parse_color("WHITE"), Some(Color::Gray));
        assert_eq!(parse_color("pink"), None);
    }

    #[test]
    fn references_resolve() {
        let t = Theme::default_theme();
        assert_eq!(t.get(ColorItem::TabInactive), t.get(ColorItem::Header));
        assert_eq!(t.get(ColorItem::Header), (Color::Yellow, Color::Blue));
    }

    #[test]
    fn missing_item_is_white_on_black_with_warning() {
        let (t, w) = Theme::from_toml("[colors]\nheader = \"red blue\"\n");
        assert_eq!(t.get(ColorItem::Cursor), (Color::Gray, Color::Black));
        assert!(w.iter().any(|s| s.contains("cursor")));
    }

    #[test]
    fn bad_reference_warns() {
        let (_, w) = Theme::from_toml("[colors]\nheader = \"=nothing\"\n");
        assert!(w.iter().any(|s| s.contains("nothing")));
    }
}
