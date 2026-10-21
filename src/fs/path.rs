//! Path helpers: `~` and `$VAR` expansion and lexical normalization
//! (Python `expanduser`, `expandvars`, `normpath`; lfm's `get_norm_path`).

use std::path::{Component, Path, PathBuf};

/// `~` / `~/x` from `$HOME` (else the passwd entry), `~user/x` from passwd;
/// anything else, or an unknown user, unchanged.
pub fn expand_user(p: &str) -> String {
    let Some(rest) = p.strip_prefix('~') else {
        return p.to_string();
    };
    let (user, tail) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
    let home = if user.is_empty() {
        std::env::var("HOME").ok().or_else(|| {
            nix::unistd::User::from_uid(nix::unistd::getuid())
                .ok()
                .flatten()
                .map(|u| u.dir.to_string_lossy().into_owned())
        })
    } else {
        nix::unistd::User::from_name(user)
            .ok()
            .flatten()
            .map(|u| u.dir.to_string_lossy().into_owned())
    };
    match home {
        Some(h) => match format!("{}{tail}", h.trim_end_matches('/')) {
            s if s.is_empty() => "/".to_string(),
            s => s,
        },
        None => p.to_string(),
    }
}

/// `$NAME` and `${NAME}` (NAME = letters, digits, `_`); unset variables stay as written.
pub fn expand_vars(p: &str) -> String {
    let mut out = String::new();
    let mut rest = p;
    while let Some(i) = rest.find('$') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        let (name, consumed) = if let Some(braced) = after.strip_prefix('{') {
            match braced.find('}') {
                Some(end) => (&braced[..end], end + 2),
                None => ("", 0),
            }
        } else {
            let end = after
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .unwrap_or(after.len());
            (&after[..end], end)
        };
        match std::env::var(name) {
            Ok(v) if !name.is_empty() => {
                out.push_str(&v);
                rest = &after[consumed..];
            }
            _ => {
                out.push('$');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Lexical normalization like Python `posixpath.normpath`.
pub fn normpath(p: &Path) -> PathBuf {
    let absolute = p.is_absolute();
    let mut parts: Vec<&std::ffi::OsStr> = Vec::new();
    for c in p.components() {
        match c {
            Component::Normal(n) => parts.push(n),
            Component::ParentDir => match parts.last() {
                Some(last) if *last != ".." => {
                    parts.pop();
                }
                _ if absolute => {}
                _ => parts.push(std::ffi::OsStr::new("..")),
            },
            Component::CurDir | Component::RootDir | Component::Prefix(_) => {}
        }
    }
    let mut out = PathBuf::from(if absolute { "/" } else { "" });
    out.extend(parts);
    if out.as_os_str().is_empty() {
        out.push(".");
    }
    out
}

pub fn get_norm_path(path: &str, basedir: &Path) -> PathBuf {
    let expanded = PathBuf::from(expand_vars(&expand_user(path)));
    normpath(&if expanded.is_absolute() {
        expanded
    } else {
        basedir.join(expanded)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn norm_paths() {
        std::env::set_var("YAFM_T", "/opt");
        assert_eq!(
            get_norm_path("../b/./c//d", Path::new("/x/y")),
            PathBuf::from("/x/b/c/d")
        );
        assert_eq!(
            get_norm_path("$YAFM_T/z", Path::new("/")),
            PathBuf::from("/opt/z")
        );
        assert_eq!(
            get_norm_path("${YAFM_T}/z", Path::new("/")),
            PathBuf::from("/opt/z")
        );
        assert_eq!(
            get_norm_path("$NOPE_NOT_SET/z", Path::new("/a")),
            PathBuf::from("/a/$NOPE_NOT_SET/z")
        );
        assert_eq!(normpath(Path::new("/../..")), PathBuf::from("/"));
        assert_eq!(normpath(Path::new("a/../..")), PathBuf::from(".."));
        assert!(get_norm_path("~", Path::new("/")).is_absolute());
        assert_eq!(expand_user("~root/x"), "/root/x");
        assert_eq!(expand_user("a~b"), "a~b");
    }
}
