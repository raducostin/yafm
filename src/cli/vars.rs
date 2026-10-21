//! PowerCLI text processing: splitting `{...}` code chunks out of a command,
//! Python-style quotes, and lfm's `$` variables. Port of `PowerCLI.__replace_*`
//! in lfm's ui.py.

use chrono::NaiveDateTime;

/// A file time (`$tm`…) or date (`$dm`…), shown like Python's `str(datetime)`.
#[derive(Clone, Debug, PartialEq)]
pub struct Stamp {
    pub dt: NaiveDateTime,
    pub date_only: bool,
}

impl std::fmt::Display for Stamp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let pattern = if self.date_only {
            "%Y-%m-%d"
        } else {
            "%Y-%m-%d %H:%M:%S"
        };
        write!(f, "{}", self.dt.format(pattern))
    }
}

/// Everything a PowerCLI command can refer to for one file.
#[derive(Clone, Debug)]
pub struct CliVars {
    pub f: String,
    pub full: String,
    pub name_noext: String,
    pub ext: String,
    pub cur_dir: String,
    pub other_dir: String,
    pub selected: Vec<String>,
    pub all: Vec<String>,
    pub index: i64,
    pub tm: Stamp,
    pub ta: Stamp,
    pub tc: Stamp,
    pub tn: Stamp,
    pub bookmarks: Vec<(char, String)>,
}

#[derive(Debug, PartialEq)]
pub enum Piece {
    Text(String),
    Code(String),
}

/// Splits `cmd` into text and `{code}` chunks like lfm (each `{` closes at the
/// next `}`, no nesting). In code, `$name` becomes the Rhai variable `v_name`
/// and Python single-quoted strings become double-quoted.
pub fn split_chunks(cmd: &str) -> Result<Vec<Piece>, String> {
    let chars: Vec<char> = cmd.chars().collect();
    let mut pieces = Vec::new();
    let mut text = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '{' {
            text.push(chars[i]);
            i += 1;
            continue;
        }
        let Some(len) = closing_brace(&chars[i + 1..]) else {
            return Err(format!("{{ at {i} position has not ending }}"));
        };
        if !text.is_empty() {
            pieces.push(Piece::Text(std::mem::take(&mut text)));
        }
        let code: String = chars[i + 1..i + 1 + len].iter().collect();
        pieces.push(Piece::Code(python_quotes_to_rhai(&code.replace('$', "v_"))));
        i += len + 2;
    }
    if !text.is_empty() {
        pieces.push(Piece::Text(text));
    }
    Ok(pieces)
}

/// Length up to the `}` that closes a chunk: nested `{}` and braces inside
/// quoted strings are skipped (lfm stopped at the first `}`, which made Rhai
/// blocks impossible).
fn closing_brace(chars: &[char]) -> Option<usize> {
    let (mut depth, mut quote, mut escaped) = (0usize, None::<char>, false);
    for (k, &c) in chars.iter().enumerate() {
        match (quote, c) {
            (Some(_), _) if escaped => escaped = false,
            (Some(_), '\\') => escaped = true,
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '\'' | '"') => quote = Some(c),
            (None, '{') => depth += 1,
            (None, '}') if depth == 0 => return Some(k),
            (None, '}') => depth -= 1,
            _ => {}
        }
    }
    None
}

/// `'text'` → `"text"` (a `"` inside is escaped); `"text"` is left as is.
pub fn python_quotes_to_rhai(code: &str) -> String {
    let mut out = String::with_capacity(code.len());
    let mut quote: Option<char> = None;
    let mut chars = code.chars();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (None, '\'') => {
                quote = Some('\'');
                out.push('"');
            }
            (None, '"') => {
                quote = Some('"');
                out.push('"');
            }
            (Some(q), '\\') => match chars.next() {
                // \' needs no escape once the string is double-quoted
                Some('\'') if q == '\'' => out.push('\''),
                Some(n) => {
                    out.push('\\');
                    out.push(n);
                }
                None => out.push('\\'),
            },
            (Some('\''), '\'') => {
                quote = None;
                out.push('"');
            }
            (Some('\''), '"') => out.push_str("\\\""),
            (Some('"'), '"') => {
                quote = None;
                out.push('"');
            }
            _ => out.push(c),
        }
    }
    out
}

