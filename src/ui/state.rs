//! The screen's model: two panes of up to four tabs each, cursor, selection and
//! every action that needs no dialog. Ported from `Pane`/`Tab` in lfm's ui.py
//! and the dialog-free actions in actions.py.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use crate::config::history::History;
use crate::config::keymap::KeyMap;
use crate::config::paths::ConfigPaths;
use crate::config::settings::{PaneSort, Settings, SortType, BOOKMARK_KEYS};
use crate::config::theme::Theme;
use crate::fs::entry::FsEntry;
use crate::fs::folder::{FsConfig, LocalFolder};
use crate::fs::vfs::{display_path, VfsLayer};
use crate::startup::Loaded;

pub const MAX_TABS: usize = 4;
pub const TAB_HISTORY_MAX: usize = 100;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PaneMode {
    Half,
    Full,
    Hidden,
}

pub struct Tab {
    pub fs: LocalFolder,
    pub i: usize,
    pub a: usize,
    pub selected: Vec<OsString>,
    pub history: Vec<PathBuf>,
    /// Open virtual folders, innermost last.
    pub vfs: Vec<VfsLayer>,
}

fn chdir_reason(e: &std::io::Error) -> String {
    match (e.kind(), e.raw_os_error()) {
        (std::io::ErrorKind::NotFound, _) => "Invalid directory?".to_string(),
        (_, Some(n)) => nix::errno::Errno::from_raw(n).desc().to_string(),
        _ => "Invalid directory?".to_string(),
    }
}

impl Tab {
    pub fn new(path: &Path, cfg: FsConfig) -> std::io::Result<Tab> {
        Ok(Tab {
            fs: LocalFolder::new(path, cfg)?,
            i: 0,
            a: 0,
            selected: Vec::new(),
            history: Vec::new(),
            vfs: Vec::new(),
        })
    }

    pub fn dir(&self) -> &Path {
        &self.fs.dir
    }

    /// The directory as the user sees it (`archive#vfs://rel` inside a VFS).
    pub fn display_dir(&self) -> String {
        display_path(&self.vfs, self.dir())
    }

    pub fn in_vfs(&self) -> bool {
        !self.vfs.is_empty()
    }

    /// Name in the tabs bar: the archive's name at a VFS root, "/" for the root.
    pub fn tab_name(&self) -> String {
        if let Some(l) = self.vfs.iter().rev().find(|l| l.tmp == self.dir()) {
            return l.source.file_name().map_or_else(
                || l.display_base.clone(),
                |n| n.to_string_lossy().into_owned(),
            );
        }
        self.dir()
            .file_name()
            .map_or_else(|| "/".to_string(), |n| n.to_string_lossy().into_owned())
    }

    pub fn current(&self) -> &FsEntry {
        &self.fs.entries()[self.i.min(self.fs.len().saturating_sub(1))]
    }

    /// Changes directory keeping the filters; the old directory goes to the tab history.
    pub fn goto_folder(&mut self, path: &Path, fh: usize) -> Result<(), String> {
        let fs = LocalFolder::new(path, self.fs.cfg.clone())
            .map_err(|e| format!("Cannot chdir {}\n{}", path.display(), chdir_reason(&e)))?;
        let old = std::mem::replace(&mut self.fs, fs).dir;
        // Temp dirs of virtual folders disappear, so they never go to the history (lfm).
        if !self.vfs.iter().any(|l| old.starts_with(&l.tmp)) {
            self.history.retain(|p| *p != old);
            self.history.push(old);
        }
        if self.history.len() > TAB_HISTORY_MAX {
            self.history.remove(0);
        }
        self.i = 0;
        self.a = 0;
        self.selected.clear();
        self.fix_limits(fh);
        Ok(())
    }

    /// Re-reads the directory; if it vanished, moves to its nearest existing parent.
    pub fn reload(&mut self, fh: usize) {
        if self.fs.load().is_err() {
            let mut parent = self.fs.dir.clone();
            while !parent.is_dir() && parent.pop() {}
            if self.goto_folder(&parent, fh).is_ok() {
                return;
            }
        }
        let cfg = self.fs.cfg.clone();
        self.refresh(cfg, fh);
    }

