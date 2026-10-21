//! File types and directory entries, ported from `FSEntry` (folders.py) and
//! `get_filetype` (utils.py).

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::path::{Path, PathBuf};

use nix::sys::stat::{major, minor};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FileType {
    Dir,
    Link2Dir,
    Link,
    NLink,
    CDev,
    BDev,
    Fifo,
    Socket,
    Exe,
    Reg,
    Unknown,
}

impl FileType {
    pub fn symbol(self) -> char {
        self.info().0
    }

    pub fn description(self) -> &'static str {
        self.info().1
    }

    /// lfm's `FILETYPES` table.
    fn info(self) -> (char, &'static str) {
        match self {
            FileType::Dir => ('/', "Directory"),
            FileType::Link2Dir => ('~', "Link to Directory"),
            FileType::Link => ('@', "Link"),
            FileType::NLink => ('!', "No Link"),
            FileType::CDev => ('-', "Char Device"),
            FileType::BDev => ('+', "Block Device"),
            FileType::Fifo => ('|', "Fifo"),
            FileType::Socket => ('#', "Socket"),
            FileType::Exe => ('*', "Executable"),
            FileType::Reg => (' ', "File"),
            FileType::Unknown => ('?', "Unknown"),
        }
    }
}

fn filetype_from_meta(path: &Path, meta: &std::fs::Metadata) -> FileType {
    let ft = meta.file_type();
    if ft.is_dir() {
        FileType::Dir
    } else if ft.is_symlink() {
        match std::fs::metadata(path) {
            Ok(m) if m.is_dir() => FileType::Link2Dir,
            Ok(_) => FileType::Link,
            Err(_) => FileType::NLink,
        }
    } else if ft.is_char_device() {
        FileType::CDev
    } else if ft.is_block_device() {
        FileType::BDev
    } else if ft.is_fifo() {
        FileType::Fifo
    } else if ft.is_socket() {
        FileType::Socket
    } else if ft.is_file() && meta.mode() & 0o111 != 0 {
        FileType::Exe
    } else {
        FileType::Reg
    }
}

pub fn get_filetype(path: &Path) -> std::io::Result<FileType> {
    let meta = std::fs::symlink_metadata(path)?;
    Ok(filetype_from_meta(path, &meta))
}

/// Last path component as written, so "dir/.." gives "..".
fn last_component(path: &Path) -> OsString {
    let bytes = path.as_os_str().as_bytes();
    let trimmed = bytes.strip_suffix(b"/").unwrap_or(bytes);
    let start = trimmed
        .iter()
        .rposition(|b| *b == b'/')
        .map_or(0, |i| i + 1);
    OsStr::from_bytes(&trimmed[start..]).to_os_string()
}

/// Python `os.path.splitext`: leading dots never start an extension.
fn splitext(name: &str) -> (String, String) {
    let first_non_dot = name.find(|c| c != '.');
    match (name.rfind('.'), first_non_dot) {
        (Some(dot), Some(start)) if dot > start => {
            (name[..dot].to_string(), name[dot..].to_string())
        }
        _ => (name.to_string(), String::new()),
    }
}

#[derive(Clone, Debug)]
pub struct FsEntry {
    pub path: PathBuf,
    pub name: OsString,
    /// With the dot (".gz"), empty when there is none.
    pub ext: String,
    pub name_noext: String,
    pub size: u64,
    pub mode: u32,
    pub uid: u32,
    pub gid: u32,
    pub mtime: i64,
    pub ftype: FileType,
    pub rdev: (u64, u64),
}

impl FsEntry {
    pub fn from_path(path: &Path) -> std::io::Result<FsEntry> {
        let meta = std::fs::symlink_metadata(path)?;
        let ftype = filetype_from_meta(path, &meta);
        let name = last_component(path);
        let (mut name_noext, ext) = splitext(&name.to_string_lossy());
        if let Some(stripped) = name_noext.strip_suffix(".tar") {
            name_noext = stripped.to_string();
        }
        let rdev = match ftype {
            FileType::CDev | FileType::BDev => (major(meta.rdev()), minor(meta.rdev())),
            _ => (0, 0),
        };
        Ok(FsEntry {
            path: path.to_path_buf(),
            name,
            ext,
            name_noext,
            size: meta.size(),
            mode: meta.mode(),
            uid: meta.uid(),
            gid: meta.gid(),
            mtime: meta.mtime(),
            ftype,
            rdev,
        })
    }

    pub fn display_name(&self) -> String {
        self.name.to_string_lossy().into_owned()
    }

    pub fn is_dir(&self) -> bool {
        matches!(self.ftype, FileType::Dir | FileType::Link2Dir)
    }

    pub fn is_link(&self) -> bool {
        matches!(
            self.ftype,
            FileType::Link2Dir | FileType::Link | FileType::NLink
        )
    }

    pub fn is_dotfile(&self) -> bool {
        self.name.as_bytes().first() == Some(&b'.')
    }

