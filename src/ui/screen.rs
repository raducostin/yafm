//! The terminal: a `Screen` trait with a crossterm implementation and a test
//! implementation (ratatui `TestBackend` + scripted keys), the modal dialog
//! runner, the `Progress` adapter for bulk operations and the main event loop.

use std::collections::VecDeque;
use std::io::Stdout;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crossterm::event::{Event, KeyEventKind};
use ratatui::backend::{CrosstermBackend, TestBackend};
use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use ratatui::Terminal;

use crate::actions::Action;
use crate::keys::{from_crossterm, parse_key, Key, KeyCode};
use crate::ops::bulk::{Answer, Progress};
use crate::ui::dialogs::{
    ConfirmAllDialog, ConfirmDialog, Dialog, ErrorDialog, ProgressView, SelectDialog, Step,
};
use crate::ui::draw::{buffer_line, draw_app};
use crate::ui::entry::EntryCtx;
use crate::ui::state::App;

/// Paints into the frame buffer; returns where the text cursor goes, if anywhere.
pub type Painter<'p> = dyn FnMut(Rect, &mut Buffer) -> Option<(u16, u16)> + 'p;
/// Like `Painter`, for overlays that only read state.
pub type Overlay<'p> = dyn Fn(Rect, &mut Buffer) -> Option<(u16, u16)> + 'p;

pub enum Input {
    Key(Key),
    Resize(u16, u16),
}

pub trait Screen {
    fn size(&self) -> (u16, u16);
    fn draw(&mut self, f: &mut Painter) -> std::io::Result<()>;
    fn next_input(&mut self) -> std::io::Result<Input>;
    fn poll_input(&mut self, timeout: Duration) -> std::io::Result<Option<Input>>;
    fn suspend(&mut self, run: &mut dyn FnMut()) -> std::io::Result<()>;
    fn clear(&mut self) -> std::io::Result<()>;
}

/// Leaves raw mode and the alternate screen; harmless when they were never entered.
pub fn restore_terminal() {
    let _ = crossterm::terminal::disable_raw_mode();
    let _ = crossterm::execute!(
        std::io::stdout(),
        crossterm::terminal::LeaveAlternateScreen,
        crossterm::cursor::Show
    );
}

/// Restores the terminal before the default panic message is printed.
pub fn install_panic_hook() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if crossterm::terminal::is_raw_mode_enabled().unwrap_or(false) {
            restore_terminal();
        }
        default(info);
    }));
}

/// The real terminal. Creating it enters raw mode and the alternate screen;
/// dropping it restores the terminal on every exit path.
pub struct CrosstermScreen {
    terminal: Terminal<CrosstermBackend<Stdout>>,
}

impl CrosstermScreen {
    pub fn new() -> std::io::Result<CrosstermScreen> {
        Self::enter()?;
        let terminal = match Terminal::new(CrosstermBackend::new(std::io::stdout())) {
            Ok(t) => t,
            Err(e) => {
                restore_terminal();
                return Err(e);
            }
        };
        Ok(CrosstermScreen { terminal })
    }

    fn enter() -> std::io::Result<()> {
        crossterm::terminal::enable_raw_mode()?;
        crossterm::execute!(
            std::io::stdout(),
            crossterm::terminal::EnterAlternateScreen,
            crossterm::cursor::Hide
        )
    }

    fn translate(ev: Event) -> Option<Input> {
        match ev {
            Event::Key(k) if k.kind != KeyEventKind::Release => {
                Some(Input::Key(from_crossterm(&k)))
            }
            Event::Resize(w, h) => Some(Input::Resize(w, h)),
            _ => None,
        }
    }
}

impl Drop for CrosstermScreen {
    fn drop(&mut self) {
        restore_terminal();
    }
}

