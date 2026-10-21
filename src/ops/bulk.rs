//! Copy, move and delete of selections, with lfm's confirm-all / overwrite-all
//! / skip / stop semantics (ProcessFuncCopyLoop and ProcessFuncDeleteLoop in
//! utils.py, copy_file/move_file/move_file2/delete_file in actions.py).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::ops::contents::{collect_entries, format_os_error, partial, ContentEntry};
use crate::ops::simple::{
    copy_bulk, copy_file_or_link, copy_times_and_owner, remove_any, same_file, temp_sibling,
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Answer {
    Yes,
    All,
    No,
    NoneAll,
    Stop,
}

pub trait Progress {
    /// Before each entry: display name (partial), its size, 1-based index, count, accumulated and total size.
    fn next(&mut self, item: &str, size: u64, index: usize, count: usize, acc: u64, total: u64);
    /// Only called when delete confirmation is on; Yes/All/No/Stop.
    fn confirm_delete(&mut self, item: &str) -> Answer;
    /// Only called when overwrite confirmation is on.
    fn confirm_overwrite(&mut self, dest: &Path) -> Answer;
    /// Already wrapped ("Cannot …").
    fn error(&mut self, msg: &str);
    /// Polled before each entry (Ctrl-C).
    fn should_stop(&mut self) -> bool;
}

#[derive(Debug, Default, PartialEq)]
pub struct OpReport {
    pub stopped: bool,
    pub done: Vec<PathBuf>,
    pub skipped: Vec<PathBuf>,
    pub errors: Vec<String>,
}

#[derive(Debug, PartialEq)]
pub enum CopyError {
    Exists(PathBuf),
    /// Source and destination are the same inode; copying would destroy it.
    SameFile(String),
    Failed(String),
}

/// Messages starting with "Cannot" pass as is, others get lfm's loop prefix.
fn wrap(title: &str, msg: &str) -> String {
    if msg.starts_with("Cannot") {
        msg.to_string()
    } else {
        format!("Cannot {}!\n{msg}", title.to_lowercase())
    }
}

fn report_error(report: &mut OpReport, p: &mut dyn Progress, title: &str, msg: &str) {
    let msg = wrap(title, msg);
    p.error(&msg);
    report.errors.push(msg);
}

/// Where each source path goes, decided once before the loop.
struct DestMap<'a> {
    dest: &'a Path,
    basepath: &'a Path,
    /// `None`: `dest` is an existing dir. `Some(root)`: the single selected item `root` becomes `dest`.
    root: Option<&'a Path>,
}

impl<'a> DestMap<'a> {
    fn new(
        files: &'a [PathBuf],
        basepath: &'a Path,
        dest: &'a Path,
        title: &str,
    ) -> Result<DestMap<'a>, String> {
        if dest.is_dir() {
            return Ok(DestMap {
                dest,
                basepath,
                root: None,
            });
        }
        match files {
            [one] => Ok(DestMap {
                dest,
                basepath,
                root: Some(one),
            }),
            _ => {
                let act = title.split(' ').next().unwrap_or("copy").to_lowercase();
                Err(format!(
                    "Cannot {act} files\nTried to {act} many items to one file name"
                ))
            }
        }
    }

    fn target(&self, path: &Path) -> PathBuf {
        let (base, from) = match self.root {
            Some(root) => (root, path),
            None => (self.basepath, path),
        };
        match from.strip_prefix(base) {
            Ok(rest) if rest.as_os_str().is_empty() => self.dest.to_path_buf(),
            Ok(rest) => self.dest.join(rest),
            Err(_) => self.dest.join(path.file_name().unwrap_or(path.as_os_str())),
        }
    }
}

fn os_failed(prefix: &str, e: &std::io::Error, partial: &str) -> String {
    format!("{prefix}\n{}", format_os_error(e, partial))
}

