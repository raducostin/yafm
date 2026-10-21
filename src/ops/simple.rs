//! Single-step file operations from lfm's utils.py: make_dir, touch_file,
//! rename_file, link_create, link_edit, copy_bulk, backup_file, get_dir_size,
//! plus the destination check of `__copymove_helper` (actions.py).

use std::collections::HashSet;
use std::fs::FileTimes;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::SystemTime;

use crate::fs::path::get_norm_path;

pub fn make_dir(path: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(path)
}

/// Creates the file if needed and sets its access and modification times to now.
pub fn touch_file(path: &Path) -> std::io::Result<()> {
    let f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    let now = SystemTime::now();
    f.set_times(FileTimes::new().set_accessed(now).set_modified(now))
}

pub fn rename_file(src: &Path, dest: &Path) -> std::io::Result<()> {
    std::fs::rename(src, dest)
}

pub fn link_create(linkname: &Path, pointto: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(pointto, linkname)
}

pub fn link_edit(linkname: &Path, pointto: &Path) -> std::io::Result<()> {
    std::fs::remove_file(linkname)?;
    std::os::unix::fs::symlink(pointto, linkname)
}

/// Copies times (and, best effort, ownership) from `src` metadata to `dest`, not following links.
pub(crate) fn copy_times_and_owner(meta: &std::fs::Metadata, dest: &Path) {
    let _ = std::os::unix::fs::lchown(dest, Some(meta.uid()), Some(meta.gid()));
    if !meta.file_type().is_symlink() {
        if let Ok(f) = std::fs::File::open(dest) {
            let mut times = FileTimes::new();
            if let Ok(a) = meta.accessed() {
                times = times.set_accessed(a);
            }
            if let Ok(m) = meta.modified() {
                times = times.set_modified(m);
            }
            let _ = f.set_times(times);
        }
    }
}

/// Like Python `copy2(follow_symlinks=False)` for a symlink or a regular file.
/// Other types (FIFOs, devices, sockets) are refused: opening a FIFO would block.
pub(crate) fn copy_file_or_link(src: &Path, dest: &Path) -> std::io::Result<()> {
    let meta = std::fs::symlink_metadata(src)?;
    if meta.file_type().is_symlink() {
        std::os::unix::fs::symlink(std::fs::read_link(src)?, dest)
    } else if meta.is_file() {
        std::fs::copy(src, dest)?;
        copy_times_and_owner(&meta, dest);
        Ok(())
    } else {
        Err(std::io::Error::other(format!(
            "Cannot copy special file: '{}'",
            src.display()
        )))
    }
}

/// A unique hidden name next to `dest`, for copy-then-rename replacement.
pub(crate) fn temp_sibling(dest: &Path) -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let name = dest
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    dest.with_file_name(format!(".{name}.yafm-tmp-{}-{n}", std::process::id()))
}

/// Removes a file, symlink or whole directory tree.
pub(crate) fn remove_any(path: &Path) -> std::io::Result<()> {
    if std::fs::symlink_metadata(path)?.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
}

/// True when both paths name the same inode (intermediate symlinks resolved, the last one not).
pub(crate) fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::symlink_metadata(a), std::fs::symlink_metadata(b)) {
        (Ok(x), Ok(y)) => (x.dev(), x.ino()) == (y.dev(), y.ino()),
        _ => false,
    }
}

/// Like Python `copytree(symlinks=True)` for dirs and `copy2` for files; `dest` must not exist.
pub fn copy_bulk(src: &Path, dest: &Path) -> std::io::Result<()> {
    let meta = std::fs::symlink_metadata(src)?;
    if !meta.is_dir() {
        return copy_file_or_link(src, dest);
    }
    std::fs::create_dir(dest)?;
    for de in std::fs::read_dir(src)? {
        let de = de?;
        copy_bulk(&de.path(), &dest.join(de.file_name()))?;
    }
    std::fs::set_permissions(dest, meta.permissions())?;
    copy_times_and_owner(&meta, dest);
    Ok(())
}

pub fn backup_file(src: &Path, ext: &str) -> Result<(), String> {
    let mut dest = src.as_os_str().to_os_string();
    dest.push(ext);
    let dest = PathBuf::from(dest);
    if std::fs::symlink_metadata(&dest).is_ok() {
        return Err(format!(
            "Cannot backup file:\n\"{}\" already exists",
            dest.display()
        ));
    }
    copy_bulk(src, &dest).map_err(|e| format!("Cannot backup file:\n{e}"))
}

/// Apparent size of everything below `path`, like `du -sb`: symlinks are not
/// followed, hard-linked inodes count once, directories themselves count 0. `None` if `path` can't be stat'ed.
pub fn dir_size(path: &Path) -> Option<u64> {
    fn walk(path: &Path, seen: &mut HashSet<(u64, u64)>) -> u64 {
        let Ok(meta) = std::fs::symlink_metadata(path) else {
            return 0;
        };
        if !seen.insert((meta.dev(), meta.ino())) {
            return 0;
        }
        // GNU du (coreutils 9) counts directories themselves as 0 with --apparent-size.
        let mut total = if meta.is_dir() { 0 } else { meta.len() };
        if meta.is_dir() {
            if let Ok(rd) = std::fs::read_dir(path) {
                total += rd
                    .filter_map(Result::ok)
                    .map(|de| walk(&de.path(), seen))
                    .sum::<u64>();
            }
        }
        total
    }
    std::fs::symlink_metadata(path).ok()?;
    Some(walk(path, &mut HashSet::new()))
}

