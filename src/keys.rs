//! Key representation and parsing of key strings (`C-up`, `A-s`, `S-F1`, `spc`),
//! ported from lfm's `key_defs.py`.

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum KeyCode {
    Char(char),
    Up,
    Down,
    Left,
    Right,
    PageUp,
    PageDown,
    Home,
    End,
    Del,
    Ins,
    Backspace,
    Enter,
    Esc,
    Tab,
    BackTab,
    F(u8),
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Key {
    pub code: KeyCode,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyParseError(pub String);

impl std::fmt::Display for KeyParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid key definition \"{}\"", self.0)
    }
}

impl std::error::Error for KeyParseError {}

const NAMED_KEYS: &[(&str, KeyCode)] = &[
    ("up", KeyCode::Up),
    ("down", KeyCode::Down),
    ("left", KeyCode::Left),
    ("right", KeyCode::Right),
    ("pageup", KeyCode::PageUp),
    ("pagedown", KeyCode::PageDown),
    ("home", KeyCode::Home),
    ("end", KeyCode::End),
    ("del", KeyCode::Del),
    ("ins", KeyCode::Ins),
    ("backspace", KeyCode::Backspace),
    ("enter", KeyCode::Enter),
    ("spc", KeyCode::Char(' ')),
    ("esc", KeyCode::Esc),
    ("tab", KeyCode::Tab),
    ("btab", KeyCode::BackTab),
    ("backtab", KeyCode::BackTab),
];

impl Key {
    fn plain(code: KeyCode) -> Key {
        Key {
            code,
            ctrl: false,
            alt: false,
            shift: false,
        }
    }

    /// curses reported S-F1..S-F12 as F13..F24; keep a single spelling.
    fn normalized(mut self) -> Key {
        if let KeyCode::F(n) = self.code {
            if (13..=24).contains(&n) {
                self.code = KeyCode::F(n - 12);
                self.shift = true;
            }
        }
        self
    }
}

pub fn parse_key(s: &str) -> Result<Key, KeyParseError> {
    let err = || KeyParseError(s.to_string());
    let mut key = Key::plain(KeyCode::Char('\0'));
    let mut rest = s;
    loop {
        let mut chars = rest.chars();
        if let (Some(c), None) = (chars.next(), chars.next()) {
            if !(c.is_ascii_alphanumeric() || c.is_ascii_punctuation()) {
                return Err(err());
            }
            key.code = KeyCode::Char(if key.ctrl { c.to_ascii_lowercase() } else { c });
            return Ok(key);
        }
        match rest.get(..2) {
            Some("C-") if rest.len() > 2 => key.ctrl = true,
            Some("A-") if rest.len() > 2 => key.alt = true,
            Some("S-") if rest.len() > 2 => key.shift = true,
            _ => break,
        }
        rest = &rest[2..];
    }
    let name = rest.to_ascii_lowercase();
    if let Some((_, code)) = NAMED_KEYS.iter().find(|(n, _)| *n == name) {
        key.code = *code;
        return Ok(key);
    }
    match name.strip_prefix('f').and_then(|n| n.parse::<u8>().ok()) {
        Some(n) if (1..=24).contains(&n) => {
            key.code = KeyCode::F(n);
            Ok(key.normalized())
        }
        _ => Err(err()),
    }
}

impl std::fmt::Display for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.ctrl {
            f.write_str("C-")?;
        }
        if self.alt {
            f.write_str("A-")?;
        }
        if self.shift {
            f.write_str("S-")?;
        }
        match self.code {
            KeyCode::F(n) => write!(f, "F{n}"),
            KeyCode::Char(' ') => f.write_str("spc"),
            KeyCode::Char(c) => write!(f, "{c}"),
            KeyCode::BackTab => f.write_str("backtab"),
            code => {
                let (name, _) = NAMED_KEYS
                    .iter()
                    .find(|(_, c)| *c == code)
                    .expect("all named keys listed");
                f.write_str(name)
            }
        }
    }
}

