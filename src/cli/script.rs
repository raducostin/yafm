//! The `{...}` expressions of PowerCLI, evaluated with Rhai. lfm used Python
//! `eval`; yafm registers Python-like string helpers so common expressions
//! read the same. Limits keep a runaway expression from hanging the UI.

use crate::cli::vars::CliVars;

use rhai::{Array, Dynamic, Engine, EvalAltResult, ImmutableString, Scope};

use crate::cli::vars::{replace_variables, split_chunks, Piece, Stamp};

type RResult<T> = Result<T, Box<EvalAltResult>>;

pub struct Script {
    engine: Engine,
}

/// Python's `str.title()`: first letter of each word upper, the rest lower.
fn title(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut start = true;
    for c in s.chars() {
        if c.is_alphabetic() {
            if start {
                out.extend(c.to_uppercase());
            } else {
                out.extend(c.to_lowercase());
            }
            start = false;
        } else {
            out.push(c);
            start = true;
        }
    }
    out
}

/// Python-style slice of chars; negative indices count from the end.
fn slice(s: &str, a: i64, b: i64) -> String {
    let chars: Vec<char> = s.chars().collect();
    let n = chars.len() as i64;
    let idx = |i: i64| if i < 0 { (n + i).max(0) } else { i.min(n) } as usize;
    let (a, b) = (idx(a), idx(b));
    if a >= b {
        String::new()
    } else {
        chars[a..b].iter().collect()
    }
}

fn dyn_text(d: &Dynamic) -> String {
    match d.clone().try_cast::<Stamp>() {
        Some(st) => st.to_string(),
        None => d.to_string(),
    }
}

/// Python `%` formatting subset: `%s`, `%d`, `%Nd`, `%0Nd`, `%N.Md`, `%%`.
fn fmt(template: &str, args: Array) -> RResult<String> {
    let mut out = String::new();
    let mut args = args.into_iter();
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        let mut spec = String::new();
        while let Some(&d) = chars.peek() {
            if d.is_ascii_digit() || d == '.' {
                spec.push(d);
                chars.next();
            } else {
                break;
            }
        }
        let conv = chars.next().ok_or("fmt: template ends with %")?;
        if conv == '%' {
            out.push('%');
            continue;
        }
        let arg = args.next().ok_or("fmt: not enough arguments")?;
        let (width_s, prec_s) = spec.split_once('.').unwrap_or((spec.as_str(), ""));
        let zero = width_s.starts_with('0');
        // Empty means 0; digits too long for i64 are as invalid as any huge width.
        let num = |s: &str| {
            if s.is_empty() {
                0
            } else {
                s.parse().unwrap_or(i64::MAX)
            }
        };
        let width = width_arg(num(width_s))?;
        let prec = width_arg(num(prec_s))?;
        let text = match conv {
            's' => dyn_text(&arg),
            'd' => {
                let n = if arg.is_int() {
                    arg.as_int().unwrap_or(0)
                } else {
                    dyn_text(&arg)
                        .trim()
                        .parse::<i64>()
                        .map_err(|_| "fmt: %d needs a number")?
                };
                let digits = zero_pad(&n.unsigned_abs().to_string(), prec);
                format!("{}{digits}", if n < 0 { "-" } else { "" })
            }
            other => return Err(format!("fmt: unsupported %{other}").into()),
        };
        let pad = width.saturating_sub(text.chars().count());
        if zero && conv == 'd' {
            let (sign, body) = text
                .strip_prefix('-')
                .map_or(("", text.as_str()), |b| ("-", b));
            out.push_str(&format!("{sign}{}{body}", "0".repeat(pad)));
        } else {
            out.push_str(&" ".repeat(pad));
            out.push_str(&text);
        }
    }
    Ok(out)
}