    /// Re-filters and re-sorts with `cfg` (keeping this tab's filters); the cursor
    /// stays on the same name if it still exists, else on the same index.
    pub fn refresh(&mut self, cfg: FsConfig, fh: usize) {
        let old_name = self.current().name.clone();
        let filters = std::mem::take(&mut self.fs.cfg.filters);
        self.fs.cfg = FsConfig { filters, ..cfg };
        self.fs.refresh();
        if self.fs.is_empty() {
            // The directory vanished (even ".." can't be listed): go to its nearest existing parent.
            let mut parent = self.fs.dir.clone();
            while !parent.is_dir() && parent.pop() {}
            if self.goto_folder(&parent, fh).is_ok() {
                return;
            }
        }
        if let Some(pos) = self.fs.pos(&old_name) {
            self.i = pos;
        }
        self.fix_limits(fh);
        let fs = &self.fs;
        self.selected.retain(|n| fs.lookup(n).is_some());
    }

    pub fn fix_limits(&mut self, fh: usize) {
        let fh = fh.max(1);
        self.i = self.i.min(self.fs.len().saturating_sub(1));
        self.a = self.i / fh * fh;
    }

    pub fn focus_file(&mut self, name: &OsStr, fh: usize) {
        if let Some(pos) = self.fs.pos(name) {
            self.i = pos;
            self.fix_limits(fh);
        }
    }

    pub fn is_selected(&self, name: &OsStr) -> bool {
        self.selected.iter().any(|n| n == name)
    }

    pub fn selected_entries(&self) -> Vec<&FsEntry> {
        self.fs
            .entries()
            .iter()
            .filter(|e| self.is_selected(&e.name))
            .collect()
    }

    /// The selection, or the entry under the cursor; never "..".
    pub fn selected_or_current(&self) -> Vec<&FsEntry> {
        if !self.selected.is_empty() {
            return self.selected_entries();
        }
        let cur = self.current();
        if cur.name == ".." {
            Vec::new()
        } else {
            vec![cur]
        }
    }
}

pub struct Pane {
    pub mode: PaneMode,
    pub tabs: Vec<Tab>,
    pub active: usize,
}

pub struct App {
    pub panes: [Pane; 2],
    pub active: usize,
    pub settings: Settings,
    pub keymap: KeyMap,
    pub theme: Theme,
    pub history: History,
    pub paths: ConfigPaths,
    pub width: u16,
    pub height: u16,
    /// PowerCLI text kept while the line is hidden with Ctrl-X.
    pub cli_text: String,
    pub cli_pos: usize,
}

#[derive(Debug, PartialEq)]
pub enum Enter {
    Parent,
    Dir,
    File(String),
}

const GLOB_OPTS: glob::MatchOptions = glob::MatchOptions {
    case_sensitive: true,
    require_literal_separator: true,
    require_literal_leading_dot: true,
};

impl App {
    pub fn new(
        loaded: Loaded,
        paths: ConfigPaths,
        path1: &Path,
        path2: &Path,
        width: u16,
        height: u16,
    ) -> std::io::Result<App> {
        let s = &loaded.settings;
        let pane = |p: &Path, i: usize| -> std::io::Result<Pane> {
            let cfg = FsConfig::from_options(&s.options).with_sort(s.pane_sort[i]);
            Ok(Pane {
                mode: PaneMode::Half,
                tabs: vec![Tab::new(p, cfg)?],
                active: 0,
            })
        };
        let mut app = App {
            panes: [pane(path1, 0)?, pane(path2, 1)?],
            active: 0,
            settings: loaded.settings,
            keymap: loaded.keymap,
            theme: loaded.theme,
            history: loaded.history,
            paths,
            width,
            height,
            cli_text: String::new(),
            cli_pos: 0,
        };
        app.resize(width, height);
        Ok(app)
    }

    /// Listing settings of the active pane.
    pub fn fs_config(&self) -> FsConfig {
        self.fs_config_for(self.active)
    }

    fn fs_config_for(&self, pane: usize) -> FsConfig {
        FsConfig::from_options(&self.settings.options).with_sort(self.settings.pane_sort[pane])
    }

    /// File rows of the active pane: a half pane loses its box and titles, and
    /// the function-key bar (when shown) takes a row.
    pub fn fh(&self) -> usize {
        let bar = usize::from(self.settings.options.show_function_keys);
        let chrome = if self.pane().mode == PaneMode::Half {
            5
        } else {
            2
        } + bar;
        (self.height as usize).saturating_sub(chrome).max(1)
    }

