//! Rendering of the main screen into a ratatui buffer: tabs bars, panes (half
//! view with box and columns, full view with all attributes) and the status
//! bar. Layout ported from `Pane.display`/`StatusBar.display` in lfm's ui.py.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;

use std::str::FromStr;

use crate::actions::Action;
use crate::config::settings::{PaneSort, SortType};
use crate::config::theme::{ColorItem, Theme};
use crate::fs::entry::{FileType, FsEntry};
use crate::fs::format::{group2str, num2str, owner2str, perms2str, rdev2str, size_human, time2str};
use crate::keys::{Key, KeyCode};
use crate::ui::state::{App, PaneMode, Tab, MAX_TABS};
use crate::ui::text::{scrollbar, text2wrap, width, ScrollCell};

pub fn style(theme: &Theme, item: ColorItem) -> Style {
    let (fg, bg) = theme.get(item);
    Style::new().fg(fg).bg(bg)
}

/// Writes `text` at (x, y), clipped to `area`; out-of-area positions are ignored.
pub(crate) fn put(buf: &mut Buffer, area: Rect, x: u16, y: u16, text: &str, st: Style) {
    if y < area.top() || y >= area.bottom() || x < area.left() || x >= area.right() {
        return;
    }
    buf.set_stringn(x, y, text, (area.right() - x) as usize, st);
}

pub(crate) fn fill(buf: &mut Buffer, area: Rect, st: Style) {
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            buf[(x, y)].set_symbol(" ").set_style(st);
        }
    }
}

pub(crate) fn draw_box(buf: &mut Buffer, r: Rect, st: Style) {
    if r.width < 2 || r.height < 2 {
        return;
    }
    let (l, rt, top, bot) = (r.left(), r.right() - 1, r.top(), r.bottom() - 1);
    for x in l..=rt {
        buf[(x, top)].set_symbol("─").set_style(st);
        buf[(x, bot)].set_symbol("─").set_style(st);
    }
    for y in top..=bot {
        buf[(l, y)].set_symbol("│").set_style(st);
        buf[(rt, y)].set_symbol("│").set_style(st);
    }
    buf[(l, top)].set_symbol("┌");
    buf[(rt, top)].set_symbol("┐");
    buf[(l, bot)].set_symbol("└");
    buf[(rt, bot)].set_symbol("┘");
}

fn scroll_symbol(c: ScrollCell) -> &'static str {
    match c {
        ScrollCell::Line => "│",
        ScrollCell::Thumb => "▒",
        ScrollCell::Up => "^",
        ScrollCell::Down => "v",
    }
}

fn draw_scrollbar(buf: &mut Buffer, area: Rect, x: u16, y0: u16, h: usize, tab: &Tab, st: Style) {
    if let Some(cells) = scrollbar(h, tab.fs.len(), tab.i, tab.a) {
        for (k, c) in cells.into_iter().enumerate() {
            put(buf, area, x, y0 + k as u16, scroll_symbol(c), st);
        }
    }
}

fn entry_item(app: &App, tab: &Tab, idx: usize, show_cursor: bool) -> ColorItem {
    let e = &tab.fs.entries()[idx];
    let selected = tab.is_selected(&e.name);
    match (show_cursor && idx == tab.i, selected) {
        (true, true) => ColorItem::CursorSelected,
        (true, false) => ColorItem::Cursor,
        (false, true) => ColorItem::SelectedFiles,
        (false, false) => {
            ColorItem::from_str(&format!("files_{}", e.file_kind(&app.settings.files)))
                .unwrap_or(ColorItem::FilesReg)
        }
    }
}

/// "date ↓": a pane's sort for the hints; ↓ means reversed.
fn sort_label(s: PaneSort) -> String {
    let name = match s.sort_type {
        SortType::None => "none",
        SortType::ByName => "name",
        SortType::ByExt => "ext",
        SortType::ByPath => "path",
        SortType::BySize => "size",
        SortType::ByMTime => "date",
    };
    format!("{name} {}", if s.sort_reverse { '↓' } else { '↑' })
}

/// The sort hint of the active pane in the one-pane view (no border to put it on).
fn status_sort_hint(app: &App) -> Option<String> {
    (app.pane().mode == PaneMode::Full)
        .then(|| format!("Sort: {}", sort_label(app.settings.pane_sort[app.active])))
}

