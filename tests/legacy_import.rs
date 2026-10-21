use yafm::config::legacy::import_legacy;
use yafm::config::paths::ConfigPaths;

#[test]
fn import_writes_then_skips_without_force() {
    let d = tempfile::tempdir().unwrap();
    let p = ConfigPaths {
        dir: d.path().join("yafm"),
        legacy_dir: d.path().join("lfm"),
    };
    std::fs::create_dir(&p.legacy_dir).unwrap();
    for f in ["lfm.ini", "lfm.keys", "lfm.theme"] {
        std::fs::copy(format!("tests/fixtures/legacy/{f}"), p.legacy_dir.join(f)).unwrap();
    }
    let r = import_legacy(&p, false).unwrap();
    assert_eq!(r.written.len(), 3);
    let r = import_legacy(&p, false).unwrap();
    assert_eq!(r.skipped.len(), 3);
    let r = import_legacy(&p, true).unwrap();
    assert_eq!(r.written.len(), 3);
    assert!(p.legacy_dir.join("lfm.ini").exists());
    assert!(p.dir.join("config.toml").exists());
}

#[test]
fn legacy_files_are_not_modified() {
    let d = tempfile::tempdir().unwrap();
    let p = ConfigPaths {
        dir: d.path().join("yafm"),
        legacy_dir: d.path().join("lfm"),
    };
    std::fs::create_dir(&p.legacy_dir).unwrap();
    std::fs::copy("tests/fixtures/legacy/lfm.ini", p.legacy_ini()).unwrap();
    let before = std::fs::read(p.legacy_ini()).unwrap();
    let r = import_legacy(&p, false).unwrap();
    assert_eq!(r.written, vec![p.config()]);
    assert_eq!(std::fs::read(p.legacy_ini()).unwrap(), before);
}
