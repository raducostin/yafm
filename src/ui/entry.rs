//! The single-line text entry used by dialogs, with lfm's editing keys,
//! history and path completion. Port of `EntryLine` in lfm's ui_widgets.py
//! and its PowerCLI mode.

use std::path::{Path, PathBuf};

use crate::fs::path::expand_user;
use crate::keys::{Key, KeyCode};
use crate::ui::text::{next_step, prev_step};

const CLI_STORED: &str = "----- Stored:  -----";
const CLI_HISTORY: &str = "----- History: -----";

pub struct EntryCtx {
    pub current_name: String,
    pub cur_dir: PathBuf,
    pub other_dir: PathBuf,
    pub bookmarks: Vec<String>,
    pub prev_paths: Vec<String>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SelectKind {
    Bookmark,
    PrevPath,
    Complete,
    History,
    Owner,
    Group,
    /// PowerCLI: replace the word under the cursor.
    CliComplete,
    /// PowerCLI: C-g list of stored commands and history (headers ignored).
    CliHistory,
}

#[derive(Debug, PartialEq)]
pub enum EntryEvent {
    None,
    Cancel,
    Accept,
    /// PowerCLI: C-x hides the line, keeping its text.
    Hide,
    Tab,
    BackTab,
    Beep,
    Select {
        title: &'static str,
        entries: Vec<String>,
        kind: SelectKind,
    },
}

pub struct EntryLine {
    pub text: String,
    /// Cursor, in chars.
    pub pos: usize,
    orig: String,
    insert: bool,
    history: Vec<String>,
    history_i: usize,
    is_files: bool,
    complete_base: PathBuf,
    /// PowerCLI mode (lfm's `cli=True`).
    cli: bool,
    favs: Vec<String>,
    /// PowerCLI completion: chars start..end are replaced; `None` base = program names.
    cli_word: (usize, usize, Option<PathBuf>),
}

fn chars(s: &str) -> Vec<char> {
    s.chars().collect()
}

impl EntryLine {
    /// The PowerCLI line (lfm's EntryLine with cli=True).
    pub fn new_cli(text: &str, pos: usize, history: Vec<String>, favs: Vec<String>) -> EntryLine {
        let mut e = EntryLine::new(text, history, true);
        e.pos = pos.min(text.chars().count());
        e.cli = true;
        e.favs = favs;
        e
    }

    /// In PowerCLI inserted names are wrapped in double quotes.
    fn quoted(&self, s: &str) -> String {
        if self.cli {
            format!("\"{s}\"")
        } else {
            s.to_string()
        }
    }

    /// lfm's PowerCLI Tab: complete the word at the cursor — a program name for
    /// the first word, else a path (inside `"..."` the word starts after ` "`).
    fn cli_complete(&mut self, ctx: &EntryCtx) -> EntryEvent {
        let cs = chars(&self.text);
        let pos = self.pos.min(cs.len());
        let before: String = cs[..pos].iter().collect();
        let (start, end) = match before.rfind(" \"") {
            Some(_) => {
                let end = if pos > 0 && cs[pos - 1] == '"' {
                    pos - 1
                } else {
                    pos
                };
                let upto: String = cs[..end].iter().collect();
                (
                    upto.rfind(" \"")
                        .map_or(0, |b| upto[..b].chars().count() + 2),
                    end,
                )
            }
            None => (
                before
                    .rfind(' ')
                    .map_or(0, |b| before[..b].chars().count() + 1),
                pos,
            ),
        };
        let word: String = cs[start..end].iter().collect();
        let (base, entries) = if start == 0 {
            let programs = crate::fs::programs::binary_programs();
            (
                None,
                programs
                    .into_iter()
                    .filter(|p| p.starts_with(&word))
                    .collect(),
            )
        } else {
            let (b, e) = complete(&word, &ctx.cur_dir);
            (Some(b), e)
        };
        self.cli_word = (start, end, base);
        if entries.len() == 1 {
            self.apply_selection(SelectKind::CliComplete, &entries[0], ctx);
            return EntryEvent::None;
        }
        self.select("Complete", entries, SelectKind::CliComplete)
    }