/// Width of the size column: 7, or wider when exact bytes need it.
fn size_width(app: &App, tab: &Tab) -> usize {
    if app.settings.options.human_readable_sizes {
        return 7;
    }
    let widest = tab.fs.entries().iter().map(|e| e.size).max().unwrap_or(0);
    widest.to_string().len().max(7)
}

fn size_field(e: &FsEntry, human: bool, width: usize) -> String {
    match e.ftype {
        FileType::CDev | FileType::BDev => format!("{:>width$}", rdev2str(e.rdev.0, e.rdev.1)),
        _ if human => format!("{:>width$}", size_human(e.size)),
        _ => format!("{:>width$}", e.size),
    }
}

fn draw_half(app: &App, pane_idx: usize, body: Rect, buf: &mut Buffer) {
    let pane = &app.panes[pane_idx];
    let tab = &pane.tabs[pane.active];
    let focused = pane_idx == app.active;
    let th = &app.theme;
    let (attr, attr_path) = if focused {
        (
            style(th, ColorItem::PaneActive),
            style(th, ColorItem::PaneHeaderPath),
        )
    } else {
        (
            style(th, ColorItem::PaneInactive),
            style(th, ColorItem::PaneInactive),
        )
    };
    fill(buf, body, style(th, ColorItem::PaneInactive));
    draw_box(buf, body, attr);
    let (x0, y0, w, h) = (body.x, body.y, body.width as usize, body.height as usize);
    let hint = format!(" Sort: {} ", sort_label(app.settings.pane_sort[pane_idx]));
    if h >= 2 && w >= width(&hint) + 6 {
        let x = x0 + (w - width(&hint) - 3) as u16;
        put(buf, body, x, y0 + h as u16 - 1, &hint, attr);
    }
    let dir = tab.display_dir();
    put(
        buf,
        body,
        x0 + 2,
        y0,
        &text2wrap(&dir, w.saturating_sub(5), true, 0.33, false),
        attr_path,
    );
    if tab.fs.cfg.show_dotfiles && w >= 3 {
        put(buf, body, x0 + w as u16 - 3, y0, "·", attr);
    }
    if !tab.fs.cfg.filters.is_empty() && w >= 2 {
        put(buf, body, x0 + w as u16 - 2, y0, "f", attr);
    }
    let (human, size_w) = (
        app.settings.options.human_readable_sizes,
        size_width(app, tab),
    );
    let col2 = w.saturating_sub(14);
    let col1 = col2.saturating_sub(size_w + 1);
    let name_w = col1.saturating_sub(2);
    let titles = style(th, ColorItem::PaneHeaderTitles);
    put(
        buf,
        body,
        x0 + 1,
        y0 + 1,
        &format!("{:^name_w$}", "Name"),
        titles,
    );
    put(buf, body, x0 + col1 as u16 + 2, y0 + 1, "Size", titles);
    put(buf, body, x0 + col2 as u16 + 5, y0 + 1, "Date", titles);
    let rows = h.saturating_sub(3);
    let line_area = Rect::new(x0 + 1, y0, (w as u16).saturating_sub(2), body.height);
    for r in 0..rows {
        let idx = tab.a + r;
        if idx >= tab.fs.len() {
            break;
        }
        let e = &tab.fs.entries()[idx];
        let line = format!(
            "{}{}│{}│{}",
            e.ftype.symbol(),
            text2wrap(&e.display_name(), name_w, true, 0.66, true),
            size_field(e, human, size_w),
            text2wrap(&time2str(e.mtime, true), 12, true, 0.66, true)
        );
        let st = style(th, entry_item(app, tab, idx, focused));
        put(buf, line_area, x0 + 1, y0 + 2 + r as u16, &line, st);
    }
    let cursor_row = (focused && tab.i >= tab.a).then(|| y0 + 2 + (tab.i - tab.a) as u16);
    for y in y0 + 1..(y0 + h as u16).saturating_sub(1) {
        let st = if Some(y) == cursor_row {
            style(th, ColorItem::Cursor)
        } else {
            attr
        };
        for col in [col1, col2] {
            if col > 0 {
                put(buf, body, x0 + col as u16, y, "│", st);
            }
        }
    }
    if w >= 1 {
        draw_scrollbar(buf, body, x0 + w as u16 - 1, y0 + 2, rows, tab, attr);
    }
}

