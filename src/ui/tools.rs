//! Phase 4 dialogs and actions: tree view, find/grep, permissions, owner,
//! archives (VFS) and file information.

use std::cell::Cell;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use std::os::unix::fs::MetadataExt;

use chrono::TimeZone;
use nix::sys::stat::{major, minor};

use crate::archive::{compress_cmd, detect, uncompress_cmd, ArchiveType};
use crate::config::history::HistorySection;
use crate::config::theme::{ColorItem, Theme};
use crate::fs::entry::FsEntry;
use crate::fs::format::{group2str, owner2str, perms2str};
use crate::fs::format::{perms_display, str2perms};
use crate::fs::sysinfo::{
    chgrp_cmd, chmod_cmd, chown_cmd, diff_with_backup, file_type_info, filesystems_info, groups,
    mountpoint_for, user_fullname, users,
};
use crate::fs::tree::DirsTree;
use crate::fs::vfs::{self, VfsLayer};
use crate::jobs::shell_quote;
use crate::keys::{Key, KeyCode};
use crate::ui::actions::{
    ask_entry, clear_selection_and_refresh, error, program, run_on_file, selected_paths, CONTINUE,
};
use crate::ui::dialogs::{
    centered, frame, ConfirmDialog, Dialog, DoubleEntryDialog, ErrorDialog, GetKeyDialog, Step,
    ViewDialog,
};
use crate::ui::draw::{draw_box, fill, put, style};
use crate::ui::entry::{EntryCtx, SelectKind};
use crate::ui::jobs::run_job;
use crate::ui::screen::Screen;
use crate::ui::screen::{run_modal, Flow};
use crate::ui::state::App;
use crate::ui::state::PaneMode;
use crate::ui::text::{text2wrap, width};

/// Where the tree view is drawn: over one half, or over the whole pane area.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Region {
    Left,
    Right,
    Full,
}

pub struct TreeDialog {
    pub tree: DirsTree,
    pub region: Region,
    /// Rows used at the bottom by the status bar (and function-key bar).
    pub bottom_rows: u16,
}

impl TreeDialog {
    /// The pane body the tree covers (below the tabs row, above the bottom rows).
    fn rect(&self, area: Rect) -> Rect {
        let h = area.height.saturating_sub(1 + self.bottom_rows);
        let half = area.width / 2;
        let (x, w) = match self.region {
            Region::Left => (area.x, half),
            Region::Right => (area.x + half, half),
            Region::Full => (area.x, area.width),
        };
        Rect::new(x, area.y + area.height.min(1), w, h)
    }

    fn rows(&self, area: Rect) -> usize {
        (self.rect(area).height as usize).saturating_sub(2).max(1)
    }
}

impl Dialog for TreeDialog {
    type Output = Option<PathBuf>;
    fn handle(&mut self, key: Key, _ctx: &EntryCtx) -> Step<Option<PathBuf>> {
        let page = 10;
        let ch = match key.code {
            KeyCode::Char(c) if !key.alt => Some((c, key.ctrl)),
            _ => None,
        };
        match (key.code, ch) {
            (KeyCode::Up, _) | (_, Some(('k' | 'K', false))) => self.tree.up(),
            (KeyCode::Down, _) | (_, Some(('j' | 'J', false))) => self.tree.down(),
            (KeyCode::PageUp | KeyCode::Backspace, _) | (_, Some(('b', true))) => {
                self.tree.page(page, false)
            }
            (KeyCode::PageDown, _) | (_, Some((' ', false))) | (_, Some(('f', true))) => {
                self.tree.page(page, true)
            }
            (KeyCode::Home, _) | (_, Some(('a', true))) => self.tree.home(),
            (KeyCode::End, _) | (_, Some(('e', true))) => self.tree.end(),
            (KeyCode::Left, _) => self.tree.parent(),
            (KeyCode::Right, _) => {
                self.tree.to_child();
            }
            (KeyCode::Enter, _) => return Step::Done(Some(self.tree.path.clone())),
            (KeyCode::Esc | KeyCode::F(10), _)
            | (_, Some(('q' | 'Q', false)))
            | (_, Some(('c', true))) => return Step::Done(None),
            _ => {}
        }
        Step::Continue
    }

    fn render(&self, area: Rect, buf: &mut Buffer, theme: &Theme) -> Option<(u16, u16)> {
        let r = self.rect(area);
        if r.width < 2 || r.height < 2 {
            return None;
        }
        fill(buf, r, style(theme, ColorItem::FilesReg));
        draw_box(buf, r, style(theme, ColorItem::PaneActive));
        put(
            buf,
            r,
            r.x + 2,
            r.y,
            " Tree ",
            style(theme, ColorItem::PaneHeaderPath),
        );
        let rows = self.rows(area);
        let n = self.tree.nodes.len();
        let pos = self.tree.pos();
        let mut a = pos / rows * rows;
        let mut z = a + rows;
        if z > n {
            (a, z) = (n.saturating_sub(rows), n);
        }
        let inner_w = (r.width as usize).saturating_sub(2);
        for (j, i) in (a..z).enumerate() {
            let (name, depth, full) = &self.tree.nodes[i];
            let mut line = String::new();
            if *depth >= 0 {
                line.push_str(&" │ ".repeat(*depth as usize));
                let last = i == n - 1 || *depth > self.tree.depth(i + 1);
                line.push_str(if last { " └─ " } else { " ├─ " });
            } else {
                line.push(' ');
            }
            let y = r.y + 1 + j as u16;
            put(buf, r, r.x + 1, y, &line, style(theme, ColorItem::FilesReg));
            let x = r.x + 1 + width(&line) as u16;
            let avail = inner_w.saturating_sub(width(&line));
            if *full == self.tree.path {
                let label = text2wrap(name, avail.saturating_sub(3), true, 0.66, false);
                put(buf, r, x, y, &label, style(theme, ColorItem::Cursor));
                if self.tree.has_children() {
                    put(
                        buf,
                        r,
                        x + width(&label) as u16,
                        y,
                        " ➩",
                        style(theme, ColorItem::FilesReg),
                    );
                }
            } else {
                put(
                    buf,
                    r,
                    x,
                    y,
                    &text2wrap(name, avail, true, 0.66, false),
                    style(theme, ColorItem::FilesReg),
                );
            }
        }
        if area.height > self.bottom_rows {
            let sy = area.bottom() - self.bottom_rows;
            let row = Rect::new(area.x, sy, area.width, 1);
            fill(buf, row, style(theme, ColorItem::Statusbar));
            let text = format!("Path: {}", self.tree.path.display());
            put(
                buf,
                row,
                area.x + 1,
                sy,
                &text2wrap(
                    &text,
                    (area.width as usize).saturating_sub(2),
                    true,
                    0.66,
                    false,
                ),
                style(theme, ColorItem::Statusbar),
            );
        }
        None
    }
}

fn q(s: impl AsRef<std::ffi::OsStr>) -> String {
    shell_quote(s.as_ref()).to_string_lossy().into_owned()
}

/// `find DIR -[i]name PATTERN -print`
pub fn find_cmd(dir: &Path, pattern: &str, ignorecase: bool) -> OsString {
    let i = if ignorecase { "i" } else { "" };
    OsString::from(format!("find {} -{i}name {} -print", q(dir), q(pattern)))
}

/// `grep -R[E]Hn[i] --include FILES -e PATTERN .` (run in the searched dir).
/// `--include` must come before any operand: after `--` grep takes it as a file.
pub fn grep_cmd(pattern: &str, files: &str, regex: bool, ignorecase: bool) -> OsString {
    let (e, i) = (
        if regex { "E" } else { "" },
        if ignorecase { "i" } else { "" },
    );
    OsString::from(format!(
        "grep -R{e}Hn{i} --include {} -e {} .",
        q(files),
        q(pattern)
    ))
}

/// Found paths relative to `dir`, sorted; `dir` itself left out.
pub fn parse_find(out: &str, dir: &Path) -> Vec<String> {
    let prefix = format!("{}/", dir.display()).replace("//", "/");
    let mut v: Vec<String> = out
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && Path::new(l) != dir)
        .map(|l| l.strip_prefix(&prefix).unwrap_or(l).to_string())
        .collect();
    v.sort();
    v
}

/// Grep output lines without the leading "./", sorted.
pub fn parse_grep(out: &str) -> Vec<String> {
    let mut v: Vec<String> = out
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.strip_prefix("./").unwrap_or(l).to_string())
        .collect();
    v.sort();
    v
}

