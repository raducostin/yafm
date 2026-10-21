//! Virtual folders: an archive (or a set of search results) copied into a temp
//! directory, shown as `archive#vfs://rel`, and optionally written back.
//! Ported from `CompressedFileFolder`/`SearchFolder` in lfm's folders.py.

use std::ffi::{OsStr, OsString};
use std::path::{Component, Path, PathBuf};

use crate::archive::{compress2_cmd, compress2_dest, detect, uncompress_cmd, ArchiveType};
use crate::ops::simple::{copy_bulk, temp_sibling};

pub const VFS_STRING: &str = "#vfs://";

/// Runs a shell command in a dir: Err(message) on failure or when stopped.
pub type Runner<'a> = dyn FnMut(&str, &OsStr, &Path) -> Result<(), String> + 'a;

#[derive(Debug, Clone, PartialEq)]
pub enum LayerKind {
    Archive(ArchiveType),
    Search { files: Vec<PathBuf> },
}

#[derive(Debug, Clone)]
pub struct VfsLayer {
    pub source: PathBuf,
    pub tmp: PathBuf,
    pub display_base: String,
    pub kind: LayerKind,
}

fn make_tmp() -> Result<PathBuf, String> {
    tempfile::Builder::new()
        .prefix("yafm-")
        .suffix(".yafm")
        .tempdir()
        .map(|d| d.keep())
        .map_err(|e| format!("Cannot create temporary directory: {e}"))
}

fn join_display(parent: &str, name: &str) -> String {
    if parent.ends_with('/') {
        format!("{parent}{name}")
    } else {
        format!("{parent}/{name}")
    }
}

/// Extracts `archive` into a new temp dir. `display_parent` is how the
/// directory holding the archive is shown (it may itself be inside a VFS).
pub fn open_archive(
    archive: &Path,
    display_parent: &str,
    run: &mut Runner,
) -> Result<VfsLayer, String> {
    let t = match detect(archive) {
        Some(t) if t.can_vfs() => t,
        _ => return Err("Cannot open this file type".to_string()),
    };
    let tmp = make_tmp()?;
    if let Err(e) = run("Creating VFS", &uncompress_cmd(t, archive), &tmp) {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(e);
    }
    let name = archive
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    Ok(VfsLayer {
        source: archive.to_path_buf(),
        tmp,
        display_base: format!("{}{VFS_STRING}", join_display(display_parent, &name)),
        kind: LayerKind::Archive(t),
    })
}

/// A search result must stay inside the searched directory: relative, no "..".
fn check_inside(rel: &Path) -> Result<(), String> {
    let ok = !rel.as_os_str().is_empty()
        && rel
            .components()
            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir));
    if ok {
        Ok(())
    } else {
        Err(format!(
            "Cannot use a file outside the searched directory: {}",
            rel.display()
        ))
    }
}

/// Copies `files` (relative to `dir`) into a new temp dir, keeping their paths.
/// Entries whose parent directory is also listed come along with it.
pub fn open_search(dir: &Path, display_dir: &str, files: &[PathBuf]) -> Result<VfsLayer, String> {
    files.iter().try_for_each(|f| check_inside(f))?;
    let tmp = make_tmp()?;
    let copied: std::io::Result<()> = files
        .iter()
        .filter(|f| {
            !files
                .iter()
                .any(|other| other != *f && f.starts_with(other))
        })
        .try_for_each(|f| {
            let dest = tmp.join(f);
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)?;
            }
            copy_bulk(&dir.join(f), &dest)
        });
    if let Err(e) = copied {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(e.to_string());
    }
    Ok(VfsLayer {
        source: dir.to_path_buf(),
        tmp,
        display_base: format!("{display_dir}{VFS_STRING}"),
        kind: LayerKind::Search {
            files: files.to_vec(),
        },
    })
}

/// Writes the layer back when `rebuild`, then removes its temp dir. If the
/// rebuild fails the temp dir is kept, so the user's edits are not lost.
pub fn close(layer: &VfsLayer, rebuild: bool, run: &mut Runner) -> Result<(), String> {
    let result = match (&layer.kind, rebuild) {
        (_, false) => Ok(()),
        (LayerKind::Archive(t), true) => rebuild_archive(layer, *t, run),
        (LayerKind::Search { files }, true) => rebuild_search(layer, files),
    };
    match result {
        Ok(()) => {
            let _ = std::fs::remove_dir_all(&layer.tmp);
            Ok(())
        }
        Err(e) => Err(format!(
            "{e}\nYour changes are kept in {}",
            layer.tmp.display()
        )),
    }
}