    pub fn new(text: &str, history: Vec<String>, is_files: bool) -> EntryLine {
        let history_i = history.len();
        EntryLine {
            text: text.to_string(),
            pos: text.chars().count(),
            orig: text.to_string(),
            insert: true,
            history,
            history_i,
            is_files,
            complete_base: PathBuf::new(),
            cli: false,
            favs: Vec::new(),
            cli_word: (0, 0, None),
        }
    }

    fn set(&mut self, cs: Vec<char>, pos: usize) {
        self.pos = pos.min(cs.len());
        self.text = cs.into_iter().collect();
    }

    fn replace_all(&mut self, text: &str) {
        self.set(chars(text), text.chars().count());
    }

    fn insert_str(&mut self, s: &str) {
        let mut cs = chars(&self.text);
        let pos = self.pos.min(cs.len());
        let add = chars(s);
        let n = add.len();
        cs.splice(pos..pos, add);
        self.set(cs, pos + n);
    }

    fn select(&self, title: &'static str, entries: Vec<String>, kind: SelectKind) -> EntryEvent {
        if entries.is_empty() {
            EntryEvent::Beep
        } else {
            EntryEvent::Select {
                title,
                entries,
                kind,
            }
        }
    }

    pub fn handle(&mut self, key: Key, ctx: &EntryCtx) -> EntryEvent {
        let mut cs = chars(&self.text);
        let pos = self.pos.min(cs.len());
        let ctrl = |c: char| key.ctrl && !key.alt && key.code == KeyCode::Char(c);
        if let KeyCode::Char(c) = key.code {
            if !key.ctrl && !key.alt && !c.is_control() {
                if self.insert || pos == cs.len() {
                    cs.insert(pos, c);
                } else {
                    cs[pos] = c;
                }
                self.set(cs, pos + 1);
                return EntryEvent::None;
            }
        }
        match key.code {
            _ if ctrl('c') || ctrl('u') || key.code == KeyCode::Esc => return EntryEvent::Cancel,
            _ if self.cli && ctrl('x') => return EntryEvent::Hide,
            KeyCode::Tab if self.cli => return self.cli_complete(ctx),
            _ if self.cli && ctrl('t') => return self.cli_complete(ctx),
            _ if self.cli && ctrl('g') => {
                let mut entries = vec![CLI_STORED.to_string()];
                entries.extend(self.favs.iter().filter(|c| !c.trim().is_empty()).cloned());
                entries.push(CLI_HISTORY.to_string());
                entries.extend(
                    self.history
                        .iter()
                        .filter(|c| !c.trim().is_empty())
                        .cloned(),
                );
                return EntryEvent::Select {
                    title: "History",
                    entries,
                    kind: SelectKind::CliHistory,
                };
            }
            KeyCode::Tab => return EntryEvent::Tab,
            KeyCode::BackTab => return EntryEvent::BackTab,
            KeyCode::Enter => return EntryEvent::Accept,
            KeyCode::Home => self.pos = 0,
            KeyCode::End => self.pos = cs.len(),
            KeyCode::Left if key.ctrl => self.pos = prev_step(&self.text, pos),
            KeyCode::Right if key.ctrl => self.pos = next_step(&self.text, pos),
            KeyCode::Left => self.pos = pos.saturating_sub(1),
            KeyCode::Right => self.pos = (pos + 1).min(cs.len()),
            KeyCode::Backspace => {
                if pos > 0 {
                    cs.remove(pos - 1);
                    self.set(cs, pos - 1);
                }
            }
            KeyCode::Del if key.ctrl => self.delete_word_right(cs, pos),
            KeyCode::Del => {
                if pos < cs.len() {
                    cs.remove(pos);
                    self.set(cs, pos);
                }
            }
            KeyCode::Ins => self.insert = !self.insert,
            KeyCode::Up => {
                if self.history_i > 0 {
                    if self.history_i == self.history.len() {
                        self.history.push(self.text.clone());
                    }
                    self.history_i -= 1;
                    let h = self.history[self.history_i].clone();
                    self.replace_all(&h);
                }
            }
            KeyCode::Down => {
                if self.history_i + 1 < self.history.len() {
                    self.history_i += 1;
                    let h = self.history[self.history_i].clone();
                    self.replace_all(&h);
                }
            }
            _ if ctrl('a') => self.pos = 0,
            _ if ctrl('e') => self.pos = cs.len(),
            _ if ctrl('b') => self.pos = pos.saturating_sub(1),
            _ if ctrl('f') => self.pos = (pos + 1).min(cs.len()),
            _ if ctrl('p') => self.pos = prev_step(&self.text, pos),
            _ if ctrl('n') => self.pos = next_step(&self.text, pos),
            _ if ctrl('q') => {
                let p = prev_step(&self.text, pos);
                cs.drain(p..pos);
                self.set(cs, p);
            }
            _ if ctrl('r') => self.delete_word_right(cs, pos),
            _ if ctrl('w') => self.set(Vec::new(), 0),
            _ if ctrl('h') => self.set(cs[pos..].to_vec(), 0),
            _ if ctrl('k') => {
                cs.truncate(pos);
                self.set(cs, pos);
            }
            _ if ctrl('z') => {
                let o = self.orig.clone();
                self.replace_all(&o);
            }
            _ if ctrl('g') => {
                let entries: Vec<String> = self
                    .history
                    .iter()
                    .filter(|h| !h.trim().is_empty())
                    .cloned()
                    .collect();
                return self.select("History", entries, SelectKind::History);
            }
            _ if !self.is_files => return EntryEvent::Beep,
            _ if ctrl('v') => self.insert_str(&self.quoted(&ctx.current_name)),
            _ if ctrl('s') => {
                let dir = format!("{}/", ctx.cur_dir.display()).replace("//", "/");
                self.insert_str(&self.quoted(&dir))
            }
            _ if ctrl('o') => {
                let dir = format!("{}/", ctx.other_dir.display()).replace("//", "/");
                self.insert_str(&self.quoted(&dir))
            }
            _ if ctrl('d') || ctrl('\\') => {
                return self.select("Bookmarks", ctx.bookmarks.clone(), SelectKind::Bookmark)
            }
            _ if ctrl('y') => {
                return self.select(
                    "Previous paths",
                    ctx.prev_paths.clone(),
                    SelectKind::PrevPath,
                )
            }
            _ if ctrl('t') => {
                let (base, entries) = complete(&self.text, &ctx.cur_dir);
                self.complete_base = base;
                if entries.len() == 1 {
                    self.apply_selection(SelectKind::Complete, &entries[0], ctx);
                    return EntryEvent::None;
                }
                return self.select("Complete", entries, SelectKind::Complete);
            }
            _ => return EntryEvent::Beep,
        }
        EntryEvent::None
    }