    pub fn pane(&self) -> &Pane {
        &self.panes[self.active]
    }

    pub fn other_pane(&self) -> &Pane {
        &self.panes[1 - self.active]
    }

    fn pane_mut(&mut self) -> &mut Pane {
        &mut self.panes[self.active]
    }

    pub fn tab(&self) -> &Tab {
        let p = self.pane();
        &p.tabs[p.active]
    }

    pub fn tab_mut(&mut self) -> &mut Tab {
        let p = self.pane_mut();
        &mut p.tabs[p.active]
    }

    pub fn other_tab(&self) -> &Tab {
        let p = self.other_pane();
        &p.tabs[p.active]
    }

    fn all_tabs_mut(&mut self) -> impl Iterator<Item = &mut Tab> {
        self.panes.iter_mut().flat_map(|p| p.tabs.iter_mut())
    }

    pub fn resize(&mut self, width: u16, height: u16) {
        self.width = width;
        self.height = height;
        let fh = self.fh();
        self.all_tabs_mut().for_each(|t| t.fix_limits(fh));
    }

    pub fn cursor_by(&mut self, delta: isize) {
        let fh = self.fh();
        let tab = self.tab_mut();
        tab.i = (tab.i as isize + delta).max(0) as usize;
        tab.fix_limits(fh);
    }

    pub fn cursor_home(&mut self) {
        let fh = self.fh();
        self.tab_mut().i = 0;
        self.tab_mut().fix_limits(fh);
    }

    pub fn cursor_end(&mut self) {
        let fh = self.fh();
        let tab = self.tab_mut();
        tab.i = tab.fs.len().saturating_sub(1);
        tab.fix_limits(fh);
    }

    pub fn cursor_page(&mut self, down: bool) {
        let fh = self.fh() as isize;
        self.cursor_by(if down { fh } else { -fh });
    }

    fn goto_first_after(&mut self, pred: impl Fn(&str) -> bool) -> bool {
        let fh = self.fh();
        let tab = self.tab_mut();
        let found = tab
            .fs
            .entries()
            .iter()
            .enumerate()
            .skip(tab.i + 1)
            .find(|(_, e)| pred(&e.display_name()));
        match found.map(|(idx, _)| idx) {
            Some(idx) => {
                tab.i = idx;
                tab.fix_limits(fh);
                true
            }
            None => false,
        }
    }

    /// First name after the cursor containing `text` (no wrap-around, like lfm).
    pub fn goto_name_containing(&mut self, text: &str) {
        self.goto_first_after(|n| n.contains(text));
    }

    pub fn goto_name_starting(&mut self, c: char) -> bool {
        self.goto_first_after(|n| n.starts_with(c))
    }

    pub fn dir_up(&mut self) -> Result<(), String> {
        let dir = self.tab().dir().to_path_buf();
        let (Some(parent), Some(name)) = (dir.parent(), dir.file_name()) else {
            return Ok(());
        };
        self.goto(parent)?;
        let fh = self.fh();
        self.tab_mut().focus_file(name, fh);
        Ok(())
    }

    pub fn classify_current(&self) -> Enter {
        let cur = self.tab().current();
        if cur.name == ".." {
            Enter::Parent
        } else if cur.is_dir() {
            Enter::Dir
        } else {
            Enter::File(cur.file_kind(&self.settings.files))
        }
    }

    pub fn goto(&mut self, path: &Path) -> Result<(), String> {
        let fh = self.fh();
        self.tab_mut().goto_folder(path, fh)
    }

    /// In full view the focused pane is the only one shown, so the full view
    /// moves with the focus (lfm kept drawing the old pane while keys acted on
    /// the hidden one).
    pub fn pane_change_focus(&mut self) {
        let full = self.pane().mode == PaneMode::Full;
        self.active = 1 - self.active;
        if full {
            self.panes[self.active].mode = PaneMode::Full;
            self.panes[1 - self.active].mode = PaneMode::Hidden;
        }
    }

    pub fn pane_other_tab_equal(&mut self) -> Result<(), String> {
        let dir = self.tab().dir().to_path_buf();
        let fh = self.fh();
        let other = &mut self.panes[1 - self.active];
        let idx = other.active;
        other.tabs[idx].goto_folder(&dir, fh)
    }