impl Script {
    pub fn new() -> Script {
        let mut e = Engine::new();
        e.set_max_operations(100_000);
        e.set_max_string_size(1_000_000);
        e.set_max_array_size(100_000);
        e.set_max_expr_depths(64, 32);
        // print/debug would write over the screen.
        e.on_print(|_| {});
        e.on_debug(|_, _, _| {});
        e.register_type_with_name::<Stamp>("Timestamp");
        e.register_fn("strftime", |s: &mut Stamp, f: ImmutableString| {
            strftime(s, &f)
        });
        e.register_fn("to_string", |s: &mut Stamp| s.to_string());
        e.register_fn("lower", |s: ImmutableString| s.to_lowercase());
        e.register_fn("upper", |s: ImmutableString| s.to_uppercase());
        e.register_fn("title", |s: ImmutableString| title(&s));
        e.register_fn("capitalize", |s: ImmutableString| {
            let mut c = s.chars();
            c.next().map_or(String::new(), |f| {
                f.to_uppercase()
                    .chain(c.flat_map(char::to_lowercase))
                    .collect()
            })
        });
        e.register_fn("strip", |s: ImmutableString| s.trim().to_string());
        // Returns a new string (Rhai's built-in `replace` edits in place and returns nothing).
        e.register_fn(
            "replace",
            |s: ImmutableString, a: ImmutableString, b: ImmutableString| -> RResult<String> {
                let hits = if a.is_empty() {
                    s.chars().count() + 1
                } else {
                    s.matches(a.as_str()).count()
                };
                check_size(s.len() + hits.saturating_mul(b.len()))?;
                Ok(s.replace(a.as_str(), &b))
            },
        );
        e.register_fn("startswith", |s: ImmutableString, p: ImmutableString| {
            s.starts_with(p.as_str())
        });
        e.register_fn("endswith", |s: ImmutableString, p: ImmutableString| {
            s.ends_with(p.as_str())
        });
        e.register_fn(
            "split",
            |s: ImmutableString, sep: ImmutableString| -> Array {
                s.split(sep.as_str())
                    .map(|x| Dynamic::from(x.to_string()))
                    .collect()
            },
        );
        e.register_fn("slice", |s: ImmutableString, a: i64, b: i64| {
            slice(&s, a, b)
        });
        e.register_fn("zfill", |s: ImmutableString, n: i64| -> RResult<String> {
            Ok(zero_pad(&s, width_arg(n)?))
        });
        e.register_fn("pad", |i: i64, n: i64| -> RResult<String> {
            Ok(match i {
                i if i < 0 => format!(
                    "-{}",
                    zero_pad(
                        &i.unsigned_abs().to_string(),
                        width_arg(n)?.saturating_sub(1)
                    )
                ),
                i => zero_pad(&i.to_string(), width_arg(n)?),
            })
        });
        e.register_fn(
            "re_sub",
            |s: ImmutableString, pat: ImmutableString, rep: ImmutableString| -> RResult<String> {
                let re = regex::Regex::new(&pat).map_err(|e| e.to_string())?;
                // A replacement may expand groups ($1…), so each match may add up to the whole input.
                let per_match = rep.len() + if rep.contains('$') { s.len() } else { 0 };
                check_size(s.len() + re.find_iter(&s).count().saturating_mul(per_match))?;
                Ok(re.replace_all(&s, rep.as_str()).into_owned())
            },
        );
        e.register_fn("fmt", fmt);
        Script { engine: e }
    }