    /// Color category: "dir", "exe", the first `files` category listing the
    /// extension (compared lowercased), else "reg".
    pub fn file_kind(&self, files: &BTreeMap<String, Vec<String>>) -> String {
        if self.is_dir() {
            return "dir".to_string();
        }
        if self.ftype == FileType::Exe {
            return "exe".to_string();
        }
        let ext = self.ext.get(1..).unwrap_or("").to_lowercase();
        files
            .iter()
            .find(|(_, exts)| exts.contains(&ext))
            .map_or_else(|| "reg".to_string(), |(kind, _)| kind.clone())
    }

    pub fn update_size(&mut self, size: u64) {
        self.size = size;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn mk(d: &Path) {
        std::fs::create_dir(d.join("dir")).unwrap();
        std::fs::write(d.join("a.tar.gz"), b"x").unwrap();
        std::fs::write(d.join("run.sh"), b"#!/bin/sh").unwrap();
        std::fs::set_permissions(d.join("run.sh"), std::fs::Permissions::from_mode(0o755)).unwrap();
        std::os::unix::fs::symlink("dir", d.join("l2d")).unwrap();
        std::os::unix::fs::symlink("a.tar.gz", d.join("l2f")).unwrap();
        std::os::unix::fs::symlink("nowhere", d.join("broken")).unwrap();
        nix::unistd::mkfifo(&d.join("fifo"), nix::sys::stat::Mode::S_IRWXU).unwrap();
        std::os::unix::net::UnixListener::bind(d.join("sock")).unwrap();
    }

    #[test]
    fn file_types() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path();
        mk(d);
        let ft = |n: &str| get_filetype(&d.join(n)).unwrap();
        assert_eq!(ft("dir"), FileType::Dir);
        assert_eq!(ft("a.tar.gz"), FileType::Reg);
        assert_eq!(ft("run.sh"), FileType::Exe);
        assert_eq!(ft("l2d"), FileType::Link2Dir);
        assert_eq!(ft("l2f"), FileType::Link);
        assert_eq!(ft("broken"), FileType::NLink);
        assert_eq!(ft("fifo"), FileType::Fifo);
        assert_eq!(ft("sock"), FileType::Socket);
        assert_eq!(
            get_filetype(Path::new("/dev/null")).unwrap(),
            FileType::CDev
        );
        assert_eq!(FileType::Link2Dir.symbol(), '~');
        assert_eq!(FileType::Reg.symbol(), ' ');
        assert_eq!(FileType::NLink.description(), "No Link");
    }

    #[test]
    fn names_and_extensions() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path();
        mk(d);
        std::fs::write(d.join(".bashrc"), b"").unwrap();
        std::fs::write(d.join("my.tarball.tar.gz"), b"").unwrap();
        let e = FsEntry::from_path(&d.join("a.tar.gz")).unwrap();
        assert_eq!((e.ext.as_str(), e.name_noext.as_str()), (".gz", "a"));
        let e = FsEntry::from_path(&d.join(".bashrc")).unwrap();
        assert_eq!(e.ext, "");
        assert!(e.is_dotfile());
        assert_eq!(
            FsEntry::from_path(&d.join("my.tarball.tar.gz"))
                .unwrap()
                .name_noext,
            "my.tarball"
        );
        assert_eq!(
            FsEntry::from_path(&d.join("..")).unwrap().display_name(),
            ".."
        );
        assert!(FsEntry::from_path(&d.join("l2d")).unwrap().is_dir());
        assert!(FsEntry::from_path(&d.join("l2d")).unwrap().is_link());
        assert!(!FsEntry::from_path(&d.join("dir")).unwrap().is_link());
    }

    #[test]
    fn file_kind_uses_categories() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path();
        mk(d);
        std::fs::write(d.join("PHOTO.JPG"), b"").unwrap();
        std::fs::write(d.join("clip.mpg"), b"").unwrap();
        let files = crate::config::settings::Settings::default().files;
        let k = |n: &str| FsEntry::from_path(&d.join(n)).unwrap().file_kind(&files);
        assert_eq!(k("dir"), "dir");
        assert_eq!(k("run.sh"), "exe");
        assert_eq!(k("a.tar.gz"), "archive");
        assert_eq!(k("PHOTO.JPG"), "graphics");
        assert_eq!(k("clip.mpg"), "audio"); // in audio and video; first category wins, like lfm
        assert_eq!(k("l2f"), "reg");
    }

    #[test]
    fn non_utf8_name() {
        use std::os::unix::ffi::OsStrExt;
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join(std::ffi::OsStr::from_bytes(b"bad\xff.txt"));
        std::fs::write(&p, b"").unwrap();
        let e = FsEntry::from_path(&p).unwrap();
        assert_eq!(e.ext, ".txt");
        assert!(e.display_name().starts_with("bad"));
    }
}
