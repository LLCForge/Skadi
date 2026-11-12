//! Kurum politikası: bu makinede hangi uygulamaların ve tweak'lerin kullanılabileceği. Politika
//! yöneticinin kurduğu, imzalı bir dosyadır. İmza doğrulanamazsa Skadi hiçbir şeyi kurmaz ya da
//! uygulamaz (hata durumunda kapalı).

use std::path::PathBuf;

use serde::Deserialize;

use crate::{
    error::{Error, Result},
    platform::Os,
    signing,
};

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    /// Yalnızca bu uygulamalar kurulabilir; yoksa uygulamalar kısıtlanmaz.
    #[serde(default)]
    pub allowed_apps: Option<Vec<String>>,
    /// Yalnızca bu tweak'ler uygulanabilir; yoksa tweak'ler kısıtlanmaz.
    #[serde(default)]
    pub allowed_tweaks: Option<Vec<String>>,
}

impl Policy {
    pub fn allows_app(&self, id: &str) -> bool {
        self.allowed_apps
            .as_ref()
            .is_none_or(|list| list.iter().any(|a| a == id))
    }

    pub fn allows_tweak(&self, id: &str) -> bool {
        self.allowed_tweaks
            .as_ref()
            .is_none_or(|list| list.iter().any(|t| t == id))
    }
}

/// Makine genelindeki politika klasörü (yalnızca yöneticinin yazabildiği bir yer).
pub fn policy_dir(os: Os) -> PathBuf {
    match os {
        Os::Windows => PathBuf::from(std::env::var_os("ProgramData").unwrap_or_else(|| "C:\\ProgramData".into()))
            .join("Skadi")
            .join("policy"),
        Os::Linux => PathBuf::from("/etc/skadi/policy"),
        Os::Macos => PathBuf::from("/Library/Application Support/Skadi/policy"),
    }
}

pub const POLICY_FILE: &str = "policy.toml";
pub const PUBLIC_KEY_FILE: &str = "policy.pub";

/// Kurulu politika; yoksa `None`. Dosya var ama imzası ya da biçimi geçersizse hata döner ve
/// çağıranlar işlemi durdurur.
pub fn load(os: Os) -> Result<Option<Policy>> {
    let dir = policy_dir(os);
    let file = dir.join(POLICY_FILE);
    if !file.exists() {
        return Ok(None);
    }
    let data = signing::read_verified(&file, &dir.join(PUBLIC_KEY_FILE))
        .map_err(|e| Error::Policy(format!("policy cannot be trusted: {}", e.message(crate::Lang::En))))?;
    let text = String::from_utf8(data).map_err(|_| Error::Policy("policy is not UTF-8".into()))?;
    parse(&text).map(Some)
}

pub fn parse(text: &str) -> Result<Policy> {
    toml::from_str(text).map_err(|e| Error::Policy(format!("policy is invalid: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restricts_only_what_is_listed() {
        let policy: Policy = toml::from_str("allowed_apps = [\"git\"]").unwrap();
        assert!(policy.allows_app("git") && !policy.allows_app("steam"));
        assert!(policy.allows_tweak("win-telemetry"));
        assert!(toml::from_str::<Policy>("allow_everything = true").is_err());
    }
}
