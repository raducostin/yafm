//! Modal dialogs as key-driven state machines that render into a buffer.
//! Ported from ui_widgets.py: DialogError, DialogConfirm, DialogConfirmAll(None),
//! DialogGetKey, SelectItem, DialogEntry, DialogDoubleEntry, InternalView and
//! DialogProgress2Panel.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use std::cell::Cell;

use crate::config::theme::{ColorItem, Theme};
use crate::keys::{Key, KeyCode};
use crate::ops::bulk::Answer;
use crate::ui::draw::{draw_box, fill, put, style};
use crate::ui::entry::{EntryCtx, EntryEvent, EntryLine, SelectKind};
use crate::ui::text::{scrollbar, text2wrap, width, ScrollCell};

pub enum Step<T> {
    Continue,
    Done(T),
    Select {
        title: &'static str,
        entries: Vec<String>,
        kind: SelectKind,
    },
}

pub trait Dialog {
    type Output;
    fn handle(&mut self, key: Key, ctx: &EntryCtx) -> Step<Self::Output>;
    fn apply_selection(&mut self, _kind: SelectKind, _chosen: &str, _ctx: &EntryCtx) {}
    /// Draws the dialog over `area`; returns the text cursor position, if any.
    fn render(&self, area: Rect, buf: &mut Buffer, theme: &Theme) -> Option<(u16, u16)>;
}

/// A `w`×`h` box centered in `area`, shrunk to fit.
pub(crate) fn centered(area: Rect, w: usize, h: usize) -> Rect {
    let w = (w as u16).min(area.width);
    let h = (h as u16).min(area.height);
    Rect::new(
        area.x + (area.width - w) / 2,
        area.y + (area.height - h) / 2,
        w,
        h,
    )
}

/// Background, border and centered " title " of a dialog box.
pub(crate) fn frame(
    buf: &mut Buffer,
    r: Rect,
    theme: &Theme,
    body: ColorItem,
    title_item: ColorItem,
    title: &str,
) {
    fill(buf, r, style(theme, body));
    draw_box(buf, r, style(theme, body));
    if !title.is_empty() {
        let label = format!(" {title} ");
        let x = r.x + (r.width.saturating_sub(width(&label) as u16)) / 2;
        put(buf, r, x, r.y, &label, style(theme, title_item));
    }
}

pub(crate) fn footer(buf: &mut Buffer, r: Rect, text: &str, st: ratatui::style::Style) {
    if r.height > 1 {
        let x = r.x + (r.width.saturating_sub(width(text) as u16)) / 2;
        put(buf, r, x, r.bottom() - 1, text, st);
    }
}

fn is_cancel(key: &Key) -> bool {
    key.code == KeyCode::Esc || (key.ctrl && key.code == KeyCode::Char('c'))
}

fn plain_char(key: &Key) -> Option<char> {
    match key.code {
        KeyCode::Char(c) if !key.ctrl && !key.alt => Some(c),
        _ => None,
    }
}

fn lines_of(text: &str) -> Vec<String> {
    text.replace('\t', "    ")
        .split('\n')
        .map(str::to_string)
        .collect()
}

fn max_width(lines: &[String]) -> usize {
    lines.iter().map(|l| width(l)).max().unwrap_or(0)
}

/// A row of buttons; `active` (if any) is highlighted.
fn buttons(
    buf: &mut Buffer,
    r: Rect,
    row: u16,
    cols: &[(u16, &str)],
    active: Option<usize>,
    theme: &Theme,
) {
    for (k, (x, label)) in cols.iter().enumerate() {
        let item = if Some(k) == active {
            ColorItem::ButtonActive
        } else {
            ColorItem::ButtonInactive
        };
        put(buf, r, r.x + x, row, label, style(theme, item));
    }
}

pub struct ErrorDialog {
    lines: Vec<String>,
}

impl ErrorDialog {
    pub fn new(text: &str) -> Self {
        ErrorDialog {
            lines: lines_of(text),
        }
    }
}

impl Dialog for ErrorDialog {
    type Output = ();
    fn handle(&mut self, _key: Key, _ctx: &EntryCtx) -> Step<()> {
        Step::Done(())
    }
    fn render(&self, area: Rect, buf: &mut Buffer, theme: &Theme) -> Option<(u16, u16)> {
        let w = (max_width(&self.lines).max(31) + 4).min(area.width.saturating_sub(2) as usize);
        let h = (self.lines.len() + 4).min(area.height.saturating_sub(2) as usize);
        let r = centered(area, w, h);
        frame(
            buf,
            r,
            theme,
            ColorItem::DialogError,
            ColorItem::DialogErrorTitle,
            "Error",
        );
        for (i, l) in self.lines.iter().enumerate() {
            put(
                buf,
                r,
                r.x + 2,
                r.y + 2 + i as u16,
                l,
                style(theme, ColorItem::DialogErrorText),
            );
        }
        footer(
            buf,
            r,
            " Press any key to continue ",
            style(theme, ColorItem::DialogError),
        );
        None
    }
}

