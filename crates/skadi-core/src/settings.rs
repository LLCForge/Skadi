//! Kullanıcı ayarları: veri dizinindeki `settings.toml`.
//!
//! ```toml
//! [install]
//! prefer = ["flatpak", "apt"]   # paket yöneticisi tercih sırası
//! flatpak_user = true           # Flatpak'i root gerektirmeden kullanıcıya kur
//! ```

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::{
    error::{Error, Result},
    manager::ManagerId,
    platform::Os,
};

const MAX_BYTES: u64 = 64 * 1024;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub install: InstallSettings,
    pub state: StateSettings,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StateSettings {
    /// Tweak durum kaydını işletim sisteminin anahtar kasasındaki anahtarla şifrele.
    pub encrypt: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct InstallSettings {
    /// Önce bu yöneticiler denenir; listede olmayanlar varsayılan sırayla gelir.
    pub prefer: Vec<ManagerId>,
    /// Flatpak uygulamalarını sistem yerine kullanıcıya kur (`--user`).
    pub flatpak_user: bool,
}

impl Settings {
    pub fn path(os: Os) -> Option<PathBuf> {
        crate::paths::data_dir(os).map(|dir| dir.join("settings.toml"))
    }

    /// Dosya yoksa varsayılanlar; bozuksa açık bir hata (sessizce yok sayılmaz).
    pub fn load(os: Os) -> Result<Self> {
        let Some(path) = Self::path(os) else {
            return Ok(Self::default());
        };
        let fail = |reason: String| Error::Profile(format!("{}: {reason}", path.display()));
        match std::fs::metadata(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(fail(e.to_string())),
            Ok(meta) if meta.len() > MAX_BYTES => return Err(fail("file is too large".into())),
            Ok(_) => {}
        }
        let text = std::fs::read_to_string(&path).map_err(|e| fail(e.to_string()))?;
        Self::parse(&text).map_err(|e| fail(e.message(crate::Lang::En)))
    }

    pub fn parse(text: &str) -> Result<Self> {
        toml::from_str(text).map_err(|e| Error::Profile(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_rejects_settings() {
        let settings = Settings::parse("[install]\nprefer = [\"flatpak\", \"apt\"]\nflatpak_user = true\n").unwrap();
        assert_eq!(settings.install.prefer, [ManagerId::Flatpak, ManagerId::Apt]);
        assert!(settings.install.flatpak_user);
        assert_eq!(Settings::parse("").unwrap(), Settings::default());
        assert!(Settings::parse("[install]\nprefer = [\"npm\"]\n").is_err());
        assert!(Settings::parse("[install]\nrun = \"x\"\n").is_err());
    }
}
