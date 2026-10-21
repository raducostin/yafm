//! PowerCLI: a command line on the status row (Ctrl-X) that runs a shell
//! command for each selected file, with lfm's `$` variables and `{...}`
//! expressions. Port of `PowerCLI` in lfm's ui.py.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use std::ffi::OsStr;

use crate::cli::script::{expand, Script};
use crate::cli::vars::{CliVars, Stamp};
use crate::config::history::HistorySection;
use crate::config::theme::{ColorItem, Theme};
use crate::fs::entry::FsEntry;
use crate::jobs::run_in_background;
use crate::keys::Key;
use crate::ui::actions::{run_in_terminal, CONTINUE};
use crate::ui::dialogs::{ConfirmDialog, Dialog, ErrorDialog, Step, ViewDialog};
use crate::ui::draw::{fill, put, style};
use crate::ui::entry::{EntryCtx, EntryEvent, EntryLine, SelectKind};
use crate::ui::jobs::run_job;
use crate::ui::screen::{run_modal, Flow, Screen};
use crate::ui::state::App;
use crate::ui::text::{text2wrap, width};

#[derive(Debug, PartialEq)]
pub enum CliOutcome {
    Run(String),
    Cancel,
    Hide { text: String, pos: usize },
}

/// The PowerCLI line, drawn over the status bar: `[path]$ ` prompt and entry.
pub struct PowerCliDialog {
    entry: EntryLine,
    prompt: String,
    /// Rows under the status bar (the function-key bar, when shown).
    below: u16,
}

impl PowerCliDialog {
    pub fn new(app: &App) -> PowerCliDialog {
        let history = app.history.entries(HistorySection::Cli).to_vec();
        let entry = EntryLine::new_cli(
            &app.cli_text,
            app.cli_pos,
            history,
            app.settings.powercli_favs.clone(),
        );
        let path = text2wrap(
            &app.tab().display_dir(),
            app.width as usize / 6,
            true,
            0.0,
            false,
        );
        let root = nix::unistd::geteuid().is_root();
        PowerCliDialog {
            entry,
            prompt: format!("[{path}]{} ", if root { '#' } else { '$' }),
            below: u16::from(app.settings.options.show_function_keys),
        }
    }
}

impl Dialog for PowerCliDialog {
    type Output = CliOutcome;
    fn handle(&mut self, key: Key, ctx: &EntryCtx) -> Step<CliOutcome> {
        match self.entry.handle(key, ctx) {
            EntryEvent::Cancel => Step::Done(CliOutcome::Cancel),
            EntryEvent::Hide => Step::Done(CliOutcome::Hide {
                text: self.entry.text.clone(),
                pos: self.entry.pos,
            }),
            EntryEvent::Accept => Step::Done(CliOutcome::Run(self.entry.text.clone())),
            EntryEvent::Select {
                title,
                entries,
                kind,
            } => Step::Select {
                title,
                entries,
                kind,
            },
            _ => Step::Continue,
        }
    }

    fn apply_selection(&mut self, kind: SelectKind, chosen: &str, ctx: &EntryCtx) {
        self.entry.apply_selection(kind, chosen, ctx);
    }

    fn render(&self, area: Rect, buf: &mut Buffer, theme: &Theme) -> Option<(u16, u16)> {
        if area.height <= self.below {
            return None;
        }
        let y = area.bottom() - 1 - self.below;
        let row = Rect::new(area.x, y, area.width, 1);
        fill(buf, row, style(theme, ColorItem::PowercliText));
        put(
            buf,
            row,
            area.x,
            y,
            &self.prompt,
            style(theme, ColorItem::PowercliPrompt),
        );
        let pw = width(&self.prompt) as u16;
        let ew = area.width.saturating_sub(pw) as usize;
        let (text, col) = self.entry.visible(ew);
        put(
            buf,
            row,
            area.x + pw,
            y,
            &text,
            style(theme, ColorItem::PowercliText),
        );
        Some((
            area.x + pw + (col as u16).min(area.width.saturating_sub(pw + 1)),
            y,
        ))
    }
}

fn stamp(t: std::io::Result<std::time::SystemTime>) -> Stamp {
    let dt = t
        .ok()
        .map(chrono::DateTime::<chrono::Local>::from)
        .unwrap_or_else(chrono::Local::now);
    Stamp {
        dt: dt.naive_local(),
        date_only: false,
    }
}