fn draw_full(app: &App, pane_idx: usize, body: Rect, buf: &mut Buffer) {
    let pane = &app.panes[pane_idx];
    let tab = &pane.tabs[pane.active];
    let th = &app.theme;
    let attr = style(th, ColorItem::PaneInactive);
    fill(buf, body, attr);
    let (x0, y0, w, h) = (body.x, body.y, body.width as usize, body.height as usize);
    let (human, size_w) = (
        app.settings.options.human_readable_sizes,
        size_width(app, tab),
    );
    let name_w = w.saturating_sub(57 + size_w);
    for r in 0..h {
        let idx = tab.a + r;
        if idx >= tab.fs.len() {
            break;
        }
        let e = &tab.fs.entries()[idx];
        let line = format!(
            "{}{}  {}  {}  {}  {}  {}",
            e.ftype.symbol(),
            perms2str(e.mode),
            text2wrap(&owner2str(e.uid), 10, true, 0.66, true),
            text2wrap(&group2str(e.gid), 10, true, 0.66, true),
            size_field(e, human, size_w),
            text2wrap(&time2str(e.mtime, false), 16, true, 0.66, true),
            text2wrap(&e.display_name(), name_w, true, 0.66, true)
        );
        put(
            buf,
            body,
            x0,
            y0 + r as u16,
            &line,
            style(th, entry_item(app, tab, idx, true)),
        );
    }
    if w >= 1 {
        draw_scrollbar(buf, body, x0 + w as u16 - 1, y0, h, tab, attr);
    }
}

fn draw_pane(app: &App, pane_idx: usize, rect: Rect, buf: &mut Buffer) {
    let pane = &app.panes[pane_idx];
    let th = &app.theme;
    let bar = Rect::new(rect.x, rect.y, rect.width, rect.height.min(1));
    fill(buf, bar, style(th, ColorItem::Header));
    let wtab = rect.width as usize / MAX_TABS;
    for (k, tab) in pane.tabs.iter().enumerate() {
        let name = tab.tab_name();
        let label = format!(
            "[{}]",
            text2wrap(&name, wtab.saturating_sub(2), true, 0.5, true)
        );
        let item = if k == pane.active {
            ColorItem::TabActive
        } else {
            ColorItem::TabInactive
        };
        put(
            buf,
            bar,
            rect.x + (wtab * k) as u16,
            rect.y,
            &label,
            style(th, item),
        );
    }
    if rect.height < 2 {
        return;
    }
    let body = Rect::new(rect.x, rect.y + 1, rect.width, rect.height - 1);
    match pane.mode {
        PaneMode::Half => draw_half(app, pane_idx, body, buf),
        PaneMode::Full => draw_full(app, pane_idx, body, buf),
        PaneMode::Hidden => {}
    }
}

/// "-> target" for links, else the full path of the entry under the cursor.
fn realpath(tab: &Tab) -> String {
    let cur = tab.current();
    if cur.is_link() {
        if let Ok(target) = std::fs::read_link(&cur.path) {
            return format!("-> {}", target.display());
        }
    }
    let dir = tab.display_dir();
    let sep = if dir.ends_with('/') { "" } else { "/" };
    format!("{dir}{sep}{}", cur.display_name())
}

/// Short label for an action in the function-key bar (Midnight Commander style).
fn short_label(a: Action) -> String {
    let label = match a {
        Action::HelpMenu => "Help",
        Action::RenameFile => "Rename",
        Action::ViewFile => "View",
        Action::EditFile => "Edit",
        Action::CopyFile => "Copy",
        Action::MoveFile | Action::MoveFile2 => "Move",
        Action::MakeDir => "Mkdir",
        Action::DeleteFile => "Delete",
        Action::MainMenu => "Menu",
        Action::FileMenu => "FMenu",
        Action::QuitChdir | Action::QuitNochdir => "Quit",
        Action::ShowTree => "Tree",
        Action::FindGrep => "Find",
        Action::TouchFile => "Touch",
        Action::LinkCreate => "Link",
        Action::ExecOnFile => "Exec",
        Action::OpenShell => "Shell",
        Action::TogglePowercli => "CLI",
        Action::Refresh => "Reread",
        Action::SortFiles => "Sort",
        Action::ShowFileInfo => "Info",
        Action::Select => "Select",
        other => {
            let name: &str = other.into();
            return name.chars().take(6).collect();
        }
    };
    label.to_string()
}