pub struct ConfirmDialog {
    title: String,
    question: String,
    yes: bool,
}

impl ConfirmDialog {
    pub fn new(title: &str, question: &str, default_yes: bool) -> Self {
        ConfirmDialog {
            title: title.into(),
            question: question.into(),
            yes: default_yes,
        }
    }
}

impl Dialog for ConfirmDialog {
    type Output = bool;
    fn handle(&mut self, key: Key, _ctx: &EntryCtx) -> Step<bool> {
        match key.code {
            KeyCode::Up
            | KeyCode::Down
            | KeyCode::Left
            | KeyCode::Right
            | KeyCode::Tab
            | KeyCode::BackTab => {
                self.yes = !self.yes;
                Step::Continue
            }
            KeyCode::Enter => Step::Done(self.yes),
            _ if is_cancel(&key) => Step::Done(false),
            _ => match plain_char(&key) {
                Some('y' | 'Y') => Step::Done(true),
                Some('n' | 'N') => Step::Done(false),
                _ => Step::Continue,
            },
        }
    }
    fn render(&self, area: Rect, buf: &mut Buffer, theme: &Theme) -> Option<(u16, u16)> {
        let w = (width(&self.question) + 5)
            .max(34)
            .min(area.width.saturating_sub(2) as usize);
        let r = centered(area, w, 5);
        frame(
            buf,
            r,
            theme,
            ColorItem::Dialog,
            ColorItem::DialogTitle,
            &self.title,
        );
        put(
            buf,
            r,
            r.x + 2,
            r.y + 1,
            &self.question,
            style(theme, ColorItem::Dialog),
        );
        let (w, row) = (r.width, r.y + 3);
        let cols = [
            (w / 5 + 1, "[ Yes ]"),
            ((w * 4 / 5).saturating_sub(6), "[ No ]"),
        ];
        buttons(
            buf,
            r,
            row,
            &cols,
            Some(if self.yes { 0 } else { 1 }),
            theme,
        );
        None
    }
}

pub struct ConfirmAllDialog {
    title: String,
    question: String,
    order: Vec<Answer>,
    cur: usize,
}

impl ConfirmAllDialog {
    pub fn new(title: &str, question: &str, with_none: bool) -> Self {
        let order = if with_none {
            vec![
                Answer::Yes,
                Answer::All,
                Answer::No,
                Answer::NoneAll,
                Answer::Stop,
            ]
        } else {
            vec![Answer::Yes, Answer::All, Answer::No, Answer::Stop]
        };
        ConfirmAllDialog {
            title: title.into(),
            question: question.into(),
            order,
            cur: 0,
        }
    }
}

fn answer_label(a: Answer) -> &'static str {
    match a {
        Answer::Yes => "[ Yes ]",
        Answer::All => "[ All ]",
        Answer::No => "[ No ]",
        Answer::NoneAll => "[ NOne ]",
        Answer::Stop => "[ Stop ]",
    }
}

impl Dialog for ConfirmAllDialog {
    type Output = Answer;
    fn handle(&mut self, key: Key, _ctx: &EntryCtx) -> Step<Answer> {
        let n = self.order.len();
        match key.code {
            KeyCode::Up | KeyCode::Left | KeyCode::BackTab => self.cur = (self.cur + n - 1) % n,
            KeyCode::Down | KeyCode::Right | KeyCode::Tab => self.cur = (self.cur + 1) % n,
            KeyCode::Enter => return Step::Done(self.order[self.cur]),
            _ if is_cancel(&key) => return Step::Done(Answer::Stop),
            _ => {
                let pick = match plain_char(&key).map(|c| c.to_ascii_lowercase()) {
                    Some('y') => Some(Answer::Yes),
                    Some('a') => Some(Answer::All),
                    Some('n') => Some(Answer::No),
                    Some('o') if self.order.contains(&Answer::NoneAll) => Some(Answer::NoneAll),
                    Some('s') => Some(Answer::Stop),
                    _ => None,
                };
                if let Some(a) = pick {
                    return Step::Done(a);
                }
            }
        }
        Step::Continue
    }
    fn render(&self, area: Rect, buf: &mut Buffer, theme: &Theme) -> Option<(u16, u16)> {
        let base = if self.order.len() == 5 { 50 } else { 48 };
        let w = (width(&self.question) + 5)
            .max(base)
            .min(area.width.saturating_sub(2) as usize);
        let r = centered(area, w, 5);
        frame(
            buf,
            r,
            theme,
            ColorItem::Dialog,
            ColorItem::DialogTitle,
            &self.title,
        );
        put(
            buf,
            r,
            r.x + 2,
            r.y + 1,
            &self.question,
            style(theme, ColorItem::Dialog),
        );
        let labels: Vec<&str> = self.order.iter().map(|a| answer_label(*a)).collect();
        let used: usize = labels.iter().map(|l| l.len()).sum();
        let gap = (r.width as usize).saturating_sub(used) / (labels.len() + 1);
        let mut x = gap;
        let mut cols = Vec::new();
        for l in &labels {
            cols.push((x as u16, *l));
            x += l.len() + gap;
        }
        buttons(buf, r, r.y + 3, &cols, Some(self.cur), theme);
        None
    }
}