impl Screen for CrosstermScreen {
    fn size(&self) -> (u16, u16) {
        crossterm::terminal::size().unwrap_or((80, 24))
    }
    fn draw(&mut self, f: &mut Painter) -> std::io::Result<()> {
        self.terminal.draw(|frame| {
            let area = frame.area();
            if let Some((x, y)) = f(area, frame.buffer_mut()) {
                frame.set_cursor_position(Position::new(x, y));
            }
        })?;
        Ok(())
    }
    fn next_input(&mut self) -> std::io::Result<Input> {
        loop {
            if let Some(i) = Self::translate(crossterm::event::read()?) {
                return Ok(i);
            }
        }
    }
    fn poll_input(&mut self, timeout: Duration) -> std::io::Result<Option<Input>> {
        if crossterm::event::poll(timeout)? {
            return Ok(Self::translate(crossterm::event::read()?));
        }
        Ok(None)
    }
    fn suspend(&mut self, run: &mut dyn FnMut()) -> std::io::Result<()> {
        restore_terminal();
        run();
        Self::enter()?;
        self.terminal.clear()
    }
    fn clear(&mut self) -> std::io::Result<()> {
        self.terminal.clear()
    }
}

pub struct TestScreen {
    pub terminal: Terminal<TestBackend>,
    pub keys: VecDeque<Key>,
    pub suspended: usize,
}

impl TestScreen {
    /// `keys`: space-separated key strings, e.g. "F7 n e w enter" ("spc" for space).
    pub fn new(w: u16, h: u16, keys: &str) -> TestScreen {
        let keys = keys
            .split_whitespace()
            .map(|k| parse_key(k).unwrap_or_else(|_| panic!("bad key {k:?}")))
            .collect();
        TestScreen {
            terminal: Terminal::new(TestBackend::new(w, h)).expect("test terminal"),
            keys,
            suspended: 0,
        }
    }

    pub fn line(&self, y: u16) -> String {
        buffer_line(self.terminal.backend().buffer(), y)
    }

    pub fn screen_text(&self) -> String {
        let h = self.terminal.backend().buffer().area.height;
        (0..h).map(|y| self.line(y)).collect::<Vec<_>>().join("\n")
    }
}

impl Screen for TestScreen {
    fn size(&self) -> (u16, u16) {
        let a = self.terminal.backend().buffer().area;
        (a.width, a.height)
    }
    fn draw(&mut self, f: &mut Painter) -> std::io::Result<()> {
        self.terminal.draw(|frame| {
            let area = frame.area();
            if let Some((x, y)) = f(area, frame.buffer_mut()) {
                frame.set_cursor_position(Position::new(x, y));
            }
        })?;
        Ok(())
    }
    /// Scripted keys; when they run out, an UnexpectedEof error ends any loop.
    fn next_input(&mut self) -> std::io::Result<Input> {
        self.keys
            .pop_front()
            .map(Input::Key)
            .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::UnexpectedEof))
    }
    /// Only a queued Ctrl-C counts as "pressed during an operation"; other
    /// scripted keys stay queued for the dialog that will ask for them.
    fn poll_input(&mut self, _timeout: Duration) -> std::io::Result<Option<Input>> {
        let ctrl_c = self
            .keys
            .front()
            .is_some_and(|k| k.ctrl && k.code == KeyCode::Char('c'));
        Ok(if ctrl_c {
            self.keys.pop_front().map(Input::Key)
        } else {
            None
        })
    }
    fn suspend(&mut self, run: &mut dyn FnMut()) -> std::io::Result<()> {
        self.suspended += 1;
        run();
        Ok(())
    }
    fn clear(&mut self) -> std::io::Result<()> {
        self.terminal.clear()
    }
}

pub enum Flow {
    Continue,
    Quit(Option<PathBuf>),
}

/// What the entry line needs from the app (C-v, C-s, C-o, C-d, C-y).
pub fn entry_ctx(app: &App) -> EntryCtx {
    let mut prev_paths: Vec<String> = app
        .tab()
        .history
        .iter()
        .rev()
        .map(|p| p.display().to_string())
        .collect();
    for p in app.other_tab().history.iter().rev() {
        let p = p.display().to_string();
        if !prev_paths.contains(&p) {
            prev_paths.push(p);
        }
    }
    EntryCtx {
        current_name: app.tab().current().display_name(),
        cur_dir: app.tab().dir().to_path_buf(),
        other_dir: app.other_tab().dir().to_path_buf(),
        bookmarks: app.settings.bookmarks.values().cloned().collect(),
        prev_paths,
    }
}

/// Draws the main screen, then `overlay` on top.
pub fn redraw(
    screen: &mut dyn Screen,
    app: &App,
    overlay: Option<&Overlay>,
) -> std::io::Result<()> {
    screen.draw(&mut |area, buf| {
        draw_app(app, area, buf);
        overlay.and_then(|o| o(area, buf))
    })
}