/// Bottom row: F1-F10 (plus F11/F12 when bound) with the actions bound to them.
fn draw_fkeys_bar(app: &App, row: Rect, buf: &mut Buffer) {
    let plain_f = |n: u8| {
        app.keymap.get(&Key {
            code: KeyCode::F(n),
            ctrl: false,
            alt: false,
            shift: false,
        })
    };
    let slots: Vec<u8> = (1..=12)
        .filter(|&n| n <= 10 || plain_f(n).is_some())
        .collect();
    let slot_w = row.width as usize / slots.len();
    fill(buf, row, style(&app.theme, ColorItem::Header));
    for (k, n) in slots.iter().enumerate() {
        let x = row.x + (k * slot_w) as u16;
        let num = n.to_string();
        let label = plain_f(*n).map(short_label).unwrap_or_default();
        put(
            buf,
            row,
            x,
            row.y,
            &num,
            style(&app.theme, ColorItem::TabActive),
        );
        let label = text2wrap(&label, slot_w.saturating_sub(num.len()), true, 0.66, true);
        put(
            buf,
            row,
            x + num.len() as u16,
            row.y,
            &label,
            style(&app.theme, ColorItem::Header),
        );
    }
}

fn draw_statusbar(app: &App, area: Rect, buf: &mut Buffer) {
    let y = area.bottom() - 1;
    let row = Rect::new(area.x, y, area.width, 1);
    let st = style(&app.theme, ColorItem::Statusbar);
    fill(buf, row, st);
    let w = area.width as usize;
    let tab = app.tab();
    let selected = tab.selected_entries();
    let hint = status_sort_hint(app);
    if !selected.is_empty() {
        if w >= 45 {
            let size: u64 = selected.iter().map(|e| e.size).sum();
            put(
                buf,
                row,
                area.x,
                y,
                &format!("    {} bytes in {} files", num2str(size), selected.len()),
                st,
            );
        }
    } else if w >= 80 {
        let hint_w = hint.as_ref().map_or(0, |h| width(h) + 2);
        let mut s = format!("File: {:>4} of {:<4}", tab.i + 1, tab.fs.len());
        if !tab.fs.cfg.filters.is_empty() {
            s.push_str(&format!("  [{} filtered]", tab.fs.nfiltered()));
        }
        put(buf, row, area.x, y, &s, st);
        let path = text2wrap(
            &realpath(tab),
            w.saturating_sub(20 + s.len() + hint_w),
            true,
            0.66,
            false,
        );
        put(
            buf,
            row,
            area.x + (s.len() + 4) as u16,
            y,
            &format!("Path: {path}"),
            st,
        );
    }
    if w > 10 {
        if let Some(key) = app.keymap.keys_for(Action::HelpMenu).first() {
            let label = format!("{key}=Help");
            let x = w.saturating_sub(1 + width(&label));
            put(buf, row, area.x + x as u16, y, &label, st);
            if let Some(hint) = hint.filter(|_| w >= 80 && selected.is_empty()) {
                let hx = x.saturating_sub(2 + width(&hint));
                put(buf, row, area.x + hx as u16, y, &hint, st);
            }
        }
    }
}

pub fn draw_app(app: &App, area: Rect, buf: &mut Buffer) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let half = area.width / 2;
    let bar = app.settings.options.show_function_keys && area.height >= 3;
    let bottom = if bar { 2 } else { 1 };
    let pane_h = area.height.saturating_sub(bottom);
    for (idx, pane) in app.panes.iter().enumerate() {
        let rect = match pane.mode {
            PaneMode::Hidden => continue,
            PaneMode::Full => Rect::new(area.x, area.y, area.width, pane_h),
            PaneMode::Half => Rect::new(
                area.x + if idx == 0 { 0 } else { half },
                area.y,
                half,
                pane_h,
            ),
        };
        if rect.height > 0 && rect.width > 0 {
            draw_pane(app, idx, rect, buf);
        }
    }
    if bar {
        let status_area = Rect::new(area.x, area.y, area.width, area.height - 1);
        draw_statusbar(app, status_area, buf);
        draw_fkeys_bar(
            app,
            Rect::new(area.x, area.bottom() - 1, area.width, 1),
            buf,
        );
    } else {
        draw_statusbar(app, area, buf);
    }
}