    pub fn panes_swap(&mut self) {
        self.panes.swap(0, 1);
        self.settings.pane_sort.swap(0, 1);
        self.active = 1 - self.active;
    }

    pub fn panes_cycle_view(&mut self) {
        let (mine, other) = match self.pane().mode {
            PaneMode::Half => (PaneMode::Full, PaneMode::Hidden),
            _ => (PaneMode::Half, PaneMode::Half),
        };
        self.panes[self.active].mode = mine;
        self.panes[1 - self.active].mode = other;
        let (w, h) = (self.width, self.height);
        self.resize(w, h);
    }

    pub fn tab_new(&mut self) -> Result<(), String> {
        let dir = self.tab().dir().to_path_buf();
        self.tab_new_at(&dir)
    }

    /// A new tab on `dir`, right of the current one.
    pub fn tab_new_at(&mut self, dir: &Path) -> Result<(), String> {
        if self.pane().tabs.len() >= MAX_TABS {
            return Err("Cannot create more tabs".to_string());
        }
        let tab = Tab::new(dir, self.fs_config()).map_err(|e| e.to_string())?;
        let p = self.pane_mut();
        p.tabs.insert(p.active + 1, tab);
        p.active += 1;
        Ok(())
    }

    pub fn tab_close(&mut self) -> Result<(), String> {
        let p = self.pane_mut();
        if p.tabs.len() == 1 {
            return Err("Cannot close last tab".to_string());
        }
        p.tabs.remove(p.active);
        p.active = p.active.saturating_sub(1);
        Ok(())
    }

    pub fn tab_left(&mut self) {
        let p = self.pane_mut();
        p.active = p.active.saturating_sub(1);
    }

    pub fn tab_right(&mut self) {
        let p = self.pane_mut();
        if p.active + 1 < p.tabs.len() {
            p.active += 1;
        }
    }

    pub fn select_current(&mut self) {
        let fh = self.fh();
        let tab = self.tab_mut();
        if tab.i == 0 {
            return;
        }
        let name = tab.current().name.clone();
        if tab.is_selected(&name) {
            tab.selected.retain(|n| *n != name);
        } else {
            tab.selected.push(name);
        }
        tab.i += 1;
        tab.fix_limits(fh);
    }

    /// Adds (or removes) the names matching `pattern`, with shell glob rules.
    pub fn select_glob(&mut self, pattern: &str, select: bool) {
        let Ok(pat) = glob::Pattern::new(pattern) else {
            return;
        };
        let tab = self.tab_mut();
        let matching: Vec<OsString> = tab
            .fs
            .entries()
            .iter()
            .filter(|e| e.name != ".." && pat.matches_with(&e.display_name(), GLOB_OPTS))
            .map(|e| e.name.clone())
            .collect();
        for name in matching {
            let present = tab.is_selected(&name);
            if select && !present {
                tab.selected.push(name);
            } else if !select && present {
                tab.selected.retain(|n| *n != name);
            }
        }
    }

    pub fn select_invert(&mut self) {
        let tab = self.tab_mut();
        let inverted: Vec<OsString> = tab
            .fs
            .entries()
            .iter()
            .filter(|e| e.name != ".." && !tab.is_selected(&e.name))
            .map(|e| e.name.clone())
            .collect();
        tab.selected = inverted;
    }

    fn refresh_tabs(&mut self) {
        let fh = self.fh();
        for i in 0..self.panes.len() {
            let cfg = self.fs_config_for(i);
            self.panes[i]
                .tabs
                .iter_mut()
                .for_each(|t| t.refresh(cfg.clone(), fh));
        }
    }

    pub fn toggle_human_sizes(&mut self) {
        let o = &mut self.settings.options;
        o.human_readable_sizes = !o.human_readable_sizes;
    }

    pub fn toggle_dotfiles(&mut self) {
        self.settings.options.show_dotfiles = !self.settings.options.show_dotfiles;
        self.refresh_tabs();
    }

    /// lfm's sort dialog keys: o n e s d; uppercase reverses. Sorts the active pane only.
    pub fn set_sort(&mut self, key: char) -> bool {
        let sort = match key.to_ascii_lowercase() {
            'o' => SortType::None,
            'n' => SortType::ByName,
            'e' => SortType::ByExt,
            's' => SortType::BySize,
            'd' => SortType::ByMTime,
            _ => return false,
        };
        self.settings.pane_sort[self.active] = PaneSort {
            sort_type: sort,
            sort_reverse: key.is_ascii_uppercase(),
        };
        self.refresh_tabs();
        true
    }