    /// Evaluates one `{...}` chunk with the PowerCLI variables in scope.
    pub fn eval(&self, code: &str, v: &CliVars) -> Result<String, String> {
        let mut scope = Scope::new();
        let strs =
            |xs: &[String]| -> Array { xs.iter().map(|x| Dynamic::from(x.clone())).collect() };
        for (k, val) in [
            ("f", &v.f),
            ("v", &v.f),
            ("F", &v.full),
            ("E", &v.name_noext),
            ("e", &v.ext),
            ("p", &v.cur_dir),
            ("o", &v.other_dir),
        ] {
            scope.push(format!("v_{k}"), val.clone());
        }
        scope.push("v_s", strs(&v.selected));
        scope.push("v_a", strs(&v.all));
        scope.push("v_i", v.index);
        for (k, st) in [("tm", &v.tm), ("ta", &v.ta), ("tc", &v.tc), ("tn", &v.tn)] {
            scope.push(format!("v_{k}"), st.clone());
            let mut date = st.clone();
            date.date_only = true;
            scope.push(format!("v_d{}", &k[1..]), date);
        }
        for (k, p) in &v.bookmarks {
            scope.push(format!("v_b{k}"), p.clone());
        }
        if code.trim().is_empty() {
            return Err("empty expression {}".to_string());
        }
        let result: Dynamic = self
            .engine
            .eval_with_scope(&mut scope, code)
            .map_err(|e| shown_names(&e.to_string()))?;
        if result.is_unit() {
            return Err("the expression has no value".to_string());
        }
        if result.is_array() || result.is_map() {
            return Err("the expression gives a list; use .len() or pick an element".to_string());
        }
        Ok(dyn_text(&result))
    }
}

/// Rhai errors mention the internal `v_name` variables; show them as `$name`.
fn shown_names(msg: &str) -> String {
    let re = regex::Regex::new(r"\bv_").expect("valid regex");
    re.replace_all(msg, "$$").into_owned()
}

/// `s` left-padded with zeros to `width` chars (Rust's `{:0w$}` stops at 65535).
fn zero_pad(s: &str, width: usize) -> String {
    let n = s.chars().count();
    format!("{}{s}", "0".repeat(width.saturating_sub(n)))
}

/// Largest string an expression may build (also Rhai's own limit).
const MAX_STRING: usize = 1_000_000;

fn check_size(n: usize) -> RResult<()> {
    if n > MAX_STRING {
        Err(format!("result would be too large ({n} bytes)").into())
    } else {
        Ok(())
    }
}

/// A padding width for zfill/pad/fmt: negative counts as 0, too large is an error.
fn width_arg(n: i64) -> RResult<usize> {
    let w = n.max(0) as usize;
    check_size(w)?;
    Ok(w)
}

/// `strftime` that reports a bad format instead of panicking (chrono panics on
/// `%Q`, a lone `%`, …).
fn strftime(s: &Stamp, f: &str) -> RResult<String> {
    use chrono::format::{Item, StrftimeItems};
    use std::fmt::Write;
    let items: Vec<Item> = StrftimeItems::new(f).collect();
    let bad = || -> Box<EvalAltResult> { format!("invalid date format \"{f}\"").into() };
    if items.iter().any(|i| matches!(i, Item::Error)) {
        return Err(bad());
    }
    let mut out = String::new();
    write!(out, "{}", s.dt.format_with_items(items.into_iter())).map_err(|_| bad())?;
    Ok(out)
}

impl Default for Script {
    fn default() -> Script {
        Script::new()
    }
}