/// The variables for `entry` (lfm's `__replace_cli` table).
pub fn vars_for(app: &App, entry: &FsEntry) -> CliVars {
    let tab = app.tab();
    let selected: Vec<String> = tab
        .selected_entries()
        .iter()
        .map(|e| e.display_name())
        .collect();
    let name = entry.display_name();
    let meta = std::fs::symlink_metadata(&entry.path).ok();
    let time = |f: fn(&std::fs::Metadata) -> std::io::Result<std::time::SystemTime>| {
        stamp(meta.as_ref().map_or(Err(std::io::Error::other("")), f))
    };
    CliVars {
        f: name.clone(),
        full: entry.path.to_string_lossy().into_owned(),
        name_noext: entry.name_noext.clone(),
        ext: entry.ext.clone(),
        cur_dir: tab.dir().to_string_lossy().into_owned(),
        other_dir: app.other_tab().dir().to_string_lossy().into_owned(),
        index: selected
            .iter()
            .position(|s| *s == name)
            .map_or(0, |i| i as i64 + 1),
        selected,
        all: tab
            .fs
            .entries()
            .iter()
            .filter(|e| e.name != "..")
            .map(|e| e.display_name())
            .collect(),
        tm: time(|m| m.modified()),
        ta: time(|m| m.accessed()),
        // ctime is not exposed portably; Python's st_ctime on Linux is the change time.
        tc: meta.as_ref().map_or_else(
            || stamp(Err(std::io::Error::other(""))),
            |m| {
                let secs = std::os::unix::fs::MetadataExt::ctime(m);
                stamp(Ok(
                    std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs.max(0) as u64)
                ))
            },
        ),
        tn: stamp(Ok(std::time::SystemTime::now())),
        bookmarks: app
            .settings
            .bookmarks
            .iter()
            .map(|(k, p)| (*k, p.clone()))
            .collect(),
    }
}

enum Mode {
    Normal,
    Background,
    Terminal,
}