    /// lfm's C-r: delete to the next word, with its bracket/space adjustments.
    fn delete_word_right(&mut self, mut cs: Vec<char>, pos: usize) {
        let mut end = next_step(&self.text, pos);
        if end > 0 && ".([{<\"'".contains(cs[end - 1]) {
            end -= 1;
        }
        if end + 1 < cs.len() && cs[end] == ' ' {
            end += 1;
        }
        if end > pos {
            cs.drain(pos..end);
        }
        self.set(cs, pos);
    }

    pub fn apply_selection(&mut self, kind: SelectKind, chosen: &str, ctx: &EntryCtx) {
        match kind {
            SelectKind::Bookmark | SelectKind::PrevPath if self.cli => {
                self.insert_str(&self.quoted(chosen))
            }
            SelectKind::Bookmark
            | SelectKind::PrevPath
            | SelectKind::History
            | SelectKind::Owner
            | SelectKind::Group => self.replace_all(chosen),
            SelectKind::CliHistory if chosen == CLI_STORED || chosen == CLI_HISTORY => {}
            SelectKind::CliHistory => self.replace_all(chosen),
            SelectKind::CliComplete => {
                let (start, end, base) = self.cli_word.clone();
                let piece = match base {
                    None => format!("{chosen} "),
                    Some(b) if b == ctx.cur_dir => chosen.to_string(),
                    Some(b) => {
                        let b = b.to_string_lossy();
                        if b.ends_with('/') {
                            format!("{b}{chosen}")
                        } else {
                            format!("{b}/{chosen}")
                        }
                    }
                };
                let mut cs = chars(&self.text);
                let end = end.min(cs.len());
                let n = piece.chars().count();
                cs.splice(start.min(end)..end, piece.chars());
                self.set(cs, start + n);
            }
            SelectKind::Complete => {
                let base = self.complete_base.to_string_lossy();
                let full = if base.ends_with('/') {
                    format!("{base}{chosen}")
                } else {
                    format!("{base}/{chosen}")
                };
                let prefix = format!("{}/", ctx.cur_dir.display());
                let text = full
                    .strip_prefix(&prefix)
                    .map(str::to_string)
                    .unwrap_or(full);
                self.replace_all(&text);
            }
        }
    }

