//! Quit-and-cd: on quit with chdir, the last directory is written to the file
//! given with `--cwd-file`, so a shell function can `cd` there (lfm wrote a
//! fixed /tmp/lfm-$PPID.path instead).

use std::path::Path;

use std::os::unix::ffi::OsStrExt;

/// Writes `dir` (raw bytes, no newline) to `file`.
pub fn write_cwd_file(file: &Path, dir: &Path) -> Result<(), String> {
    std::fs::write(file, dir.as_os_str().as_bytes())
        .map_err(|e| format!("yafm - ERROR: cannot write {}\n{e}", file.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_the_dir_without_newline() {
        let t = tempfile::tempdir().unwrap();
        let f = t.path().join("cwd");
        write_cwd_file(&f, Path::new("/some/where")).unwrap();
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "/some/where");
        write_cwd_file(&f, Path::new("/x")).unwrap();
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "/x");
    }

    #[test]
    fn unwritable_file_is_an_error_message() {
        let e =
            write_cwd_file(Path::new("/nonexistent-dir-yafm/cwd"), Path::new("/x")).unwrap_err();
        assert!(
            e.starts_with("yafm - ERROR: cannot write /nonexistent-dir-yafm/cwd\n"),
            "{e}"
        );
    }
}