    pub fn set_filters(&mut self, text: &str) {
        let (cfg, fh) = (self.fs_config(), self.fh());
        let tab = self.tab_mut();
        tab.fs.cfg.filters = text.to_string();
        tab.refresh(cfg, fh);
    }

    pub fn refresh_all(&mut self) {
        let fh = self.fh();
        self.all_tabs_mut().for_each(|t| t.reload(fh));
    }

    pub fn set_bookmark(&mut self, key: char) -> bool {
        if !BOOKMARK_KEYS.contains(key) {
            return false;
        }
        let dir = self.tab().dir().to_string_lossy().into_owned();
        self.settings.bookmarks.insert(key, dir);
        true
    }

    /// True for paths inside any tab's virtual folder (temp dirs that vanish).
    pub fn is_vfs_temp(&self, path: &Path) -> bool {
        self.panes
            .iter()
            .flat_map(|p| &p.tabs)
            .flat_map(|t| &t.vfs)
            .any(|l| path.starts_with(&l.tmp))
    }

    pub fn bookmark(&self, key: char) -> Option<PathBuf> {
        self.settings.bookmarks.get(&key).map(PathBuf::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::testutil::test_app as app_in_wh;

    fn app_in(d: &Path) -> App {
        app_in_wh(d, 100, 30)
    }

    fn names(app: &App) -> Vec<String> {
        app.tab()
            .fs
            .entries()
            .iter()
            .map(|e| e.display_name())
            .collect()
    }

    #[test]
    fn cursor_moves_and_clamps() {
        let t = tempfile::tempdir().unwrap();
        let mut app = app_in(t.path());
        assert_eq!(app.fh(), 24);
        app.settings.options.show_function_keys = false;
        assert_eq!(app.fh(), 25);
        app.settings.options.show_function_keys = true;
        app.cursor_by(-1);
        assert_eq!(app.tab().i, 0);
        app.cursor_end();
        let last = app.tab().fs.len() - 1;
        assert_eq!(app.tab().i, last);
        app.cursor_by(10);
        assert_eq!(app.tab().i, last);
        app.cursor_home();
        app.goto_name_containing("rs");
        assert_eq!(app.tab().current().display_name(), "b.rs");
        app.cursor_home();
        assert!(app.goto_name_starting('a'));
        assert_eq!(app.tab().current().display_name(), "a.txt");
    }

    #[test]
    fn enter_and_leave_dirs_with_history() {
        let t = tempfile::tempdir().unwrap();
        let mut app = app_in(t.path());
        app.goto(&t.path().join("sub")).unwrap();
        assert_eq!(app.tab().history, vec![t.path().to_path_buf()]);
        app.dir_up().unwrap();
        assert_eq!(app.tab().current().display_name(), "sub");
        assert_eq!(app.classify_current(), Enter::Dir);
        assert!(app
            .goto(&t.path().join("nope"))
            .unwrap_err()
            .starts_with("Cannot chdir"));
    }

    #[test]
    fn selection_rules() {
        let t = tempfile::tempdir().unwrap();
        let mut app = app_in(t.path());
        app.cursor_home();
        app.select_current();
        assert!(app.tab().selected.is_empty()); // ".."
        assert!(app.tab().selected_or_current().is_empty());
        assert_eq!(app.classify_current(), Enter::Parent);
        app.select_glob("*", true);
        assert!(
            !app.tab().is_selected(OsStr::new(".dot"))
                && app.tab().is_selected(OsStr::new("a.txt"))
        );
        app.select_invert();
        assert!(
            app.tab().is_selected(OsStr::new(".dot")) && !app.tab().is_selected(OsStr::new(".."))
        );
        app.select_glob(".*", false);
        assert!(app.tab().selected.is_empty());
    }

    #[test]
    fn refresh_keeps_cursor_and_selection_by_name() {
        let t = tempfile::tempdir().unwrap();
        let mut app = app_in(t.path());
        let fh = app.fh();
        let pos = app.tab().fs.pos(OsStr::new("b.rs")).unwrap();
        app.tab_mut().i = pos;
        app.select_glob("a.txt", true);
        std::fs::write(t.path().join("0first"), b"").unwrap();
        std::fs::remove_file(t.path().join("a.txt")).unwrap();
        app.refresh_all();
        assert_eq!(app.tab().current().display_name(), "b.rs");
        assert!(app.tab().selected.is_empty());
        app.goto(&t.path().join("cfg")).unwrap();
        std::fs::remove_dir_all(t.path().join("cfg")).unwrap();
        app.tab_mut().reload(fh);
        assert_eq!(app.tab().dir(), t.path());
    }

    #[test]
    fn panes_and_tabs() {
        let t = tempfile::tempdir().unwrap();
        let mut app = app_in(t.path());
        app.panes_cycle_view();
        assert_eq!(
            (app.pane().mode, app.other_pane().mode),
            (PaneMode::Full, PaneMode::Hidden)
        );
        assert_eq!(app.fh(), 27);
        app.panes_cycle_view();
        assert_eq!(app.other_pane().mode, PaneMode::Half);
        for _ in 0..3 {
            app.tab_new().unwrap();
        }
        assert_eq!(app.tab_new().unwrap_err(), "Cannot create more tabs");
        app.tab_left();
        assert_eq!(app.pane().active, 2);
        app.tab_right();
        app.tab_right();
        assert_eq!(app.pane().active, 3);
        while app.pane().tabs.len() > 1 {
            app.tab_close().unwrap();
        }
        assert_eq!(app.tab_close().unwrap_err(), "Cannot close last tab");
        app.goto(&t.path().join("sub")).unwrap();
        app.pane_other_tab_equal().unwrap();
        assert_eq!(app.other_tab().dir(), t.path().join("sub"));
        let before = app.tab().dir().to_path_buf();
        app.panes_swap();
        assert_eq!(app.tab().dir(), before);
        assert_eq!(app.active, 1);
        app.pane_change_focus();
        assert_eq!(app.active, 0);
    }

    fn pane_names(app: &App, pane: usize) -> Vec<String> {
        let p = &app.panes[pane];
        p.tabs[p.active]
            .fs
            .entries()
            .iter()
            .map(|e| e.display_name())
            .filter(|n| n.starts_with("sz"))
            .collect()
    }

    #[test]
    fn sort_changes_only_the_active_pane() {
        let t = tempfile::tempdir().unwrap();
        for (n, len) in [("sz_a", 300), ("sz_b", 100), ("sz_c", 200)] {
            std::fs::write(t.path().join(n), vec![0u8; len]).unwrap();
        }
        let mut app = app_in(t.path());
        let by_name = vec!["sz_a", "sz_b", "sz_c"];
        assert!(app.set_sort('S'));
        assert_eq!(pane_names(&app, 0), vec!["sz_a", "sz_c", "sz_b"]);
        assert_eq!(pane_names(&app, 1), by_name);
        assert_eq!(
            app.settings.pane_sort[0],
            PaneSort {
                sort_type: SortType::BySize,
                sort_reverse: true
            }
        );
        assert_eq!(app.settings.pane_sort[1].sort_type, SortType::ByName);
        // the shared starting sort is left alone
        assert_eq!(app.settings.options.sort_type, SortType::ByName);
        // other refreshes keep each pane's sort
        app.toggle_dotfiles();
        assert_eq!(pane_names(&app, 0), vec!["sz_a", "sz_c", "sz_b"]);
        assert_eq!(pane_names(&app, 1), by_name);
        // a new tab uses its pane's sort
        app.tab_new().unwrap();
        assert_eq!(pane_names(&app, 0), vec!["sz_a", "sz_c", "sz_b"]);
        // swapping carries each pane's sort along
        app.panes_swap();
        assert_eq!(pane_names(&app, 1), vec!["sz_a", "sz_c", "sz_b"]);
        assert_eq!(app.settings.pane_sort[1].sort_type, SortType::BySize);
        assert_eq!(app.settings.pane_sort[0].sort_type, SortType::ByName);
        app.refresh_tabs();
        assert_eq!(pane_names(&app, 0), by_name);
        assert_eq!(pane_names(&app, 1), vec!["sz_a", "sz_c", "sz_b"]);
    }

    #[test]
    fn pane_sorts_survive_save_and_restart() {
        let t = tempfile::tempdir().unwrap();
        for (n, len) in [("sz_a", 300), ("sz_b", 100), ("sz_c", 200)] {
            std::fs::write(t.path().join(n), vec![0u8; len]).unwrap();
        }
        let mut app = app_in(t.path());
        app.pane_change_focus();
        assert!(app.set_sort('S'));
        // what the exit path does when save_configuration_at_exit is on
        app.settings.save(&app.paths.config()).unwrap();
        let saved = std::fs::read_to_string(app.paths.config()).unwrap();
        assert!(saved.contains("[panes.right]"), "{saved}");
        let loaded = crate::startup::load_all(&app.paths);
        assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);
        let again = App::new(loaded, app.paths.clone(), t.path(), t.path(), 100, 30).unwrap();
        assert_eq!(pane_names(&again, 0), vec!["sz_a", "sz_b", "sz_c"]);
        assert_eq!(pane_names(&again, 1), vec!["sz_a", "sz_c", "sz_b"]);
    }

    #[test]
    fn panes_start_with_their_saved_sort() {
        let t = tempfile::tempdir().unwrap();
        for (n, len) in [("sz_a", 300), ("sz_b", 100)] {
            std::fs::write(t.path().join(n), vec![0u8; len]).unwrap();
        }
        let paths = ConfigPaths::from_base(&t.path().join("cfg"), &t.path().join("home"));
        let mut loaded = crate::startup::load_all(&paths);
        loaded.settings.pane_sort[1] = PaneSort {
            sort_type: SortType::BySize,
            sort_reverse: false,
        };
        let app = App::new(loaded, paths, t.path(), t.path(), 100, 30).unwrap();
        assert_eq!(pane_names(&app, 0), vec!["sz_a", "sz_b"]);
        assert_eq!(pane_names(&app, 1), vec!["sz_b", "sz_a"]);
    }

    #[test]
    fn sort_dotfiles_filters_bookmarks() {
        let t = tempfile::tempdir().unwrap();
        let mut app = app_in(t.path());
        assert!(app.set_sort('S'));
        assert!(app.settings.pane_sort[0].sort_reverse);
        assert_eq!(app.settings.pane_sort[0].sort_type, SortType::BySize);
        assert!(app.set_sort('e'));
        assert_eq!(app.settings.pane_sort[0].sort_type, SortType::ByExt);
        assert!(!app.set_sort('x'));
        app.toggle_dotfiles();
        assert!(!names(&app).contains(&".dot".to_string()));
        app.set_filters("*.rs");
        assert!(!names(&app).contains(&"b.rs".to_string()));
        assert!(app.set_bookmark('3'));
        assert_eq!(app.bookmark('3').unwrap(), t.path());
        assert!(!app.set_bookmark('#'));
    }

    #[test]
    fn tab_in_full_view_moves_the_full_view_with_the_focus() {
        let t = tempfile::tempdir().unwrap();
        let mut app = app_in(t.path());
        app.panes_cycle_view();
        app.pane_change_focus();
        assert_eq!(app.active, 1);
        assert_eq!(
            (app.pane().mode, app.other_pane().mode),
            (PaneMode::Full, PaneMode::Hidden)
        );
        app.pane_change_focus();
        assert_eq!(
            (app.active, app.pane().mode, app.other_pane().mode),
            (0, PaneMode::Full, PaneMode::Hidden)
        );
    }

    #[test]
    fn empty_dir_removed_under_us_does_not_panic() {
        let t = tempfile::tempdir().unwrap();
        let mut app = app_in(t.path());
        std::fs::create_dir(t.path().join("empty")).unwrap();
        app.goto(&t.path().join("empty")).unwrap();
        std::fs::remove_dir(t.path().join("empty")).unwrap();
        assert!(app.set_sort('n'));
        app.toggle_dotfiles();
        app.set_filters("*.x");
        let _ = app.tab().current();
        assert_eq!(app.tab().dir(), t.path());
    }

    #[test]
    fn resize_clamps_cursor_window() {
        let t = tempfile::tempdir().unwrap();
        let mut app = app_in(t.path());
        app.cursor_end();
        app.resize(80, 6);
        assert_eq!(app.fh(), 1);
        assert_eq!(app.tab().a, app.tab().i);
    }
}
