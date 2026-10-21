//! Archive formats and the external commands that pack and unpack them.
//! Ported from lfm's compress.py; every file name is shell-quoted.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::jobs::shell_quote;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ArchiveType {
    Tbz2,
    Bz2,
    Tgz,
    Gz,
    Txz,
    Xz,
    Tlz,
    Lz,
    Tlz4,
    Lz4,
    Tar,
    Zip,
    Rar,
    SevenZ,
}

use ArchiveType as T;

/// lfm's detection order (`tbz2` before `bz2`, ...).
const ORDER: [ArchiveType; 14] = [
    T::Tbz2,
    T::Bz2,
    T::Tgz,
    T::Gz,
    T::Txz,
    T::Xz,
    T::Tlz,
    T::Lz,
    T::Tlz4,
    T::Lz4,
    T::Tar,
    T::Zip,
    T::Rar,
    T::SevenZ,
];

impl ArchiveType {
    pub fn exts(self) -> &'static [&'static str] {
        match self {
            T::Tbz2 => &[".tar.bz2", ".tbz2"],
            T::Bz2 => &[".bz2"],
            T::Tgz => &[".tar.gz", ".tgz", ".tar.Z"],
            T::Gz => &[".gz"],
            T::Txz => &[".tar.xz", ".txz"],
            T::Xz => &[".xz"],
            T::Tlz => &[".tar.lz", ".tlz"],
            T::Lz => &[".lz"],
            T::Tlz4 => &[".tar.lz4", ".tlz4"],
            T::Lz4 => &[".lz4"],
            T::Tar => &[".tar"],
            T::Zip => &[".zip", ".jar", ".apk"],
            T::Rar => &[".rar"],
            T::SevenZ => &[".7z"],
        }
    }

    pub fn can_vfs(self) -> bool {
        !self.is_plain_compressor()
    }

    pub fn need_tar(self) -> bool {
        matches!(self, T::Tbz2 | T::Tgz | T::Txz | T::Tlz | T::Tlz4)
    }

    pub fn is_plain_compressor(self) -> bool {
        matches!(self, T::Bz2 | T::Gz | T::Xz | T::Lz | T::Lz4)
    }

    pub fn name(self) -> &'static str {
        match self {
            T::Tbz2 => "tbz2",
            T::Bz2 => "bz2",
            T::Tgz => "tgz",
            T::Gz => "gz",
            T::Txz => "txz",
            T::Xz => "xz",
            T::Tlz => "tlz",
            T::Lz => "lz",
            T::Tlz4 => "tlz4",
            T::Lz4 => "lz4",
            T::Tar => "tar",
            T::Zip => "zip",
            T::Rar => "rar",
            T::SevenZ => "7z",
        }
    }

    /// The compressor behind a tar-piped or plain type.
    fn compressor(self) -> &'static str {
        match self {
            T::Tbz2 | T::Bz2 => "bzip2",
            T::Tgz | T::Gz => "gzip",
            T::Txz | T::Xz => "xz",
            T::Tlz | T::Lz => "lzip",
            T::Tlz4 | T::Lz4 => "lz4",
            T::Tar => "tar",
            T::Zip => "zip",
            T::Rar => "rar",
            T::SevenZ => "7z",
        }
    }
}

fn q(p: impl AsRef<std::ffi::OsStr>) -> String {
    shell_quote(p.as_ref()).to_string_lossy().into_owned()
}

fn with_ext(name: &std::ffi::OsStr, ext: &str) -> OsString {
    let mut n = name.to_os_string();
    n.push(ext);
    n
}

pub fn detect(path: &Path) -> Option<ArchiveType> {
    let name = path.file_name()?.to_string_lossy();
    ORDER
        .into_iter()
        .find(|t| t.exts().iter().any(|e| name.ends_with(e)))
}