/// Ctrl-X: edit a command and run it for each selected file (or the one under the cursor).
pub fn run(screen: &mut dyn Screen, app: &mut App) -> std::io::Result<Flow> {
    let outcome = run_modal(screen, app, PowerCliDialog::new(app))?;
    let cmd = match outcome {
        CliOutcome::Cancel => {
            (app.cli_text, app.cli_pos) = (String::new(), 0);
            return CONTINUE;
        }
        CliOutcome::Hide { text, pos } => {
            (app.cli_text, app.cli_pos) = (text, pos);
            return CONTINUE;
        }
        CliOutcome::Run(cmd) => {
            (app.cli_text, app.cli_pos) = (String::new(), 0);
            cmd.trim().to_string()
        }
    };
    if cmd.is_empty() {
        return CONTINUE;
    }
    app.history.append(HistorySection::Cli, &cmd);
    let (mode, body) = match (cmd.strip_suffix('&'), cmd.strip_suffix('$')) {
        (Some(b), _) => (Mode::Background, b.trim().to_string()),
        (_, Some(b)) => (Mode::Terminal, b.trim().to_string()),
        _ => (Mode::Normal, cmd.clone()),
    };
    let targets: Vec<FsEntry> = match app.tab().selected_entries() {
        s if !s.is_empty() => s.into_iter().cloned().collect(),
        _ => vec![app.tab().current().clone()],
    };
    let dir = app.tab().dir().to_path_buf();
    let script = Script::new();
    for (k, f) in targets.iter().enumerate() {
        let line = match expand(&body, &vars_for(app, f), &script) {
            Ok(l) => l,
            Err(e) => {
                run_modal(
                    screen,
                    app,
                    ErrorDialog::new(&format!("Cannot execute PowerCLI command:\n  {cmd}\n\n{e}")),
                )?;
                continue;
            }
        };
        let os = OsStr::new(&line);
        match mode {
            Mode::Background => {
                let _ = run_in_background(os, &dir);
            }
            Mode::Terminal => run_in_terminal(screen, os, &dir)?,
            Mode::Normal => {
                let r = run_job(screen, app, "Executing PowerCLI", &line, os, &dir)?;
                if !r.stderr.trim().is_empty() {
                    let msg = format!(
                        "Error running PowerCLI command:\n  {line}\n\n{}",
                        r.stderr.trim_end()
                    );
                    run_modal(screen, app, ErrorDialog::new(&msg))?;
                }
                if r.stopped {
                    let more = k + 1 < targets.len();
                    if more
                        && run_modal(
                            screen,
                            app,
                            ConfirmDialog::new(
                                "Error running PowerCLI",
                                "Do you want to stop now?",
                                true,
                            ),
                        )?
                    {
                        break;
                    }
                    continue;
                }
                if !r.stdout.is_empty()
                    && app.settings.options.show_output_after_exec
                    && run_modal(
                        screen,
                        app,
                        ConfirmDialog::new("Executing PowerCLI", "Show output?", true),
                    )?
                {
                    let lines = r
                        .stdout
                        .lines()
                        .map(|l| (l.to_string(), ColorItem::ViewWhiteOnBlack))
                        .collect();
                    run_modal(
                        screen,
                        app,
                        ViewDialog::new(&format!("Output of: {line}"), lines),
                    )?;
                }
            }
        }
    }
    app.tab_mut().selected.clear();
    app.refresh_all();
    CONTINUE
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::actions::event_loop;
    use crate::ui::screen::TestScreen;
    use crate::ui::testutil::test_app;

    fn ui(d: &std::path::Path, keys: &str) -> (App, TestScreen) {
        (test_app(d, 100, 30), TestScreen::new(100, 30, keys))
    }

    fn go(app: &mut App, s: &mut TestScreen) {
        let _ = event_loop(s, app);
    }

    #[test]
    fn echo_current_file_shows_output() {
        let t = tempfile::tempdir().unwrap();
        let (mut app, mut s) = ui(t.path(), "C-x e c h o spc $ f enter y");
        app.tab_mut().focus_file(OsStr::new("a.txt"), 24);
        go(&mut app, &mut s);
        let text = s.screen_text();
        assert!(text.contains("Output of: echo a.txt"), "{text}");
        assert_eq!(
            app.history
                .entries(crate::config::history::HistorySection::Cli),
            ["echo $f"]
        );
    }

    #[test]
    fn loops_over_the_selection() {
        let t = tempfile::tempdir().unwrap();
        let (mut app, mut s) = ui(t.path(), "C-x t o u c h spc \" $ f . c o p y \" enter");
        app.select_glob("*.*", true);
        go(&mut app, &mut s);
        assert!(t.path().join("a.txt.copy").is_file() && t.path().join("b.rs.copy").is_file());
        assert!(app.tab().selected.is_empty());
    }

    #[test]
    fn expression_errors_run_nothing() {
        let t = tempfile::tempdir().unwrap();
        let (mut app, mut s) = ui(t.path(), "C-x t o u c h spc { 1 spc + enter");
        app.tab_mut().focus_file(OsStr::new("a.txt"), 24);
        go(&mut app, &mut s);
        assert!(s.screen_text().contains("Cannot execute PowerCLI command"));
        assert_eq!(std::fs::read_dir(t.path()).unwrap().count(), 5); // nothing created
    }

    #[test]
    fn expression_renames_like_lfm() {
        let t = tempfile::tempdir().unwrap();
        std::fs::write(t.path().join("FiLeFOO.bak"), b"").unwrap();
        let (mut app, mut s) = ui(t.path(), "");
        app.refresh_all();
        app.tab_mut().focus_file(OsStr::new("FiLeFOO.bak"), 24);
        s.keys = TestScreen::new(1, 1, "C-x m v spc $ f spc { $ f . l o w e r ( ) . r e p l a c e ( ' . b a k ' , spc ' . o r i g ' ) } enter").keys;
        go(&mut app, &mut s);
        assert!(
            t.path().join("filefoo.orig").is_file(),
            "{}",
            s.screen_text()
        );
    }

    #[test]
    fn hide_keeps_text_and_cancel_clears_it() {
        let t = tempfile::tempdir().unwrap();
        let (mut app, mut s) = ui(t.path(), "C-x a b C-x");
        go(&mut app, &mut s);
        assert_eq!((app.cli_text.as_str(), app.cli_pos), ("ab", 2));
        s.keys = TestScreen::new(1, 1, "C-x c C-c").keys;
        go(&mut app, &mut s);
        assert_eq!(app.cli_text, "");
    }

    #[test]
    fn background_returns_at_once_and_parent_entry_works() {
        let t = tempfile::tempdir().unwrap();
        let (mut app, mut s) = ui(t.path(), "C-x s l e e p spc 5 spc & enter");
        app.tab_mut().focus_file(OsStr::new("a.txt"), 24);
        let start = std::time::Instant::now();
        go(&mut app, &mut s);
        assert!(start.elapsed().as_secs() < 3);
        let t2 = tempfile::tempdir().unwrap();
        let (mut app2, mut s2) = ui(t2.path(), "home C-x e c h o spc [ $ f ] enter y");
        go(&mut app2, &mut s2);
        assert!(s2.screen_text().contains("[..]"), "{}", s2.screen_text());
    }
}
