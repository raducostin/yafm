use std::path::Path;
use std::process::Command;

fn yafm(cfg: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_yafm"))
        .env("XDG_CONFIG_HOME", cfg)
        .env("HOME", cfg.join("home"))
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn first_run_creates_files() {
    let d = tempfile::tempdir().unwrap();
    let o = yafm(d.path(), &["--check-config"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stdout));
    for f in ["config.toml", "keys.toml", "theme.toml"] {
        assert!(d.path().join("yafm").join(f).exists(), "{f}");
    }
}

#[test]
fn restore_keys_deletes_file() {
    let d = tempfile::tempdir().unwrap();
    yafm(d.path(), &["--check-config"]);
    let o = yafm(d.path(), &["--restore-keys"]);
    assert_eq!(
        String::from_utf8_lossy(&o.stdout).trim(),
        "yafm: restore default key bindings!"
    );
    assert!(!d.path().join("yafm/keys.toml").exists());
}

#[test]
fn bad_path_exits_255() {
    let d = tempfile::tempdir().unwrap();
    let o = yafm(d.path(), &["/no/such/dir"]);
    assert_eq!(o.status.code(), Some(255));
    assert!(String::from_utf8_lossy(&o.stdout).starts_with("yafm - ERROR: cannot initialize path"));
}

#[test]
fn auto_imports_legacy_on_first_run() {
    let d = tempfile::tempdir().unwrap();
    let legacy = d.path().join("home/.config/lfm");
    std::fs::create_dir_all(&legacy).unwrap();
    std::fs::copy("tests/fixtures/legacy/lfm.ini", legacy.join("lfm.ini")).unwrap();
    yafm(d.path(), &["--check-config"]);
    assert!(std::fs::read_to_string(d.path().join("yafm/config.toml"))
        .unwrap()
        .contains("sort_type = \"bySize\""));
}

#[test]
fn check_config_reports_problems() {
    let d = tempfile::tempdir().unwrap();
    yafm(d.path(), &["--check-config"]);
    std::fs::write(d.path().join("yafm/keys.toml"), "[main]\nfly = [\"x\"]\n").unwrap();
    let o = yafm(d.path(), &["--check-config"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&o.stdout).contains("fly"));
}

#[test]
fn import_legacy_flag_respects_force() {
    let d = tempfile::tempdir().unwrap();
    let legacy = d.path().join("home/.config/lfm");
    std::fs::create_dir_all(&legacy).unwrap();
    std::fs::copy("tests/fixtures/legacy/lfm.keys", legacy.join("lfm.keys")).unwrap();
    yafm(d.path(), &["--check-config"]); // creates default TOML files, so auto-import is skipped
    let o = yafm(d.path(), &["--import-legacy"]);
    assert!(o.status.success());
    assert!(String::from_utf8_lossy(&o.stdout).contains("skipped"));
    let o = yafm(d.path(), &["--import-legacy", "--force"]);
    assert!(String::from_utf8_lossy(&o.stdout).contains("keys.toml"));
    assert!(std::fs::read_to_string(d.path().join("yafm/keys.toml"))
        .unwrap()
        .contains("imported from lfm.keys"));
}

#[test]
fn plain_run_without_tty_refuses() {
    let d = tempfile::tempdir().unwrap();
    let o = yafm(d.path(), &[]);
    assert_eq!(o.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&o.stdout).contains("not a terminal"));
}

#[test]
fn help_lists_cwd_file() {
    let d = tempfile::tempdir().unwrap();
    let o = yafm(d.path(), &["--help"]);
    assert!(String::from_utf8_lossy(&o.stdout).contains("--cwd-file"));
}
