//! A local directory listing with lfm's filter, dotfile and sort rules.
//! Ported from `FSConfig`, `BaseFolder` and `LocalFolder` in folders.py.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use crate::config::settings::{Options, PaneSort, SortType};
use crate::fs::entry::FsEntry;
use crate::fs::path::normpath;

#[derive(Clone, Debug, PartialEq)]
pub struct FsConfig {
    pub filters: String,
    pub show_dotfiles: bool,
    pub sort_type: SortType,
    pub sort_reverse: bool,
    pub sort_mix_dirs: bool,
    pub sort_mix_cases: bool,
}

impl Default for FsConfig {
    fn default() -> FsConfig {
        FsConfig::from_options(&Options::default())
    }
}

impl FsConfig {
    pub fn from_options(o: &Options) -> FsConfig {
        FsConfig {
            filters: String::new(),
            show_dotfiles: o.show_dotfiles,
            sort_type: o.sort_type,
            sort_reverse: o.sort_reverse,
            sort_mix_dirs: o.sort_mix_dirs,
            sort_mix_cases: o.sort_mix_cases,
        }
    }

    /// The same settings with one pane's own sort.
    pub fn with_sort(self, s: PaneSort) -> FsConfig {
        FsConfig {
            sort_type: s.sort_type,
            sort_reverse: s.sort_reverse,
            ..self
        }
    }

    fn sort_key(&self, e: &FsEntry) -> SortKey {
        let text = |s: String| {
            SortKey::Text(if self.sort_mix_cases {
                s.to_lowercase()
            } else {
                s
            })
        };
        match self.sort_type {
            SortType::None => SortKey::Num(0),
            SortType::ByName => text(e.display_name()),
            SortType::ByExt => text(e.ext.clone()),
            SortType::ByPath => text(e.path.to_string_lossy().into_owned()),
            SortType::BySize => SortKey::Num(e.size as i64),
            SortType::ByMTime => SortKey::Num(e.mtime),
        }
    }

    fn sort(&self, entries: &mut [FsEntry]) {
        let mut keyed: Vec<(SortKey, FsEntry)> = entries
            .iter()
            .map(|e| (self.sort_key(e), e.clone()))
            .collect();
        // Stable either way, so equal keys keep readdir order, like Python's sort(reverse=True).
        if self.sort_reverse {
            keyed.sort_by(|a, b| b.0.cmp(&a.0));
        } else {
            keyed.sort_by(|a, b| a.0.cmp(&b.0));
        }
        for (slot, (_, e)) in entries.iter_mut().zip(keyed) {
            *slot = e;
        }
    }
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum SortKey {
    Num(i64),
    Text(String),
}

/// Comma-separated globs; `!glob` shows, a plain glob hides, show wins.
struct Filters {
    hide: Vec<glob::Pattern>,
    show: Vec<glob::Pattern>,
}

impl Filters {
    const OPTS: glob::MatchOptions = glob::MatchOptions {
        case_sensitive: true,
        require_literal_separator: true,
        require_literal_leading_dot: true,
    };

    fn parse(filters: &str) -> Filters {
        let mut f = Filters {
            hide: Vec::new(),
            show: Vec::new(),
        };
        for g in filters.split(',').map(str::trim).filter(|g| !g.is_empty()) {
            let (list, pat) = match g.strip_prefix('!') {
                Some(p) => (&mut f.show, p),
                None => (&mut f.hide, g),
            };
            if let Ok(p) = glob::Pattern::new(pat) {
                list.push(p);
            }
        }
        f
    }

    fn is_hidden(&self, name: &str) -> bool {
        let matches = |ps: &[glob::Pattern]| ps.iter().any(|p| p.matches_with(name, Self::OPTS));
        !matches(&self.show) && matches(&self.hide)
    }
}

pub struct LocalFolder {
    pub dir: PathBuf,
    pub cfg: FsConfig,
    items: Vec<FsEntry>,
    sorted: Vec<FsEntry>,
}

impl LocalFolder {
    pub fn new(path: &Path, cfg: FsConfig) -> std::io::Result<LocalFolder> {
        let abs = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()?.join(path)
        };
        let dir = normpath(&abs);
        if !std::fs::metadata(&dir)?.is_dir() {
            return Err(std::io::Error::other(format!(
                "{}: Not a directory",
                dir.display()
            )));
        }
        let mut f = LocalFolder {
            dir,
            cfg,
            items: Vec::new(),
            sorted: Vec::new(),
        };
        f.load()?;
        f.refresh();
        Ok(f)
    }

    /// Re-reads the directory. Entries that vanish before they can be stat'ed are skipped.
    pub fn load(&mut self) -> std::io::Result<()> {
        self.items = std::fs::read_dir(&self.dir)?
            .filter_map(|de| de.ok())
            .filter_map(|de| FsEntry::from_path(&de.path()).ok())
            .collect();
        Ok(())
    }

    /// Applies dotfile and glob filters and sorting; ".." always comes first.
    pub fn refresh(&mut self) {
        let filters = Filters::parse(&self.cfg.filters);
        let visible = self
            .items
            .iter()
            .filter(|e| self.cfg.show_dotfiles || !e.is_dotfile())
            .filter(|e| !filters.is_hidden(&e.display_name()))
            .cloned();
        let (mut dirs, mut files): (Vec<FsEntry>, Vec<FsEntry>) = if self.cfg.sort_mix_dirs {
            (Vec::new(), visible.collect())
        } else {
            visible.partition(|e| e.is_dir())
        };
        self.cfg.sort(&mut dirs);
        self.cfg.sort(&mut files);
        let mut sorted = Vec::with_capacity(dirs.len() + files.len() + 1);
        if let Ok(parent) = FsEntry::from_path(&self.dir.join("..")) {
            sorted.push(parent);
        }
        sorted.extend(dirs);
        sorted.extend(files);
        self.sorted = sorted;
    }