/// Copies one entry without recursing (directories are only created; their
/// metadata is applied by the caller once their contents are in place).
pub fn copy_one(src: &Path, dest: &Path, partial: &str, overwrite: bool) -> Result<(), CopyError> {
    let failed = |e: std::io::Error| CopyError::Failed(os_failed("Cannot copy file!", &e, partial));
    let dest_exists = std::fs::symlink_metadata(dest).is_ok();
    if dest_exists && same_file(src, dest) {
        return Err(CopyError::SameFile(format!(
            "Cannot copy file!\nSource and destination are the same file: '{partial}'"
        )));
    }
    if dest_exists && !overwrite {
        return Err(CopyError::Exists(dest.to_path_buf()));
    }
    let meta = std::fs::symlink_metadata(src).map_err(failed)?;
    let ft = meta.file_type();
    if ft.is_symlink() || ft.is_file() {
        if !dest_exists {
            return copy_file_or_link(src, dest).map_err(failed);
        }
        // Copy beside the old file first, so a failing source never costs the destination.
        let tmp = temp_sibling(dest);
        if let Err(e) = copy_file_or_link(src, &tmp).and_then(|_| std::fs::rename(&tmp, dest)) {
            let _ = remove_any(&tmp);
            return Err(failed(e));
        }
        Ok(())
    } else if ft.is_dir() {
        if dest_exists {
            match std::fs::remove_dir(dest) {
                Err(e) if e.raw_os_error() != Some(nix::libc::ENOTEMPTY) => return Err(failed(e)),
                _ => {}
            }
        }
        match std::fs::create_dir(dest) {
            Err(e) if e.kind() != std::io::ErrorKind::AlreadyExists => Err(failed(e)),
            _ => Ok(()),
        }
    } else {
        Err(CopyError::Failed(format!(
            "Cannot copy file!\nCannot copy special file: '{partial}'"
        )))
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Overwrite {
    Ask,
    All,
    None,
}

/// Resolves an existing destination: Some(true) overwrite, Some(false) skip, None stop.
fn decide_overwrite(policy: &mut Overwrite, target: &Path, p: &mut dyn Progress) -> Option<bool> {
    match *policy {
        Overwrite::All => Some(true),
        Overwrite::None => Some(false),
        Overwrite::Ask => match p.confirm_overwrite(target) {
            Answer::Yes => Some(true),
            Answer::All => {
                *policy = Overwrite::All;
                Some(true)
            }
            Answer::No => Some(false),
            Answer::NoneAll => {
                *policy = Overwrite::None;
                Some(false)
            }
            Answer::Stop => None,
        },
    }
}

fn copy_loop(
    files: &[PathBuf],
    basepath: &Path,
    dest: &Path,
    confirm_overwrite: bool,
    p: &mut dyn Progress,
    title: &str,
) -> OpReport {
    let mut report = OpReport::default();
    let map = match DestMap::new(files, basepath, dest, title) {
        Ok(m) => m,
        Err(msg) => {
            report_error(&mut report, p, title, &msg);
            return report;
        }
    };
    let contents = collect_entries(files, basepath);
    let count = contents.entries.len();
    let mut policy = if confirm_overwrite {
        Overwrite::Ask
    } else {
        Overwrite::All
    };
    let mut created_dirs = Vec::new();
    let mut acc = 0;
    let mut same_roots: Vec<PathBuf> = Vec::new();
    for (i, entry) in contents.entries.iter().enumerate() {
        if p.should_stop() {
            report.stopped = true;
            break;
        }
        if same_roots.iter().any(|r| entry.path.starts_with(r)) {
            continue;
        }
        let part = partial(&entry.path, basepath);
        acc += entry.size;
        p.next(&part, entry.size, i + 1, count, acc, contents.total_size);
        if let Some(err) = &entry.error {
            report_error(&mut report, p, title, err);
            continue;
        }
        let target = map.target(&entry.path);
        let mut result = copy_one(&entry.path, &target, &part, false);
        if let Err(CopyError::Exists(_)) = result {
            match decide_overwrite(&mut policy, &target, p) {
                Some(true) => result = copy_one(&entry.path, &target, &part, true),
                Some(false) => {
                    report.skipped.push(entry.path.clone());
                    continue;
                }
                None => {
                    report.stopped = true;
                    break;
                }
            }
        }
        match result {
            Ok(()) => {
                if entry.path.is_dir() && !entry.path.is_symlink() {
                    created_dirs.push((entry.path.clone(), target));
                }
                report.done.push(entry.path.clone());
            }
            Err(CopyError::SameFile(msg)) => {
                same_roots.push(entry.path.clone());
                report_error(&mut report, p, title, &msg);
            }
            Err(CopyError::Failed(msg)) => report_error(&mut report, p, title, &msg),
            Err(CopyError::Exists(t)) => {
                report_error(&mut report, p, title, &format!("'{}' exists", t.display()))
            }
        }
    }
    // Children first, so a read-only or time-stamped dir is finalized after its contents.
    for (src, target) in created_dirs.iter().rev() {
        if let Ok(meta) = std::fs::symlink_metadata(src) {
            let _ = std::fs::set_permissions(target, meta.permissions());
            copy_times_and_owner(&meta, target);
        }
    }
    report
}

pub fn copy_files(
    files: &[PathBuf],
    basepath: &Path,
    dest: &Path,
    confirm_overwrite: bool,
    p: &mut dyn Progress,
) -> OpReport {
    copy_loop(files, basepath, dest, confirm_overwrite, p, "Copy file(s)")
}

/// True when `dest` (existing or not) lies inside the real directory `src`.
fn inside_itself(src: &Path, dest: &Path) -> bool {
    let is_real_dir = std::fs::symlink_metadata(src).is_ok_and(|m| m.is_dir());
    let canon_dest = dest.canonicalize().or_else(|_| {
        let parent = dest.parent().unwrap_or(Path::new("/")).canonicalize()?;
        Ok::<_, std::io::Error>(parent.join(dest.file_name().unwrap_or_default()))
    });
    match (is_real_dir, src.canonicalize(), canon_dest) {
        (true, Ok(s), Ok(d)) => d.starts_with(s),
        _ => false,
    }
}

/// Drops (with an error) selected directories that would be moved into themselves.
fn reject_moves_into_itself(
    files: &[PathBuf],
    basepath: &Path,
    dest: &Path,
    report: &mut OpReport,
    p: &mut dyn Progress,
) -> Vec<PathBuf> {
    files
        .iter()
        .filter(|f| {
            let bad = inside_itself(f, dest);
            if bad {
                let msg = format!(
                    "Cannot move file!\nCannot move a directory into itself: '{}'",
                    partial(f, basepath)
                );
                report_error(report, p, "Move file(s)", &msg);
            }
            !bad
        })
        .cloned()
        .collect()
}

/// Copy, then delete only the sources that were copied successfully.
pub fn move_files(
    files: &[PathBuf],
    basepath: &Path,
    dest: &Path,
    confirm_overwrite: bool,
    p: &mut dyn Progress,
) -> OpReport {
    const TITLE: &str = "Move file(s)";
    let mut rejected = OpReport::default();
    let files = &reject_moves_into_itself(files, basepath, dest, &mut rejected, p)[..];
    if files.is_empty() {
        return rejected;
    }
    let mut copied = copy_loop(files, basepath, dest, confirm_overwrite, p, TITLE);
    copied.errors.splice(0..0, rejected.errors);
    if copied.stopped {
        return copied;
    }
    let done: HashSet<&PathBuf> = copied.done.iter().collect();
    let entries: Vec<ContentEntry> = collect_entries(files, basepath)
        .entries_rev()
        .into_iter()
        .filter(|e| done.contains(&e.path))
        .collect();
    let mut deleted = delete_loop(entries, basepath, false, p, TITLE);
    deleted.skipped = copied.skipped;
    let mut errors = copied.errors;
    errors.append(&mut deleted.errors);
    deleted.errors = errors;
    deleted
}

/// `rename` across filesystems: copy to a temp name beside `target`, swap it in,
/// then delete `src`. An existing non-empty directory at `target` is never removed.
fn move_across_filesystems(src: &Path, target: &Path) -> std::io::Result<()> {
    if let Ok(m) = std::fs::symlink_metadata(target) {
        if m.is_dir() {
            if !std::fs::symlink_metadata(src)?.is_dir() {
                return Err(std::io::Error::from_raw_os_error(nix::libc::EISDIR));
            }
            std::fs::remove_dir(target)?; // ENOTEMPTY keeps a populated directory
        }
    }
    let tmp = temp_sibling(target);
    if let Err(e) = copy_bulk(src, &tmp).and_then(|_| std::fs::rename(&tmp, target)) {
        let _ = remove_any(&tmp);
        return Err(e);
    }
    remove_any(src)
}

/// Moves each selected item with one rename (copy + delete across filesystems).
pub fn move_files2(
    files: &[PathBuf],
    basepath: &Path,
    dest: &Path,
    confirm_overwrite: bool,
    p: &mut dyn Progress,
) -> OpReport {
    const TITLE: &str = "Move file(s)";
    let mut report = OpReport::default();
    let files = &reject_moves_into_itself(files, basepath, dest, &mut report, p)[..];
    if files.is_empty() {
        return report;
    }
    let map = match DestMap::new(files, basepath, dest, TITLE) {
        Ok(m) => m,
        Err(msg) => {
            report_error(&mut report, p, TITLE, &msg);
            return report;
        }
    };
    let sizes: Vec<u64> = files
        .iter()
        .map(|f| std::fs::symlink_metadata(f).map_or(0, |m| m.len()))
        .collect();
    let total = sizes.iter().sum::<u64>().max(1);
    let mut policy = if confirm_overwrite {
        Overwrite::Ask
    } else {
        Overwrite::All
    };
    let mut acc = 0;
    for (i, (src, size)) in files.iter().zip(&sizes).enumerate() {
        if p.should_stop() {
            report.stopped = true;
            break;
        }
        let part = partial(src, basepath);
        acc += size;
        p.next(&part, *size, i + 1, files.len(), acc, total);
        let target = map.target(src);
        if same_file(src, &target) {
            let msg =
                format!("Cannot move file!\nSource and destination are the same file: '{part}'");
            report_error(&mut report, p, TITLE, &msg);
            continue;
        }
        if std::fs::symlink_metadata(&target).is_ok() {
            match decide_overwrite(&mut policy, &target, p) {
                Some(true) => {}
                Some(false) => {
                    report.skipped.push(src.clone());
                    continue;
                }
                None => {
                    report.stopped = true;
                    break;
                }
            }
        }
        let moved = match std::fs::rename(src, &target) {
            Err(e) if e.raw_os_error() == Some(nix::libc::EXDEV) => {
                move_across_filesystems(src, &target)
            }
            other => other,
        };
        match moved {
            Ok(()) => report.done.push(src.clone()),
            Err(e) => report_error(
                &mut report,
                p,
                TITLE,
                &os_failed("Cannot move file!", &e, &part),
            ),
        }
    }
    report
}

fn delete_loop(
    entries: Vec<ContentEntry>,
    basepath: &Path,
    mut confirm: bool,
    p: &mut dyn Progress,
    title: &str,
) -> OpReport {
    let mut report = OpReport::default();
    let count = entries.len();
    let total = entries.iter().map(|e| e.size).sum::<u64>().max(1);
    let mut acc = 0;
    for (i, entry) in entries.iter().enumerate() {
        if p.should_stop() {
            report.stopped = true;
            break;
        }
        let part = partial(&entry.path, basepath);
        acc += entry.size;
        p.next(&part, entry.size, i + 1, count, acc, total);
        if let Some(err) = &entry.error {
            report_error(&mut report, p, title, err);
            continue;
        }
        if confirm {
            match p.confirm_delete(&part) {
                Answer::Yes => {}
                Answer::All => confirm = false,
                Answer::No | Answer::NoneAll => {
                    report.skipped.push(entry.path.clone());
                    continue;
                }
                Answer::Stop => {
                    report.stopped = true;
                    break;
                }
            }
        }
        let removed = match std::fs::symlink_metadata(&entry.path) {
            Ok(m) if m.is_dir() => std::fs::remove_dir(&entry.path),
            Ok(_) => std::fs::remove_file(&entry.path),
            Err(e) => Err(e),
        };
        match removed {
            Ok(()) => report.done.push(entry.path.clone()),
            Err(e) => report_error(
                &mut report,
                p,
                title,
                &os_failed("Cannot delete file!", &e, &part),
            ),
        }
    }
    report
}

/// Deletes children before parents; asks per entry when `confirm` is on.
pub fn delete_files(
    files: &[PathBuf],
    basepath: &Path,
    confirm: bool,
    p: &mut dyn Progress,
) -> OpReport {
    delete_loop(
        collect_entries(files, basepath).entries_rev(),
        basepath,
        confirm,
        p,
        "Delete file(s)",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[derive(Default)]
    struct Script {
        answers: Vec<Answer>,
        asked: Vec<String>,
        errors: Vec<String>,
        stop_after: Option<usize>,
        seen: usize,
    }

    impl Progress for Script {
        fn next(&mut self, _: &str, _: u64, _: usize, _: usize, _: u64, _: u64) {
            self.seen += 1;
        }
        fn confirm_delete(&mut self, item: &str) -> Answer {
            self.asked.push(item.into());
            self.answers.remove(0)
        }
        fn confirm_overwrite(&mut self, d: &Path) -> Answer {
            self.asked.push(d.display().to_string());
            self.answers.remove(0)
        }
        fn error(&mut self, m: &str) {
            self.errors.push(m.into());
        }
        fn should_stop(&mut self) -> bool {
            self.stop_after.is_some_and(|n| self.seen >= n)
        }
    }

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
    fn copy_into_existing_dir() {
        let t = tree();
        let d = t.path();
        std::fs::create_dir(d.join("dst")).unwrap();
        let r = copy_files(
            &[d.join("a"), d.join("b"), d.join("lnk")],
            d,
            &d.join("dst"),
            true,
            &mut Script::default(),
        );
        assert!(r.errors.is_empty(), "{r:?}");
        assert!(d.join("dst/a/sub/f2").is_file() && d.join("dst/b").is_file());
        assert!(std::fs::symlink_metadata(d.join("dst/lnk"))
            .unwrap()
            .file_type()
            .is_symlink());
    }

    #[test]
    fn copy_dir_to_new_name() {
        let t = tree();
        let d = t.path();
        let r = copy_files(
            &[d.join("a")],
            d,
            &d.join("a2"),
            true,
            &mut Script::default(),
        );
        assert!(r.errors.is_empty(), "{r:?}");
        assert!(d.join("a2/f1").is_file() && d.join("a2/sub/f2").is_file());
    }

    #[test]
    fn copy_file_to_new_name_keeps_mode() {
        let t = tree();
        let d = t.path();
        std::fs::set_permissions(d.join("b"), std::fs::Permissions::from_mode(0o751)).unwrap();
        let r = copy_files(
            &[d.join("b")],
            d,
            &d.join("b2"),
            true,
            &mut Script::default(),
        );
        assert!(r.errors.is_empty(), "{r:?}");
        assert_eq!(
            std::fs::metadata(d.join("b2"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o751
        );
    }

    #[test]
    fn overwrite_answers() {
        let t = tree();
        let d = t.path();
        std::fs::create_dir(d.join("dst")).unwrap();
        for n in ["x", "y", "z"] {
            std::fs::write(d.join(n), b"new").unwrap();
            std::fs::write(d.join("dst").join(n), b"old").unwrap();
        }
        let files: Vec<_> = ["x", "y", "z"].iter().map(|n| d.join(n)).collect();
        let mut s = Script {
            answers: vec![Answer::No, Answer::All],
            ..Default::default()
        };
        let r = copy_files(&files, d, &d.join("dst"), true, &mut s);
        assert_eq!(s.asked.len(), 2); // z overwritten without asking after All
        assert_eq!(std::fs::read(d.join("dst/x")).unwrap(), b"old");
        assert_eq!(std::fs::read(d.join("dst/z")).unwrap(), b"new");
        assert_eq!(r.skipped, vec![d.join("x")]);
        let mut s = Script {
            answers: vec![Answer::NoneAll],
            ..Default::default()
        };
        for n in ["x", "y", "z"] {
            std::fs::write(d.join(n), b"newer").unwrap();
        }
        let r = copy_files(&files, d, &d.join("dst"), true, &mut s);
        assert_eq!((s.asked.len(), r.skipped.len()), (1, 3));
        let r = copy_files(&files, d, &d.join("dst"), false, &mut Script::default()); // no confirmation → overwrite
        assert_eq!(
            (r.done.len(), std::fs::read(d.join("dst/x")).unwrap()),
            (3, b"newer".to_vec())
        );
    }

    #[test]
    fn copy_into_own_subdir_terminates() {
        let t = tree();
        let d = t.path();
        let r = copy_files(
            &[d.join("a")],
            d,
            &d.join("a/sub"),
            true,
            &mut Script::default(),
        );
        assert!(d.join("a/sub/a/f1").is_file(), "{r:?}");
        assert!(!d.join("a/sub/a/sub/a").exists());
    }

    #[test]
    fn special_file_error_text() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path();
        nix::unistd::mkfifo(&d.join("p"), nix::sys::stat::Mode::S_IRWXU).unwrap();
        std::fs::create_dir(d.join("dst")).unwrap();
        let mut s = Script::default();
        let r = copy_files(&[d.join("p")], d, &d.join("dst"), true, &mut s);
        assert_eq!(
            r.errors,
            vec!["Cannot copy file!\nCannot copy special file: 'p'".to_string()]
        );
        assert_eq!(s.errors, r.errors);
    }

    #[test]
    fn move_keeps_sources_that_failed_or_were_skipped() {
        let t = tree();
        let d = t.path();
        std::fs::create_dir(d.join("dst")).unwrap();
        std::fs::write(d.join("dst/b"), b"keep").unwrap();
        let mut s = Script {
            answers: vec![Answer::No],
            ..Default::default()
        };
        let r = move_files(&[d.join("a"), d.join("b")], d, &d.join("dst"), true, &mut s);
        assert!(
            d.join("dst/a/sub/f2").is_file() && !d.join("a").exists(),
            "{r:?}"
        );
        assert!(d.join("b").exists()); // skipped → not deleted
        if !nix::unistd::geteuid().is_root() {
            std::fs::create_dir(d.join("c")).unwrap();
            std::fs::write(d.join("c/f"), b"1").unwrap();
            std::fs::set_permissions(d.join("c/f"), std::fs::Permissions::from_mode(0o000))
                .unwrap();
            let r = move_files(
                &[d.join("c")],
                d,
                &d.join("dst"),
                true,
                &mut Script::default(),
            );
            assert!(!r.errors.is_empty() && d.join("c/f").exists()); // unreadable file not copied → not deleted
            std::fs::set_permissions(d.join("c/f"), std::fs::Permissions::from_mode(0o644))
                .unwrap();
        }
    }

    #[test]
    fn move2_renames_top_level_items() {
        let t = tree();
        let d = t.path();
        std::fs::create_dir(d.join("dst")).unwrap();
        let r = move_files2(
            &[d.join("a"), d.join("lnk")],
            d,
            &d.join("dst"),
            true,
            &mut Script::default(),
        );
        assert!(
            r.errors.is_empty() && d.join("dst/a/sub/f2").is_file() && !d.join("a").exists(),
            "{r:?}"
        );
    }

    #[test]
    fn move2_asks_before_overwriting() {
        let t = tree();
        let d = t.path();
        std::fs::create_dir(d.join("dst")).unwrap();
        std::fs::write(d.join("dst/b"), b"old").unwrap();
        let mut s = Script {
            answers: vec![Answer::No],
            ..Default::default()
        };
        let r = move_files2(&[d.join("b")], d, &d.join("dst"), true, &mut s);
        assert_eq!((s.asked.len(), r.skipped.len()), (1, 1));
        assert_eq!(std::fs::read(d.join("dst/b")).unwrap(), b"old");
        assert!(d.join("b").exists());
    }

    #[test]
    fn move2_across_filesystems() {
        use std::os::unix::fs::MetadataExt;
        let shm = Path::new("/dev/shm");
        let t = tree();
        let d = t.path();
        if !shm.is_dir()
            || std::fs::metadata(shm).unwrap().dev() == std::fs::metadata(d).unwrap().dev()
        {
            return;
        }
        let target = tempfile::tempdir_in(shm).unwrap();
        let r = move_files2(
            &[d.join("a")],
            d,
            target.path(),
            true,
            &mut Script::default(),
        );
        assert!(r.errors.is_empty(), "{r:?}");
        assert!(target.path().join("a/sub/f2").is_file() && !d.join("a").exists());
    }

    #[test]
    fn delete_answers_and_symlink_safety() {
        let t = tree();
        let d = t.path();
        let r = delete_files(&[d.join("lnk")], d, false, &mut Script::default());
        assert!(r.errors.is_empty() && !d.join("lnk").exists() && d.join("a/f1").exists());
        std::fs::write(d.join("c"), b"").unwrap();
        let mut s = Script {
            answers: vec![Answer::No, Answer::Yes],
            ..Default::default()
        };
        delete_files(&[d.join("b"), d.join("c")], d, true, &mut s);
        assert_eq!(s.asked, ["c", "b"]); // reverse order
        assert!(d.join("c").exists() && !d.join("b").exists());
        let mut s = Script {
            answers: vec![Answer::All],
            ..Default::default()
        };
        let r = delete_files(&[d.join("a")], d, true, &mut s);
        assert_eq!(s.asked.len(), 1);
        assert!(!d.join("a").exists(), "{r:?}");
    }

    #[test]
    fn stop_ends_loop() {
        let t = tree();
        let d = t.path();
        std::fs::create_dir(d.join("dst")).unwrap();
        let mut s = Script {
            stop_after: Some(1),
            ..Default::default()
        };
        let r = copy_files(&[d.join("a"), d.join("b")], d, &d.join("dst"), true, &mut s);
        assert!(r.stopped && !d.join("dst/b").exists());
        let mut s = Script {
            answers: vec![Answer::Stop],
            ..Default::default()
        };
        let r = delete_files(&[d.join("b")], d, true, &mut s);
        assert!(r.stopped && d.join("b").exists());
    }

    #[test]
    fn copy_onto_itself_keeps_source() {
        let t = tree();
        let d = t.path();
        let mut s = Script::default();
        let r = copy_files(&[d.join("b")], d, d, false, &mut s);
        assert_eq!(std::fs::read(d.join("b")).unwrap(), vec![0u8; 5], "{r:?}");
        assert!(r.errors.iter().any(|e| e.contains("same file")), "{r:?}");
        // destination reached through a symlink alias of the source dir
        std::os::unix::fs::symlink(d.join("a"), d.join("alias")).unwrap();
        let r = copy_files(
            &[d.join("a/f1")],
            &d.join("a"),
            &d.join("alias"),
            false,
            &mut Script::default(),
        );
        assert_eq!(
            std::fs::read(d.join("a/f1")).unwrap(),
            vec![0u8; 10],
            "{r:?}"
        );
    }

    #[test]
    fn move_onto_itself_keeps_source() {
        let t = tree();
        let d = t.path();
        let r = move_files(
            &[d.join("a")],
            d,
            d,
            true,
            &mut Script {
                answers: vec![Answer::All],
                ..Default::default()
            },
        );
        assert!(
            d.join("a/f1").is_file() && d.join("a/sub/f2").is_file(),
            "{r:?}"
        );
        assert!(!r.errors.is_empty());
    }

    #[test]
    fn move_into_own_subdir_is_rejected() {
        let t = tree();
        let d = t.path();
        let r = move_files(
            &[d.join("a"), d.join("b")],
            d,
            &d.join("a/sub"),
            true,
            &mut Script::default(),
        );
        assert!(r.errors.iter().any(|e| e.contains("into itself")), "{r:?}");
        assert!(d.join("a/f1").is_file() && !d.join("a/sub/a").exists());
        assert!(d.join("a/sub/b").is_file() && !d.join("b").exists()); // other items still move
        let r = move_files2(
            &[d.join("a")],
            d,
            &d.join("a/sub"),
            true,
            &mut Script::default(),
        );
        assert!(r.errors.iter().any(|e| e.contains("into itself")), "{r:?}");
    }

    #[test]
    fn overwrite_with_unreadable_source_keeps_destination() {
        if nix::unistd::geteuid().is_root() {
            return;
        }
        let t = tree();
        let d = t.path();
        std::fs::create_dir(d.join("dst")).unwrap();
        std::fs::write(d.join("dst/b"), b"old").unwrap();
        std::fs::set_permissions(d.join("b"), std::fs::Permissions::from_mode(0o000)).unwrap();
        let r = copy_files(
            &[d.join("b")],
            d,
            &d.join("dst"),
            false,
            &mut Script::default(),
        );
        std::fs::set_permissions(d.join("b"), std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(!r.errors.is_empty(), "{r:?}");
        assert_eq!(std::fs::read(d.join("dst/b")).unwrap(), b"old");
    }

    #[test]
    fn move2_across_filesystems_never_wipes_existing_dir() {
        use std::os::unix::fs::MetadataExt;
        let shm = Path::new("/dev/shm");
        let t = tree();
        let d = t.path();
        if !shm.is_dir()
            || std::fs::metadata(shm).unwrap().dev() == std::fs::metadata(d).unwrap().dev()
        {
            return;
        }
        let target = tempfile::tempdir_in(shm).unwrap();
        std::fs::create_dir_all(target.path().join("b/deep")).unwrap();
        std::fs::write(target.path().join("b/deep/important"), b"keep").unwrap();
        let r = move_files2(
            &[d.join("b")],
            d,
            target.path(),
            false,
            &mut Script::default(),
        );
        assert!(!r.errors.is_empty(), "{r:?}");
        assert!(target.path().join("b/deep/important").is_file());
        assert!(d.join("b").is_file());
        // a plain existing file is still replaced
        std::fs::write(target.path().join("f1"), b"old").unwrap();
        let r = move_files2(
            &[d.join("a/f1")],
            &d.join("a"),
            target.path(),
            false,
            &mut Script::default(),
        );
        assert!(r.errors.is_empty(), "{r:?}");
        assert_eq!(
            std::fs::read(target.path().join("f1")).unwrap(),
            vec![0u8; 10]
        );
    }

    #[test]
    fn non_utf8_copy_and_delete() {
        use std::os::unix::ffi::OsStrExt;
        let t = tempfile::tempdir().unwrap();
        let d = t.path();
        std::fs::create_dir(d.join("dst")).unwrap();
        let p = d.join(std::ffi::OsStr::from_bytes(b"x\xff"));
        std::fs::write(&p, b"1").unwrap();
        let r = copy_files(
            std::slice::from_ref(&p),
            d,
            &d.join("dst"),
            true,
            &mut Script::default(),
        );
        assert!(
            r.errors.is_empty()
                && d.join("dst")
                    .join(std::ffi::OsStr::from_bytes(b"x\xff"))
                    .exists()
        );
        let r = delete_files(std::slice::from_ref(&p), d, false, &mut Script::default());
        assert!(r.errors.is_empty() && !p.exists());
    }
}