/// Each name in POSIX single quotes (`'` → `'\''`), space separated, so no
/// name can break out (lfm used plain double quotes).
fn quoted_list(names: &[String]) -> String {
    names
        .iter()
        .map(|n| format!("'{}'", n.replace('\'', "'\\''")))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Replaces `$f`, `$v`, `$F`, … in lfm's order (so `$F` and `$f` never clash).
pub fn replace_variables(cmd: &str, v: &CliVars) -> String {
    let date = |s: &Stamp| {
        Stamp {
            dt: s.dt,
            date_only: true,
        }
        .to_string()
    };
    let mut table: Vec<(String, String)> = vec![
        ("f".into(), v.f.clone()),
        ("v".into(), v.f.clone()),
        ("F".into(), v.full.clone()),
        ("E".into(), v.name_noext.clone()),
        ("e".into(), v.ext.clone()),
        ("p".into(), v.cur_dir.clone()),
        ("o".into(), v.other_dir.clone()),
        ("s".into(), quoted_list(&v.selected)),
        ("a".into(), quoted_list(&v.all)),
        ("i".into(), v.index.to_string()),
        ("dm".into(), date(&v.tm)),
        ("da".into(), date(&v.ta)),
        ("dc".into(), date(&v.tc)),
        ("dn".into(), date(&v.tn)),
        ("tm".into(), v.tm.to_string()),
        ("ta".into(), v.ta.to_string()),
        ("tc".into(), v.tc.to_string()),
        ("tn".into(), v.tn.to_string()),
    ];
    table.extend(
        v.bookmarks
            .iter()
            .map(|(k, p)| (format!("b{k}"), p.clone())),
    );
    table.iter().fold(cmd.to_string(), |acc, (k, val)| {
        acc.replace(&format!("${k}"), val)
    })
}

#[cfg(test)]
pub(crate) fn sample() -> CliVars {
    let t = |s: &str| Stamp {
        dt: NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").unwrap(),
        date_only: false,
    };
    CliVars {
        f: "IMG1234.jpeg".into(),
        full: "/p/IMG1234.jpeg".into(),
        name_noext: "IMG1234".into(),
        ext: ".jpeg".into(),
        cur_dir: "/p".into(),
        other_dir: "/o".into(),
        selected: vec!["a b".into(), "IMG1234.jpeg".into()],
        all: vec!["a b".into(), "IMG1234.jpeg".into(), "z".into()],
        index: 13,
        tm: t("2010-07-22 19:43:22"),
        ta: t("2010-07-23 08:00:00"),
        tc: t("2010-07-24 09:30:00"),
        tn: t("2026-10-20 12:00:00"),
        bookmarks: vec![('3', "/bm three".into())],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunks() {
        assert_eq!(
            split_chunks("mv $f {$f.lower()}").unwrap(),
            vec![
                Piece::Text("mv $f ".into()),
                Piece::Code("v_f.lower()".into())
            ]
        );
        assert_eq!(
            split_chunks("a{1}b{$E}c").unwrap(),
            vec![
                Piece::Text("a".into()),
                Piece::Code("1".into()),
                Piece::Text("b".into()),
                Piece::Code("v_E".into()),
                Piece::Text("c".into())
            ]
        );
        assert_eq!(
            split_chunks("plain").unwrap(),
            vec![Piece::Text("plain".into())]
        );
        assert_eq!(
            split_chunks("{if x { 'a}' } else { 1 }}!").unwrap(),
            vec![
                Piece::Code(r#"if x { "a}" } else { 1 }"#.into()),
                Piece::Text("!".into())
            ]
        );
        assert_eq!(
            split_chunks("x {oops").unwrap_err(),
            "{ at 2 position has not ending }"
        );
    }

    #[test]
    fn quotes() {
        assert_eq!(
            python_quotes_to_rhai("x.replace('.bak', '.orig')"),
            r#"x.replace(".bak", ".orig")"#
        );
        assert_eq!(python_quotes_to_rhai(r#"'it"s'"#), r#""it\"s""#);
        assert_eq!(python_quotes_to_rhai(r#""x'y""#), r#""x'y""#);
    }

    #[test]
    fn variables_in_lfm_order() {
        let v = sample();
        assert_eq!(
            replace_variables("$f|$v|$F|$E|$e", &v),
            "IMG1234.jpeg|IMG1234.jpeg|/p/IMG1234.jpeg|IMG1234|.jpeg"
        );
        assert_eq!(replace_variables("$p $o $i", &v), "/p /o 13");
        assert_eq!(replace_variables("$s", &v), "'a b' 'IMG1234.jpeg'");
        assert_eq!(replace_variables("$a", &v), "'a b' 'IMG1234.jpeg' 'z'");
        assert_eq!(replace_variables("$tm", &v), "2010-07-22 19:43:22");
        let mut d = v.clone();
        d.tm.date_only = true;
        assert_eq!(replace_variables("$tm", &d), "2010-07-22");
        assert_eq!(replace_variables("$dm/$dn", &v), "2010-07-22/2026-10-20");
        assert_eq!(replace_variables("cd \"$b3\"", &v), "cd \"/bm three\"");
    }

    #[test]
    fn lists_are_shell_quoted() {
        let mut v = sample();
        v.selected = vec!["q\"u".into(), "it's".into(), "x\"; touch PWNED; \"".into()];
        assert_eq!(
            replace_variables("$s", &v),
            r#"'q"u' 'it'\''s' 'x"; touch PWNED; "'"#
        );
        let (out, _) = crate::jobs::run_in_cli(
            std::ffi::OsStr::new(&replace_variables("printf '[%s]' $s", &v)),
            std::path::Path::new("/tmp"),
        )
        .unwrap();
        assert_eq!(out, "[q\"u][it's][x\"; touch PWNED; \"]");
    }

    #[test]
    fn escaped_quote_in_single_quoted_string() {
        assert_eq!(python_quotes_to_rhai(r"'it\'s'"), r#""it's""#);
    }
}