/// Unpacks `archive` into the current directory.
pub fn uncompress_cmd(t: ArchiveType, archive: &Path) -> OsString {
    let a = q(archive);
    let cmd = match t {
        T::Tlz4 => format!("lz4 -q -d {a} -c | tar xfi -"),
        _ if t.need_tar() => format!("{} -d {a} -c | tar xfi -", t.compressor()),
        T::Lz4 => format!("lz4 --rm -m -q -d {a}"),
        _ if t.is_plain_compressor() => format!("{} -d {a}", t.compressor()),
        T::Tar => format!("tar xf {a}"),
        T::Zip => format!("unzip -o -q {a}"),
        T::Rar => format!("rar x {a}"),
        _ => format!("7z x {a}"),
    };
    OsString::from(cmd)
}

/// Packs `path` (run in its parent directory); `None` where lfm refuses:
/// a plain compressor on a directory, or a tar format on a file. Names are
/// passed as `./name` so a leading "-" is never read as an option.
pub fn compress_cmd(t: ArchiveType, path: &Path) -> Option<OsString> {
    let name = path.file_name()?;
    let (n, out) = (
        q(dot_slash(name)),
        q(dot_slash(&with_ext(name, t.exts()[0]))),
    );
    let is_dir = path.is_dir();
    let cmd = match t {
        _ if t.is_plain_compressor() && is_dir => return None,
        _ if t.need_tar() && !is_dir => return None,
        T::Tlz4 => format!("tar cf - {n} | lz4 -9 -q > {out}"),
        _ if t.need_tar() => format!("tar cf - {n} | {} > {out}", t.compressor()),
        T::Lz4 => format!("lz4 --rm -m -q {n}"),
        _ if t.is_plain_compressor() => format!("{} {n}", t.compressor()),
        T::Tar => format!("tar cf {out} {n}"),
        T::Zip => format!("zip -qr {out} {n}"),
        T::Rar => format!("rar a {out} {n}"),
        _ => format!("7z a {out} {n}"),
    };
    Some(OsString::from(cmd))
}

fn dot_slash(name: &std::ffi::OsStr) -> OsString {
    let mut s = OsString::from("./");
    s.push(name);
    s
}

/// `dest` with the type's main extension, unless it already ends with it.
pub fn compress2_dest(t: ArchiveType, dest: &Path) -> PathBuf {
    let ext = t.exts()[0];
    if dest.to_string_lossy().ends_with(ext) {
        dest.to_path_buf()
    } else {
        PathBuf::from(with_ext(dest.as_os_str(), ext))
    }
}

