//! Profiller ve hazır setler.
//!
//! Profil, uygulama ve tweak kimliklerinden oluşan bir listedir; komut, betik ya da yol
//! içeremez. Başkasından gelen bir profil bu yüzden yalnızca katalogda zaten tanımlı olan
//! işlemleri seçebilir. Hazır setler (`catalog/presets.toml`) ikiliye gömülü profillerdir.

use std::{collections::HashSet, path::Path};

use serde::{Deserialize, Serialize};

use crate::{
    catalog::Catalog,
    error::{Error, Result},
    i18n::Text,
    tweak::TweakCatalog,
};

const PRESETS: &str = include_str!("../../../catalog/presets.toml");
const PROFILE_VERSION: u32 = 1;
const MAX_PROFILE_BYTES: u64 = 256 * 1024;

fn version_one() -> u32 {
    PROFILE_VERSION
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    #[serde(default = "version_one")]
    pub version: u32,
    #[serde(default)]
    pub apps: Vec<String>,
    #[serde(default)]
    pub tweaks: Vec<String>,
    /// VS Code / VSCodium eklentileri (`yayıncı.ad`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub vscode_extensions: Vec<String>,
}

/// `yayıncı.ad` biçimi: harf, rakam ve `-`, tam olarak bir nokta.
pub fn is_valid_extension_id(id: &str) -> bool {
    let mut parts = id.split('.');
    let ok = |p: Option<&str>| {
        p.is_some_and(|p| {
            !p.is_empty() && !p.starts_with('-') && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        })
    };
    id.len() <= 128 && ok(parts.next()) && ok(parts.next()) && parts.next().is_none()
}

impl Profile {
    /// Dosyadan okur ve kataloglara karşı doğrular.
    pub fn load(path: &Path, apps: &Catalog, tweaks: &TweakCatalog) -> Result<Self> {
        let io = |e: std::io::Error| Error::Profile(format!("{}: {e}", path.display()));
        // SECURITY FIX: CWE-400 başkasından gelen dosya; boyut sınırı belleği korur.
        if std::fs::metadata(path).map_err(io)?.len() > MAX_PROFILE_BYTES {
            return Err(Error::Profile("profile file is too large".into()));
        }
        let text = std::fs::read_to_string(path).map_err(io)?;
        Self::parse(&text, apps, tweaks)
    }

    /// Metinden okur ve doğrular (ör. imzası doğrulanmış içerik).
    pub fn parse(text: &str, apps: &Catalog, tweaks: &TweakCatalog) -> Result<Self> {
        if text.len() as u64 > MAX_PROFILE_BYTES {
            return Err(Error::Profile("profile file is too large".into()));
        }
        let profile: Self = toml::from_str(text).map_err(|e| Error::Profile(e.to_string()))?;
        profile.validate(apps, tweaks)?;
        Ok(profile)
    }

    pub fn validate(&self, apps: &Catalog, tweaks: &TweakCatalog) -> Result<()> {
        if self.version != PROFILE_VERSION {
            return Err(Error::Profile(format!("unsupported profile version {}", self.version)));
        }
        if let Some(id) = self.apps.iter().find(|id| apps.get(id).is_none()) {
            return Err(Error::UnknownApp(id.clone()));
        }
        if let Some(id) = self.tweaks.iter().find(|id| tweaks.get(id).is_none()) {
            return Err(Error::UnknownTweak(id.clone()));
        }
        if let Some(bad) = self.vscode_extensions.iter().find(|id| !is_valid_extension_id(id)) {
            return Err(Error::Profile(format!("invalid editor extension id {bad}")));
        }
        let mut seen = HashSet::new();
        if let Some(id) = self
            .apps
            .iter()
            .chain(&self.tweaks)
            .find(|id| !seen.insert(id.as_str()))
        {
            return Err(Error::Profile(format!("{id} is listed twice")));
        }
        Ok(())
    }

    pub fn to_toml(&self) -> Result<String> {
        let body = toml::to_string_pretty(self).map_err(|e| Error::Profile(e.to_string()))?;
        Ok(format!(
            "# Skadi profile. Apply with: skadi profile apply <this file>\n{body}"
        ))
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        std::fs::write(path, self.to_toml()?).map_err(|e| Error::Profile(format!("{}: {e}", path.display())))
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preset {
    pub id: String,
    pub name: Text,
    pub description: Text,
    #[serde(default)]
    pub apps: Vec<String>,
    #[serde(default)]
    pub tweaks: Vec<String>,
    #[serde(default)]
    pub vscode_extensions: Vec<String>,
}

impl Preset {
    pub fn profile(&self) -> Profile {
        Profile {
            version: PROFILE_VERSION,
            apps: self.apps.clone(),
            tweaks: self.tweaks.clone(),
            vscode_extensions: self.vscode_extensions.clone(),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPresets {
    #[serde(rename = "preset")]
    presets: Vec<Preset>,
}

pub fn presets() -> Result<Vec<Preset>> {
    let raw: RawPresets = toml::from_str(PRESETS).map_err(|e| Error::Catalog(format!("presets.toml: {e}")))?;
    Ok(raw.presets)
}

pub fn preset(id: &str) -> Result<Preset> {
    presets()?
        .into_iter()
        .find(|p| p.id == id)
        .ok_or_else(|| Error::Profile(format!("no such preset: {id}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_reference_existing_ids() {
        let apps = Catalog::builtin().unwrap();
        let tweaks = TweakCatalog::builtin().unwrap();
        let presets = presets().unwrap();
        assert!(presets.len() >= 7);
        for preset in presets {
            preset
                .profile()
                .validate(&apps, &tweaks)
                .unwrap_or_else(|e| panic!("{}: {e}", preset.id));
        }
    }

    #[test]
    fn profile_round_trips_and_rejects_unknown_content() {
        let apps = Catalog::builtin().unwrap();
        let tweaks = TweakCatalog::builtin().unwrap();
        let profile = Profile {
            version: 1,
            apps: vec!["git".into()],
            tweaks: vec!["win-telemetry".into()],
            vscode_extensions: vec!["rust-lang.rust-analyzer".into()],
        };
        assert!(is_valid_extension_id("editorconfig.editorconfig"));
        assert!(!is_valid_extension_id("--evil.x"));
        assert!(!is_valid_extension_id("a.b.c"));
        let text = profile.to_toml().unwrap();
        let parsed: Profile = toml::from_str(&text).unwrap();
        assert_eq!(parsed, profile);
        parsed.validate(&apps, &tweaks).unwrap();

        for bad in [
            "apps = [\"nope\"]",
            "tweaks = [\"nope\"]",
            "apps = [\"git\", \"git\"]",
            "version = 2",
            "run = \"rm -rf /\"",
        ] {
            let result = toml::from_str::<Profile>(bad)
                .map_err(|e| Error::Profile(e.to_string()))
                .and_then(|p| p.validate(&apps, &tweaks));
            assert!(result.is_err(), "{bad}");
        }
    }
}
