//! System information used by the file tools: users and groups, `file -b`,
//! mount points, `df -h`, diffs, and chmod/chown/chgrp command lines.

use std::ffi::OsString;
use std::path::Path;
use std::process::Command;

use crate::config::settings::DiffType;
use crate::jobs::shell_quote;

fn q(p: impl AsRef<std::ffi::OsStr>) -> String {
    shell_quote(p.as_ref()).to_string_lossy().into_owned()
}

/// First field of each line of `getent DB`, else of the local file.
fn names_from(db: &str, file: &str) -> Vec<String> {
    let text = Command::new("getent")
        .arg(db)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .or_else(|| std::fs::read_to_string(file).ok())
        .unwrap_or_default();
    let mut v: Vec<String> = text
        .lines()
        .filter_map(|l| l.split(':').next())
        .filter(|n| !n.is_empty())
        .map(str::to_string)
        .collect();
    v.sort();
    v.dedup();
    v
}

pub fn users() -> Vec<String> {
    names_from("passwd", "/etc/passwd")
}

pub fn groups() -> Vec<String> {
    names_from("group", "/etc/group")
}

/// The GECOS field of `user` (lfm's get_user_fullname).
pub fn user_fullname(user: &str) -> String {
    match nix::unistd::User::from_name(user) {
        Ok(Some(u)) => u.gecos.to_string_lossy().into_owned(),
        _ => "<unknown user name>".to_string(),
    }
}

/// `file -b PATH`, or "no type information".
pub fn file_type_info(path: &Path) -> String {
    Command::new("file")
        .arg("-b")
        .arg(path)
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "no type information".to_string())
}