/// File and line of a grep result: `file:line:text` (the file may contain ':'),
/// or a binary-match notice (line 0).
pub fn grep_entry_target(entry: &str) -> (String, usize) {
    for (i, _) in entry.match_indices(':') {
        let rest = &entry[i + 1..];
        if let Some(j) = rest.find(':') {
            if let Ok(line) = rest[..j].parse::<usize>() {
                if j > 0 {
                    return (entry[..i].to_string(), line);
                }
            }
        }
    }
    let strip = |s: &str| s.trim().to_string();
    if let Some(x) = entry
        .strip_prefix("Binary file ")
        .and_then(|r| r.strip_suffix(" matches"))
    {
        return (strip(x), 0);
    }
    if let Some(x) = entry
        .strip_prefix("grep: ")
        .and_then(|r| r.strip_suffix(": binary file matches"))
    {
        return (strip(x), 0);
    }
    (entry.to_string(), 0)
}

/// Distinct files of the results, in result order (for the search VFS).
pub fn panelize_files(entries: &[String], grep: bool) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = Vec::new();
    for e in entries {
        let f = PathBuf::from(if grep {
            grep_entry_target(e).0
        } else {
            e.clone()
        });
        if !files.contains(&f) {
            files.push(f);
        }
    }
    files
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum FindAction {
    Go,
    Panelize,
    View,
    Edit,
    Do,
}

const FIND_BUTTONS: [&str; 6] = [
    "[ Go ]",
    "[ PAnelize ]",
    "[ View ]",
    "[ Edit ]",
    "[ Do ]",
    "[ Quit ]",
];

/// Results list with lfm's Go/Panelize/View/Edit/Do/Quit buttons (DialogFindGrep).
pub struct FindGrepDialog {
    title: String,
    entries: Vec<String>,
    cur: usize,
    btn: usize,
    page: Cell<usize>,
    searching: bool,
}

impl FindGrepDialog {
    pub fn new(title: &str, entries: Vec<String>) -> Self {
        FindGrepDialog {
            title: title.into(),
            entries,
            cur: 0,
            btn: 0,
            page: Cell::new(10),
            searching: false,
        }
    }

    fn pick(&self, a: FindAction) -> Step<Option<(FindAction, String)>> {
        Step::Done(self.entries.get(self.cur).map(|e| (a, e.clone())))
    }
}

impl Dialog for FindGrepDialog {
    type Output = Option<(FindAction, String)>;
    fn handle(&mut self, key: Key, _ctx: &EntryCtx) -> Step<Option<(FindAction, String)>> {
        let last = self.entries.len().saturating_sub(1);
        let page = self.page.get().max(1);
        let ch = match key.code {
            KeyCode::Char(c) if !key.alt => Some((c, key.ctrl)),
            _ => None,
        };
        if self.searching {
            self.searching = false;
            if let Some((c, false)) = ch {
                if let Some(i) = self
                    .entries
                    .iter()
                    .skip(self.cur)
                    .position(|e| e.starts_with(c))
                {
                    self.cur += i;
                }
            }
            return Step::Continue;
        }
        match (key.code, ch) {
            (KeyCode::Esc, _) | (_, Some(('q' | 'Q', false))) | (_, Some(('c', true))) => {
                return Step::Done(None)
            }
            (KeyCode::Up, _) | (_, Some(('k' | 'K', false))) => {
                self.cur = self.cur.saturating_sub(1)
            }
            (KeyCode::Down, _) | (_, Some(('j' | 'J', false))) => {
                self.cur = (self.cur + 1).min(last)
            }
            (KeyCode::PageUp | KeyCode::Backspace, _) | (_, Some(('h' | 'b', true))) => {
                self.cur = self.cur.saturating_sub(page)
            }
            (KeyCode::PageDown, _) | (_, Some((' ', false))) | (_, Some(('f', true))) => {
                self.cur = (self.cur + page).min(last)
            }
            (KeyCode::Home, _) | (_, Some(('a', true))) => self.cur = 0,
            (KeyCode::End, _) | (_, Some(('e', true))) => self.cur = last,
            (_, Some(('l', true))) => self.cur = ((self.cur / page) * page + page / 2).min(last),
            (_, Some(('s', true))) => self.searching = true,
            (KeyCode::Left | KeyCode::BackTab, _) => self.btn = (self.btn + 5) % 6,
            (KeyCode::Right | KeyCode::Tab, _) => self.btn = (self.btn + 1) % 6,
            (KeyCode::Enter, _) => {
                return match self.btn {
                    0 => self.pick(FindAction::Go),
                    1 => self.pick(FindAction::Panelize),
                    2 => self.pick(FindAction::View),
                    3 => self.pick(FindAction::Edit),
                    4 => self.pick(FindAction::Do),
                    _ => Step::Done(None),
                }
            }
            (_, Some(('a' | 'A', false))) => return self.pick(FindAction::Panelize),
            (KeyCode::F(3), _) | (_, Some(('v' | 'V', false))) => {
                return self.pick(FindAction::View)
            }
            (KeyCode::F(4), _) | (_, Some(('e' | 'E', false))) => {
                return self.pick(FindAction::Edit)
            }
            (_, Some(('@' | 'd' | 'D', false))) => return self.pick(FindAction::Do),
            _ => {}
        }
        Step::Continue
    }

    fn render(&self, area: Rect, buf: &mut Buffer, theme: &Theme) -> Option<(u16, u16)> {
        let n = self.entries.len();
        let h = n.min((area.height as usize).saturating_sub(8)).min(12) + 5;
        let wmax = self
            .entries
            .iter()
            .map(|e| width(e))
            .max()
            .unwrap_or(0)
            .max(60)
            .max(width(&self.title));
        let w = wmax.min((area.width as usize).saturating_sub(6)).min(100) + 4;
        let r = centered(area, w, h);
        frame(
            buf,
            r,
            theme,
            ColorItem::Selectitem,
            ColorItem::SelectitemTitle,
            &self.title,
        );
        let h0 = (r.height as usize).saturating_sub(4).max(1);
        self.page.set(h0);
        let inner_w = (r.width as usize).saturating_sub(4);
        let a = self.cur / h0 * h0;
        for i in 0..h0 {
            let Some(e) = self.entries.get(a + i) else {
                break;
            };
            let item = if a + i == self.cur {
                ColorItem::SelectitemCursor
            } else {
                ColorItem::Selectitem
            };
            put(
                buf,
                r,
                r.x + 2,
                r.y + 1 + i as u16,
                &text2wrap(e, inner_w, true, 0.9, true),
                style(theme, item),
            );
        }
        if r.height >= 4 {
            let sep_y = r.bottom() - 3;
            let st = style(theme, ColorItem::Selectitem);
            put(
                buf,
                r,
                r.x,
                sep_y,
                &format!("├{}┤", "─".repeat((r.width as usize).saturating_sub(2))),
                st,
            );
            let mut x = r.x + 3;
            for (k, label) in FIND_BUTTONS.iter().enumerate() {
                let item = if k == self.btn {
                    ColorItem::SelectitemCursor
                } else {
                    ColorItem::Selectitem
                };
                put(buf, r, x, r.bottom() - 2, label, style(theme, item));
                x += label.len() as u16 + 2;
            }
        }
        None
    }
}

pub enum PermsAnswer {
    Apply {
        mode: u32,
        recursive: bool,
        for_all: bool,
    },
    Ignore,
    Stop,
}

const OK: usize = 11;
const CANCEL: usize = 12;
const ALL: usize = 13;
const IGNORE: usize = 14;

/// Focus order shared by both dialogs: fields, then [All] [Ignore] when many files, then [Ok] [Cancel].
fn focus_order(fields: &[usize], n: usize) -> Vec<usize> {
    let mut v = fields.to_vec();
    if n > 1 {
        v.extend([ALL, IGNORE]);
    }
    v.extend([OK, CANCEL]);
    v
}

fn cycle(order: &[usize], cur: usize, back: bool) -> usize {
    let i = order.iter().position(|&f| f == cur).unwrap_or(0);
    let n = order.len();
    order[if back { (i + n - 1) % n } else { (i + 1) % n }]
}

/// Header, file line and buttons common to the permissions and owner dialogs.
#[allow(clippy::too_many_arguments)]
fn tool_frame(
    buf: &mut Buffer,
    area: Rect,
    theme: &Theme,
    title: &str,
    w: usize,
    name: &str,
    i: usize,
    n: usize,
    focus: usize,
) -> Rect {
    let r = centered(area, w, 11);
    frame(
        buf,
        r,
        theme,
        ColorItem::Dialog,
        ColorItem::DialogTitle,
        title,
    );
    let st = style(theme, ColorItem::Dialog);
    let counter = if n > 1 {
        format!("{i}/{n}")
    } else {
        String::new()
    };
    if !counter.is_empty() {
        put(
            buf,
            r,
            r.right().saturating_sub(2 + counter.len() as u16),
            r.y + 2,
            &counter,
            st,
        );
    }
    put(buf, r, r.x + 2, r.y + 2, "File:", st);
    let name_w = (r.width as usize).saturating_sub(11 + counter.len());
    put(
        buf,
        r,
        r.x + 8,
        r.y + 2,
        &text2wrap(name, name_w, true, 0.66, true),
        style(theme, ColorItem::DialogTitle),
    );
    let btn = |b: usize| {
        style(
            theme,
            if focus == b {
                ColorItem::ButtonActive
            } else {
                ColorItem::ButtonInactive
            },
        )
    };
    let y = r.bottom().saturating_sub(2);
    put(buf, r, r.right().saturating_sub(21), y, "[<Ok>]", btn(OK));
    put(
        buf,
        r,
        r.right().saturating_sub(13),
        y,
        "[ Cancel ]",
        btn(CANCEL),
    );
    if n > 1 {
        put(buf, r, r.x + 3, y, "[ All ]", btn(ALL));
        put(buf, r, r.x + 12, y, "[ Ignore ]", btn(IGNORE));
    }
    r
}