pub struct GetKeyDialog {
    title: String,
    lines: Vec<String>,
}

impl GetKeyDialog {
    pub fn new(title: &str, question: &str) -> Self {
        GetKeyDialog {
            title: title.into(),
            lines: lines_of(question),
        }
    }
}

impl Dialog for GetKeyDialog {
    type Output = Option<char>;
    fn handle(&mut self, key: Key, _ctx: &EntryCtx) -> Step<Option<char>> {
        if is_cancel(&key) {
            return Step::Done(None);
        }
        match plain_char(&key) {
            Some(c) => Step::Done(Some(c)),
            None => Step::Continue,
        }
    }
    fn render(&self, area: Rect, buf: &mut Buffer, theme: &Theme) -> Option<(u16, u16)> {
        let w = (max_width(&self.lines) + 4).max(width(&self.title) + 6);
        let r = centered(
            area,
            w.min(area.width.saturating_sub(2) as usize),
            self.lines.len() + 4,
        );
        frame(
            buf,
            r,
            theme,
            ColorItem::Dialog,
            ColorItem::DialogTitle,
            &self.title,
        );
        for (i, l) in self.lines.iter().enumerate() {
            put(
                buf,
                r,
                r.x + 2,
                r.y + 2 + i as u16,
                l,
                style(theme, ColorItem::Dialog),
            );
        }
        None
    }
}

pub struct SelectDialog {
    title: String,
    entries: Vec<String>,
    quick_key: bool,
    min_height: bool,
    cur: usize,
    page: Cell<usize>,
    searching: bool,
}

impl SelectDialog {
    pub fn new(title: &str, entries: Vec<String>, quick_key: bool, min_height: bool) -> Self {
        SelectDialog {
            title: title.into(),
            entries,
            quick_key,
            min_height,
            cur: 0,
            page: Cell::new(10),
            searching: false,
        }
    }
}