/// Packs `members` (top-level names of the current directory, dotfiles
/// included) into `dest` (VFS rebuild). Every member is passed as `./name`,
/// never through a shell glob, so names like `--checkpoint-action=...` stay names.
pub fn compress2_cmd(t: ArchiveType, dest: &Path, members: &[OsString]) -> OsString {
    let d = q(compress2_dest(t, dest));
    let m: Vec<String> = members.iter().map(|n| q(dot_slash(n))).collect();
    let tar_files = if m.is_empty() {
        "--files-from /dev/null".to_string()
    } else {
        format!("-- {}", m.join(" "))
    };
    let m = m.join(" ");
    let cmd = match t {
        T::Tlz4 => format!("tar cf - {tar_files} | lz4 -9 -q > {d}"),
        _ if t.need_tar() => format!("tar cf - {tar_files} | {} > {d}", t.compressor()),
        T::Lz4 => format!("lz4 --rm -m -q {m}"),
        _ if t.is_plain_compressor() => format!("{} {m}", t.compressor()),
        T::Tar => format!("tar cf {d} {tar_files}"),
        T::Zip => format!("zip -qr {d} {m}"),
        T::Rar => format!("rar a -r {d} {m}"),
        _ => format!("7z a {d} {m}"),
    };
    OsString::from(cmd)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detection_order() {
        assert_eq!(detect(Path::new("a.tar.bz2")), Some(ArchiveType::Tbz2));
        assert_eq!(detect(Path::new("a.bz2")), Some(ArchiveType::Bz2));
        assert_eq!(detect(Path::new("a.tar.Z")), Some(ArchiveType::Tgz));
        assert_eq!(detect(Path::new("x.jar")), Some(ArchiveType::Zip));
        assert_eq!(detect(Path::new("x.txt")), None);
        assert!(ArchiveType::Tgz.can_vfs() && !ArchiveType::Gz.can_vfs());
        assert_eq!(ArchiveType::SevenZ.name(), "7z");
    }

    #[test]
    fn commands_quote_names() {
        let p = Path::new("/d/it's $x.tar.gz");
        assert_eq!(
            uncompress_cmd(ArchiveType::Tgz, p),
            OsString::from("gzip -d '/d/it'\\''s $x.tar.gz' -c | tar xfi -")
        );
        let td = tempfile::tempdir().unwrap();
        std::fs::create_dir(td.path().join("my dir")).unwrap();
        assert_eq!(
            compress_cmd(ArchiveType::Tgz, &td.path().join("my dir")).unwrap(),
            OsString::from("tar cf - './my dir' | gzip > './my dir.tar.gz'")
        );
        assert_eq!(
            compress2_dest(ArchiveType::Zip, Path::new("/t/x")),
            PathBuf::from("/t/x.zip")
        );
        assert_eq!(
            compress2_dest(ArchiveType::Zip, Path::new("/t/x.zip")),
            PathBuf::from("/t/x.zip")
        );
        assert_eq!(
            compress2_cmd(
                ArchiveType::Tgz,
                Path::new("/t/o"),
                &["a".into(), "-v".into()]
            ),
            OsString::from("tar cf - -- './a' './-v' | gzip > '/t/o.tar.gz'")
        );
        assert_eq!(
            uncompress_cmd(ArchiveType::Lz4, Path::new("a.lz4")),
            OsString::from("lz4 --rm -m -q -d 'a.lz4'")
        );
    }

    #[test]
    fn refusals_follow_lfm() {
        let t = tempfile::tempdir().unwrap();
        std::fs::create_dir(t.path().join("d")).unwrap();
        std::fs::write(t.path().join("f"), b"x").unwrap();
        assert!(compress_cmd(ArchiveType::Gz, &t.path().join("d")).is_none());
        assert!(compress_cmd(ArchiveType::Tgz, &t.path().join("f")).is_none());
        assert_eq!(
            compress_cmd(ArchiveType::Gz, &t.path().join("f")).unwrap(),
            OsString::from("gzip './f'")
        );
        assert_eq!(
            compress_cmd(ArchiveType::Zip, &t.path().join("d")).unwrap(),
            OsString::from("zip -qr './d.zip' './d'")
        );
    }

    #[test]
    fn real_tar_gz_round_trip() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path();
        std::fs::create_dir(d.join("my dir")).unwrap();
        std::fs::write(d.join("my dir/a"), b"A").unwrap();
        let (_, err) = crate::jobs::run_in_cli(
            &compress_cmd(ArchiveType::Tgz, &d.join("my dir")).unwrap(),
            d,
        )
        .unwrap();
        assert!(err.is_empty(), "{err}");
        let out = d.join("out");
        std::fs::create_dir(&out).unwrap();
        crate::jobs::run_in_cli(
            &uncompress_cmd(ArchiveType::Tgz, &d.join("my dir.tar.gz")),
            &out,
        )
        .unwrap();
        assert_eq!(std::fs::read(out.join("my dir/a")).unwrap(), b"A");
    }

    #[test]
    fn dash_names_compress_as_files() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path();
        std::fs::create_dir(d.join("-v")).unwrap();
        std::fs::write(d.join("-v/a"), b"A").unwrap();
        std::fs::write(d.join("-k.txt"), b"K").unwrap();
        let (_, err) =
            crate::jobs::run_in_cli(&compress_cmd(ArchiveType::Tgz, &d.join("-v")).unwrap(), d)
                .unwrap();
        assert!(err.is_empty(), "{err}");
        let (_, err) = crate::jobs::run_in_cli(
            &compress_cmd(ArchiveType::Gz, &d.join("-k.txt")).unwrap(),
            d,
        )
        .unwrap();
        assert!(err.is_empty(), "{err}");
        assert!(d.join("-k.txt.gz").is_file());
        let out = d.join("out");
        std::fs::create_dir(&out).unwrap();
        crate::jobs::run_in_cli(
            &uncompress_cmd(ArchiveType::Tgz, &d.join("-v.tar.gz")),
            &out,
        )
        .unwrap();
        assert_eq!(std::fs::read(out.join("-v/a")).unwrap(), b"A");
    }
}