/// lfm's DialogPerms: toggle r/w/x/s/t per triplet, recursive flag, All/Ignore for many files.
pub struct PermsDialog {
    name: String,
    old: String,
    perms: Vec<char>,
    recursive: bool,
    i: usize,
    n: usize,
    focus: usize,
}

impl PermsDialog {
    pub fn new(name: &str, mode: u32, i: usize, n: usize) -> Self {
        let old = perms_display(mode);
        PermsDialog {
            name: name.into(),
            perms: old.chars().collect(),
            old,
            recursive: false,
            i,
            n,
            focus: 0,
        }
    }

    fn apply(&self, for_all: bool) -> Step<PermsAnswer> {
        let s: String = self.perms.iter().collect();
        Step::Done(PermsAnswer::Apply {
            mode: str2perms(&s),
            recursive: self.recursive,
            for_all,
        })
    }

    fn toggle(&mut self, offset: usize, on: char) {
        let d = offset + 3 * self.focus;
        self.perms[d] = if self.perms[d] == '-' { on } else { '-' };
    }
}

impl Dialog for PermsDialog {
    type Output = PermsAnswer;
    fn handle(&mut self, key: Key, _ctx: &EntryCtx) -> Step<PermsAnswer> {
        let order = focus_order(&[0, 1, 2, 3], self.n);
        let ch = match key.code {
            KeyCode::Char(c) if !key.alt => Some((c.to_ascii_lowercase(), key.ctrl)),
            _ => None,
        };
        match (key.code, ch) {
            (KeyCode::Esc, _) | (_, Some(('q', false))) | (_, Some(('c', true))) => {
                return Step::Done(PermsAnswer::Stop)
            }
            (KeyCode::Tab | KeyCode::Down | KeyCode::Right, _) => {
                self.focus = cycle(&order, self.focus, false)
            }
            (KeyCode::BackTab | KeyCode::Up | KeyCode::Left, _) => {
                self.focus = cycle(&order, self.focus, true)
            }
            (_, Some(('r', false))) if self.focus <= 2 => self.toggle(0, 'r'),
            (_, Some(('w', false))) if self.focus <= 2 => self.toggle(1, 'w'),
            (_, Some(('x', false))) if self.focus <= 2 => self.toggle(2, 'x'),
            (_, Some(('s', false))) if self.focus <= 1 => self.toggle(2, 's'),
            (_, Some(('t', false))) if self.focus == 2 => self.toggle(2, 't'),
            (KeyCode::Enter, _) | (_, Some((' ', false))) => match self.focus {
                3 => self.recursive = !self.recursive,
                CANCEL => return Step::Done(PermsAnswer::Stop),
                ALL => return self.apply(true),
                IGNORE => return Step::Done(PermsAnswer::Ignore),
                _ => return self.apply(false),
            },
            (_, Some(('i', false))) => return Step::Done(PermsAnswer::Ignore),
            (_, Some(('a', false))) => return self.apply(true),
            _ => {}
        }
        Step::Continue
    }

    fn render(&self, area: Rect, buf: &mut Buffer, theme: &Theme) -> Option<(u16, u16)> {
        let r = tool_frame(
            buf,
            area,
            theme,
            "Change file(s) permissions",
            46,
            &self.name,
            self.i,
            self.n,
            self.focus,
        );
        let st = style(theme, ColorItem::Dialog);
        let sel = |f: usize| {
            style(
                theme,
                if self.focus == f {
                    ColorItem::DialogPerms
                } else {
                    ColorItem::Dialog
                },
            )
        };
        put(
            buf,
            r,
            r.x + 2,
            r.y + 4,
            "      owner  group  other        recursive",
            st,
        );
        put(
            buf,
            r,
            r.x + 2,
            r.y + 5,
            "new:  [---]  [---]  [---]           [ ]",
            st,
        );
        put(
            buf,
            r,
            r.x + 2,
            r.y + 6,
            "old:  [---]  [---]  [---]           [ ]",
            st,
        );
        let new: String = self.perms.iter().collect();
        for (k, x) in [9u16, 16, 23].into_iter().enumerate() {
            put(buf, r, r.x + x, r.y + 5, &new[3 * k..3 * k + 3], sel(k));
            put(buf, r, r.x + x, r.y + 6, &self.old[3 * k..3 * k + 3], st);
        }
        put(
            buf,
            r,
            r.x + 39,
            r.y + 5,
            if self.recursive { "X" } else { " " },
            sel(3),
        );
        None
    }
}

pub enum OwnerAnswer {
    Apply {
        owner: String,
        group: String,
        recursive: bool,
        for_all: bool,
    },
    Ignore,
    Stop,
}

/// lfm's DialogOwner: owner and group picked from lists, recursive on by default.
pub struct OwnerDialog {
    name: String,
    owner: String,
    group: String,
    owner_old: String,
    group_old: String,
    owners: Vec<String>,
    groups: Vec<String>,
    recursive: bool,
    i: usize,
    n: usize,
    focus: usize,
}

impl OwnerDialog {
    pub fn new(
        name: &str,
        owner: &str,
        group: &str,
        owners: Vec<String>,
        groups: Vec<String>,
        i: usize,
        n: usize,
    ) -> Self {
        OwnerDialog {
            name: name.into(),
            owner: owner.into(),
            group: group.into(),
            owner_old: owner.into(),
            group_old: group.into(),
            owners,
            groups,
            recursive: true,
            i,
            n,
            focus: 0,
        }
    }

    fn apply(&self, for_all: bool) -> Step<OwnerAnswer> {
        Step::Done(OwnerAnswer::Apply {
            owner: self.owner.clone(),
            group: self.group.clone(),
            recursive: self.recursive,
            for_all,
        })
    }
}

/// lfm's `__fmt_text`: cut or pad with '-' to `n` chars.
fn dash_pad(text: &str, n: usize) -> String {
    let mut s: String = text.chars().take(n).collect();
    while s.chars().count() < n {
        s.push('-');
    }
    s
}

impl Dialog for OwnerDialog {
    type Output = OwnerAnswer;
    fn handle(&mut self, key: Key, _ctx: &EntryCtx) -> Step<OwnerAnswer> {
        let order = focus_order(&[0, 1, 3], self.n);
        let ch = match key.code {
            KeyCode::Char(c) if !key.alt => Some((c.to_ascii_lowercase(), key.ctrl)),
            _ => None,
        };
        match (key.code, ch) {
            (KeyCode::Esc, _) | (_, Some(('q', false))) | (_, Some(('c', true))) => {
                return Step::Done(OwnerAnswer::Stop)
            }
            (KeyCode::Tab | KeyCode::Down | KeyCode::Right, _) => {
                self.focus = cycle(&order, self.focus, false)
            }
            (KeyCode::BackTab | KeyCode::Up | KeyCode::Left, _) => {
                self.focus = cycle(&order, self.focus, true)
            }
            (KeyCode::Enter, _) | (_, Some((' ', false))) => match self.focus {
                0 => {
                    return Step::Select {
                        title: "Select new owner",
                        entries: self.owners.clone(),
                        kind: SelectKind::Owner,
                    }
                }
                1 => {
                    return Step::Select {
                        title: "Select new group",
                        entries: self.groups.clone(),
                        kind: SelectKind::Group,
                    }
                }
                3 => self.recursive = !self.recursive,
                CANCEL => return Step::Done(OwnerAnswer::Stop),
                ALL => return self.apply(true),
                IGNORE => return Step::Done(OwnerAnswer::Ignore),
                _ => return self.apply(false),
            },
            (_, Some(('i', false))) => return Step::Done(OwnerAnswer::Ignore),
            (_, Some(('a', false))) => return self.apply(true),
            _ => {}
        }
        Step::Continue
    }

    fn apply_selection(&mut self, kind: SelectKind, chosen: &str, _ctx: &EntryCtx) {
        match kind {
            SelectKind::Owner => self.owner = chosen.to_string(),
            SelectKind::Group => self.group = chosen.to_string(),
            _ => {}
        }
    }