/// Top-level names of `dir`, dotfiles included, sorted.
fn members(dir: &Path) -> Vec<OsString> {
    let mut v: Vec<OsString> = std::fs::read_dir(dir)
        .map(|rd| rd.filter_map(Result::ok).map(|de| de.file_name()).collect())
        .unwrap_or_default();
    v.sort();
    v
}

/// Packs the temp dir into a file in a private temp dir, then swaps it in next
/// to the archive; the original is only replaced once the new archive exists.
fn rebuild_archive(layer: &VfsLayer, t: ArchiveType, run: &mut Runner) -> Result<(), String> {
    let work = tempfile::Builder::new()
        .prefix("yafm-rebuild-")
        .tempdir()
        .map_err(|e| format!("Cannot rebuild\n{e}"))?;
    let base = work.path().join("packed");
    let packed = compress2_dest(t, &base);
    run(
        "Rebuilding VFS",
        &compress2_cmd(t, &base, &members(&layer.tmp)),
        &layer.tmp,
    )?;
    let sibling = temp_sibling(&layer.source);
    let swapped = std::fs::copy(&packed, &sibling)
        .and_then(|_| {
            if let Ok(meta) = std::fs::metadata(&layer.source) {
                std::fs::set_permissions(&sibling, meta.permissions())?;
            }
            std::fs::rename(&sibling, &layer.source)
        })
        .map_err(|e| format!("Cannot rebuild {}\n{e}", layer.source.display()));
    if swapped.is_err() {
        let _ = std::fs::remove_file(&sibling);
    }
    swapped
}

/// Regular files under `dir` (symlinks and special files are not followed or listed).
fn files_under(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for de in rd.filter_map(Result::ok) {
        let p = de.path();
        match std::fs::symlink_metadata(&p) {
            Ok(m) if m.is_dir() => files_under(root, &p, out),
            Ok(m) if m.is_file() => out.extend(p.strip_prefix(root).ok().map(Path::to_path_buf)),
            _ => {}
        }
    }
}

/// Copies regular files now in the temp dir back (never through a symlink at
/// the destination); deletes listed originals whose copy was removed.
fn rebuild_search(layer: &VfsLayer, original: &[PathBuf]) -> Result<(), String> {
    let mut now = Vec::new();
    files_under(&layer.tmp, &layer.tmp, &mut now);
    for rel in &now {
        check_inside(rel)?;
        let dest = layer.source.join(rel);
        let copy = || -> std::io::Result<()> {
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)?;
            }
            if std::fs::symlink_metadata(&dest).is_ok_and(|m| !m.is_file()) {
                std::fs::remove_file(&dest)?;
            }
            std::fs::copy(layer.tmp.join(rel), &dest).map(|_| ())
        };
        copy().map_err(|e| format!("Cannot write back {}\n{e}", dest.display()))?;
    }
    for rel in original.iter().filter(|f| !now.contains(f)) {
        let (src_gone, dest) = (
            std::fs::symlink_metadata(layer.tmp.join(rel)).is_err(),
            layer.source.join(rel),
        );
        if src_gone
            && check_inside(rel).is_ok()
            && std::fs::symlink_metadata(&dest).is_ok_and(|m| m.is_file())
        {
            let _ = std::fs::remove_file(&dest);
        }
    }
    Ok(())
}

/// How `real` is shown: through the innermost layer containing it.
pub fn display_path(layers: &[VfsLayer], real: &Path) -> String {
    for l in layers.iter().rev() {
        if let Ok(rel) = real.strip_prefix(&l.tmp) {
            return format!("{}{}", l.display_base, rel.to_string_lossy());
        }
    }
    real.to_string_lossy().into_owned()
}

