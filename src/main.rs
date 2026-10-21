use std::path::{Path, PathBuf};
use std::process::exit;

use std::io::IsTerminal;

use clap::Parser;
use log::{info, warn, LevelFilter};

use yafm::config::legacy::import_legacy;
use yafm::config::paths::ConfigPaths;
use yafm::cwdfile::write_cwd_file;
use yafm::startup::load_all;
use yafm::ui::run::run_ui;

/// yafm - Yet Another File Manager
#[derive(Parser, Debug)]
#[command(version, about)]
struct Args {
    /// Enable debug level in log file
    #[arg(short, long)]
    debug: bool,
    /// Accepted for lfm compatibility; wide chars are always supported
    #[arg(short = 'w', long)]
    use_wide_chars: bool,
    /// Restore default configuration
    #[arg(long)]
    restore_config: bool,
    /// Restore default key bindings
    #[arg(long)]
    restore_keys: bool,
    /// Restore default theme
    #[arg(long)]
    restore_theme: bool,
    /// Delete history
    #[arg(long)]
    delete_history: bool,
    /// Import lfm 3.1 configuration from ~/.config/lfm
    #[arg(long)]
    import_legacy: bool,
    /// With --import-legacy: overwrite existing yafm files
    #[arg(long, requires = "import_legacy")]
    force: bool,
    /// Load all configuration files, print problems, exit 1 if any
    #[arg(long)]
    check_config: bool,
    /// On quit with chdir (q), write the last directory to FILE (for a shell `cd`)
    #[arg(long, value_name = "FILE")]
    cwd_file: Option<PathBuf>,
    /// Path to show in left pane
    #[arg(default_value = ".")]
    path1: PathBuf,
    /// Path to show in right pane
    #[arg(default_value = ".")]
    path2: PathBuf,
}

fn delete_item_and_exit(path: &Path, text: &str) -> ! {
    match std::fs::remove_file(path) {
        Ok(()) => println!("yafm: {text}!"),
        Err(e) => println!("yafm - ERROR: {text}\n{e}"),
    }
    exit(0)
}

fn check_dir(p: &Path) -> std::io::Result<()> {
    if std::fs::metadata(p)?.is_dir() {
        Ok(())
    } else {
        Err(std::io::Error::other(format!(
            "{}: Not a directory",
            p.display()
        )))
    }
}

fn print_import_report(report: &yafm::config::legacy::ImportReport) {
    for p in &report.written {
        println!("yafm: imported lfm configuration into {}", p.display());
    }
    for p in &report.skipped {
        println!(
            "yafm: skipped {} (already exists, use --force to overwrite)",
            p.display()
        );
    }
    for w in &report.warnings {
        println!("yafm: warning: {w}");
    }
}

fn main() {
    let args = Args::parse();
    let paths = ConfigPaths::from_env();

    if let Err(e) = std::fs::create_dir_all(&paths.dir) {
        println!(
            "yafm - ERROR: cannot create configuration folder {}\n{e}",
            paths.dir.display()
        );
        exit(255);
    }
    if let Ok(f) = std::fs::File::create(paths.log()) {
        let level = if args.debug {
            LevelFilter::Debug
        } else {
            LevelFilter::Info
        };
        let _ = simplelog::WriteLogger::init(level, simplelog::Config::default(), f);
    }

    if args.restore_config {
        delete_item_and_exit(&paths.config(), "restore default configuration");
    }
    if args.restore_keys {
        delete_item_and_exit(&paths.keys(), "restore default key bindings");
    }
    if args.restore_theme {
        delete_item_and_exit(&paths.theme(), "restore default theme");
    }
    if args.delete_history {
        delete_item_and_exit(&paths.history(), "delete history");
    }

    if args.import_legacy {
        match import_legacy(&paths, args.force) {
            Ok(report) => {
                if report.written.is_empty() && report.skipped.is_empty() {
                    println!(
                        "yafm: no lfm configuration found in {}",
                        paths.legacy_dir.display()
                    );
                }
                print_import_report(&report);
                exit(0)
            }
            Err(e) => {
                println!("yafm - ERROR: cannot import lfm configuration\n{e}");
                exit(255)
            }
        }
    }

    if let Err(e) = check_dir(&args.path1).and_then(|_| check_dir(&args.path2)) {
        println!("yafm - ERROR: cannot initialize path\n{e}");
        exit(255);
    }

    let first_run = ![paths.config(), paths.keys(), paths.theme()]
        .iter()
        .any(|p| p.exists());
    if first_run {
        match import_legacy(&paths, false) {
            Ok(report) => print_import_report(&report),
            Err(e) => println!("yafm: warning: cannot import lfm configuration: {e}"),
        }
    }

    info!("Start yafm");
    let loaded = load_all(&paths);
    for n in &loaded.notes {
        info!("{n}");
    }
    for w in &loaded.warnings {
        warn!("{w}");
    }

    if args.check_config {
        for w in &loaded.warnings {
            println!("{w}");
        }
        exit(if loaded.warnings.is_empty() { 0 } else { 1 });
    }

    if !std::io::stdout().is_terminal() || !std::io::stdin().is_terminal() {
        println!("yafm: not a terminal");
        exit(1);
    }
    match run_ui(loaded, paths, &args.path1, &args.path2) {
        Ok(path) => {
            info!("End yafm, returns: {path:?}");
            if let (Some(file), Some(dir)) = (&args.cwd_file, &path) {
                if let Err(msg) = write_cwd_file(file, dir) {
                    println!("{msg}");
                }
            }
        }
        Err(e) => {
            println!("yafm - ERROR: {e}");
            exit(255);
        }
    }
}