    fn render(&self, area: Rect, buf: &mut Buffer, theme: &Theme) -> Option<(u16, u16)> {
        let r = tool_frame(
            buf,
            area,
            theme,
            "Change file(s) owner/group",
            49,
            &self.name,
            self.i,
            self.n,
            self.focus,
        );
        let st = style(theme, ColorItem::Dialog);
        let sel = |f: usize| {
            style(
                theme,
                if self.focus == f {
                    ColorItem::DialogPerms
                } else {
                    ColorItem::Dialog
                },
            )
        };
        put(
            buf,
            r,
            r.x + 2,
            r.y + 4,
            "          owner         group       recursive",
            st,
        );
        put(
            buf,
            r,
            r.x + 2,
            r.y + 5,
            "new:  [----------]  [----------]       [ ]",
            st,
        );
        put(
            buf,
            r,
            r.x + 2,
            r.y + 6,
            "old:  [----------]  [----------]       [ ]",
            st,
        );
        put(buf, r, r.x + 9, r.y + 5, &dash_pad(&self.owner, 10), sel(0));
        put(
            buf,
            r,
            r.x + 23,
            r.y + 5,
            &dash_pad(&self.group, 10),
            sel(1),
        );
        put(
            buf,
            r,
            r.x + 42,
            r.y + 5,
            if self.recursive { "X" } else { " " },
            sel(3),
        );
        put(buf, r, r.x + 9, r.y + 6, &self.owner_old, st);
        put(buf, r, r.x + 23, r.y + 6, &self.group_old, st);
        None
    }
}