/// lfm's split_vfs_path: everything up to the last marker, and the rest.
pub fn split_vfs_path(path: &str) -> (String, String) {
    match path.rfind(VFS_STRING) {
        Some(n) => {
            let cut = n + VFS_STRING.len();
            (path[..cut].to_string(), path[cut..].to_string())
        }
        None => (path.to_string(), String::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sh(_title: &str, cmd: &OsStr, cwd: &Path) -> Result<(), String> {
        let (_, err) = crate::jobs::run_in_cli(cmd, cwd).map_err(|e| e.to_string())?;
        if err.is_empty() {
            Ok(())
        } else {
            Err(err)
        }
    }

    fn make_tgz(d: &Path) -> PathBuf {
        std::fs::create_dir_all(d.join("src/in")).unwrap();
        std::fs::write(d.join("src/in/f"), b"F").unwrap();
        std::fs::write(d.join("src/top"), b"T").unwrap();
        crate::jobs::run_in_cli(
            OsStr::new("tar cf - * | gzip > ../a.tar.gz"),
            &d.join("src"),
        )
        .unwrap();
        d.join("a.tar.gz")
    }

    #[test]
    fn open_browse_close_cleans_up() {
        let t = tempfile::tempdir().unwrap();
        let a = make_tgz(t.path());
        let l = open_archive(&a, &t.path().display().to_string(), &mut sh).unwrap();
        assert!(l.tmp.join("in/f").is_file());
        assert!(l.tmp.to_string_lossy().ends_with(".yafm"));
        assert_eq!(
            display_path(std::slice::from_ref(&l), &l.tmp.join("in")),
            format!("{}/a.tar.gz#vfs://in", t.path().display())
        );
        assert_eq!(
            display_path(std::slice::from_ref(&l), &l.tmp),
            format!("{}/a.tar.gz#vfs://", t.path().display())
        );
        assert_eq!(
            display_path(std::slice::from_ref(&l), Path::new("/etc")),
            "/etc"
        );
        close(&l, false, &mut sh).unwrap();
        assert!(!l.tmp.exists());
    }

    #[test]
    fn corrupt_archive_fails_and_cleans() {
        let t = tempfile::tempdir().unwrap();
        let a = t.path().join("bad.tar.gz");
        std::fs::write(&a, b"nope").unwrap();
        let mut used = None;
        let mut recording = |title: &str, cmd: &OsStr, cwd: &Path| {
            used = Some(cwd.to_path_buf());
            sh(title, cmd, cwd)
        };
        assert!(open_archive(&a, "/x", &mut recording).is_err());
        let tmp = used.expect("extraction ran");
        assert!(tmp.to_string_lossy().ends_with(".yafm") && !tmp.exists());
        assert!(open_archive(&t.path().join("x.txt"), "/x", &mut sh).is_err());
    }

    #[test]
    fn rebuild_packs_changes() {
        let t = tempfile::tempdir().unwrap();
        let a = make_tgz(t.path());
        let l = open_archive(&a, "/x", &mut sh).unwrap();
        std::fs::write(l.tmp.join("new"), b"N").unwrap();
        std::fs::remove_file(l.tmp.join("top")).unwrap();
        close(&l, true, &mut sh).unwrap();
        let l2 = open_archive(&a, "/x", &mut sh).unwrap();
        assert!(
            l2.tmp.join("new").is_file()
                && !l2.tmp.join("top").exists()
                && l2.tmp.join("in/f").is_file()
        );
        close(&l2, false, &mut sh).unwrap();
    }

    #[test]
    fn failed_rebuild_keeps_archive() {
        let t = tempfile::tempdir().unwrap();
        let a = make_tgz(t.path());
        let before = std::fs::read(&a).unwrap();
        let l = open_archive(&a, "/x", &mut sh).unwrap();
        let mut failing = |_: &str, _: &OsStr, _: &Path| -> Result<(), String> {
            Err("No space left on device".into())
        };
        assert!(close(&l, true, &mut failing).is_err());
        assert_eq!(std::fs::read(&a).unwrap(), before);
        std::fs::remove_dir_all(&l.tmp).unwrap(); // kept for the user after a failed rebuild
    }

    #[test]
    fn search_layer_round_trip() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path();
        std::fs::create_dir(d.join("s")).unwrap();
        std::fs::write(d.join("s/a"), b"1").unwrap();
        std::fs::write(d.join("b"), b"2").unwrap();
        let files = vec![PathBuf::from("s/a"), PathBuf::from("b")];
        let l = open_search(d, &d.display().to_string(), &files).unwrap();
        assert!(l.tmp.join("s/a").is_file());
        assert_eq!(
            display_path(std::slice::from_ref(&l), &l.tmp),
            format!("{}#vfs://", d.display())
        );
        std::fs::write(l.tmp.join("s/a"), b"changed").unwrap();
        std::fs::remove_file(l.tmp.join("b")).unwrap();
        close(&l, true, &mut sh).unwrap();
        assert_eq!(std::fs::read(d.join("s/a")).unwrap(), b"changed");
        assert!(!d.join("b").exists());
        assert!(!l.tmp.exists());
    }

    #[test]
    fn split_paths() {
        assert_eq!(
            split_vfs_path("/a/b.tgz#vfs://x/c.zip#vfs://y"),
            ("/a/b.tgz#vfs://x/c.zip#vfs://".into(), "y".into())
        );
        assert_eq!(split_vfs_path("/plain"), ("/plain".into(), "".into()));
    }

    #[test]
    fn nested_layers_display_innermost() {
        let t = tempfile::tempdir().unwrap();
        let a = make_tgz(t.path());
        let outer = open_archive(&a, "/x", &mut sh).unwrap();
        crate::jobs::run_in_cli(OsStr::new("tar cf - in | gzip > inner.tgz"), &outer.tmp).unwrap();
        let inner = open_archive(
            &outer.tmp.join("inner.tgz"),
            &display_path(std::slice::from_ref(&outer), &outer.tmp),
            &mut sh,
        )
        .unwrap();
        let layers = [outer.clone(), inner.clone()];
        assert_eq!(
            display_path(&layers, &inner.tmp.join("in")),
            "/x/a.tar.gz#vfs://inner.tgz#vfs://in"
        );
        close(&inner, false, &mut sh).unwrap();
        close(&outer, false, &mut sh).unwrap();
    }

    #[test]
    fn search_layer_refuses_paths_outside_the_dir() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path().join("d");
        std::fs::create_dir(&d).unwrap();
        std::fs::write(t.path().join("outside.txt"), b"precious").unwrap();
        for bad in ["../outside.txt", "/etc/hostname"] {
            assert!(
                open_search(&d, "/x", &[PathBuf::from(bad)]).is_err(),
                "{bad}"
            );
        }
        assert_eq!(
            std::fs::read(t.path().join("outside.txt")).unwrap(),
            b"precious"
        );
    }

    #[test]
    fn search_rebuild_never_writes_through_symlinks() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path();
        std::fs::write(d.join("real.txt"), b"precious").unwrap();
        std::os::unix::fs::symlink(d.join("real.txt"), d.join("link.txt")).unwrap(); // absolute target
        let l = open_search(d, "/x", &[PathBuf::from("link.txt")]).unwrap();
        let _ = close(&l, true, &mut sh);
        assert_eq!(std::fs::read(d.join("real.txt")).unwrap(), b"precious");
        assert!(std::fs::symlink_metadata(d.join("link.txt"))
            .unwrap()
            .file_type()
            .is_symlink());
    }

    #[test]
    fn search_layer_handles_nested_results() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path();
        std::fs::create_dir_all(d.join("a/b")).unwrap();
        std::fs::write(d.join("a/b/f"), b"1").unwrap();
        let files: Vec<PathBuf> = ["a", "a/b", "a/b/f"].iter().map(PathBuf::from).collect();
        let l = open_search(d, "/x", &files).unwrap();
        assert!(l.tmp.join("a/b/f").is_file());
        close(&l, false, &mut sh).unwrap();
    }

    #[test]
    fn rebuild_keeps_dotfiles_dash_names_and_ignores_option_names() {
        let t = tempfile::tempdir().unwrap();
        let a = make_tgz(t.path());
        let marker = t.path().join("PWNED");
        let l = open_archive(&a, "/x", &mut sh).unwrap();
        std::fs::write(l.tmp.join(".hidden"), b"h").unwrap();
        std::fs::write(l.tmp.join("-v"), b"v").unwrap();
        std::fs::write(l.tmp.join("--checkpoint=1"), b"").unwrap();
        std::fs::write(l.tmp.join("--checkpoint-action=exec=sh x.sh"), b"").unwrap();
        std::fs::write(
            l.tmp.join("x.sh"),
            format!("touch '{}'\n", marker.display()),
        )
        .unwrap();
        close(&l, true, &mut sh).unwrap();
        assert!(!marker.exists(), "option injection ran");
        let l2 = open_archive(&a, "/x", &mut sh).unwrap();
        assert!(
            l2.tmp.join(".hidden").is_file()
                && l2.tmp.join("-v").is_file()
                && l2.tmp.join("in/f").is_file()
        );
        close(&l2, false, &mut sh).unwrap();
    }

    #[test]
    fn failed_rebuild_keeps_the_edits() {
        let t = tempfile::tempdir().unwrap();
        let a = make_tgz(t.path());
        let l = open_archive(&a, "/x", &mut sh).unwrap();
        std::fs::write(l.tmp.join("edit"), b"work").unwrap();
        let mut failing =
            |_: &str, _: &OsStr, _: &Path| -> Result<(), String> { Err("disk full".into()) };
        let err = close(&l, true, &mut failing).unwrap_err();
        assert!(err.contains(&l.tmp.display().to_string()), "{err}");
        assert_eq!(std::fs::read(l.tmp.join("edit")).unwrap(), b"work");
        std::fs::remove_dir_all(&l.tmp).unwrap();
    }
}