/// Runs `dialog` over the main screen until it returns a value. Entry-line
/// selection requests open a nested list whose choice is fed back.
pub fn run_modal<D: Dialog>(
    screen: &mut dyn Screen,
    app: &mut App,
    mut dialog: D,
) -> std::io::Result<D::Output> {
    loop {
        {
            let (d, a) = (&dialog, &*app);
            redraw(screen, a, Some(&|area, buf| d.render(area, buf, &a.theme)))?;
        }
        match screen.next_input()? {
            Input::Resize(w, h) => app.resize(w, h),
            Input::Key(k) => {
                let ctx = entry_ctx(app);
                match dialog.handle(k, &ctx) {
                    Step::Continue => {}
                    Step::Done(v) => return Ok(v),
                    Step::Select {
                        title,
                        entries,
                        kind,
                    } => {
                        let picked = run_modal(
                            screen,
                            app,
                            SelectDialog::new(title, entries.clone(), false, false),
                        )?;
                        if let Some(i) = picked {
                            dialog.apply_selection(kind, &entries[i], &ctx);
                        }
                    }
                }
            }
        }
    }
}

/// Shows bulk-operation progress and asks lfm's questions through dialogs.
pub struct UiProgress<'a> {
    screen: &'a mut dyn Screen,
    app: &'a mut App,
    view: ProgressView,
    last_draw: Option<Instant>,
}

impl<'a> UiProgress<'a> {
    pub fn new(screen: &'a mut dyn Screen, app: &'a mut App, title: &str) -> Self {
        let view = ProgressView {
            title: title.into(),
            item: String::new(),
            index: 0,
            count: 0,
            acc: 0,
            total: 1,
        };
        UiProgress {
            screen,
            app,
            view,
            last_draw: None,
        }
    }

    fn ask<D: Dialog>(&mut self, dialog: D, on_error: D::Output) -> D::Output {
        run_modal(self.screen, self.app, dialog).unwrap_or(on_error)
    }
}

impl Progress for UiProgress<'_> {
    fn next(&mut self, item: &str, _size: u64, index: usize, count: usize, acc: u64, total: u64) {
        self.view.item = item.to_string();
        (
            self.view.index,
            self.view.count,
            self.view.acc,
            self.view.total,
        ) = (index, count, acc, total);
        let due = self
            .last_draw
            .is_none_or(|t| t.elapsed() >= Duration::from_millis(50));
        if due || index == count {
            let (v, a) = (&self.view, &*self.app);
            let _ = redraw(
                self.screen,
                a,
                Some(&|area, buf| {
                    v.render(area, buf, &a.theme);
                    None
                }),
            );
            self.last_draw = Some(Instant::now());
        }
    }
    fn confirm_delete(&mut self, item: &str) -> Answer {
        let d = ConfirmAllDialog::new(&self.view.title, &format!("Delete '{item}'?"), false);
        self.ask(d, Answer::Stop)
    }
    fn confirm_overwrite(&mut self, dest: &Path) -> Answer {
        let d = ConfirmAllDialog::new(
            &self.view.title,
            &format!("Overwrite '{}'?", dest.display()),
            true,
        );
        self.ask(d, Answer::Stop)
    }
    fn error(&mut self, msg: &str) {
        self.ask(ErrorDialog::new(msg), ());
    }
    fn should_stop(&mut self) -> bool {
        match self.screen.poll_input(Duration::ZERO) {
            Ok(Some(Input::Key(k))) if k.ctrl && k.code == KeyCode::Char('c') => {
                let question = format!("Stop \"{}\"?", self.view.title);
                self.ask(ConfirmDialog::new("Stop process", &question, false), true)
            }
            Ok(Some(Input::Resize(w, h))) => {
                self.app.resize(w, h);
                false
            }
            Ok(_) => false,
            Err(_) => true,
        }
    }
}

pub type Dispatch<'d> = dyn FnMut(&mut dyn Screen, &mut App, Action) -> std::io::Result<Flow> + 'd;