/// lfm's `Tab.__check_rebuild`: the configured default, confirmed if asked to.
pub fn ask_rebuild(
    screen: &mut dyn Screen,
    app: &mut App,
    layer: &VfsLayer,
) -> std::io::Result<bool> {
    let default = app.settings.options.rebuild_vfs;
    if !app.settings.confirmations.ask_rebuild_vfs {
        return Ok(default);
    }
    let name = layer.source.file_name().map_or_else(
        || layer.source.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    run_modal(
        screen,
        app,
        ConfirmDialog::new("Rebuild vfs file", &name, default),
    )
}

/// Runs a VFS command under the job panel; stderr or a stop is an error (lfm).
fn vfs_command(
    screen: &mut dyn Screen,
    app: &mut App,
    title: &str,
    subtitle: &str,
    cmd: &OsStr,
    cwd: &Path,
) -> Result<(), String> {
    match run_job(screen, app, title, subtitle, cmd, cwd) {
        Ok(r) if r.stopped => Err("Stopped by user".to_string()),
        Ok(r) if !r.stderr.trim().is_empty() => Err(r.stderr.trim_end().to_string()),
        Ok(_) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

fn show_error(screen: &mut dyn Screen, app: &mut App, msg: &str) -> std::io::Result<()> {
    run_modal(screen, app, ErrorDialog::new(msg))
}

/// Closes `layer` (rebuilding it if asked) and reports a failed rebuild.
fn close_layer(
    screen: &mut dyn Screen,
    app: &mut App,
    layer: &VfsLayer,
    rebuild: bool,
) -> std::io::Result<()> {
    let name = layer
        .source
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let result = {
        let mut run =
            |title: &str, cmd: &OsStr, cwd: &Path| vfs_command(screen, app, title, &name, cmd, cwd);
        vfs::close(layer, rebuild, &mut run)
    };
    match result {
        Ok(()) => Ok(()),
        Err(e) => show_error(
            screen,
            app,
            &format!("Cannot rebuild vfs file {}\n{e}", layer.source.display()),
        ),
    }
}

/// Opens `archive` as a virtual folder in the active tab.
pub fn enter_archive(
    screen: &mut dyn Screen,
    app: &mut App,
    archive: &Path,
) -> std::io::Result<()> {
    let display_parent = app.tab().display_dir();
    let sep = if display_parent.ends_with('/') {
        ""
    } else {
        "/"
    };
    let name = archive
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let shown = format!("{display_parent}{sep}{name}");
    if !detect(archive).is_some_and(|t| t.can_vfs()) {
        return show_error(
            screen,
            app,
            &format!("Cannot chdir {shown}\nInvalid directory?"),
        );
    }
    let opened = {
        let mut run =
            |title: &str, cmd: &OsStr, cwd: &Path| vfs_command(screen, app, title, &name, cmd, cwd);
        vfs::open_archive(archive, &display_parent, &mut run)
    };
    let layer = match opened {
        Ok(l) => l,
        Err(e) => return show_error(screen, app, &format!("Cannot chdir {shown}\n{e}")),
    };
    let tmp = layer.tmp.clone();
    app.tab_mut().vfs.push(layer);
    if let Err(msg) = app.goto(&tmp) {
        let layer = app.tab_mut().vfs.pop().expect("just pushed");
        let _ = std::fs::remove_dir_all(&layer.tmp);
        return show_error(screen, app, &msg);
    }
    Ok(())
}

/// Goes to `target`, first leaving every virtual folder that does not contain
/// it; one question decides whether the whole left chain is rebuilt.
pub fn change_dir(screen: &mut dyn Screen, app: &mut App, target: &Path) -> std::io::Result<()> {
    let leaving = app
        .tab()
        .vfs
        .iter()
        .rev()
        .take_while(|l| !target.starts_with(&l.tmp))
        .count();
    let rebuild = match (leaving, app.tab().vfs.last().cloned()) {
        (n, Some(innermost)) if n > 0 => ask_rebuild(screen, app, &innermost)?,
        _ => false,
    };
    if let Err(msg) = app.goto(target) {
        return show_error(screen, app, &msg);
    }
    // Innermost first, so a nested archive is packed into its parent before the parent is packed.
    for _ in 0..leaving {
        let layer = app.tab_mut().vfs.pop().expect("counted");
        close_layer(screen, app, &layer, rebuild)?;
    }
    Ok(())
}

/// ".." at the root of the innermost virtual folder: back to the archive's directory.
pub fn leave_vfs_root(screen: &mut dyn Screen, app: &mut App) -> std::io::Result<()> {
    let Some(layer) = app.tab().vfs.last().cloned() else {
        return Ok(());
    };
    let parent = layer
        .source
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("/"));
    change_dir(screen, app, &parent)?;
    if let Some(name) = layer.source.file_name() {
        let fh = app.fh();
        app.tab_mut().focus_file(name, fh);
    }
    Ok(())
}

/// Leaves every virtual folder of a tab (tab close, quit), rebuilding the chain if asked.
pub fn close_all_vfs(
    screen: &mut dyn Screen,
    app: &mut App,
    tab: (usize, usize),
) -> std::io::Result<()> {
    let layers = std::mem::take(&mut app.panes[tab.0].tabs[tab.1].vfs);
    let Some(innermost) = layers.last().cloned() else {
        return Ok(());
    };
    let rebuild = ask_rebuild(screen, app, &innermost)?;
    for layer in layers.iter().rev() {
        close_layer(screen, app, layer, rebuild)?;
    }
    Ok(())
}

/// Find files by name, or grep their contents, then act on a result (lfm's find_grep).
pub fn find_grep(screen: &mut dyn Screen, app: &mut App) -> std::io::Result<Flow> {
    let d = DoubleEntryDialog::new(
        "Find files",
        "Filename",
        "Content",
        "*",
        "",
        app.history.entries(HistorySection::Find).to_vec(),
        app.history.entries(HistorySection::Grep).to_vec(),
        false,
    );
    let Some((fs, pat)) = run_modal(screen, app, d)? else {
        return CONTINUE;
    };
    if fs.is_empty() {
        return CONTINUE;
    }
    let dir = app.tab().dir().to_path_buf();
    let opts = app.settings.options.clone();
    let grep = !pat.is_empty();
    let (cmd, subtitle) = if grep {
        (
            grep_cmd(&pat, &fs, opts.grep_regex, opts.grep_ignorecase),
            format!("Searching for \"{pat}\" in files with \"{fs}\" in their name"),
        )
    } else {
        (
            find_cmd(&dir, &fs, opts.find_ignorecase),
            format!("Searching for files with \"{fs}\" in their name"),
        )
    };
    let r = run_job(screen, app, "Find files", &subtitle, &cmd, &dir)?;
    if r.stopped {
        return CONTINUE;
    }
    if r.stdout.trim().is_empty() && !r.stderr.trim().is_empty() {
        let msg = if grep {
            format!("Cannot grep \"{pat}\" in \"{fs}\" files\n{}", r.stderr)
        } else {
            format!("Cannot find \"{fs}\" files\n{}", r.stderr)
        };
        return error(screen, app, msg.trim_end());
    }
    app.history.append(HistorySection::Find, &fs);
    if grep {
        app.history.append(HistorySection::Grep, &pat);
    }
    let entries = if grep {
        parse_grep(&r.stdout)
    } else {
        parse_find(&r.stdout, &dir)
    };
    if entries.is_empty() {
        let msg = if grep {
            format!("Did not find \"{pat}\" in any file with \"{fs}\" in name")
        } else {
            format!("Did not find any file with \"{fs}\" in name")
        };
        return error(screen, app, &msg);
    }
    let title = if grep {
        format!("Find \"{pat}\" pattern in \"{fs}\" files")
    } else {
        format!("Find \"{fs}\" files")
    };
    loop {
        let Some((action, entry)) =
            run_modal(screen, app, FindGrepDialog::new(&title, entries.clone()))?
        else {
            app.refresh_all();
            return CONTINUE;
        };
        let (file, line) = if grep {
            grep_entry_target(&entry)
        } else {
            (entry.clone(), 0)
        };
        let path = dir.join(&file);
        match action {
            FindAction::Go => {
                let target = path
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| dir.clone());
                change_dir(screen, app, &target)?;
                if let Some(name) = path.file_name() {
                    let fh = app.fh();
                    app.tab_mut().focus_file(name, fh);
                }
                return CONTINUE;
            }
            FindAction::Panelize => {
                let files = panelize_files(&entries, grep);
                let display = app.tab().display_dir();
                match vfs::open_search(&dir, &display, &files) {
                    Ok(layer) => {
                        let tmp = layer.tmp.clone();
                        app.tab_mut().vfs.push(layer);
                        if let Err(msg) = app.goto(&tmp) {
                            let layer = app.tab_mut().vfs.pop().expect("just pushed");
                            let _ = std::fs::remove_dir_all(&layer.tmp);
                            return error(screen, app, &msg);
                        }
                    }
                    Err(e) => return error(screen, app, &e),
                }
                return CONTINUE;
            }
            FindAction::View | FindAction::Edit => {
                let base = program(
                    app,
                    if action == FindAction::View {
                        "pager"
                    } else {
                        "editor"
                    },
                );
                let prog = if grep && line > 0 {
                    format!("{base} +{line}")
                } else {
                    base
                };
                run_on_file(screen, app, &prog, &path)?;
            }
            FindAction::Do => {
                let title = "Execute command on file(s)";
                if let Some(cmd) = ask_entry(
                    screen,
                    app,
                    title,
                    "Enter command",
                    "",
                    Some(HistorySection::Exec),
                    false,
                )? {
                    if !cmd.is_empty() {
                        app.history.append(HistorySection::Exec, &cmd);
                        run_on_file(screen, app, &cmd, &path)?;
                    }
                }
            }
        }
    }
}

pub fn show_tree(screen: &mut dyn Screen, app: &mut App) -> std::io::Result<Flow> {
    let region = match (app.pane().mode, app.active) {
        (PaneMode::Half, 0) => Region::Right,
        (PaneMode::Half, _) => Region::Left,
        _ => Region::Full,
    };
    let bottom_rows = 1 + u16::from(app.settings.options.show_function_keys);
    let tree = DirsTree::new(app.tab().dir(), app.settings.options.show_dotfiles);
    if let Some(p) = run_modal(
        screen,
        app,
        TreeDialog {
            tree,
            region,
            bottom_rows,
        },
    )? {
        change_dir(screen, app, &p)?;
    }
    CONTINUE
}

fn asctime(t: i64) -> String {
    chrono::Local.timestamp_opt(t, 0).single().map_or_else(
        || t.to_string(),
        |d| d.format("%a %b %e %H:%M:%S %Y").to_string(),
    )
}

/// lfm's show_file_info report.
pub fn show_file_info(screen: &mut dyn Screen, app: &mut App) -> std::io::Result<Flow> {
    let f = app.tab().current().clone();
    let meta = match std::fs::symlink_metadata(&f.path) {
        Ok(m) => m,
        Err(e) => return error(screen, app, &format!("Cannot show file info\n{e}")),
    };
    let user = std::env::var("USER").unwrap_or_else(|_| owner2str(nix::unistd::getuid().as_raw()));
    let un = nix::sys::utsname::uname().ok();
    let uname_field = |f: fn(&nix::sys::utsname::UtsName) -> &std::ffi::OsStr| {
        un.as_ref()
            .map(|u| f(u).to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    let (green, red) = (ColorItem::ViewGreenOnBlack, ColorItem::ViewRedOnBlack);
    let fs_target = app
        .tab()
        .vfs
        .first()
        .map(|l| l.source.clone())
        .unwrap_or_else(|| f.path.clone());
    let (mountpoint, device, fstype) = mountpoint_for(&fs_target);
    let lines = vec![
        (
            format!(
                "yafm v{} executed by {}",
                env!("CARGO_PKG_VERSION"),
                user_fullname(&user)
            ),
            green,
        ),
        (
            format!(
                "<{user}@{}> on {} {} [{}]",
                uname_field(|u| u.nodename()),
                uname_field(|u| u.sysname()),
                uname_field(|u| u.release()),
                uname_field(|u| u.machine())
            ),
            green,
        ),
        (String::new(), green),
        (
            format!(
                "{}: {} ({})",
                f.ftype.description(),
                f.display_name(),
                file_type_info(&f.path)
            ),
            red,
        ),
        (format!("Path: {}", app.tab().display_dir()), red),
        (format!("Size: {} bytes", f.size), red),
        (format!("Mode: {} ({:o})", perms2str(f.mode), f.mode), red),
        (format!("Links: {}", meta.nlink()), red),
        (
            format!(
                "User ID: {} ({}) / Group ID: {} ({})",
                owner2str(f.uid),
                f.uid,
                group2str(f.gid),
                f.gid
            ),
            red,
        ),
        (format!("Last access: {}", asctime(meta.atime())), red),
        (format!("Last modification: {}", asctime(meta.mtime())), red),
        (format!("Last change: {}", asctime(meta.ctime())), red),
        (
            format!(
                "Location: {}, {} / Inode: #{:X} ({:X}h:{:X}h)",
                major(meta.dev()),
                minor(meta.dev()),
                meta.ino(),
                meta.dev(),
                meta.ino()
            ),
            red,
        ),
        (
            format!("File system: {device} on {mountpoint} ({fstype})"),
            red,
        ),
    ];
    run_modal(
        screen,
        app,
        ViewDialog::new(&format!("Information about '{}'", f.display_name()), lines),
    )?;
    app.refresh_all();
    CONTINUE
}

pub fn show_filesystems_info(screen: &mut dyn Screen, app: &mut App) -> std::io::Result<Flow> {
    let buf = match filesystems_info() {
        Ok(b) => b,
        Err(e) => return error(screen, app, &format!("Cannot show filesystems info:\n{e}")),
    };
    let mut rows = buf.trim().lines();
    let hdr = rows.next().unwrap_or("").trim().to_string();
    let mut lines = vec![
        (hdr.clone(), ColorItem::ViewRedOnBlack),
        ("-".repeat(width(&hdr)), ColorItem::ViewRedOnBlack),
    ];
    lines.extend(rows.map(|l| (l.trim().to_string(), ColorItem::ViewWhiteOnBlack)));
    run_modal(screen, app, ViewDialog::new("Show filesystems info", lines))?;
    app.refresh_all();
    CONTINUE
}

/// Runs a tool command; shows `Cannot "{cmd}"` with stderr. Returns false when stopped.
fn run_tool(
    screen: &mut dyn Screen,
    app: &mut App,
    title: &str,
    cmd: &OsStr,
    dir: &Path,
) -> std::io::Result<bool> {
    let shown = cmd.to_string_lossy().into_owned();
    let r = run_job(screen, app, title, &shown, cmd, dir)?;
    if r.stopped {
        return Ok(false);
    }
    if !r.stderr.trim().is_empty() {
        run_modal(
            screen,
            app,
            ErrorDialog::new(&format!("Cannot \"{shown}\"\n{}", r.stderr.trim_end())),
        )?;
    }
    Ok(true)
}

fn targets(app: &App) -> Vec<FsEntry> {
    app.tab()
        .selected_or_current()
        .into_iter()
        .cloned()
        .collect()
}

pub fn change_perms(screen: &mut dyn Screen, app: &mut App) -> std::io::Result<Flow> {
    let files = targets(app);
    if files.is_empty() {
        return CONTINUE;
    }
    app.tab_mut().selected.clear();
    let dir = app.tab().dir().to_path_buf();
    let mut all: Option<(u32, bool)> = None;
    for (i, f) in files.iter().enumerate() {
        let (mode, recursive) = match all {
            Some(a) => a,
            None => match run_modal(
                screen,
                app,
                PermsDialog::new(&f.display_name(), f.mode, i + 1, files.len()),
            )? {
                PermsAnswer::Stop => break,
                PermsAnswer::Ignore => continue,
                PermsAnswer::Apply {
                    mode,
                    recursive,
                    for_all,
                } => {
                    if for_all {
                        all = Some((mode, recursive));
                    }
                    (mode, recursive)
                }
            },
        };
        if !run_tool(
            screen,
            app,
            "Change file(s) permission",
            &chmod_cmd(mode, recursive, &f.path),
            &dir,
        )? {
            break;
        }
    }
    app.refresh_all();
    CONTINUE
}

pub fn change_owner(screen: &mut dyn Screen, app: &mut App) -> std::io::Result<Flow> {
    let files = targets(app);
    if files.is_empty() {
        return CONTINUE;
    }
    app.tab_mut().selected.clear();
    let dir = app.tab().dir().to_path_buf();
    let (owners, groups) = (users(), groups());
    let mut all: Option<(String, String, bool)> = None;
    for (i, f) in files.iter().enumerate() {
        let (owner, group, recursive) = match all.clone() {
            Some(a) => a,
            None => {
                let d = OwnerDialog::new(
                    &f.display_name(),
                    &owner2str(f.uid),
                    &group2str(f.gid),
                    owners.clone(),
                    groups.clone(),
                    i + 1,
                    files.len(),
                );
                match run_modal(screen, app, d)? {
                    OwnerAnswer::Stop => break,
                    OwnerAnswer::Ignore => continue,
                    OwnerAnswer::Apply {
                        owner,
                        group,
                        recursive,
                        for_all,
                    } => {
                        if for_all {
                            all = Some((owner.clone(), group.clone(), recursive));
                        }
                        (owner, group, recursive)
                    }
                }
            }
        };
        if !run_tool(
            screen,
            app,
            "Change file(s) owner",
            &chown_cmd(&owner, recursive, &f.path),
            &dir,
        )? || !run_tool(
            screen,
            app,
            "Change file(s) group",
            &chgrp_cmd(&group, recursive, &f.path),
            &dir,
        )? {
            break;
        }
    }
    app.refresh_all();
    CONTINUE
}

pub fn diff_file_with_backup(screen: &mut dyn Screen, app: &mut App) -> std::io::Result<Flow> {
    let ext = app.settings.misc.backup_extension.clone();
    let file = app.tab().current().path.clone();
    let s = file.to_string_lossy().into_owned();
    let (old, new) = match s.strip_suffix(&ext) {
        Some(base) if !ext.is_empty() => (file.clone(), PathBuf::from(base)),
        _ => (PathBuf::from(format!("{s}{ext}")), file.clone()),
    };
    if !old.exists() {
        return error(screen, app, "Cannot diff file\nBackup file does not exist");
    }
    if !new.exists() {
        return error(screen, app, "Cannot diff file\nOnly backup file exists");
    }
    if !old.is_file() || !new.is_file() {
        return error(
            screen,
            app,
            "Cannot diff file\nWe can only diff regular files",
        );
    }
    let diff = match diff_with_backup(&old, &new, app.settings.misc.diff_type) {
        Ok(d) => d,
        Err(e) => return error(screen, app, &format!("Cannot diff file\n{e}")),
    };
    if diff.is_empty() {
        return error(screen, app, "Files are identical");
    }
    let name = |p: &Path| {
        p.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    let tmp = match tempfile::Builder::new().prefix("yafm-diff-").tempdir() {
        Ok(t) => t,
        Err(e) => return error(screen, app, &format!("Cannot diff file\n{e}")),
    };
    let out = tmp
        .path()
        .join(format!("{} DIFF {}", name(&old), name(&new)));
    if let Err(e) = std::fs::write(&out, diff) {
        return error(screen, app, &format!("Cannot diff file\n{e}"));
    }
    let pager = program(app, "pager");
    run_on_file(screen, app, &pager, &out)
}

/// Unpacks each selected archive into `dest`.
pub fn uncompress(screen: &mut dyn Screen, app: &mut App, dest: &Path) -> std::io::Result<Flow> {
    let files = selected_paths(app);
    if files.is_empty() {
        return CONTINUE;
    }
    app.tab_mut().selected.clear();
    for f in files {
        let name = f
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if !f.is_file() {
            run_modal(
                screen,
                app,
                ErrorDialog::new(&format!("It's not a file: {name}")),
            )?;
            continue;
        }
        let Some(t) = detect(&f) else {
            run_modal(
                screen,
                app,
                ErrorDialog::new(&format!("Cannot uncompress this file type: {name}")),
            )?;
            continue;
        };
        let r = run_job(
            screen,
            app,
            "Uncompress file(s)",
            &name,
            &uncompress_cmd(t, &f),
            dest,
        )?;
        if r.stopped {
            break;
        }
        if !r.stderr.trim().is_empty() {
            run_modal(screen, app, ErrorDialog::new(r.stderr.trim_end()))?;
        }
    }
    clear_selection_and_refresh(app);
    CONTINUE
}

fn ask_format(
    screen: &mut dyn Screen,
    app: &mut App,
    title: &str,
    question: &str,
    keys: &[(char, ArchiveType)],
) -> std::io::Result<Option<ArchiveType>> {
    loop {
        match run_modal(screen, app, GetKeyDialog::new(title, question))? {
            None => return Ok(None),
            Some(c) => {
                if let Some((_, t)) = keys.iter().find(|(k, _)| *k == c) {
                    return Ok(Some(*t));
                }
            }
        }
    }
}

/// Packs each selected item (run in its directory); a missing compressor shows its error.
fn pack_each(
    screen: &mut dyn Screen,
    app: &mut App,
    files: Vec<PathBuf>,
    pick: impl Fn(&Path, &str) -> Result<OsString, String>,
) -> std::io::Result<Flow> {
    app.tab_mut().selected.clear();
    for f in files {
        let name = f
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let cmd = match pick(&f, &name) {
            Ok(c) => c,
            Err(msg) => {
                run_modal(screen, app, ErrorDialog::new(&msg))?;
                continue;
            }
        };
        let dir = f
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("/"));
        let r = run_job(screen, app, "Compress dir(s)", &name, &cmd, &dir)?;
        if r.stopped {
            break;
        }
        if !r.stderr.trim().is_empty() {
            run_modal(screen, app, ErrorDialog::new(r.stderr.trim_end()))?;
        }
    }
    clear_selection_and_refresh(app);
    CONTINUE
}

pub fn compress_dir(screen: &mut dyn Screen, app: &mut App) -> std::io::Result<Flow> {
    let files = selected_paths(app);
    if files.is_empty() {
        return CONTINUE;
    }
    use ArchiveType as T;
    let keys = [
        ('g', T::Tgz),
        ('b', T::Tbz2),
        ('x', T::Txz),
        ('l', T::Tlz),
        ('4', T::Tlz4),
        ('t', T::Tar),
        ('z', T::Zip),
        ('r', T::Rar),
        ('7', T::SevenZ),
    ];
    let q = ".tar.(g)z, .tar.(b)z2, .tar.(x)z, .tar.(l)z,\n.tar.lz(4), .(t)ar, .(z)ip, .(r)ar, .(7)z\n\n                Ctrl-C to quit";
    let Some(t) = ask_format(screen, app, "Compress directory to...", q, &keys)? else {
        return CONTINUE;
    };
    pack_each(screen, app, files, |f, name| {
        if !f.is_dir() {
            return Err(format!("It's not a directory: {name}"));
        }
        compress_cmd(t, f).ok_or_else(|| format!("Cannot compress: {name}"))
    })
}

pub fn compress_uncompress_file(screen: &mut dyn Screen, app: &mut App) -> std::io::Result<Flow> {
    let files = selected_paths(app);
    if files.is_empty() {
        return CONTINUE;
    }
    use ArchiveType as T;
    let keys = [
        ('g', T::Gz),
        ('b', T::Bz2),
        ('x', T::Xz),
        ('l', T::Lz),
        ('4', T::Lz4),
    ];
    let q = "(g)zip, (b)zip2, (x)z, (l)z, lz(4)\n\n   Ctrl-C to quit";
    let Some(t) = ask_format(screen, app, "Un/Compress file(s)", q, &keys)? else {
        return CONTINUE;
    };
    pack_each(screen, app, files, |f, name| {
        if !f.is_file() {
            return Err(format!("It's not a file: {name}"));
        }
        match detect(f) {
            None | Some(T::Tar) => {
                compress_cmd(t, f).ok_or_else(|| format!("Cannot compress: {name}"))
            }
            Some(d) if d == t => Ok(uncompress_cmd(d, f)),
            Some(_) => Err(format!(
                "Cannot un/compress '{name}' with type {}",
                t.name()
            )),
        }
    })
}

#[cfg(test)]
pub(crate) fn ctx() -> EntryCtx {
    EntryCtx {
        current_name: String::new(),
        cur_dir: "/".into(),
        other_dir: "/".into(),
        bookmarks: vec![],
        prev_paths: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::draw::buffer_line;
    use crate::ui::screen::TestScreen;

    fn k(s: &str) -> Key {
        crate::keys::parse_key(s).unwrap()
    }

    #[test]
    fn tree_dialog_moves_and_returns_path() {
        let t = tempfile::tempdir().unwrap();
        for p in ["a/a1", "a/a2", "b"] {
            std::fs::create_dir_all(t.path().join(p)).unwrap();
        }
        let theme = Theme::default_theme();
        let mut d = TreeDialog {
            tree: DirsTree::new(&t.path().join("a"), false),
            region: Region::Right,
            bottom_rows: 2,
        };
        d.handle(k("right"), &ctx());
        let r = Rect::new(0, 0, 100, 30);
        let mut b = Buffer::empty(r);
        d.render(r, &mut b, &theme);
        let text: String = (0..30)
            .map(|y| buffer_line(&b, y))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            text.contains("Tree")
                && text.contains("a1")
                && (text.contains("├─") || text.contains("└─"))
        );
        assert!(text.contains(&format!("Path: {}", t.path().join("a/a1").display())));
        match d.handle(k("enter"), &ctx()) {
            Step::Done(p) => assert_eq!(p, Some(t.path().join("a/a1"))),
            _ => panic!("not done"),
        }
        let mut d = TreeDialog {
            tree: DirsTree::new(t.path(), false),
            region: Region::Full,
            bottom_rows: 1,
        };
        assert!(matches!(d.handle(k("q"), &ctx()), Step::Done(None)));
    }

    #[test]
    fn tree_renders_in_small_areas() {
        let theme = Theme::default_theme();
        for (w, h) in [(66u16, 5u16), (10, 3), (1, 1)] {
            for region in [Region::Left, Region::Right, Region::Full] {
                let d = TreeDialog {
                    tree: DirsTree::new(std::path::Path::new("/"), false),
                    region,
                    bottom_rows: 2,
                };
                let r = Rect::new(0, 0, w, h);
                let mut b = Buffer::empty(r);
                d.render(r, &mut b, &theme);
            }
        }
    }

    #[test]
    fn find_grep_command_lines() {
        assert_eq!(
            find_cmd(Path::new("/d d"), "*.rs", true),
            OsString::from("find '/d d' -iname '*.rs' -print")
        );
        assert_eq!(
            find_cmd(Path::new("/d"), "x", false),
            OsString::from("find '/d' -name 'x' -print")
        );
        assert_eq!(
            grep_cmd("it's", "*.py", true, false),
            OsString::from("grep -REHn --include '*.py' -e 'it'\\''s' .")
        );
        assert_eq!(
            grep_cmd("a", "*", false, true),
            OsString::from("grep -RHni --include '*' -e 'a' .")
        );
    }

    #[test]
    fn parse_results() {
        assert_eq!(
            parse_find("/d\n/d/b\n/d/a/x\n", Path::new("/d")),
            ["a/x", "b"]
        );
        assert_eq!(parse_grep("./b:1:x\n./a:2:y\n\n"), ["a:2:y", "b:1:x"]);
        assert_eq!(
            grep_entry_target("a:b.txt:12:hit: more"),
            ("a:b.txt".into(), 12)
        );
        assert_eq!(grep_entry_target("plain.txt:3:x"), ("plain.txt".into(), 3));
        assert_eq!(
            grep_entry_target("Binary file x/y.bin matches"),
            ("x/y.bin".into(), 0)
        );
        assert_eq!(
            grep_entry_target("grep: z.bin: binary file matches"),
            ("z.bin".into(), 0)
        );
        assert_eq!(grep_entry_target("odd"), ("odd".into(), 0));
        assert_eq!(
            panelize_files(&["a:1:x".into(), "a:2:y".into(), "b:3:z".into()], true),
            [PathBuf::from("a"), PathBuf::from("b")]
        );
        assert_eq!(
            panelize_files(&["d/f".into()], false),
            [PathBuf::from("d/f")]
        );
    }

    #[test]
    fn real_grep_finds_hostile_names() {
        let t = tempfile::tempdir().unwrap();
        std::fs::write(t.path().join("it's $x.py"), "TODO here\n").unwrap();
        std::fs::write(t.path().join("-n.py"), "TODO too\n").unwrap();
        let (out, _) =
            crate::jobs::run_in_cli(&grep_cmd("TODO", "*.py", true, false), t.path()).unwrap();
        let found: Vec<(String, usize)> = parse_grep(&out)
            .iter()
            .map(|e| grep_entry_target(e))
            .collect();
        assert!(
            found.contains(&("it's $x.py".into(), 1)) && found.contains(&("-n.py".into(), 1)),
            "{found:?}"
        );
        let (out, _) =
            crate::jobs::run_in_cli(&find_cmd(t.path(), "*.py", false), t.path()).unwrap();
        assert_eq!(parse_find(&out, t.path()).len(), 2);
    }

    #[test]
    fn results_dialog_keys() {
        let mut d = FindGrepDialog::new("Find", vec!["a".into(), "b".into()]);
        d.handle(k("j"), &ctx());
        assert!(
            matches!(d.handle(k("v"), &ctx()), Step::Done(Some((FindAction::View, ref e))) if e == "b")
        );
        let mut d = FindGrepDialog::new("Find", vec!["a".into()]);
        d.handle(k("left"), &ctx());
        assert!(matches!(d.handle(k("enter"), &ctx()), Step::Done(None)));
        let mut d = FindGrepDialog::new("Find", vec!["a".into()]);
        assert!(matches!(
            d.handle(k("enter"), &ctx()),
            Step::Done(Some((FindAction::Go, _)))
        ));
        let mut d = FindGrepDialog::new("Find", vec!["a".into()]);
        assert!(matches!(
            d.handle(k("a"), &ctx()),
            Step::Done(Some((FindAction::Panelize, _)))
        ));
        let mut d = FindGrepDialog::new("Find", vec!["a".into()]);
        assert!(matches!(
            d.handle(k("@"), &ctx()),
            Step::Done(Some((FindAction::Do, _)))
        ));
        let theme = Theme::default_theme();
        for (w, h) in [(100u16, 30u16), (20, 4), (1, 1)] {
            let r = Rect::new(0, 0, w, h);
            let mut b = Buffer::empty(r);
            d.render(r, &mut b, &theme);
        }
    }

    #[test]
    fn perms_dialog_toggles_and_applies() {
        let mut d = PermsDialog::new("f", 0o644, 1, 1);
        d.handle(k("x"), &ctx());
        d.handle(k("tab"), &ctx());
        d.handle(k("w"), &ctx());
        match d.handle(k("enter"), &ctx()) {
            Step::Done(PermsAnswer::Apply {
                mode,
                recursive,
                for_all,
            }) => assert_eq!((mode, recursive, for_all), (0o764, false, false)),
            _ => panic!("not applied"),
        }
        let mut d = PermsDialog::new("f", 0o4644, 1, 1);
        assert!(matches!(
            d.handle(k("enter"), &ctx()),
            Step::Done(PermsAnswer::Apply { mode: 0o4644, .. })
        ));
        let mut d = PermsDialog::new("f", 0o644, 1, 3);
        assert!(matches!(
            d.handle(k("i"), &ctx()),
            Step::Done(PermsAnswer::Ignore)
        ));
        let mut d = PermsDialog::new("f", 0o644, 1, 3);
        assert!(matches!(
            d.handle(k("a"), &ctx()),
            Step::Done(PermsAnswer::Apply { for_all: true, .. })
        ));
        let mut d = PermsDialog::new("f", 0o644, 1, 1);
        assert!(matches!(
            d.handle(k("esc"), &ctx()),
            Step::Done(PermsAnswer::Stop)
        ));
        let mut d = PermsDialog::new("d", 0o755, 1, 1);
        for _ in 0..3 {
            d.handle(k("tab"), &ctx());
        }
        d.handle(k("spc"), &ctx());
        d.handle(k("tab"), &ctx());
        assert!(matches!(
            d.handle(k("enter"), &ctx()),
            Step::Done(PermsAnswer::Apply {
                recursive: true,
                ..
            })
        ));
    }

    #[test]
    fn owner_dialog_selects_from_lists() {
        let mut d = OwnerDialog::new(
            "f",
            "me",
            "me",
            vec!["me".into(), "root".into()],
            vec!["me".into(), "wheel".into()],
            1,
            1,
        );
        match d.handle(k("enter"), &ctx()) {
            Step::Select {
                kind: SelectKind::Owner,
                entries,
                ..
            } => {
                assert_eq!(entries, ["me", "root"]);
                d.apply_selection(SelectKind::Owner, "root", &ctx());
            }
            _ => panic!("no owner list"),
        }
        for _ in 0..3 {
            d.handle(k("tab"), &ctx());
        }
        match d.handle(k("enter"), &ctx()) {
            Step::Done(OwnerAnswer::Apply {
                owner,
                group,
                recursive,
                ..
            }) => {
                assert_eq!(
                    (owner.as_str(), group.as_str(), recursive),
                    ("root", "me", true)
                )
            }
            _ => panic!("not applied"),
        }
        let mut d = OwnerDialog::new("f", "me", "me", vec![], vec![], 2, 2);
        assert!(matches!(
            d.handle(k("i"), &ctx()),
            Step::Done(OwnerAnswer::Ignore)
        ));
    }

    #[test]
    fn perms_owner_render() {
        let theme = Theme::default_theme();
        for (w, h) in [(100u16, 30u16), (20, 4), (1, 1)] {
            let r = Rect::new(0, 0, w, h);
            let mut b = Buffer::empty(r);
            PermsDialog::new("file", 0o4644, 1, 2).render(r, &mut b, &theme);
            OwnerDialog::new("file", "me", "grp", vec![], vec![], 1, 2).render(r, &mut b, &theme);
        }
        let r = Rect::new(0, 0, 100, 30);
        let mut b = Buffer::empty(r);
        PermsDialog::new("file", 0o4644, 1, 2).render(r, &mut b, &theme);
        let text: String = (0..30)
            .map(|y| buffer_line(&b, y))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            text.contains("Change file(s) permissions")
                && text.contains("rwS")
                && text.contains("[ All ]")
        );
    }

    fn make_tgz(d: &Path) -> PathBuf {
        tests_support_make(d)
    }

    #[test]
    fn grep_include_filter_is_honoured() {
        let t = tempfile::tempdir().unwrap();
        std::fs::write(t.path().join("a.txt"), "hit\n").unwrap();
        std::fs::write(t.path().join("b.rs"), "hit\n").unwrap();
        std::fs::write(t.path().join("-dash.txt"), "-x hit\n").unwrap();
        let (out, err) =
            crate::jobs::run_in_cli(&grep_cmd("hit", "*.txt", true, false), t.path()).unwrap();
        let files: Vec<String> = parse_grep(&out)
            .iter()
            .map(|e| grep_entry_target(e).0)
            .collect();
        assert!(
            files.contains(&"a.txt".to_string()) && files.contains(&"-dash.txt".to_string()),
            "{files:?} {err}"
        );
        assert!(!files.contains(&"b.rs".to_string()), "{files:?}");
        let (out, _) =
            crate::jobs::run_in_cli(&grep_cmd("-x", "*", false, false), t.path()).unwrap();
        assert_eq!(parse_grep(&out).len(), 1);
    }

    #[test]
    fn leaving_nested_archives_with_rebuild_packs_the_whole_chain() {
        let t = tempfile::tempdir().unwrap();
        let a = make_tgz(t.path());
        let mut app = vfs_app(t.path(), true);
        let mut s = TestScreen::new(100, 30, "y");
        enter_archive(&mut s, &mut app, &a).unwrap();
        let outer = app.tab().vfs[0].tmp.clone();
        crate::jobs::run_in_cli(
            std::ffi::OsStr::new("tar cf - in | gzip > inner.tgz && rm -r in"),
            &outer,
        )
        .unwrap();
        app.refresh_all();
        enter_archive(&mut s, &mut app, &outer.join("inner.tgz")).unwrap();
        let inner = app.tab().vfs[1].tmp.clone();
        std::fs::write(inner.join("deep-new"), b"n").unwrap();
        change_dir(&mut s, &mut app, &t.path().join("sub")).unwrap();
        assert!(!outer.exists() && !inner.exists() && !app.tab().in_vfs());
        let check = t.path().join("check");
        std::fs::create_dir(&check).unwrap();
        crate::jobs::run_in_cli(
            std::ffi::OsStr::new("gzip -dc ../a.tar.gz | tar xf -"),
            &check,
        )
        .unwrap();
        let (list, err) = crate::jobs::run_in_cli(
            std::ffi::OsStr::new("gzip -dc inner.tgz | tar tf -"),
            &check,
        )
        .unwrap();
        assert!(list.contains("deep-new"), "{list} {err}");
    }

    pub(crate) fn tests_support_make(d: &Path) -> PathBuf {
        std::fs::create_dir_all(d.join("src/in")).unwrap();
        std::fs::write(d.join("src/in/f"), b"F").unwrap();
        std::fs::write(d.join("src/top"), b"T").unwrap();
        crate::jobs::run_in_cli(
            std::ffi::OsStr::new("tar cf - * | gzip > ../a.tar.gz"),
            &d.join("src"),
        )
        .unwrap();
        std::fs::remove_dir_all(d.join("src")).unwrap();
        d.join("a.tar.gz")
    }

    fn vfs_app(d: &Path, ask: bool) -> crate::ui::state::App {
        let mut app = crate::ui::testutil::test_app(d, 100, 30);
        app.settings.confirmations.ask_rebuild_vfs = ask;
        app.refresh_all();
        app
    }

    #[test]
    fn enter_and_leave_archive_by_parent() {
        let t = tempfile::tempdir().unwrap();
        let a = make_tgz(t.path());
        let mut app = vfs_app(t.path(), true);
        let mut s = TestScreen::new(100, 30, "n");
        enter_archive(&mut s, &mut app, &a).unwrap();
        assert!(app.tab().in_vfs());
        assert!(
            app.tab().display_dir().ends_with("a.tar.gz#vfs://"),
            "{}",
            app.tab().display_dir()
        );
        assert_eq!(app.tab().tab_name(), "a.tar.gz");
        assert!(app.tab().fs.lookup(std::ffi::OsStr::new("top")).is_some());
        let tmp = app.tab().vfs[0].tmp.clone();
        leave_vfs_root(&mut s, &mut app).unwrap();
        assert!(!tmp.exists() && !app.tab().in_vfs());
        assert_eq!(app.tab().current().display_name(), "a.tar.gz");
        assert!(!app.tab().history.iter().any(|p| p.starts_with(&tmp)));
    }

    #[test]
    fn goto_elsewhere_closes_layers_and_rebuild_yes_packs() {
        let t = tempfile::tempdir().unwrap();
        let a = make_tgz(t.path());
        let mut app = vfs_app(t.path(), true);
        let mut s = TestScreen::new(100, 30, "y");
        enter_archive(&mut s, &mut app, &a).unwrap();
        let tmp = app.tab().vfs[0].tmp.clone();
        std::fs::write(tmp.join("added"), b"x").unwrap();
        change_dir(&mut s, &mut app, &t.path().join("sub")).unwrap();
        assert!(!tmp.exists());
        assert_eq!(app.tab().dir(), t.path().join("sub"));
        let (out, _) =
            crate::jobs::run_in_cli(std::ffi::OsStr::new("tar tzf a.tar.gz"), t.path()).unwrap();
        assert!(out.contains("added"), "{out}");
    }

    #[test]
    fn change_dir_inside_vfs_keeps_layer() {
        let t = tempfile::tempdir().unwrap();
        let a = make_tgz(t.path());
        let mut app = vfs_app(t.path(), false);
        let mut s = TestScreen::new(100, 30, "");
        enter_archive(&mut s, &mut app, &a).unwrap();
        let tmp = app.tab().vfs[0].tmp.clone();
        change_dir(&mut s, &mut app, &tmp.join("in")).unwrap();
        assert!(app.tab().in_vfs() && tmp.exists());
        assert!(app.tab().display_dir().ends_with("a.tar.gz#vfs://in"));
        let pos = (app.active, app.pane().active);
        close_all_vfs(&mut s, &mut app, pos).unwrap();
        assert!(!tmp.exists() && !app.tab().in_vfs());
    }

    #[test]
    fn bad_archive_shows_error_and_stays() {
        let t = tempfile::tempdir().unwrap();
        std::fs::write(t.path().join("bad.tgz"), b"x").unwrap();
        std::fs::write(t.path().join("plain.gz"), b"x").unwrap();
        let mut app = vfs_app(t.path(), false);
        let mut s = TestScreen::new(100, 30, "x x"); // one key closes each error dialog
        enter_archive(&mut s, &mut app, &t.path().join("bad.tgz")).unwrap();
        assert!(!app.tab().in_vfs() && s.screen_text().contains("Cannot chdir"));
        enter_archive(&mut s, &mut app, &t.path().join("plain.gz")).unwrap();
        assert!(!app.tab().in_vfs() && s.screen_text().contains("Invalid directory?"));
    }

    #[test]
    fn header_shows_vfs_path() {
        let t = tempfile::tempdir().unwrap();
        let a = make_tgz(t.path());
        let mut app = vfs_app(t.path(), false);
        let mut s = TestScreen::new(100, 30, "");
        enter_archive(&mut s, &mut app, &a).unwrap();
        let r = Rect::new(0, 0, 100, 30);
        let mut b = Buffer::empty(r);
        crate::ui::draw::draw_app(&app, r, &mut b);
        assert!(
            buffer_line(&b, 1).contains("#vfs://"),
            "{}",
            buffer_line(&b, 1)
        );
        assert!(buffer_line(&b, 0).contains("[a.tar.gz"));
        let pos = (app.active, app.pane().active);
        close_all_vfs(&mut s, &mut app, pos).unwrap();
    }
}

#[cfg(test)]
pub(crate) mod tests_support {
    pub(crate) fn make_tgz(d: &std::path::Path) -> std::path::PathBuf {
        super::tests::tests_support_make(d)
    }
}