    pub fn len(&self) -> usize {
        self.sorted.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sorted.is_empty()
    }

    pub fn entries(&self) -> &[FsEntry] {
        &self.sorted
    }

    pub fn entry_mut(&mut self, name: &OsStr) -> Option<&mut FsEntry> {
        self.sorted.iter_mut().find(|e| e.name == name)
    }

    pub fn lookup(&self, name: &OsStr) -> Option<&FsEntry> {
        self.sorted.iter().find(|e| e.name == name)
    }

    pub fn pos(&self, name: &OsStr) -> Option<usize> {
        self.sorted.iter().position(|e| e.name == name)
    }

    pub fn filenames(&self) -> Vec<OsString> {
        self.sorted.iter().map(|e| e.name.clone()).collect()
    }

    pub fn dirs(&self) -> Vec<&FsEntry> {
        self.sorted
            .iter()
            .filter(|e| e.is_dir() && e.name != "..")
            .collect()
    }

    /// Hidden entries count, computed like lfm (`items - shown + 1` for "..").
    pub fn nfiltered(&self) -> usize {
        (self.items.len() + 1).saturating_sub(self.sorted.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(f: &LocalFolder) -> Vec<String> {
        f.entries().iter().map(|e| e.display_name()).collect()
    }

    fn sample() -> tempfile::TempDir {
        let t = tempfile::tempdir().unwrap();
        let d = t.path();
        for n in ["b.txt", "A.c", "c.o", ".hidden"] {
            std::fs::write(d.join(n), n).unwrap();
        }
        std::fs::create_dir(d.join("zdir")).unwrap();
        std::fs::create_dir(d.join("Adir")).unwrap();
        std::fs::write(d.join("big.bin"), vec![0u8; 5000]).unwrap();
        t
    }

    #[test]
    fn default_listing() {
        let t = sample();
        let f = LocalFolder::new(t.path(), FsConfig::default()).unwrap();
        assert_eq!(
            names(&f),
            ["..", "Adir", "zdir", ".hidden", "A.c", "b.txt", "big.bin", "c.o"]
        );
        assert_eq!(f.nfiltered(), 0);
        assert_eq!(f.dirs().len(), 2);
        assert_eq!(f.filenames().len(), 8);
    }

    #[test]
    fn case_sensitive_and_mixed_dirs() {
        let t = sample();
        let cfg = FsConfig {
            sort_mix_cases: false,
            sort_mix_dirs: true,
            ..FsConfig::default()
        };
        let f = LocalFolder::new(t.path(), cfg).unwrap();
        assert_eq!(
            names(&f),
            ["..", ".hidden", "A.c", "Adir", "b.txt", "big.bin", "c.o", "zdir"]
        );
    }

    #[test]
    fn dotfiles_filters_and_reverse_size() {
        let t = sample();
        let cfg = FsConfig {
            show_dotfiles: false,
            filters: "*.o, *.txt, !b.*".into(),
            ..FsConfig::default()
        };
        let f = LocalFolder::new(t.path(), cfg).unwrap();
        assert_eq!(names(&f), ["..", "Adir", "zdir", "A.c", "b.txt", "big.bin"]);
        assert_eq!(f.nfiltered(), 2); // .hidden, c.o
        let cfg = FsConfig {
            sort_type: SortType::BySize,
            sort_reverse: true,
            ..FsConfig::default()
        };
        let f = LocalFolder::new(t.path(), cfg).unwrap();
        assert_eq!(f.entries()[3].display_name(), "big.bin"); // after .., the two dirs
    }

    #[test]
    fn star_does_not_hide_dotfiles() {
        let t = sample();
        let f = LocalFolder::new(
            t.path(),
            FsConfig {
                filters: "*".into(),
                ..FsConfig::default()
            },
        )
        .unwrap();
        assert_eq!(names(&f), ["..", ".hidden"]);
    }

    #[test]
    fn options_map_to_config() {
        let o = crate::config::settings::Options {
            sort_type: SortType::ByExt,
            show_dotfiles: false,
            ..Default::default()
        };
        let c = FsConfig::from_options(&o);
        assert_eq!(
            (c.sort_type, c.show_dotfiles, c.filters.as_str()),
            (SortType::ByExt, false, "")
        );
    }

    #[test]
    fn lookup_pos_and_reload() {
        let t = sample();
        let mut f = LocalFolder::new(t.path(), FsConfig::default()).unwrap();
        assert_eq!(f.pos(OsStr::new("b.txt")), Some(5));
        assert!(f.lookup(OsStr::new("nope")).is_none());
        std::fs::write(t.path().join("new"), b"").unwrap();
        f.load().unwrap();
        f.refresh();
        assert!(f.lookup(OsStr::new("new")).is_some());
        f.entry_mut(OsStr::new("Adir")).unwrap().update_size(42);
        assert_eq!(f.lookup(OsStr::new("Adir")).unwrap().size, 42);
    }

    #[test]
    fn root_and_errors() {
        let f = LocalFolder::new(Path::new("/"), FsConfig::default()).unwrap();
        assert_eq!(f.entries()[0].display_name(), "..");
        assert!(LocalFolder::new(Path::new("/etc/hostname"), FsConfig::default()).is_err());
    }

    #[test]
    fn non_utf8_names_list_and_sort() {
        use std::os::unix::ffi::OsStrExt;
        let t = sample();
        std::fs::write(t.path().join(OsStr::from_bytes(b"\xffodd")), b"").unwrap();
        let f = LocalFolder::new(t.path(), FsConfig::default()).unwrap();
        assert!(f.lookup(OsStr::from_bytes(b"\xffodd")).is_some());
    }
}