/// /proc/self/mounts escapes spaces and friends as octal (`\040`).
fn unescape_mount(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let octal = (b[i] == b'\\' && i + 3 < b.len())
            .then(|| {
                std::str::from_utf8(&b[i + 1..i + 4])
                    .ok()
                    .and_then(|o| u8::from_str_radix(o, 8).ok())
            })
            .flatten();
        match octal {
            Some(v) => {
                out.push(v);
                i += 4;
            }
            None => {
                out.push(b[i]);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// (mount point, device, fs type) of the longest mount point containing `path`.
pub fn mountpoint_for(path: &Path) -> (String, String, String) {
    let mounts = std::fs::read_to_string("/proc/self/mounts").unwrap_or_default();
    mounts
        .lines()
        .filter_map(|l| {
            let mut f = l.split_whitespace();
            Some((
                unescape_mount(f.next()?),
                unescape_mount(f.next()?),
                f.next()?.to_string(),
            ))
        })
        .filter(|(_, mp, _)| path.starts_with(mp))
        .max_by_key(|(_, mp, _)| mp.len())
        .map(|(dev, mp, ty)| (mp, dev, ty))
        .unwrap_or_else(|| ("/".into(), "<unknown>".into(), "<unknown>".into()))
}

/// `df -h` output.
pub fn filesystems_info() -> Result<String, String> {
    let o = Command::new("df")
        .arg("-h")
        .output()
        .map_err(|e| e.to_string())?;
    if o.status.success() || !o.stdout.is_empty() {
        Ok(String::from_utf8_lossy(&o.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&o.stderr).into_owned())
    }
}

/// Differences between a backup and the file; "" when identical.
pub fn diff_with_backup(old: &Path, new: &Path, kind: DiffType) -> Result<String, String> {
    let flag = match kind {
        DiffType::Unified => "-u",
        DiffType::Context => "-c",
        DiffType::Ndiff => {
            let a = std::fs::read_to_string(old).map_err(|e| e.to_string())?;
            let b = std::fs::read_to_string(new).map_err(|e| e.to_string())?;
            if a == b {
                return Ok(String::new());
            }
            let mut out = String::new();
            for change in similar::TextDiff::from_lines(&a, &b).iter_all_changes() {
                let sign = match change.tag() {
                    similar::ChangeTag::Delete => "- ",
                    similar::ChangeTag::Insert => "+ ",
                    similar::ChangeTag::Equal => "  ",
                };
                out.push_str(sign);
                out.push_str(change.value());
                if change.missing_newline() {
                    out.push('\n');
                }
            }
            return Ok(out);
        }
    };
    let o = Command::new("diff")
        .arg(flag)
        .arg("--label")
        .arg(old)
        .arg("--label")
        .arg(new)
        .arg(old)
        .arg(new)
        .output()
        .map_err(|e| e.to_string())?;
    match o.status.code() {
        Some(0) => Ok(String::new()),
        Some(1) => Ok(String::from_utf8_lossy(&o.stdout).into_owned()),
        _ => Err(String::from_utf8_lossy(&o.stderr).into_owned()),
    }
}

fn recursive_flag(recursive: bool) -> &'static str {
    if recursive {
        "-R "
    } else {
        ""
    }
}

pub fn chmod_cmd(mode: u32, recursive: bool, path: &Path) -> OsString {
    OsString::from(format!(
        "chmod {}0{mode:o} -- {}",
        recursive_flag(recursive),
        q(path)
    ))
}

pub fn chown_cmd(owner: &str, recursive: bool, path: &Path) -> OsString {
    OsString::from(format!(
        "chown {}{} -- {}",
        recursive_flag(recursive),
        q(owner),
        q(path)
    ))
}

pub fn chgrp_cmd(group: &str, recursive: bool, path: &Path) -> OsString {
    OsString::from(format!(
        "chgrp {}{} -- {}",
        recursive_flag(recursive),
        q(group),
        q(path)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_lists_and_commands() {
        assert!(users().contains(&"root".to_string()) && groups().contains(&"root".to_string()));
        assert_ne!(user_fullname("root"), "");
        assert_eq!(user_fullname("no-such-user-yafm"), "<unknown user name>");
        assert_eq!(
            chmod_cmd(0o755, true, Path::new("/a b")),
            OsString::from("chmod -R 0755 -- '/a b'")
        );
        assert_eq!(
            chmod_cmd(0o4644, false, Path::new("f")),
            OsString::from("chmod 04644 -- 'f'")
        );
        assert_eq!(
            chown_cmd("u", false, Path::new("-x")),
            OsString::from("chown 'u' -- '-x'")
        );
        assert_eq!(
            chgrp_cmd("g", true, Path::new("d")),
            OsString::from("chgrp -R 'g' -- 'd'")
        );
        assert_eq!(mountpoint_for(Path::new("/proc/self")).2, "proc");
        assert!(filesystems_info().unwrap().contains("Filesystem"));
        assert!(file_type_info(Path::new("/bin/sh")).len() > 3);
    }

    #[test]
    fn diffs() {
        let t = tempfile::tempdir().unwrap();
        let (o, n) = (t.path().join("f.bak"), t.path().join("f"));
        std::fs::write(&o, "a\nb\n").unwrap();
        std::fs::write(&n, "a\nc\n").unwrap();
        let u = diff_with_backup(&o, &n, DiffType::Unified).unwrap();
        assert!(
            u.contains("-b") && u.contains("+c") && u.contains("f.bak"),
            "{u}"
        );
        let c = diff_with_backup(&o, &n, DiffType::Context).unwrap();
        assert!(c.contains("! b") && c.contains("! c"), "{c}");
        let nd = diff_with_backup(&o, &n, DiffType::Ndiff).unwrap();
        assert!(
            nd.contains("- b") && nd.contains("+ c") && nd.contains("  a"),
            "{nd}"
        );
        std::fs::write(&n, "a\nb\n").unwrap();
        assert_eq!(diff_with_backup(&o, &n, DiffType::Unified).unwrap(), "");
        assert_eq!(diff_with_backup(&o, &n, DiffType::Ndiff).unwrap(), "");
    }

    #[test]
    fn mount_names_unescape() {
        assert_eq!(unescape_mount("/mnt/my\\040disk"), "/mnt/my disk");
        assert_eq!(unescape_mount("plain"), "plain");
        assert_eq!(unescape_mount("end\\"), "end\\");
    }
}
