//! Main configuration, `config.toml`. Defaults and tolerance rules ported from
//! `Config` in lfm's `preferences.py`.

use std::collections::BTreeMap;
use std::path::Path;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use toml::{Table, Value};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum SortType {
    #[serde(rename = "none")]
    None,
    #[serde(rename = "byName")]
    ByName,
    #[serde(rename = "byExt")]
    ByExt,
    #[serde(rename = "byPath")]
    ByPath,
    #[serde(rename = "bySize")]
    BySize,
    #[serde(rename = "byMTime")]
    ByMTime,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum DiffType {
    Context,
    Unified,
    Ndiff,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Options {
    pub save_configuration_at_exit: bool,
    pub save_history_at_exit: bool,
    pub show_output_after_exec: bool,
    pub use_wide_chars: bool,
    pub rebuild_vfs: bool,
    pub detach_terminal_at_exec: bool,
    pub show_dotfiles: bool,
    pub sort_type: SortType,
    pub sort_reverse: bool,
    pub sort_mix_dirs: bool,
    pub sort_mix_cases: bool,
    /// never = -1, ask = 0, always = 1
    pub automatic_file_encoding_conversion: i8,
    pub find_ignorecase: bool,
    pub grep_ignorecase: bool,
    pub grep_regex: bool,
    /// yafm extra: function-key bar on the bottom row.
    pub show_function_keys: bool,
    /// yafm extra: sizes as 1.5K / 760K / 11G; false shows exact bytes.
    pub human_readable_sizes: bool,
}

/// The sort of one pane (yafm extra; lfm had one sort for both).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct PaneSort {
    pub sort_type: SortType,
    pub sort_reverse: bool,
}

impl PaneSort {
    /// The starting sort of a pane with no saved one.
    pub fn from_options(o: &Options) -> PaneSort {
        PaneSort {
            sort_type: o.sort_type,
            sort_reverse: o.sort_reverse,
        }
    }
}

const PANE_NAMES: [&str; 2] = ["left", "right"];

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Confirmations {
    pub delete: bool,
    pub overwrite: bool,
    pub quit: bool,
    pub ask_rebuild_vfs: bool,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Misc {
    pub backup_extension: String,
    pub diff_type: DiffType,
}

#[derive(Clone, PartialEq, Debug)]
pub struct Settings {
    pub options: Options,
    /// yafm extra: each pane's own sort, `[panes.left]` / `[panes.right]`.
    pub pane_sort: [PaneSort; 2],
    pub confirmations: Confirmations,
    pub misc: Misc,
    pub programs: BTreeMap<String, String>,
    pub files: BTreeMap<String, Vec<String>>,
    pub bookmarks: BTreeMap<char, String>,
    pub powercli_favs: Vec<String>,
}

const DEF_PROGRAMS: &[(&str, &str)] = &[
    ("shell", "bash"),
    ("pager", "less"),
    ("editor", "vi"),
    ("web", "firefox"),
    ("audio", "vlc"),
    ("video", "vlc"),
    ("graphics", "eog"),
    ("pdf", "evince"),
    ("ebook", "FBReader"),
];

const FILES_EXT: &[(&str, &[&str])] = &[
    (
        "archive",
        &[
            "gz", "bz2", "xz", "lz", "lz4", "tar", "tgz", "tbz2", "txz", "tlz", "tlz4", "Z", "zip",
            "rar", "7z", "arj", "cab", "lzh", "lha", "zoo", "arc", "ark", "rpm", "deb",
        ],
    ),
    (
        "audio",
        &[
            "au", "flac", "mid", "midi", "mp2", "mp3", "mpg", "ogg", "wma", "xm",
        ],
    ),
    (
        "data",
        &[
            "dta", "nc", "dbf", "mdn", "db", "mdb", "dat", "fox", "dbx", "mdx", "sql", "mssql",
            "msql", "ssql", "pgsql", "cdx", "dbi", "sqlite",
        ],
    ),
    (
        "devel",
        &[
            "c", "h", "cc", "hh", "cpp", "hpp", "py", "pyw", "hs", "lua", "pl", "pm", "inc", "rb",
            "asm", "pas", "f", "f90", "pov", "m", "pas", "cgi", "php", "phps", "tcl", "tk", "js",
            "java", "jav", "jasm", "vala", "glade", "ui", "diff", "patch", "css", "sh", "bash",
            "awk", "m4", "el", "st", "mak", "sl", "ada", "caml", "ml", "mli", "mly", "mll", "mlp",
            "prg",
        ],
    ),
    (
        "document",
        &[
            "txt", "text", "rtf", "odt", "odc", "odp", "abw", "gnumeric", "sxw", "sxc", "sxp",
            "sdw", "sdc", "sdp", "djvu", "dvi", "bib", "tex", "doc", "xls", "ppt", "pps", "docx",
            "xlsx", "pptx", "xml", "xsd", "xslt", "sgml", "dtd", "mail", "msg", "letter", "ics",
            "vcs", "vcard", "lsm", "po", "man", "1", "info",
        ],
    ),
    (
        "ebook",
        &[
            "azw", "azw3", "chm", "epub", "fb2", "imp", "lit", "mobi", "prc",
        ],
    ),
    (
        "graphics",
        &[
            "jpg", "jpeg", "gif", "png", "tif", "tiff", "pcx", "bmp", "xpm", "xbm", "eps", "pic",
            "rle", "ico", "wmf", "omf", "ai", "cdr", "xcf", "dwb", "dwg", "dxf", "svg", "dia",
        ],
    ),
    ("pdf", &["pdf", "ps"]),
    ("temp", &["tmp", "$$$", "~", "bak"]),
    ("web", &["html", "shtml", "htm"]),
    (
        "video",
        &[
            "acc", "avi", "asf", "flv", "mkv", "mov", "mol", "mpl", "med", "mp4", "mpg", "mpeg",
            "ogv", "swf", "ogv", "wmv",
        ],
    ),
];

const POWERCLI_FAVS: &[&str] = &[
        "mv \"$f\" \"{$f.replace('', '')}\"",
        "less \"$f\" $",
        "find \"$p\" -name \"*\" -print0 | xargs --null -0 grep -EHcni \"TODO|WARNING|FIXME|BUG\"",
        "find \"$p\" -name \"*\" -print0 | xargs --null -0 grep -EHcni \"TODO|WARNING|FIXME|BUG\" >output.txt &",
        "cp $s \"$o\"",
        "",
        "",
        "",
        "",
        "",
];

pub const BOOKMARK_KEYS: &str = "0123456789abcdefghijklmnopqrstuvwxyz";
const NUM_POWERCLI_FAVS: usize = 10;

const CONFIG_HEADER: &str = "\
# yafm configuration file
# options.sort_type: none, byName, byExt, byPath, bySize, byMTime
#   (the starting sort of a pane; [panes.left] and [panes.right] keep each pane's own)
# options.automatic_file_encoding_conversion: never = -1, ask = 0, always = 1
# misc.diff_type: context, unified, ndiff

";

impl Default for Options {
    fn default() -> Options {
        Options {
            save_configuration_at_exit: true,
            save_history_at_exit: true,
            show_output_after_exec: true,
            use_wide_chars: false,
            rebuild_vfs: false,
            detach_terminal_at_exec: true,
            show_dotfiles: true,
            sort_type: SortType::ByName,
            sort_reverse: false,
            sort_mix_dirs: false,
            sort_mix_cases: true,
            automatic_file_encoding_conversion: 0,
            find_ignorecase: false,
            grep_ignorecase: true,
            grep_regex: true,
            show_function_keys: true,
            human_readable_sizes: true,
        }
    }
}

impl Default for Confirmations {
    fn default() -> Confirmations {
        Confirmations {
            delete: true,
            overwrite: true,
            quit: true,
            ask_rebuild_vfs: true,
        }
    }
}

impl Default for Misc {
    fn default() -> Misc {
        Misc {
            backup_extension: ".bak".to_string(),
            diff_type: DiffType::Unified,
        }
    }
}

impl Default for Settings {
    fn default() -> Settings {
        let options = Options::default();
        Settings {
            pane_sort: [PaneSort::from_options(&options); 2],
            options,
            confirmations: Confirmations::default(),
            misc: Misc::default(),
            programs: DEF_PROGRAMS
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            files: FILES_EXT
                .iter()
                .map(|(k, v)| (k.to_string(), v.iter().map(|e| e.to_lowercase()).collect()))
                .collect(),
            bookmarks: BOOKMARK_KEYS
                .chars()
                .map(|c| (c, "/".to_string()))
                .collect(),
            powercli_favs: POWERCLI_FAVS.iter().map(|s| s.to_string()).collect(),
        }
    }
}

/// Applies each user key on top of the defaults one at a time, so a bad value
/// only loses that key (lfm's per-key tolerance).
fn merge_section<T: Serialize + DeserializeOwned>(
    defaults: &T,
    user: &Table,
    section: &str,
    warnings: &mut Vec<String>,
) -> T {
    let mut base = Table::try_from(defaults).expect("defaults serialize");
    for (k, v) in user {
        let Some(old) = base.insert(k.clone(), v.clone()) else {
            base.remove(k);
            warnings.push(format!("unknown \"{k}\" in section [{section}], ignoring"));
            continue;
        };
        if T::deserialize(Value::Table(base.clone())).is_err() {
            warnings.push(format!(
                "invalid value {v} for \"{k}\" in section [{section}], using default"
            ));
            base.insert(k.clone(), old);
        }
    }
    T::deserialize(Value::Table(base)).expect("merged section deserializes")
}

impl Settings {
    pub fn from_toml(src: &str) -> (Settings, Vec<String>) {
        let mut warnings = Vec::new();
        let mut s = Settings::default();
        let table: Table = match src.parse() {
            Ok(t) => t,
            Err(e) => {
                warnings.push(format!(
                    "configuration file has a syntax error, using defaults: {e}"
                ));
                return (s, warnings);
            }
        };
        let mut panes = &Table::new();
        for (section, value) in &table {
            let Some(sect) = value.as_table() else {
                warnings.push(format!("\"{section}\" must be a table, ignoring"));
                continue;
            };
            match section.as_str() {
                "options" => {
                    s.options = merge_section(&s.options, sect, section, &mut warnings);
                    if !(-1..=1).contains(&s.options.automatic_file_encoding_conversion) {
                        warnings.push(
                            "automatic_file_encoding_conversion must be -1, 0 or 1, using default"
                                .to_string(),
                        );
                        s.options.automatic_file_encoding_conversion = 0;
                    }
                }
                "confirmations" => {
                    s.confirmations = merge_section(&s.confirmations, sect, section, &mut warnings)
                }
                "misc" => s.misc = merge_section(&s.misc, sect, section, &mut warnings),
                "programs" => {
                    for (k, v) in sect {
                        match (s.programs.contains_key(k), v.as_str()) {
                            (true, Some(prog)) => {
                                s.programs.insert(k.clone(), prog.trim().to_string());
                            }
                            (true, None) => warnings.push(format!(
                                "invalid value {v} for \"{k}\" in section [programs]"
                            )),
                            (false, _) => warnings
                                .push(format!("unknown \"{k}\" in section [programs], ignoring")),
                        }
                    }
                }
                "files" => {
                    for (k, v) in sect {
                        let exts: Option<Vec<String>> = v.as_array().and_then(|a| {
                            a.iter()
                                .map(|e| e.as_str().map(|e| e.trim().to_lowercase()))
                                .collect()
                        });
                        match (s.files.contains_key(k), exts) {
                            (true, Some(exts)) => {
                                s.files.insert(k.clone(), exts);
                            }
                            (true, None) => warnings
                                .push(format!("invalid value {v} for \"{k}\" in section [files]")),
                            (false, _) => warnings
                                .push(format!("unknown \"{k}\" in section [files], ignoring")),
                        }
                    }
                }
                "bookmarks" => {
                    for (k, v) in sect {
                        let mut chars = k.chars();
                        let key = match (chars.next(), chars.next()) {
                            (Some(c), None) if BOOKMARK_KEYS.contains(c) => c,
                            _ => {
                                warnings.push(format!(
                                    "unknown bookmark key \"{k}\" (valid: 0-9, a-z), ignoring"
                                ));
                                continue;
                            }
                        };
                        match v.as_str() {
                            Some(path) => {
                                s.bookmarks.insert(key, path.trim().to_string());
                            }
                            None => {
                                warnings.push(format!("invalid value {v} for bookmark \"{k}\""))
                            }
                        }
                    }
                }
                "powercli" => {
                    for (k, v) in sect {
                        let favs: Option<Vec<String>> = v.as_array().and_then(|a| {
                            a.iter().map(|e| e.as_str().map(str::to_string)).collect()
                        });
                        match (k.as_str(), favs) {
                            ("favs", Some(mut favs)) => {
                                favs.resize(NUM_POWERCLI_FAVS, String::new());
                                s.powercli_favs = favs;
                            }
                            ("favs", None) => warnings.push(format!(
                                "invalid value {v} for \"favs\" in section [powercli]"
                            )),
                            _ => warnings
                                .push(format!("unknown \"{k}\" in section [powercli], ignoring")),
                        }
                    }
                }
                // read after the loop: a pane's starting sort comes from [options]
                "panes" => panes = sect,
                _ => warnings.push(format!("unknown section [{section}], ignoring")),
            }
        }
        let start = PaneSort::from_options(&s.options);
        for (k, v) in panes {
            if !PANE_NAMES.contains(&k.as_str()) || !v.is_table() {
                warnings.push(format!("unknown \"{k}\" in section [panes], ignoring"));
            }
        }
        for (i, name) in PANE_NAMES.iter().enumerate() {
            s.pane_sort[i] = match panes.get(*name).and_then(Value::as_table) {
                Some(t) => merge_section(&start, t, &format!("panes.{name}"), &mut warnings),
                None => start,
            };
        }
        (s, warnings)
    }

    pub fn to_toml(&self) -> String {
        let mut t = Table::new();
        t.insert(
            "options".into(),
            Value::Table(Table::try_from(&self.options).expect("serialize")),
        );
        let mut panes = Table::new();
        for (name, sort) in PANE_NAMES.iter().zip(&self.pane_sort) {
            panes.insert(
                name.to_string(),
                Value::Table(Table::try_from(sort).expect("serialize")),
            );
        }
        t.insert("panes".into(), Value::Table(panes));
        t.insert(
            "confirmations".into(),
            Value::Table(Table::try_from(&self.confirmations).expect("serialize")),
        );
        t.insert(
            "misc".into(),
            Value::Table(Table::try_from(&self.misc).expect("serialize")),
        );
        t.insert(
            "programs".into(),
            Value::Table(Table::try_from(&self.programs).expect("serialize")),
        );
        t.insert(
            "files".into(),
            Value::Table(Table::try_from(&self.files).expect("serialize")),
        );
        let bookmarks: BTreeMap<String, &String> = self
            .bookmarks
            .iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect();
        t.insert(
            "bookmarks".into(),
            Value::Table(Table::try_from(bookmarks).expect("serialize")),
        );
        let mut powercli = Table::new();
        powercli.insert(
            "favs".into(),
            Value::try_from(&self.powercli_favs).expect("serialize"),
        );
        t.insert("powercli".into(), Value::Table(powercli));
        format!("{CONFIG_HEADER}{}", toml::to_string(&t).expect("serialize"))
    }

    /// Missing file: defaults, written to `path`. Unreadable or invalid: defaults, file untouched.
    pub fn load_or_create(path: &Path) -> (Settings, Vec<String>) {
        if !path.exists() {
            let s = Settings::default();
            let mut warnings = Vec::new();
            if let Err(e) = s.save(path) {
                warnings.push(format!(
                    "cannot write configuration file \"{}\": {e}",
                    path.display()
                ));
            }
            return (s, warnings);
        }
        match std::fs::read_to_string(path) {
            Ok(src) => Settings::from_toml(&src),
            Err(e) => (
                Settings::default(),
                vec![format!(
                    "cannot read configuration file \"{}\": {e}",
                    path.display()
                )],
            ),
        }
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, self.to_toml())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_python() {
        let s = Settings::default();
        assert!(s.options.show_dotfiles && !s.options.sort_reverse && s.options.sort_mix_cases);
        assert_eq!(s.options.sort_type, SortType::ByName);
        assert_eq!(s.misc.backup_extension, ".bak");
        assert_eq!(s.programs["pager"], "less");
        assert_eq!(s.programs["editor"], "vi");
        assert!(s.files["devel"].contains(&"py".to_string()));
        assert_eq!(s.bookmarks.len(), 36);
        assert_eq!(s.bookmarks[&'a'], "/");
        assert_eq!(s.powercli_favs.len(), 10);
        assert!(s.options.human_readable_sizes);
    }

    #[test]
    fn round_trip() {
        let mut s = Settings::default();
        s.options.sort_type = SortType::BySize;
        s.bookmarks.insert('3', "/tmp".into());
        let (back, w) = Settings::from_toml(&s.to_toml());
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(back.options.sort_type, SortType::BySize);
        assert_eq!(back.bookmarks[&'3'], "/tmp");
        assert_eq!(back, s);
    }

    #[test]
    fn pane_sorts_default_to_name() {
        let by_name = PaneSort {
            sort_type: SortType::ByName,
            sort_reverse: false,
        };
        assert_eq!(Settings::default().pane_sort, [by_name, by_name]);
    }

    #[test]
    fn old_config_seeds_both_pane_sorts_from_options() {
        let (s, w) =
            Settings::from_toml("[options]\nsort_type = \"bySize\"\nsort_reverse = true\n");
        assert!(w.is_empty(), "{w:?}");
        let by_size_rev = PaneSort {
            sort_type: SortType::BySize,
            sort_reverse: true,
        };
        assert_eq!(s.pane_sort, [by_size_rev, by_size_rev]);
    }

    #[test]
    fn pane_sorts_round_trip_and_partial_sections() {
        let mut s = Settings::default();
        s.pane_sort[0] = PaneSort {
            sort_type: SortType::ByMTime,
            sort_reverse: true,
        };
        let toml = s.to_toml();
        assert!(
            toml.contains("[panes.left]") && toml.contains("[panes.right]"),
            "{toml}"
        );
        let (back, w) = Settings::from_toml(&toml);
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(back.pane_sort, s.pane_sort);
        // only the left pane saved: the right one starts from options
        let (s, w) = Settings::from_toml(
            "[options]\nsort_type = \"byExt\"\n[panes.left]\nsort_type = \"byMTime\"\n",
        );
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(s.pane_sort[0].sort_type, SortType::ByMTime);
        assert!(!s.pane_sort[0].sort_reverse);
        assert_eq!(s.pane_sort[1].sort_type, SortType::ByExt);
        // bad values warn and keep the starting sort
        let (s, w) = Settings::from_toml(
            "[panes.left]\nsort_type = \"bySmell\"\n[panes.middle]\nsort_type = \"byName\"\n",
        );
        assert_eq!(s.pane_sort[0].sort_type, SortType::ByName);
        assert_eq!(w.len(), 2, "{w:?}");
    }

    #[test]
    fn bad_value_keeps_default_and_warns() {
        let (s, w) = Settings::from_toml(
            "[options]\nshow_dotfiles = \"yes\"\nfoo = 1\n[misc]\ndiff_type = \"side\"\n",
        );
        assert!(s.options.show_dotfiles);
        assert_eq!(s.misc.diff_type, DiffType::Unified);
        assert_eq!(w.len(), 3, "{w:?}");
    }

    #[test]
    fn extensions_lowercased() {
        let (s, _) = Settings::from_toml("[files]\naudio = [\"MP3\", \"Flac\"]\n");
        assert_eq!(s.files["audio"], vec!["mp3", "flac"]);
    }

    #[test]
    fn unknown_program_and_bookmark_keys_warn() {
        let (s, w) = Settings::from_toml(
            "[programs]\nbrowser = \"x\"\n[bookmarks]\n\"A\" = \"/x\"\n\"b\" = \"/b\"\n",
        );
        assert!(!s.programs.contains_key("browser"));
        assert_eq!(s.bookmarks[&'b'], "/b");
        assert_eq!(w.len(), 2, "{w:?}");
    }

    #[test]
    fn syntax_error_defaults_and_file_untouched() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("config.toml");
        std::fs::write(&p, "[options\n").unwrap();
        let (s, w) = Settings::load_or_create(&p);
        assert_eq!(s.options.sort_type, SortType::ByName);
        assert_eq!(w.len(), 1);
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "[options\n");
    }

    #[test]
    fn missing_file_is_created() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("config.toml");
        let _ = Settings::load_or_create(&p);
        assert!(p.exists());
    }
}
