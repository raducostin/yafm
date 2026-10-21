//! Everything a bulk operation will touch, collected up front with sizes and
//! errors. Port of `PathContents` in lfm's utils.py.

use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq)]
pub struct ContentEntry {
    pub path: PathBuf,
    pub size: u64,
    pub error: Option<String>,
}

pub struct Contents {
    /// Sorted by path bytes, so parents come before their children.
    pub entries: Vec<ContentEntry>,
    /// Sum of sizes, at least 1.
    pub total_size: u64,
}

pub fn collect_entries(files: &[PathBuf], basepath: &Path) -> Contents {
    let mut entries = Vec::new();
    for f in files {
        collect_one(f, basepath, &mut entries);
    }
    entries.sort_by(|a, b| {
        a.path
            .as_os_str()
            .as_bytes()
            .cmp(b.path.as_os_str().as_bytes())
    });
    entries.dedup_by(|a, b| a.path == b.path);
    let total_size = entries.iter().map(|e| e.size).sum::<u64>().max(1);
    Contents {
        entries,
        total_size,
    }
}

/// Adds `path` and, for a real directory, everything below it (symlinks are not followed).
fn collect_one(path: &Path, basepath: &Path, out: &mut Vec<ContentEntry>) {
    let err_entry = |e: &std::io::Error| ContentEntry {
        path: path.to_path_buf(),
        size: 0,
        error: Some(format_os_error(e, &partial(path, basepath))),
    };
    let meta = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) => return out.push(err_entry(&e)),
    };
    if meta.file_type().is_symlink() {
        return out.push(ContentEntry {
            path: path.to_path_buf(),
            size: 0,
            error: None,
        });
    }
    if !meta.is_dir() {
        return out.push(ContentEntry {
            path: path.to_path_buf(),
            size: meta.len(),
            error: None,
        });
    }
    match std::fs::read_dir(path) {
        Ok(rd) => {
            out.push(ContentEntry {
                path: path.to_path_buf(),
                size: meta.len(),
                error: None,
            });
            for de in rd {
                match de {
                    Ok(de) => collect_one(&de.path(), basepath, out),
                    Err(e) => out.push(err_entry(&e)),
                }
            }
        }
        Err(e) => out.push(err_entry(&e)),
    }
}

impl Contents {
    pub fn entries_rev(&self) -> Vec<ContentEntry> {
        self.entries.iter().rev().cloned().collect()
    }
}

/// `path` relative to `basepath`, without a leading "/", lossy.
pub fn partial(path: &Path, basepath: &Path) -> String {
    match path.strip_prefix(basepath) {
        Ok(rel) if !rel.as_os_str().is_empty() => rel.to_string_lossy().into_owned(),
        _ => path.to_string_lossy().into_owned(),
    }
}

/// Python's OSError text: `[Errno 13] Permission denied: 'x'`.
pub fn format_os_error(err: &std::io::Error, partial: &str) -> String {
    match err.raw_os_error() {
        Some(n) => format!(
            "[Errno {n}] {}: '{partial}'",
            nix::errno::Errno::from_raw(n).desc()
        ),
        None => format!("{err}: '{partial}'"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn tree() -> tempfile::TempDir {
        let t = tempfile::tempdir().unwrap();
        let d = t.path();
        std::fs::create_dir_all(d.join("a/sub")).unwrap();
        std::fs::write(d.join("a/f1"), vec![0u8; 10]).unwrap();
        std::fs::write(d.join("a/sub/f2"), vec![0u8; 20]).unwrap();
        std::fs::write(d.join("b"), vec![0u8; 5]).unwrap();
        std::os::unix::fs::symlink(d.join("a"), d.join("lnk")).unwrap();
        t
    }

    #[test]
    fn collects_recursively_parents_first() {
        let t = tree();
        let d = t.path();
        let c = collect_entries(&[d.join("a"), d.join("b"), d.join("lnk")], d);
        let rel: Vec<String> = c.entries.iter().map(|e| partial(&e.path, d)).collect();
        assert_eq!(rel, ["a", "a/f1", "a/sub", "a/sub/f2", "b", "lnk"]);
        assert_eq!(
            c.entries
                .iter()
                .find(|e| e.path.ends_with("lnk"))
                .unwrap()
                .size,
            0
        );
        assert!(c.total_size >= 35);
        assert_eq!(c.entries_rev()[0].path, d.join("lnk"));
    }

    #[test]
    fn unreadable_dir_is_reported() {
        if nix::unistd::geteuid().is_root() {
            return;
        }
        let t = tree();
        let d = t.path();
        std::fs::set_permissions(d.join("a/sub"), std::fs::Permissions::from_mode(0o000)).unwrap();
        let c = collect_entries(&[d.join("a")], d);
        std::fs::set_permissions(d.join("a/sub"), std::fs::Permissions::from_mode(0o755)).unwrap();
        let err = c
            .entries
            .iter()
            .find(|e| e.error.is_some())
            .expect("an error entry");
        assert!(
            err.error
                .as_ref()
                .unwrap()
                .starts_with("[Errno 13] Permission denied"),
            "{err:?}"
        );
    }

    #[test]
    fn empty_selection_total_is_one() {
        assert_eq!(collect_entries(&[], Path::new("/")).total_size, 1);
    }

    #[test]
    fn os_error_text() {
        let e = std::io::Error::from_raw_os_error(2);
        assert_eq!(
            format_os_error(&e, "x/y"),
            "[Errno 2] No such file or directory: 'x/y'"
        );
        assert_eq!(partial(Path::new("/base/x/y"), Path::new("/base/")), "x/y");
    }
}