pub fn from_crossterm(ev: &crossterm::event::KeyEvent) -> Key {
    use crossterm::event::{KeyCode as C, KeyModifiers as M};
    let code = match ev.code {
        C::Char(c) => KeyCode::Char(c),
        C::Up => KeyCode::Up,
        C::Down => KeyCode::Down,
        C::Left => KeyCode::Left,
        C::Right => KeyCode::Right,
        C::PageUp => KeyCode::PageUp,
        C::PageDown => KeyCode::PageDown,
        C::Home => KeyCode::Home,
        C::End => KeyCode::End,
        C::Delete => KeyCode::Del,
        C::Insert => KeyCode::Ins,
        C::Backspace => KeyCode::Backspace,
        C::Enter => KeyCode::Enter,
        C::Esc => KeyCode::Esc,
        C::Tab => KeyCode::Tab,
        C::BackTab => KeyCode::BackTab,
        C::F(n) => KeyCode::F(n),
        // Keys lfm never bound (media keys, caps lock, ...) map to an unbindable key.
        _ => KeyCode::Char('\0'),
    };
    let mut key = Key {
        code,
        ctrl: ev.modifiers.contains(M::CONTROL),
        alt: ev.modifiers.contains(M::ALT),
        shift: ev.modifiers.contains(M::SHIFT),
    };
    match key.code {
        // Shift is already part of the character (and of BackTab itself).
        KeyCode::Char(c) => {
            key.shift = false;
            if key.ctrl {
                key.code = KeyCode::Char(c.to_ascii_lowercase());
            }
        }
        KeyCode::BackTab => key.shift = false,
        _ => {}
    }
    key.normalized()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_chars() {
        assert_eq!(
            parse_key("k").unwrap(),
            Key {
                code: KeyCode::Char('k'),
                ctrl: false,
                alt: false,
                shift: false
            }
        );
        assert_eq!(parse_key("B").unwrap().code, KeyCode::Char('B'));
        assert_eq!(parse_key("=").unwrap().code, KeyCode::Char('='));
    }

    #[test]
    fn parses_named_and_modifiers() {
        assert_eq!(parse_key("spc").unwrap().code, KeyCode::Char(' '));
        assert_eq!(parse_key("PageUp").unwrap().code, KeyCode::PageUp);
        let k = parse_key("C-up").unwrap();
        assert!(k.ctrl && k.code == KeyCode::Up);
        let k = parse_key("A-s").unwrap();
        assert!(k.alt && k.code == KeyCode::Char('s'));
        let k = parse_key("C-A-pagedown").unwrap();
        assert!(k.ctrl && k.alt && k.code == KeyCode::PageDown);
        assert_eq!(parse_key("C-D").unwrap(), parse_key("C-d").unwrap());
    }

    #[test]
    fn f13_is_shift_f1() {
        assert_eq!(parse_key("F13").unwrap(), parse_key("S-F1").unwrap());
    }

    #[test]
    fn rejects_garbage() {
        for s in ["", "ñ", "foo", "f25", "X-up"] {
            assert!(parse_key(s).is_err(), "{s}");
        }
    }

    #[test]
    fn display_round_trips() {
        for s in [
            "k",
            "B",
            "=",
            "spc",
            "up",
            "C-up",
            "A-s",
            "C-q",
            "F5",
            "S-F1",
            "C-A-pagedown",
            "ins",
            "tab",
        ] {
            let k = parse_key(s).unwrap();
            assert_eq!(parse_key(&k.to_string()).unwrap(), k, "{s}");
        }
        assert_eq!(parse_key("f5").unwrap().to_string(), "F5");
    }

    #[test]
    fn crossterm_shifted_letter_matches_parse() {
        use crossterm::event::{KeyCode as C, KeyEvent, KeyModifiers as M};
        assert_eq!(
            from_crossterm(&KeyEvent::new(C::Char('B'), M::SHIFT)),
            parse_key("B").unwrap()
        );
        assert_eq!(
            from_crossterm(&KeyEvent::new(C::Char('d'), M::CONTROL)),
            parse_key("C-d").unwrap()
        );
        assert_eq!(
            from_crossterm(&KeyEvent::new(C::Backspace, M::NONE)),
            parse_key("backspace").unwrap()
        );
        assert_eq!(
            from_crossterm(&KeyEvent::new(C::F(5), M::NONE)),
            parse_key("F5").unwrap()
        );
    }
}