impl Dialog for SelectDialog {
    type Output = Option<usize>;
    fn handle(&mut self, key: Key, _ctx: &EntryCtx) -> Step<Option<usize>> {
        let n = self.entries.len();
        let last = n.saturating_sub(1);
        let page = self.page.get().max(1);
        let ch = plain_char(&key);
        if self.searching {
            self.searching = false;
            if let Some(c) = ch {
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
        if self.quick_key {
            if let Some(c) = ch {
                if let Some(i) = self.entries.iter().position(|e| e.starts_with(c)) {
                    return Step::Done(Some(i));
                }
            }
        }
        let ctrl = |c: char| key.ctrl && key.code == KeyCode::Char(c);
        match key.code {
            _ if is_cancel(&key) || matches!(ch, Some('q' | 'Q')) => return Step::Done(None),
            KeyCode::Enter => return Step::Done((n > 0).then_some(self.cur)),
            KeyCode::Up => self.cur = self.cur.saturating_sub(1),
            KeyCode::Down => self.cur = (self.cur + 1).min(last),
            KeyCode::PageUp | KeyCode::Backspace => self.cur = self.cur.saturating_sub(page),
            KeyCode::PageDown => self.cur = (self.cur + page).min(last),
            KeyCode::Home => self.cur = 0,
            KeyCode::End => self.cur = last,
            _ => match ch {
                Some('k' | 'K') => self.cur = self.cur.saturating_sub(1),
                Some('j' | 'J') => self.cur = (self.cur + 1).min(last),
                Some(' ') => self.cur = (self.cur + page).min(last),
                _ if ctrl('h') || ctrl('b') => self.cur = self.cur.saturating_sub(page),
                _ if ctrl('f') => self.cur = (self.cur + page).min(last),
                _ if ctrl('a') => self.cur = 0,
                _ if ctrl('e') => self.cur = last,
                _ if ctrl('l') => self.cur = ((self.cur / page) * page + page / 2).min(last),
                _ if ctrl('s') => self.searching = true,
                _ => {}
            },
        }
        Step::Continue
    }
    fn render(&self, area: Rect, buf: &mut Buffer, theme: &Theme) -> Option<(u16, u16)> {
        let rows = if self.min_height {
            self.entries.len()
        } else {
            10
        };
        let h0 = rows.min((area.height as usize).saturating_sub(4)).max(1);
        let wmax = max_width(&self.entries).max(width(&self.title)).max(32);
        let w = wmax.min((area.width as usize).saturating_sub(12)) + 8;
        let r = centered(area, w, h0 + 2);
        self.page.set(h0);
        frame(
            buf,
            r,
            theme,
            ColorItem::Selectitem,
            ColorItem::SelectitemTitle,
            &self.title,
        );
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
                &text2wrap(e, inner_w, true, 0.66, true),
                style(theme, item),
            );
        }
        if let Some(cells) = scrollbar(h0, self.entries.len(), self.cur, a) {
            let st = style(theme, ColorItem::Selectitem);
            for (k, cell) in cells.into_iter().enumerate() {
                let sym = match cell {
                    ScrollCell::Line => "│",
                    ScrollCell::Thumb => "▒",
                    ScrollCell::Up => "^",
                    ScrollCell::Down => "v",
                };
                put(
                    buf,
                    r,
                    r.right().saturating_sub(1),
                    r.y + 1 + k as u16,
                    sym,
                    st,
                );
            }
        }
        None
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Focus {
    Entry(usize),
    Yes,
    No,
}

/// Shared logic of the one- and two-entry dialogs: entries, then Yes, then No.
struct EntryForm {
    title: String,
    helps: Vec<String>,
    entries: Vec<EntryLine>,
    focus: Focus,
}

impl EntryForm {
    fn next(&mut self, back: bool) {
        let n = self.entries.len();
        self.focus = match (self.focus, back) {
            (Focus::Entry(i), false) if i + 1 < n => Focus::Entry(i + 1),
            (Focus::Entry(_), false) => Focus::Yes,
            (Focus::Yes, false) => Focus::No,
            (Focus::No, false) => Focus::Entry(0),
            (Focus::Entry(0), true) => Focus::No,
            (Focus::Entry(i), true) => Focus::Entry(i - 1),
            (Focus::Yes, true) => Focus::Entry(n - 1),
            (Focus::No, true) => Focus::Yes,
        };
    }

    fn texts(&self) -> Vec<String> {
        self.entries
            .iter()
            .map(|e| e.text.trim().to_string())
            .collect()
    }

    fn handle(&mut self, key: Key, ctx: &EntryCtx) -> Step<Option<Vec<String>>> {
        match self.focus {
            Focus::Entry(i) => match self.entries[i].handle(key, ctx) {
                EntryEvent::Cancel => Step::Done(None),
                EntryEvent::Accept => Step::Done(Some(self.texts())),
                EntryEvent::Tab => {
                    self.next(false);
                    Step::Continue
                }
                EntryEvent::BackTab => {
                    self.next(true);
                    Step::Continue
                }
                EntryEvent::Select {
                    title,
                    entries,
                    kind,
                } => Step::Select {
                    title,
                    entries,
                    kind,
                },
                EntryEvent::None | EntryEvent::Beep | EntryEvent::Hide => Step::Continue,
            },
            Focus::Yes | Focus::No => match key.code {
                KeyCode::Tab => {
                    self.next(false);
                    Step::Continue
                }
                KeyCode::BackTab => {
                    self.next(true);
                    Step::Continue
                }
                KeyCode::Enter if self.focus == Focus::Yes => Step::Done(Some(self.texts())),
                KeyCode::Enter => Step::Done(None),
                _ if is_cancel(&key) => Step::Done(None),
                _ => Step::Continue,
            },
        }
    }

    fn apply_selection(&mut self, kind: SelectKind, chosen: &str, ctx: &EntryCtx) {
        if let Focus::Entry(i) = self.focus {
            self.entries[i].apply_selection(kind, chosen, ctx);
        }
    }

    fn render(&self, area: Rect, buf: &mut Buffer, theme: &Theme) -> Option<(u16, u16)> {
        let n = self.entries.len();
        let h = 3 * n + 3;
        let help_w = self.helps.iter().map(|s| width(s)).max().unwrap_or(0) + 5;
        let w = help_w.max(area.width as usize / 2).min(area.width as usize);
        let r = centered(area, w, h);
        frame(
            buf,
            r,
            theme,
            ColorItem::Dialog,
            ColorItem::DialogTitle,
            &self.title,
        );
        let ew = (r.width as usize).saturating_sub(4);
        let mut cursor = None;
        for (i, (help, entry)) in self.helps.iter().zip(&self.entries).enumerate() {
            let y = r.y + 1 + 3 * i as u16;
            let label = if help.ends_with(':') {
                help.clone()
            } else {
                format!("{help}:")
            };
            put(buf, r, r.x + 2, y, &label, style(theme, ColorItem::Dialog));
            let (text, col) = entry.visible(ew);
            put(
                buf,
                r,
                r.x + 2,
                y + 1,
                &text,
                style(theme, ColorItem::Entryline),
            );
            if self.focus == Focus::Entry(i) && y + 1 < r.bottom() {
                cursor = Some((r.x + 2 + col.min(ew) as u16, y + 1));
            }
        }
        let active = match self.focus {
            Focus::Entry(_) => None,
            Focus::Yes => Some(0),
            Focus::No => Some(1),
        };
        let (w, row) = (r.width, r.y + 1 + 3 * n as u16);
        buttons(
            buf,
            r,
            row,
            &[
                (w / 5 + 1, "[<Yes>]"),
                ((w * 4 / 5).saturating_sub(6), "[ No ]"),
            ],
            active,
            theme,
        );
        cursor
    }
}

pub struct EntryDialog {
    form: EntryForm,
}

impl EntryDialog {
    pub fn new(title: &str, help: &str, text: &str, history: Vec<String>, is_files: bool) -> Self {
        EntryDialog {
            form: EntryForm {
                title: title.into(),
                helps: vec![help.into()],
                entries: vec![EntryLine::new(text, history, is_files)],
                focus: Focus::Entry(0),
            },
        }
    }
}

impl Dialog for EntryDialog {
    type Output = Option<String>;
    fn handle(&mut self, key: Key, ctx: &EntryCtx) -> Step<Option<String>> {
        match self.form.handle(key, ctx) {
            Step::Done(v) => Step::Done(v.map(|mut t| t.remove(0))),
            Step::Continue => Step::Continue,
            Step::Select {
                title,
                entries,
                kind,
            } => Step::Select {
                title,
                entries,
                kind,
            },
        }
    }
    fn apply_selection(&mut self, kind: SelectKind, chosen: &str, ctx: &EntryCtx) {
        self.form.apply_selection(kind, chosen, ctx);
    }
    fn render(&self, area: Rect, buf: &mut Buffer, theme: &Theme) -> Option<(u16, u16)> {
        self.form.render(area, buf, theme)
    }
}

pub struct DoubleEntryDialog {
    form: EntryForm,
}

impl DoubleEntryDialog {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        title: &str,
        help1: &str,
        help2: &str,
        text1: &str,
        text2: &str,
        history1: Vec<String>,
        history2: Vec<String>,
        is_files: bool,
    ) -> Self {
        DoubleEntryDialog {
            form: EntryForm {
                title: title.into(),
                helps: vec![help1.into(), help2.into()],
                entries: vec![
                    EntryLine::new(text1, history1, is_files),
                    EntryLine::new(text2, history2, is_files),
                ],
                focus: Focus::Entry(0),
            },
        }
    }
}

impl Dialog for DoubleEntryDialog {
    type Output = Option<(String, String)>;
    fn handle(&mut self, key: Key, ctx: &EntryCtx) -> Step<Option<(String, String)>> {
        match self.form.handle(key, ctx) {
            Step::Done(v) => Step::Done(v.map(|t| (t[0].clone(), t[1].clone()))),
            Step::Continue => Step::Continue,
            Step::Select {
                title,
                entries,
                kind,
            } => Step::Select {
                title,
                entries,
                kind,
            },
        }
    }
    fn apply_selection(&mut self, kind: SelectKind, chosen: &str, ctx: &EntryCtx) {
        self.form.apply_selection(kind, chosen, ctx);
    }
    fn render(&self, area: Rect, buf: &mut Buffer, theme: &Theme) -> Option<(u16, u16)> {
        self.form.render(area, buf, theme)
    }
}

/// Full-screen text viewer (lfm's InternalView).
pub struct ViewDialog {
    title: String,
    lines: Vec<(String, ColorItem)>,
    y: usize,
    body_h: Cell<usize>,
}

impl ViewDialog {
    pub fn new(title: &str, lines: Vec<(String, ColorItem)>) -> Self {
        ViewDialog {
            title: title.into(),
            lines,
            y: 0,
            body_h: Cell::new(0),
        }
    }

