//! Skadi'nin kendi verilerini (durum kaydı, denetim günlüğü, profiller) tuttuğu dizin.
//!
//! Windows `%LOCALAPPDATA%\Skadi`, macOS `~/Library/Application Support/Skadi`,
//! Linux `$XDG_STATE_HOME/skadi` (yoksa `~/.local/state/skadi`).
//!
//! Taşınabilir mod: programın yanında `skadi.portable` dosyası varsa veriler programın yanındaki
//! `skadi-data` klasöründe tutulur (ör. USB bellekten çalıştırırken).

use std::{
    env,
    path::{Path, PathBuf},
};

use crate::platform::Os;

pub const PORTABLE_MARKER: &str = "skadi.portable";

/// Programın klasöründe işaret dosyası varsa taşınabilir veri klasörü.
pub fn portable_dir(exe: &Path) -> Option<PathBuf> {
    let dir = exe.parent()?;
    dir.join(PORTABLE_MARKER)
        .is_file()
        .then(|| dir.join("skadi-data"))
        .filter(|d| d.is_absolute())
}

pub fn data_dir(os: Os) -> Option<PathBuf> {
    if let Some(dir) = env::current_exe().ok().and_then(|exe| portable_dir(&exe)) {
        return Some(dir);
    }
    let var = |name: &str| env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from);
    let dir = match os {
        Os::Windows => var("LOCALAPPDATA")?.join("Skadi"),
        Os::Macos => var("HOME")?.join("Library/Application Support/Skadi"),
        Os::Linux => var("XDG_STATE_HOME")
            .filter(|p| p.is_absolute())
            .or_else(|| var("HOME").map(|home| home.join(".local/state")))?
            .join("skadi"),
    };
    dir.is_absolute().then_some(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portable_mode_needs_the_marker() {
        let dir = env::temp_dir().join(format!("skadi-portable-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("skadi.exe");
        assert_eq!(portable_dir(&exe), None);
        std::fs::write(dir.join(PORTABLE_MARKER), "").unwrap();
        assert_eq!(portable_dir(&exe), Some(dir.join("skadi-data")));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
