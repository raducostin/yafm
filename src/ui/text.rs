//! Text helpers for the screen: display-width wrapping (`text2wrap`), word
//! steps for the entry line and scrollbar geometry. Ported from lfm's utils.py
//! and `display_scrollbar` in ui_widgets.py.

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub fn width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

fn char_width(c: char) -> usize {
    UnicodeWidthChar::width(c).unwrap_or(0)
}

/// Fits `text` into `max_width` columns. Short text is padded (left-justified
/// when `ljust`, padded only if `fill`; right-justified otherwise); long text
/// keeps its start (`start_pct` of the width) and end around a `~`.
pub fn text2wrap(text: &str, max_width: usize, ljust: bool, start_pct: f32, fill: bool) -> String {
    let w = width(text);
    if w <= max_width {
        let pad = " ".repeat(max_width - w);
        return match (ljust, fill) {
            (true, true) => format!("{text}{pad}"),
            (true, false) => text.to_string(),
            (false, _) => format!("{pad}{text}"),
        };
    }
    let until = (max_width as f32 * start_pct) as usize;
    let mut head = String::new();
    for c in text.chars() {
        if width(&head) + char_width(c) > until {
            break;
        }
        head.push(c);
    }
    head.push('~');
    let mut tail: Vec<char> = Vec::new();
    let mut used = width(&head);
    for c in text.chars().rev() {
        if used + char_width(c) > max_width {
            break;
        }
        used += char_width(c);
        tail.push(c);
    }
    let mut out = head;
    out.extend(tail.iter().rev());
    out.push_str(&" ".repeat(max_width.saturating_sub(width(&out))));
    out
}

fn is_step(c: char) -> bool {
    c.is_ascii_whitespace() || c.is_ascii_punctuation()
}

/// Start of the previous word (lfm's `prev_step`), in char indices.
pub fn prev_step(text: &str, pos: usize) -> usize {
    let chars: Vec<char> = text.chars().collect();
    let mut pos = pos.saturating_sub(2);
    while pos > 0 && !chars.get(pos).copied().is_some_and(is_step) {
        pos -= 1;
    }
    if pos > 0 {
        pos + 1
    } else {
        0
    }
}

/// Start of the next word (lfm's `next_step`), in char indices.
pub fn next_step(text: &str, pos: usize) -> usize {
    let chars: Vec<char> = text.chars().collect();
    let mut pos = pos + 1;
    while pos < chars.len() && !is_step(chars[pos]) {
        pos += 1;
    }
    if pos < chars.len() {
        pos + 1
    } else {
        chars.len()
    }
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum ScrollCell {
    Line,
    Thumb,
    Up,
    Down,
}

/// Cells of a vertical scrollbar of height `h` for `n` items, cursor `i`,
/// first visible `a`; `None` when everything fits.
pub fn scrollbar(h: usize, n: usize, i: usize, a: usize) -> Option<Vec<ScrollCell>> {
    if n <= h || h == 0 {
        return None;
    }
    let ss = (h * h / n).max(1);
    let y = ((i / h) * h * h / n).min(h - ss);
    let mut cells = vec![ScrollCell::Line; h];
    for c in cells.iter_mut().skip(y).take(ss) {
        *c = ScrollCell::Thumb;
    }
    if a != 0 {
        cells[0] = ScrollCell::Up;
        if ss == 1 && y == 0 && h > 1 {
            cells[1] = ScrollCell::Thumb;
        }
    }
    if n > a + h {
        cells[h - 1] = ScrollCell::Down;
        if ss == 1 && y == h - 1 && h > 1 {
            cells[h - 2] = ScrollCell::Thumb;
        }
    }
    Some(cells)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_short_and_long() {
        assert_eq!(text2wrap("abc", 6, true, 0.66, true), "abc   ");
        assert_eq!(text2wrap("abc", 6, true, 0.66, false), "abc");
        assert_eq!(text2wrap("abc", 6, false, 0.66, true), "   abc");
        assert_eq!(text2wrap("abcdefghij", 6, true, 0.5, true), "abc~ij");
        assert_eq!(width(&text2wrap("abcdefghij", 7, true, 0.66, true)), 7);
    }

    #[test]
    fn wrap_wide_chars() {
        let s = text2wrap("世界世界世界", 7, true, 0.5, true);
        assert_eq!(width(&s), 7);
        assert!(s.contains('~'));
        assert_eq!(text2wrap("世界", 6, true, 0.66, true), "世界  ");
    }

    #[test]
    fn word_steps() {
        let t = "cd /usr/local bin";
        assert_eq!(next_step(t, 0), 3);
        assert_eq!(next_step(t, 3), 8);
        assert_eq!(prev_step(t, 13), 8);
        assert_eq!(prev_step(t, 1), 0);
        assert_eq!(next_step(t, 16), 17);
    }

    #[test]
    fn scrollbar_cells() {
        assert_eq!(scrollbar(10, 5, 0, 0), None);
        let s = scrollbar(4, 8, 0, 0).unwrap();
        assert_eq!(
            s,
            vec![
                ScrollCell::Thumb,
                ScrollCell::Thumb,
                ScrollCell::Line,
                ScrollCell::Down
            ]
        );
        let s = scrollbar(4, 8, 7, 4).unwrap();
        assert_eq!(
            s,
            vec![
                ScrollCell::Up,
                ScrollCell::Line,
                ScrollCell::Thumb,
                ScrollCell::Thumb
            ]
        );
    }
}
