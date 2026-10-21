//! Locations of yafm's own config files and of the legacy lfm 3.1 files.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigPaths {
    pub dir: PathBuf,
    pub legacy_dir: PathBuf,
}

impl ConfigPaths {
    /// `$XDG_CONFIG_HOME/yafm` (else `~/.config/yafm`); legacy always `~/.config/lfm`,
    /// because lfm 3.1 hardcoded that path and ignored XDG.
    pub fn from_env() -> ConfigPaths {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .unwrap_or_else(|| home.join(".config"));
        ConfigPaths::from_base(&base, &home)
    }

    /// `base` is the XDG config home (e.g. `~/.config`), `home` the user's home.
    pub fn from_base(base: &Path, home: &Path) -> ConfigPaths {
        ConfigPaths {
            dir: base.join("yafm"),
            legacy_dir: home.join(".config").join("lfm"),
        }
    }

    pub fn config(&self) -> PathBuf {
        self.dir.join("config.toml")
    }
    pub fn keys(&self) -> PathBuf {
        self.dir.join("keys.toml")
    }
    pub fn theme(&self) -> PathBuf {
        self.dir.join("theme.toml")
    }
    pub fn history(&self) -> PathBuf {
        self.dir.join("history.json")
    }
    pub fn log(&self) -> PathBuf {
        self.dir.join("yafm.log")
    }
    pub fn legacy_ini(&self) -> PathBuf {
        self.legacy_dir.join("lfm.ini")
    }
    pub fn legacy_keys(&self) -> PathBuf {
        self.legacy_dir.join("lfm.keys")
    }
    pub fn legacy_theme(&self) -> PathBuf {
        self.legacy_dir.join("lfm.theme")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_under_base() {
        let p = ConfigPaths::from_base(Path::new("/x/.config"), Path::new("/x"));
        assert_eq!(p.dir, PathBuf::from("/x/.config/yafm"));
        assert_eq!(p.legacy_dir, PathBuf::from("/x/.config/lfm"));
        assert_eq!(p.config(), PathBuf::from("/x/.config/yafm/config.toml"));
        assert_eq!(p.keys(), PathBuf::from("/x/.config/yafm/keys.toml"));
        assert_eq!(p.theme(), PathBuf::from("/x/.config/yafm/theme.toml"));
        assert_eq!(p.history(), PathBuf::from("/x/.config/yafm/history.json"));
        assert_eq!(p.log(), PathBuf::from("/x/.config/yafm/yafm.log"));
        assert_eq!(p.legacy_ini(), PathBuf::from("/x/.config/lfm/lfm.ini"));
        assert_eq!(p.legacy_keys(), PathBuf::from("/x/.config/lfm/lfm.keys"));
        assert_eq!(p.legacy_theme(), PathBuf::from("/x/.config/lfm/lfm.theme"));
    }

    #[test]
    fn legacy_dir_ignores_xdg_like_lfm() {
        // lfm 3.1 hardcodes ~/.config/lfm (common.py CONFIG_DIR), whatever XDG_CONFIG_HOME says.
        let p = ConfigPaths::from_base(Path::new("/xdg"), Path::new("/home/u"));
        assert_eq!(p.dir, PathBuf::from("/xdg/yafm"));
        assert_eq!(p.legacy_dir, PathBuf::from("/home/u/.config/lfm"));
    }

    #[test]
    fn from_env_ends_in_yafm() {
        assert!(ConfigPaths::from_env().dir.ends_with("yafm"));
    }
}