    fn large(&self) -> bool {
        self.lines.len() > self.body_h.get()
    }
}

impl Dialog for ViewDialog {
    type Output = ();
    fn handle(&mut self, key: Key, _ctx: &EntryCtx) -> Step<()> {
        if !self.large() {
            return Step::Done(());
        }
        let last = self.lines.len().saturating_sub(1);
        let page = self.body_h.get().max(1);
        let ch = plain_char(&key);
        let ctrl = |c: char| key.ctrl && key.code == KeyCode::Char(c);
        match key.code {
            KeyCode::Esc | KeyCode::F(3) | KeyCode::F(10) => return Step::Done(()),
            KeyCode::Up => self.y = self.y.saturating_sub(1),
            KeyCode::Down => self.y = (self.y + 1).min(last),
            KeyCode::Home => self.y = 0,
            KeyCode::End => self.y = last,
            KeyCode::PageUp | KeyCode::Backspace => self.y = self.y.saturating_sub(page),
            KeyCode::PageDown => self.y = (self.y + page).min(last),
            _ => match ch {
                Some('q' | 'Q') => return Step::Done(()),
                Some('k' | 'K') => self.y = self.y.saturating_sub(1),
                Some('j' | 'J') => self.y = (self.y + 1).min(last),
                Some(' ') => self.y = (self.y + page).min(last),
                _ if ctrl('a') => self.y = 0,
                _ if ctrl('e') => self.y = last,
                _ if ctrl('h') || ctrl('b') => self.y = self.y.saturating_sub(page),
                _ if ctrl('f') => self.y = (self.y + page).min(last),
                _ => {}
            },
        }
        Step::Continue
    }
    fn render(&self, area: Rect, buf: &mut Buffer, theme: &Theme) -> Option<(u16, u16)> {
        let body_h = (area.height as usize).saturating_sub(2);
        self.body_h.set(body_h);
        fill(buf, area, style(theme, ColorItem::ViewWhiteOnBlack));
        let top = Rect::new(area.x, area.y, area.width, area.height.min(1));
        fill(buf, top, style(theme, ColorItem::Header));
        let title = text2wrap(
            &self.title,
            (area.width as usize).saturating_sub(1),
            true,
            0.66,
            false,
        );
        let tx = area.x + (area.width.saturating_sub(width(&title) as u16)) / 2;
        put(
            buf,
            top,
            tx,
            area.y,
            &title,
            style(theme, ColorItem::Header),
        );
        let y0 = if self.large() { 0 } else { 1 };
        for (i, (l, item)) in self.lines.iter().skip(self.y).take(body_h).enumerate() {
            let line = text2wrap(
                l,
                (area.width as usize).saturating_sub(2),
                true,
                0.66,
                false,
            );
            put(
                buf,
                area,
                area.x + 1,
                area.y + 1 + y0 + i as u16,
                &line,
                style(theme, *item),
            );
        }
        if area.height >= 2 {
            let bottom = Rect::new(area.x, area.bottom() - 1, area.width, 1);
            fill(buf, bottom, style(theme, ColorItem::Statusbar));
            if !self.large() {
                footer(
                    buf,
                    bottom,
                    "Press any key to continue",
                    style(theme, ColorItem::Statusbar),
                );
            }
        }
        None
    }
}

pub struct ProgressView {
    pub title: String,
    pub item: String,
    pub index: usize,
    pub count: usize,
    pub acc: u64,
    pub total: u64,
}

impl ProgressView {
    pub fn render(&self, area: Rect, buf: &mut Buffer, theme: &Theme) {
        let w = (width(&self.title).max(60) + 6).min(area.width.saturating_sub(2) as usize);
        let r = centered(area, w, 8);
        frame(
            buf,
            r,
            theme,
            ColorItem::Dialog,
            ColorItem::DialogTitle,
            &self.title,
        );
        let st = style(theme, ColorItem::Dialog);
        let w = r.width as usize;
        put(buf, r, r.x + 2, r.y + 2, "File:", st);
        let item = text2wrap(&self.item, w.saturating_sub(22), true, 0.66, true);
        put(
            buf,
            r,
            r.x + 9,
            r.y + 2,
            &item,
            style(theme, ColorItem::DialogTitle),
        );
        let count = format!("{}/{}", self.index, self.count);
        put(
            buf,
            r,
            r.x + w.saturating_sub(2 + count.len()) as u16,
            r.y + 2,
            &count,
            st,
        );
        let bar_w = w.saturating_sub(18);
        let bars = [
            ("Bytes", self.acc, self.total.max(1)),
            ("Count", self.index as u64, self.count.max(1) as u64),
        ];
        for (k, (label, done, total)) in bars.into_iter().enumerate() {
            let y = r.y + 4 + k as u16;
            put(buf, r, r.x + 2, y, label, st);
            put(
                buf,
                r,
                r.x + 9,
                y,
                &" ".repeat(bar_w),
                style(theme, ColorItem::ProgressbarBg),
            );
            let filled = (bar_w as u64 * done.min(total) / total) as usize;
            put(
                buf,
                r,
                r.x + 9,
                y,
                &" ".repeat(filled),
                style(theme, ColorItem::ProgressbarFg),
            );
            let pct = format!("[{:3}%]", 100 * done.min(total) / total);
            put(buf, r, r.x + w.saturating_sub(8) as u16, y, &pct, st);
        }
        footer(buf, r, " Press Ctrl-C to stop ", st);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::draw::buffer_line;

    fn k(s: &str) -> Key {
        crate::keys::parse_key(s).unwrap()
    }

    fn c() -> EntryCtx {
        EntryCtx {
            current_name: "f".into(),
            cur_dir: "/".into(),
            other_dir: "/".into(),
            bookmarks: vec![],
            prev_paths: vec![],
        }
    }

    fn done<T>(s: Step<T>) -> T {
        match s {
            Step::Done(v) => v,
            _ => panic!("not done"),
        }
    }

    fn screen_of(d: &dyn Fn(Rect, &mut Buffer)) -> String {
        let r = Rect::new(0, 0, 80, 24);
        let mut b = Buffer::empty(r);
        d(r, &mut b);
        (0..24)
            .map(|y| buffer_line(&b, y))
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn confirm_keys() {
        let mut d = ConfirmDialog::new("T", "Q?", true);
        assert!(done(d.handle(k("enter"), &c())));
        let mut d = ConfirmDialog::new("T", "Q?", true);
        d.handle(k("tab"), &c());
        assert!(!done(d.handle(k("enter"), &c())));
        let mut d = ConfirmDialog::new("T", "Q?", false);
        assert!(done(d.handle(k("y"), &c())));
        let mut d = ConfirmDialog::new("T", "Q?", true);
        assert!(!done(d.handle(k("esc"), &c())));
    }

    #[test]
    fn confirm_all_keys() {
        let mut d = ConfirmAllDialog::new("T", "Q?", true);
        assert_eq!(done(d.handle(k("o"), &c())), Answer::NoneAll);
        let mut d = ConfirmAllDialog::new("T", "Q?", false);
        assert!(matches!(d.handle(k("o"), &c()), Step::Continue));
        let mut d = ConfirmAllDialog::new("T", "Q?", false);
        d.handle(k("right"), &c());
        assert_eq!(done(d.handle(k("enter"), &c())), Answer::All);
        let mut d = ConfirmAllDialog::new("T", "Q?", false);
        d.handle(k("left"), &c());
        assert_eq!(done(d.handle(k("enter"), &c())), Answer::Stop);
        let mut d = ConfirmAllDialog::new("T", "Q?", false);
        assert_eq!(done(d.handle(k("C-c"), &c())), Answer::Stop);
    }

    #[test]
    fn getkey_and_select() {
        let mut g = GetKeyDialog::new("T", "press");
        assert_eq!(done(g.handle(k("3"), &c())), Some('3'));
        let mut g = GetKeyDialog::new("T", "press");
        assert_eq!(done(g.handle(k("C-c"), &c())), None);
        let entries: Vec<String> = ["/    Find", "#    Size", "s    Sort"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let mut s = SelectDialog::new("Menu", entries.clone(), true, true);
        assert_eq!(done(s.handle(k("#"), &c())), Some(1));
        let mut s = SelectDialog::new("Menu", entries.clone(), false, true);
        assert!(matches!(s.handle(k("#"), &c()), Step::Continue));
        s.handle(k("j"), &c());
        s.handle(k("end"), &c());
        assert_eq!(done(s.handle(k("enter"), &c())), Some(2));
        let mut s = SelectDialog::new("Menu", entries.clone(), false, true);
        s.handle(k("C-s"), &c());
        s.handle(k("s"), &c());
        assert_eq!(done(s.handle(k("enter"), &c())), Some(2));
        let mut s = SelectDialog::new("Menu", entries, false, true);
        assert_eq!(done(s.handle(k("q"), &c())), None);
    }

    #[test]
    fn entry_dialog_focus_cycle() {
        let mut d = EntryDialog::new("Make directory", "Type directory name", "", vec![], true);
        for ch in ['x', ' '] {
            d.handle(
                Key {
                    code: KeyCode::Char(ch),
                    ctrl: false,
                    alt: false,
                    shift: false,
                },
                &c(),
            );
        }
        assert_eq!(done(d.handle(k("enter"), &c())), Some("x".into()));
        let mut d = EntryDialog::new("T", "H", "abc", vec![], true);
        d.handle(k("tab"), &c());
        d.handle(k("tab"), &c());
        assert_eq!(done(d.handle(k("enter"), &c())), None);
        let mut d = EntryDialog::new("T", "H", "abc", vec![], true);
        d.handle(k("tab"), &c());
        assert_eq!(done(d.handle(k("enter"), &c())), Some("abc".into()));
        let mut d = EntryDialog::new("T", "H", "", vec![], true);
        assert!(matches!(d.handle(k("C-g"), &c()), Step::Continue)); // empty history → beep
    }

    #[test]
    fn double_entry_returns_both() {
        let mut d = DoubleEntryDialog::new(
            "Create link",
            "Link name",
            "Pointing to",
            "a",
            "b",
            vec![],
            vec![],
            true,
        );
        d.handle(k("tab"), &c());
        assert_eq!(
            done(d.handle(k("enter"), &c())),
            Some(("a".into(), "b".into()))
        );
        let mut d = DoubleEntryDialog::new("T", "1", "2", "a", "b", vec![], vec![], true);
        assert_eq!(done(d.handle(k("esc"), &c())), None);
    }

    #[test]
    fn view_scrolls_and_closes() {
        let theme = Theme::default_theme();
        let lines: Vec<(String, ColorItem)> = (0..100)
            .map(|i| (format!("line {i}"), ColorItem::ViewWhiteOnBlack))
            .collect();
        let mut v = ViewDialog::new("License", lines);
        let r = Rect::new(0, 0, 80, 24);
        let mut b = Buffer::empty(r);
        v.render(r, &mut b, &theme);
        assert!(matches!(v.handle(k("j"), &c()), Step::Continue)); // large: keys scroll
        assert!(matches!(v.handle(k("q"), &c()), Step::Done(())));
        let mut small = ViewDialog::new("Small", vec![("hi".into(), ColorItem::ViewWhiteOnBlack)]);
        small.render(r, &mut b, &theme);
        assert!(matches!(small.handle(k("j"), &c()), Step::Done(())));
    }

    #[test]
    fn renders_texts() {
        let theme = Theme::default_theme();
        let s = screen_of(&|r, b| {
            ErrorDialog::new("Cannot chdir /x\nPermission denied").render(r, b, &theme);
        });
        assert!(
            s.contains("Error")
                && s.contains("Permission denied")
                && s.contains("Press any key to continue")
        );
        let s = screen_of(&|r, b| {
            ConfirmAllDialog::new("Copy file(s)", "Overwrite '/x'?", true).render(r, b, &theme);
        });
        assert!(
            s.contains("[ Yes ]")
                && s.contains("[ All ]")
                && s.contains("[ NOne ]")
                && s.contains("[ Stop ]")
        );
        let s = screen_of(&|r, b| {
            ProgressView {
                title: "Copy file(s)".into(),
                item: "x".into(),
                index: 1,
                count: 2,
                acc: 1,
                total: 2,
            }
            .render(r, b, &theme);
        });
        assert!(
            s.contains("Bytes") && s.contains("Count") && s.contains("[ 50%]") && s.contains("1/2")
        );
    }

    #[test]
    fn entry_render_returns_cursor() {
        let theme = Theme::default_theme();
        let r = Rect::new(0, 0, 80, 24);
        let mut b = Buffer::empty(r);
        let d = EntryDialog::new(
            "Go to file",
            "Type part of the file name",
            "abc",
            vec![],
            false,
        );
        let cur = d.render(r, &mut b, &theme).expect("cursor");
        assert!(cur.0 > 0 && cur.1 > 0);
    }

    #[test]
    fn renders_inside_small_areas() {
        let theme = Theme::default_theme();
        for (w, h) in [(66u16, 8u16), (20, 3), (200, 60), (1, 1)] {
            let r = Rect::new(0, 0, w, h);
            let mut b = Buffer::empty(r);
            ErrorDialog::new("Cannot chdir /x\nPermission denied").render(r, &mut b, &theme);
            EntryDialog::new(
                "T",
                "a fairly long help text for the entry",
                "x",
                vec![],
                true,
            )
            .render(r, &mut b, &theme);
            DoubleEntryDialog::new("T", "1", "2", "a", "b", vec![], vec![], true)
                .render(r, &mut b, &theme);
            SelectDialog::new("M", vec!["a".into(); 40], false, false).render(r, &mut b, &theme);
            ConfirmDialog::new("T", "Q?", true).render(r, &mut b, &theme);
            ConfirmAllDialog::new("T", "Q?", true).render(r, &mut b, &theme);
            GetKeyDialog::new("T", "line1\nline2").render(r, &mut b, &theme);
            ViewDialog::new("V", vec![("x".into(), ColorItem::ViewWhiteOnBlack)])
                .render(r, &mut b, &theme);
            ProgressView {
                title: "Copy file(s)".into(),
                item: "x".into(),
                index: 1,
                count: 2,
                acc: 1,
                total: 2,
            }
            .render(r, &mut b, &theme);
        }
    }
}