/// lfm's order: evaluate every `{...}` chunk, then replace the `$` variables.
pub fn expand(cmd: &str, v: &CliVars, script: &Script) -> Result<String, String> {
    let mut out = String::new();
    for piece in split_chunks(cmd)? {
        match piece {
            Piece::Text(t) => out.push_str(&t),
            Piece::Code(c) => out.push_str(&script.eval(&c, v)?),
        }
    }
    Ok(replace_variables(&out, v))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::vars::sample;

    fn run(cmd: &str) -> Result<String, String> {
        expand(cmd, &sample(), &Script::new())
    }

    #[test]
    fn readme_examples() {
        let mut v = sample();
        v.f = "FiLeFOO.bak".into();
        assert_eq!(
            expand(
                "mv $f {$f.lower().replace('.bak', '.orig')}",
                &v,
                &Script::new()
            )
            .unwrap(),
            "mv FiLeFOO.bak filefoo.orig"
        );
        assert_eq!(
            run(r#"cp $f "$o/{fmt("%2.2d. %s - %s", [$i, $tm.strftime("%Y%m%d%H%S"), $E.upper()])}.jpg""#).unwrap(),
            r#"cp IMG1234.jpeg "/o/13. 201007221922 - IMG1234.jpg""#
        );
    }

    #[test]
    fn helpers() {
        assert_eq!(run("{'hello world'.title()}").unwrap(), "Hello World");
        assert_eq!(run("{'hELLO'.capitalize()}").unwrap(), "Hello");
        assert_eq!(run("{'  x '.strip()}").unwrap(), "x");
        assert_eq!(run("{'7'.zfill(3)}").unwrap(), "007");
        assert_eq!(run("{$i.pad(4)}").unwrap(), "0013");
        assert_eq!(run("{'abcdef'.slice(1, 3)}").unwrap(), "bc");
        assert_eq!(run("{'abcdef'.slice(-2, 6)}").unwrap(), "ef");
        assert_eq!(run(r"{'a1b22'.re_sub('\\d+', '#')}").unwrap(), "a#b#");
        assert_eq!(run("{'a.b.c'.split('.').len()}").unwrap(), "3");
        assert_eq!(
            run("{if $f.endswith('.jpeg') { 'yes' } else { 'no' }}").unwrap(),
            "yes"
        );
        assert_eq!(run("{$f.startswith('IMG')}").unwrap(), "true");
        assert_eq!(run("{fmt('%05d|%s|%%', [42, 'x'])}").unwrap(), "00042|x|%");
        assert_eq!(run("{$tm}").unwrap(), "2010-07-22 19:43:22");
        assert_eq!(run("{$s.len()}").unwrap(), "2");
        assert_eq!(run("{$b3}").unwrap(), "/bm three");
    }

    #[test]
    fn errors_and_limits() {
        assert!(run("{1 +}").is_err());
        assert!(run("{$f.nope()}").is_err());
        assert!(run("{'a'.re_sub('(', 'x')}").is_err());
        let start = std::time::Instant::now();
        assert!(run("{loop {}}").is_err());
        assert!(start.elapsed().as_secs() < 5);
        assert!(run("{let s = 'x'; loop { s += s; }}").is_err());
        assert!(run("x {unclosed").is_err());
    }

    #[test]
    fn bad_input_is_an_error_not_a_crash() {
        assert!(run("{$tm.strftime('%Q')}").is_err());
        assert!(run("{$tm.strftime('100%')}").is_err());
        assert_eq!(run("{'7'.zfill(65536)}").unwrap().len(), 65536); // past Rust's format-width limit
        assert!(run("{'7'.zfill(2000000)}").is_err());
        assert_eq!(run("{$i.pad(70000)}").unwrap().len(), 70000);
        assert!(run("{$i.pad(2000000)}").is_err());
        assert_eq!(run("{(-5).pad(4)}").unwrap(), "-005");
        assert!(run("{fmt('%99999999999999d', [1])}").is_err());
        assert!(run("{fmt('%99999999999999s', ['x'])}").is_err());
        assert_eq!(run("{'7'.zfill(3)}").unwrap(), "007");
    }

    #[test]
    fn replacement_blowup_is_refused_before_allocating() {
        let start = std::time::Instant::now();
        assert!(run("{let s = 'x'; for i in 0..14 { s += s; } s.replace('', s)}").is_err());
        assert!(run("{let s = 'x'; for i in 0..14 { s += s; } s.re_sub('', s)}").is_err());
        assert!(start.elapsed().as_secs() < 5);
        assert_eq!(run("{'aaa'.replace('a', 'bb')}").unwrap(), "bbbbbb");
        // ~16 GB if it were allocated; refused up front
        let start = std::time::Instant::now();
        assert!(run("{let s = 'x'; for i in 0..17 { s += s; } s.replace('', s)}").is_err());
        assert!(run("{let s = 'x'; for i in 0..17 { s += s; } s.re_sub('', s)}").is_err());
        assert!(start.elapsed().as_secs() < 5);
    }

    #[test]
    fn empty_or_valueless_chunks_are_errors() {
        assert!(run("find . -exec rm {} ;").is_err());
        assert!(run("x{let a = 1;}y").is_err());
        assert!(run("{$s}").is_err());
        assert_eq!(run("{$s.len()}").unwrap(), "2");
    }
}