/// Draw, read a key, run its action; until an action quits.
pub fn event_loop_with(
    screen: &mut dyn Screen,
    app: &mut App,
    dispatch: &mut Dispatch,
) -> std::io::Result<Option<PathBuf>> {
    loop {
        redraw(screen, app, None)?;
        match screen.next_input()? {
            Input::Resize(w, h) => app.resize(w, h),
            Input::Key(k) => {
                if let Some(action) = app.keymap.get(&k) {
                    if let Flow::Quit(path) = dispatch(screen, app, action)? {
                        return Ok(path);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::dialogs::EntryDialog;
    use crate::ui::testutil::test_app;

    #[test]
    fn test_screen_scripts_keys_and_draws() {
        let mut s = TestScreen::new(40, 5, "a C-x spc F5");
        assert_eq!(s.keys.len(), 4);
        s.draw(&mut |r, b| {
            b.set_string(r.x, r.y, "hello", ratatui::style::Style::default());
            None
        })
        .unwrap();
        assert!(s.line(0).starts_with("hello"));
        assert!(matches!(s.next_input().unwrap(), Input::Key(_)));
        s.keys.clear();
        assert!(s.next_input().is_err());
        assert!(s.poll_input(Duration::ZERO).unwrap().is_none());
    }

    #[test]
    fn modal_runner_feeds_keys_and_nested_select() {
        let t = tempfile::tempdir().unwrap();
        let mut app = test_app(t.path(), 100, 30);
        let mut s = TestScreen::new(100, 30, "x enter");
        let out = run_modal(
            &mut s,
            &mut app,
            EntryDialog::new("T", "H", "", vec![], true),
        )
        .unwrap();
        assert_eq!(out, Some("x".into()));
        app.settings.bookmarks.insert('1', "/bm".into());
        let mut s = TestScreen::new(100, 30, "C-d down enter enter");
        let out = run_modal(
            &mut s,
            &mut app,
            EntryDialog::new("T", "H", "", vec![], true),
        )
        .unwrap();
        assert_eq!(out, Some("/bm".into()));
    }

    #[test]
    fn entry_context_from_app() {
        let t = tempfile::tempdir().unwrap();
        let mut app = test_app(t.path(), 100, 30);
        app.goto(&t.path().join("sub")).unwrap();
        let c = entry_ctx(&app);
        assert_eq!(c.cur_dir, t.path().join("sub"));
        assert_eq!(c.prev_paths, vec![t.path().display().to_string()]);
        assert_eq!(c.bookmarks.len(), 36);
    }

    #[test]
    fn progress_asks_and_stops() {
        let t = tempfile::tempdir().unwrap();
        let mut app = test_app(t.path(), 100, 30);
        let mut s = TestScreen::new(100, 30, "a");
        let mut p = UiProgress::new(&mut s, &mut app, "Copy file(s)");
        assert_eq!(p.confirm_overwrite(Path::new("/x")), Answer::All);
        p.next("x", 1, 1, 2, 1, 2);
        drop(p);
        assert!(s.screen_text().contains("Copy file(s)"));
        let mut s = TestScreen::new(100, 30, "C-c y");
        let mut p = UiProgress::new(&mut s, &mut app, "Copy file(s)");
        assert!(p.should_stop());
        let mut s = TestScreen::new(100, 30, "");
        let mut p = UiProgress::new(&mut s, &mut app, "Copy file(s)");
        assert!(!p.should_stop());
    }

    #[test]
    fn event_loop_quits_with_path() {
        let t = tempfile::tempdir().unwrap();
        let mut app = test_app(t.path(), 100, 30);
        let mut s = TestScreen::new(100, 30, "down q y");
        let mut dispatch =
            |scr: &mut dyn Screen, app: &mut App, a: Action| -> std::io::Result<Flow> {
                Ok(match a {
                    Action::CursorDown => {
                        app.cursor_by(1);
                        Flow::Continue
                    }
                    Action::QuitChdir
                        if run_modal(scr, app, ConfirmDialog::new("yafm", "Quit yafm?", true))? =>
                    {
                        Flow::Quit(Some(app.tab().dir().to_path_buf()))
                    }
                    _ => Flow::Continue,
                })
            };
        assert_eq!(
            event_loop_with(&mut s, &mut app, &mut dispatch).unwrap(),
            Some(t.path().to_path_buf())
        );
        assert_eq!(app.tab().i, 1);
    }

    #[test]
    fn panic_hook_is_safe_without_a_terminal() {
        install_panic_hook();
        let r = std::panic::catch_unwind(|| panic!("boom"));
        assert!(r.is_err());
    }
}