    /// The part of the text shown in `w` columns and the cursor column (EntryLine.show).
    pub fn visible(&self, w: usize) -> (String, usize) {
        let cs = chars(&self.text);
        let (n, pos) = (cs.len(), self.pos.min(cs.len()));
        if w == 0 {
            return (String::new(), 0);
        }
        if pos < w {
            let mut s: String = cs.iter().take(w).collect();
            s.push_str(&" ".repeat(w.saturating_sub(n)));
            (s, pos)
        } else if pos + w > n + 1 {
            let mut s: String = cs[n + 1 - w..].iter().collect();
            s.push(' ');
            (s, w - 1 - (n - pos))
        } else {
            let start = pos / w * w;
            (cs[start..(start + w).min(n)].iter().collect(), pos - start)
        }
    }
}

/// Names in the directory `text` points into that start with its last part;
/// directories first (with a trailing "/"), each group sorted.
pub fn complete(text: &str, cur_dir: &Path) -> (PathBuf, Vec<String>) {
    let expanded = PathBuf::from(expand_user(text));
    let path = if expanded.is_absolute() {
        expanded
    } else {
        cur_dir.join(expanded)
    };
    let (base, start) = if text.ends_with('/') && path.is_dir() {
        (path, String::new())
    } else {
        let start = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        (
            path.parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from("/")),
            start,
        )
    };
    let Ok(rd) = std::fs::read_dir(&base) else {
        return (base, Vec::new());
    };
    let (mut dirs, mut files): (Vec<String>, Vec<String>) = (Vec::new(), Vec::new());
    for de in rd.filter_map(Result::ok) {
        let name = de.file_name().to_string_lossy().into_owned();
        if !name.starts_with(&start) {
            continue;
        }
        if de.path().is_dir() {
            dirs.push(format!("{name}/"));
        } else {
            files.push(name);
        }
    }
    dirs.sort();
    files.sort();
    dirs.extend(files);
    (base, dirs)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(s: &str) -> Key {
        crate::keys::parse_key(s).unwrap()
    }

    fn ctx(d: &Path) -> EntryCtx {
        EntryCtx {
            current_name: "file.txt".into(),
            cur_dir: d.into(),
            other_dir: "/o".into(),
            bookmarks: vec!["/b1".into(), "/b2".into()],
            prev_paths: vec![],
        }
    }

    fn typed(e: &mut EntryLine, s: &str, c: &EntryCtx) {
        for ch in s.chars() {
            e.handle(
                Key {
                    code: KeyCode::Char(ch),
                    ctrl: false,
                    alt: false,
                    shift: false,
                },
                c,
            );
        }
    }

