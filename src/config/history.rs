//! Input history per dialog kind, `history.json`. Ported from `History` in
//! lfm's `preferences.py` (which used pickle).

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use strum::{EnumIter, IntoEnumIterator};

pub const HISTORY_MAX: usize = 100;

#[derive(
    Serialize, Deserialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, EnumIter,
)]
#[serde(rename_all = "lowercase")]
pub enum HistorySection {
    File,
    Path,
    Glob,
    Find,
    Grep,
    Exec,
    Cli,
}

#[derive(Debug, Clone, PartialEq)]
pub struct History {
    data: BTreeMap<HistorySection, Vec<String>>,
}

impl History {
    pub fn new() -> History {
        History {
            data: HistorySection::iter().map(|s| (s, Vec::new())).collect(),
        }
    }

    /// Empty entries are ignored; a repeated entry moves to the end; only the
    /// last `HISTORY_MAX` are kept.
    pub fn append(&mut self, s: HistorySection, entry: &str) {
        if entry.is_empty() {
            return;
        }
        let v = self.data.entry(s).or_default();
        v.retain(|e| e != entry);
        v.push(entry.to_string());
        if v.len() > HISTORY_MAX {
            v.drain(..v.len() - HISTORY_MAX);
        }
    }

    pub fn entries(&self, s: HistorySection) -> &[String] {
        self.data.get(&s).map(Vec::as_slice).unwrap_or(&[])
    }

    /// Missing file: empty history. Unreadable or invalid: empty history plus a warning.
    pub fn load(path: &Path) -> (History, Vec<String>) {
        if !path.exists() {
            return (History::new(), Vec::new());
        }
        let parsed = std::fs::read_to_string(path)
            .map_err(|e| e.to_string())
            .and_then(|src| {
                serde_json::from_str::<BTreeMap<HistorySection, Vec<String>>>(&src)
                    .map_err(|e| e.to_string())
            });
        match parsed {
            Ok(data) => {
                let mut h = History::new();
                for (s, entries) in data {
                    for e in entries {
                        h.append(s, &e);
                    }
                }
                (h, Vec::new())
            }
            Err(e) => (
                History::new(),
                vec![format!(
                    "cannot load history file \"{}\": {e}",
                    path.display()
                )],
            ),
        }
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(&self.data).map_err(std::io::Error::other)?;
        std::fs::write(path, json)
    }
}

impl Default for History {
    fn default() -> History {
        History::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_dedupes_to_end() {
        let mut h = History::new();
        for e in ["a", "b", "a", ""] {
            h.append(HistorySection::Path, e);
        }
        assert_eq!(h.entries(HistorySection::Path), ["b", "a"]);
    }

    #[test]
    fn caps_at_100() {
        let mut h = History::new();
        for i in 0..150 {
            h.append(HistorySection::Cli, &i.to_string());
        }
        let e = h.entries(HistorySection::Cli);
        assert_eq!(e.len(), 100);
        assert_eq!(e[0], "50");
    }

    #[test]
    fn save_load_round_trip_and_bad_json() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("history.json");
        let mut h = History::new();
        h.append(HistorySection::Grep, "TODO");
        h.save(&p).unwrap();
        assert_eq!(History::load(&p).0.entries(HistorySection::Grep), ["TODO"]);
        std::fs::write(&p, "{oops").unwrap();
        let (h2, w) = History::load(&p);
        assert!(h2.entries(HistorySection::Grep).is_empty());
        assert_eq!(w.len(), 1);
    }

    #[test]
    fn missing_file_is_empty_without_warning() {
        let d = tempfile::tempdir().unwrap();
        let (h, w) = History::load(&d.path().join("history.json"));
        assert!(w.is_empty());
        assert!(h.entries(HistorySection::File).is_empty());
    }
}