/// The destination typed in the copy/move dialog, normalized; many items need a directory.
pub fn resolve_destination(
    input: &str,
    basedir: &Path,
    nfiles: usize,
    act: &str,
) -> Result<PathBuf, String> {
    let dest = get_norm_path(input, basedir);
    if !dest.is_dir() && nfiles > 1 {
        return Err(format!(
            "Cannot {act} files\nTried to {act} many items to one file name"
        ));
    }
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn simple_ops() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path();
        make_dir(&d.join("x/y")).unwrap();
        assert!(d.join("x/y").is_dir());
        touch_file(&d.join("x/t")).unwrap();
        assert!(d.join("x/t").exists());
        rename_file(&d.join("x/t"), &d.join("x/u")).unwrap();
        assert!(d.join("x/u").exists());
        link_create(&d.join("l"), Path::new("x/u")).unwrap();
        link_edit(&d.join("l"), Path::new("x/y")).unwrap();
        assert_eq!(
            std::fs::read_link(d.join("l")).unwrap(),
            PathBuf::from("x/y")
        );
        assert!(make_dir(&d.join("x/u/z")).is_err());
    }

    #[test]
    fn touch_updates_mtime() {
        use std::os::unix::fs::MetadataExt;
        let t = tempfile::tempdir().unwrap();
        let f = t.path().join("f");
        std::fs::write(&f, b"keep").unwrap();
        let old = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000);
        std::fs::File::options()
            .write(true)
            .open(&f)
            .unwrap()
            .set_modified(old)
            .unwrap();
        touch_file(&f).unwrap();
        assert!(std::fs::metadata(&f).unwrap().mtime() > 1_000_000);
        assert_eq!(std::fs::read(&f).unwrap(), b"keep");
    }

    #[test]
    fn copy_bulk_keeps_symlinks() {
        let t = tree();
        let d = t.path();
        std::os::unix::fs::symlink("f1", d.join("a/l1")).unwrap();
        copy_bulk(&d.join("a"), &d.join("c")).unwrap();
        assert!(d.join("c/sub/f2").is_file());
        assert_eq!(
            std::fs::read_link(d.join("c/l1")).unwrap(),
            PathBuf::from("f1")
        );
    }

    #[test]
    fn copy_bulk_rejects_fifo_instead_of_hanging() {
        let t = tree();
        let d = t.path().to_path_buf();
        nix::unistd::mkfifo(&d.join("a/pipe"), nix::sys::stat::Mode::S_IRWXU).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let (src, dst) = (d.join("a"), d.join("c"));
        std::thread::spawn(move || tx.send(copy_bulk(&src, &dst)).unwrap());
        let res = rx.recv_timeout(std::time::Duration::from_secs(3));
        if res.is_err() {
            // unblock the hung reader so the test process can exit
            let _ = std::fs::OpenOptions::new()
                .write(true)
                .open(d.join("a/pipe"));
        }
        let err = res.expect("copy_bulk hung on a FIFO").unwrap_err();
        assert!(err.to_string().contains("special file"), "{err}");
    }

    #[test]
    fn backup() {
        let t = tempfile::tempdir().unwrap();
        let f = t.path().join("f.txt");
        std::fs::write(&f, b"1").unwrap();
        backup_file(&f, ".bak").unwrap();
        assert_eq!(std::fs::read(t.path().join("f.txt.bak")).unwrap(), b"1");
        let e = backup_file(&f, ".bak").unwrap_err();
        assert!(
            e.starts_with("Cannot backup file:\n\"") && e.ends_with("f.txt.bak\" already exists"),
            "{e}"
        );
    }

    #[test]
    fn dir_size_matches_du_and_skips_links() {
        let t = tree();
        let d = t.path();
        let s = dir_size(&d.join("a")).unwrap();
        if let Ok(out) = std::process::Command::new("du")
            .arg("-sb")
            .arg(d.join("a"))
            .output()
        {
            let du: u64 = String::from_utf8_lossy(&out.stdout)
                .split_whitespace()
                .next()
                .unwrap()
                .parse()
                .unwrap();
            assert_eq!(s, du);
        }
        std::os::unix::fs::symlink("/usr", d.join("a/usr")).unwrap();
        assert!(dir_size(&d.join("a")).unwrap() < s + 4096);
        assert_eq!(dir_size(&d.join("nope")), None);
    }

    #[test]
    fn destination_rules() {
        let t = tree();
        let d = t.path();
        assert_eq!(resolve_destination("a", d, 2, "copy").unwrap(), d.join("a"));
        assert_eq!(
            resolve_destination("newname", d, 1, "copy").unwrap(),
            d.join("newname")
        );
        assert_eq!(
            resolve_destination("newname", d, 2, "move").unwrap_err(),
            "Cannot move files\nTried to move many items to one file name"
        );
    }
}