    #[test]
    fn editing_keys() {
        let c = ctx(Path::new("/tmp"));
        let mut e = EntryLine::new("hello world", vec![], true);
        e.handle(k("C-a"), &c);
        typed(&mut e, ">", &c);
        assert_eq!(e.text, ">hello world");
        e.handle(k("C-e"), &c);
        e.handle(k("C-q"), &c);
        assert_eq!(e.text, ">hello ");
        e.handle(k("C-z"), &c);
        assert_eq!(e.text, "hello world");
        e.handle(k("home"), &c);
        e.handle(k("C-k"), &c);
        assert_eq!(e.text, "");
        e.handle(k("C-v"), &c);
        assert_eq!(e.text, "file.txt");
        e.handle(k("C-w"), &c);
        e.handle(k("C-o"), &c);
        assert_eq!(e.text, "/o/");
        e.handle(k("backspace"), &c);
        assert_eq!(e.text, "/o");
        e.handle(k("ins"), &c);
        e.handle(k("home"), &c);
        typed(&mut e, "X", &c);
        assert_eq!(e.text, "Xo");
        assert_eq!(e.handle(k("esc"), &c), EntryEvent::Cancel);
        assert_eq!(e.handle(k("enter"), &c), EntryEvent::Accept);
        assert_eq!(e.handle(k("tab"), &c), EntryEvent::Tab);
    }

    #[test]
    fn more_editing_keys() {
        let c = ctx(Path::new("/tmp"));
        let mut e = EntryLine::new("abc def", vec![], true);
        e.handle(k("left"), &c);
        e.handle(k("C-h"), &c);
        assert_eq!((e.text.as_str(), e.pos), ("f", 0));
        let mut e = EntryLine::new("abc", vec![], true);
        e.handle(k("home"), &c);
        e.handle(k("del"), &c);
        assert_eq!(e.text, "bc");
        e.handle(k("C-s"), &c);
        assert_eq!(e.text, "/tmp/bc");
        let mut e = EntryLine::new("ünï", vec![], true);
        e.handle(k("backspace"), &c);
        assert_eq!(e.text, "ün");
        assert_eq!(e.handle(k("F5"), &c), EntryEvent::Beep);
    }

    #[test]
    fn files_only_keys_are_inert_otherwise() {
        let c = ctx(Path::new("/tmp"));
        let mut e = EntryLine::new("", vec![], false);
        e.handle(k("C-v"), &c);
        assert_eq!(e.text, "");
    }

    #[test]
    fn history_navigation() {
        let c = ctx(Path::new("/tmp"));
        let mut e = EntryLine::new("now", vec!["one".into(), "two".into()], true);
        e.handle(k("up"), &c);
        assert_eq!(e.text, "two");
        e.handle(k("up"), &c);
        assert_eq!(e.text, "one");
        e.handle(k("down"), &c);
        e.handle(k("down"), &c);
        assert_eq!(e.text, "now");
    }