/// The text of row `y` (one symbol per cell), for tests and debugging.
pub fn buffer_line(buf: &Buffer, y: u16) -> String {
    let a = buf.area;
    (a.left()..a.right())
        .map(|x| buf[(x, y)].symbol())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::testutil::test_app as app_with;
    use std::path::Path;

    fn render(app: &App) -> Buffer {
        let r = Rect::new(0, 0, app.width, app.height);
        let mut b = Buffer::empty(r);
        draw_app(app, r, &mut b);
        b
    }

    fn colors(b: &Buffer, x: u16, y: u16) -> (ratatui::style::Color, ratatui::style::Color) {
        let c = &b[(x, y)];
        (c.fg, c.bg)
    }

    #[test]
    fn half_layout_rows() {
        let t = tempfile::tempdir().unwrap();
        let app = app_with(t.path(), 100, 20);
        let b = render(&app);
        assert!(buffer_line(&b, 0).starts_with('['));
        assert!(buffer_line(&b, 1).contains(&t.path().display().to_string()[..10]));
        let titles = buffer_line(&b, 2);
        assert!(titles.contains("Name") && titles.contains("Size") && titles.contains("Date"));
        assert!(buffer_line(&b, 3).contains("/.."));
        let status = buffer_line(&b, 18);
        assert!(
            status.contains("File:") && status.trim_end().ends_with("F1=Help"),
            "{status:?}"
        );
    }

    #[test]
    fn function_keys_bar_follows_the_keymap() {
        let t = tempfile::tempdir().unwrap();
        let mut app = app_with(t.path(), 100, 20);
        let bar = buffer_line(&render(&app), 19);
        for label in [
            "2Rename", "3View", "4Edit", "5Copy", "6Move", "7Mkdir", "8Delete", "9Menu", "10Quit",
            "12FMenu",
        ] {
            assert!(bar.contains(label), "{label} missing in {bar:?}");
        }
        let (km, _) = crate::config::keymap::KeyMap::from_toml("[main]\nshow_tree = [\"F5\"]\n");
        app.keymap = km;
        assert!(buffer_line(&render(&app), 19).contains("5Tree"));
        app.settings.options.show_function_keys = false;
        let b = render(&app);
        assert!(!buffer_line(&b, 19).contains("Tree") && buffer_line(&b, 19).contains("File:"));
    }

    #[test]
    fn size_column_human_by_default_bytes_when_toggled() {
        let t = tempfile::tempdir().unwrap();
        std::fs::write(t.path().join("mid"), vec![0u8; 777_777]).unwrap();
        std::fs::write(t.path().join("huge"), b"").unwrap();
        let mut app = app_with(t.path(), 100, 20);
        let huge = std::ffi::OsStr::new("huge");
        for pane in &mut app.panes {
            for tab in &mut pane.tabs {
                tab.fs.entry_mut(huge).unwrap().update_size(12_345_678_901);
            }
        }
        let row = |app: &App, name: &str| {
            let b = render(app);
            (0..20)
                .map(|y| buffer_line(&b, y))
                .find(|l| l.contains(name))
                .unwrap()
        };
        assert!(
            row(&app, "mid").contains("│   760K│"),
            "{}",
            row(&app, "mid")
        );
        assert!(
            row(&app, "huge").contains("│    11G│"),
            "{}",
            row(&app, "huge")
        );
        app.settings.options.human_readable_sizes = false;
        // the column widens so the full byte count fits
        let huge_row = row(&app, "huge");
        assert!(huge_row.contains("│12345678901│"), "{huge_row}");
        assert!(
            row(&app, "mid").contains("│     777777│"),
            "{}",
            row(&app, "mid")
        );
        app.panes_cycle_view();
        assert!(
            row(&app, "huge").contains(" 12345678901 "),
            "{}",
            row(&app, "huge")
        );
    }

    #[test]
    fn each_pane_shows_its_sort_on_the_bottom_border() {
        let t = tempfile::tempdir().unwrap();
        let mut app = app_with(t.path(), 100, 20);
        let border = buffer_line(&render(&app), 17);
        assert_eq!(border.matches("Sort: name ↑").count(), 2, "{border:?}");
        assert!(
            border.starts_with('└') && border.ends_with('┘'),
            "{border:?}"
        );
        app.set_sort('D');
        let border = buffer_line(&render(&app), 17);
        let (left, right) = border.split_at(border.char_indices().nth(50).unwrap().0);
        assert!(left.contains("─ Sort: date ↓ ─"), "{left:?}");
        assert!(right.contains("─ Sort: name ↑ ─"), "{right:?}");
        // two panes: nothing in the status line
        assert!(!buffer_line(&render(&app), 18).contains("Sort:"));
        for (key, label) in [('s', "size ↑"), ('E', "ext ↓"), ('o', "none ↑")] {
            app.set_sort(key);
            assert!(buffer_line(&render(&app), 17).contains(&format!("Sort: {label}")));
        }
    }

    #[test]
    fn full_view_shows_the_sort_in_the_status_line() {
        let t = tempfile::tempdir().unwrap();
        let mut app = app_with(t.path(), 100, 20);
        app.set_sort('S');
        app.panes_cycle_view();
        let status = buffer_line(&render(&app), 18);
        assert!(status.contains("Sort: size ↓"), "{status:?}");
        assert!(
            status.contains("File:") && status.trim_end().ends_with("F1=Help"),
            "{status:?}"
        );
    }

    #[test]
    fn cursor_row_uses_cursor_style() {
        let t = tempfile::tempdir().unwrap();
        let app = app_with(t.path(), 100, 20);
        let b = render(&app);
        assert_eq!(colors(&b, 5, 3), app.theme.get(ColorItem::Cursor));
        assert_ne!(colors(&b, 55, 3), app.theme.get(ColorItem::Cursor)); // other pane not focused
    }

    #[test]
    fn selection_status_text() {
        let t = tempfile::tempdir().unwrap();
        let mut app = app_with(t.path(), 100, 20);
        app.select_glob("*.txt", true);
        assert!(buffer_line(&render(&app), 18).contains("bytes in 1 files"));
    }

    #[test]
    fn full_mode_shows_perms_and_owner() {
        let t = tempfile::tempdir().unwrap();
        let mut app = app_with(t.path(), 100, 20);
        app.panes_cycle_view();
        let row = buffer_line(&render(&app), 2);
        assert!(
            row.contains("rw")
                && row.contains(&crate::fs::format::owner2str(
                    nix::unistd::getuid().as_raw()
                )),
            "{row:?}"
        );
    }

    #[test]
    fn wide_names_keep_columns() {
        let t = tempfile::tempdir().unwrap();
        std::fs::write(t.path().join("世界世界世界世界世界世界世界世界.txt"), b"x").unwrap();
        let app = app_with(t.path(), 100, 20);
        let b = render(&app);
        let col1 = 50 - 14 - 8;
        for y in 3..10 {
            let sym = b[(col1, y)].symbol();
            assert!(sym == "│" || sym == " ", "row {y}: {sym:?}");
        }
    }

    #[test]
    fn non_utf8_names_render() {
        use std::os::unix::ffi::OsStrExt;
        let t = tempfile::tempdir().unwrap();
        std::fs::write(
            t.path().join(std::ffi::OsStr::from_bytes(b"bad\xffname")),
            b"",
        )
        .unwrap();
        let app = app_with(t.path(), 100, 20);
        let b = render(&app);
        assert!((3..19).any(|y| buffer_line(&b, y).contains("bad\u{fffd}name")));
    }

    #[test]
    fn tiny_and_huge_areas_do_not_panic() {
        let t = tempfile::tempdir().unwrap();
        let mut app = app_with(t.path(), 100, 20);
        for (w, h) in [(66, 5), (66, 3), (200, 60), (1, 1), (66, 2)] {
            app.resize(w, h);
            render(&app);
            app.panes_cycle_view();
            render(&app);
            app.panes_cycle_view();
        }
        let _ = Path::new("/");
    }
}
#[cfg(test)]
mod preview {
    #[test]
    #[ignore]
    fn print_screen() {
        let t = tempfile::tempdir().unwrap();
        let mut app = crate::ui::testutil::test_app(t.path(), 100, 14);
        app.cursor_by(2);
        let r = ratatui::layout::Rect::new(0, 0, 100, 14);
        let mut b = ratatui::buffer::Buffer::empty(r);
        super::draw_app(&app, r, &mut b);
        for y in 0..14 {
            println!("{}", super::buffer_line(&b, y));
        }
    }
}