    #[test]
    fn bookmark_select_and_completion() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path();
        std::fs::create_dir(d.join("alpha")).unwrap();
        std::fs::write(d.join("alps"), b"").unwrap();
        std::fs::write(d.join("beta"), b"").unwrap();
        let c = ctx(d);
        let mut e = EntryLine::new("", vec![], true);
        match e.handle(k("C-d"), &c) {
            EntryEvent::Select {
                kind: SelectKind::Bookmark,
                entries,
                ..
            } => assert_eq!(entries, ["/b1", "/b2"]),
            x => panic!("{x:?}"),
        }
        e.apply_selection(SelectKind::Bookmark, "/b2", &c);
        assert_eq!(e.text, "/b2");
        let mut e = EntryLine::new("al", vec![], true);
        match e.handle(k("C-t"), &c) {
            EntryEvent::Select {
                entries,
                kind: SelectKind::Complete,
                ..
            } => {
                assert_eq!(entries, ["alpha/", "alps"]);
                e.apply_selection(SelectKind::Complete, "alpha/", &c);
            }
            x => panic!("{x:?}"),
        }
        assert_eq!(e.text, "alpha/");
        let mut e = EntryLine::new("be", vec![], true);
        e.handle(k("C-t"), &c);
        assert_eq!(e.text, "beta");
        let mut e = EntryLine::new("zz", vec![], true);
        assert_eq!(e.handle(k("C-t"), &c), EntryEvent::Beep);
        let mut e = EntryLine::new("", vec![], true);
        assert_eq!(e.handle(k("C-y"), &c), EntryEvent::Beep); // no previous paths
    }

    #[test]
    fn visible_window_scrolls() {
        let e = EntryLine::new("abcdefghijklmnopqrstuvwxyz", vec![], true);
        let (s, col) = e.visible(10);
        assert_eq!((s.as_str(), col), ("rstuvwxyz ", 9));
        let e = EntryLine::new("abc", vec![], true);
        assert_eq!(e.visible(10), ("abc       ".to_string(), 3));
    }

    fn cli_ctx(d: &Path) -> EntryCtx {
        EntryCtx {
            current_name: "my file".into(),
            cur_dir: d.into(),
            other_dir: "/o".into(),
            bookmarks: vec!["/b1".into()],
            prev_paths: vec![],
        }
    }

    #[test]
    fn cli_inserts_quoted_and_hides() {
        let c = cli_ctx(Path::new("/tmp"));
        let mut e = EntryLine::new_cli("cat ", 4, vec![], vec![]);
        e.handle(k("C-v"), &c);
        assert_eq!(e.text, "cat \"my file\"");
        e.handle(k("C-o"), &c);
        assert_eq!(e.text, "cat \"my file\"\"/o/\"");
        match e.handle(k("C-d"), &c) {
            EntryEvent::Select {
                kind: SelectKind::Bookmark,
                ..
            } => e.apply_selection(SelectKind::Bookmark, "/b1", &c),
            x => panic!("{x:?}"),
        }
        assert!(e.text.ends_with("\"/b1\""), "{}", e.text);
        assert_eq!(e.handle(k("C-x"), &c), EntryEvent::Hide);
        let mut plain = EntryLine::new("", vec![], true);
        assert_eq!(plain.handle(k("tab"), &c), EntryEvent::Tab);
    }

    #[test]
    fn cli_completes_programs_and_paths() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path();
        std::fs::create_dir(d.join("alpha")).unwrap();
        std::fs::write(d.join("alps"), b"").unwrap();
        let c = cli_ctx(d);
        let mut e = EntryLine::new_cli("ech", 3, vec![], vec![]);
        match e.handle(k("tab"), &c) {
            EntryEvent::Select {
                kind: SelectKind::CliComplete,
                entries,
                ..
            } => {
                assert!(entries.contains(&"echo".to_string()));
                e.apply_selection(SelectKind::CliComplete, "echo", &c);
            }
            EntryEvent::None => {}
            x => panic!("{x:?}"),
        }
        assert!(e.text.starts_with("echo "), "{}", e.text);
        let mut e = EntryLine::new_cli("ls \"al", 6, vec![], vec![]);
        match e.handle(k("tab"), &c) {
            EntryEvent::Select {
                kind: SelectKind::CliComplete,
                entries,
                ..
            } => {
                assert_eq!(entries, ["alpha/", "alps"]);
                e.apply_selection(SelectKind::CliComplete, "alps", &c);
            }
            x => panic!("{x:?}"),
        }
        assert_eq!((e.text.as_str(), e.pos), ("ls \"alps", 8));
        let mut e = EntryLine::new_cli("cp x /etc/hostn", 15, vec![], vec![]);
        e.handle(k("C-t"), &c);
        assert_eq!(e.text, "cp x /etc/hostname");
    }

    #[test]
    fn cli_history_list_with_favs() {
        let c = cli_ctx(Path::new("/tmp"));
        let mut e = EntryLine::new_cli(
            "",
            0,
            vec!["ls".into()],
            vec!["less \"$f\" $".into(), "".into()],
        );
        match e.handle(k("C-g"), &c) {
            EntryEvent::Select {
                kind: SelectKind::CliHistory,
                entries,
                ..
            } => {
                assert_eq!(
                    entries,
                    [
                        "----- Stored:  -----",
                        "less \"$f\" $",
                        "----- History: -----",
                        "ls"
                    ]
                );
            }
            x => panic!("{x:?}"),
        }
        e.apply_selection(SelectKind::CliHistory, "----- Stored:  -----", &c);
        assert_eq!(e.text, "");
        e.apply_selection(SelectKind::CliHistory, "ls", &c);
        assert_eq!(e.text, "ls");
    }
}
