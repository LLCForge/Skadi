use std::fmt;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Os {
    Windows,
    Linux,
    Macos,
}

impl Os {
    pub fn current() -> Result<Self> {
        Self::from_target(std::env::consts::OS)
    }

    fn from_target(os: &str) -> Result<Self> {
        match os {
            "windows" => Ok(Self::Windows),
            "linux" => Ok(Self::Linux),
            "macos" => Ok(Self::Macos),
            other => Err(Error::UnsupportedOs(other.to_owned())),
        }
    }
}

impl fmt::Display for Os {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(match self {
            Self::Windows => "Windows",
            Self::Linux => "Linux",
            Self::Macos => "macOS",
        })
    }
}

/// `/etc/os-release` içeriği (freedesktop.org standardı).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OsRelease {
    pub id: String,
    pub id_like: Vec<String>,
    pub name: String,
    pub pretty_name: Option<String>,
    pub version_id: Option<String>,
}

impl OsRelease {
    const PATHS: [&str; 2] = ["/etc/os-release", "/usr/lib/os-release"];

    pub fn load() -> Option<Self> {
        Self::PATHS
            .iter()
            .find_map(|path| std::fs::read_to_string(path).ok())
            .map(|content| Self::parse(&content))
    }

    pub fn parse(content: &str) -> Self {
        let mut release = Self::default();
        for line in content.lines().map(str::trim) {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, raw)) = line.split_once('=') else {
                continue;
            };
            let value = unquote(raw.trim());
            match key.trim() {
                "ID" => release.id = value.to_ascii_lowercase(),
                "ID_LIKE" => {
                    release.id_like = value.split_whitespace().map(str::to_ascii_lowercase).collect();
                }
                "NAME" => release.name = value,
                "PRETTY_NAME" => release.pretty_name = Some(value),
                "VERSION_ID" => release.version_id = Some(value),
                _ => {}
            }
        }
        release
    }

    /// Dağıtım `id` ise ya da ondan türemişse doğru döner.
    pub fn is_like(&self, id: &str) -> bool {
        self.id == id || self.id_like.iter().any(|like| like == id)
    }

    pub fn display_name(&self) -> &str {
        self.pretty_name.as_deref().unwrap_or(&self.name)
    }
}

fn unquote(raw: &str) -> String {
    if raw.len() >= 2 {
        let inner = &raw[1..raw.len() - 1];
        if raw.starts_with('\'') && raw.ends_with('\'') {
            return inner.to_owned();
        }
        if raw.starts_with('"') && raw.ends_with('"') {
            let mut out = String::with_capacity(inner.len());
            let mut chars = inner.chars();
            while let Some(c) = chars.next() {
                if c == '\\' {
                    out.extend(chars.next());
                } else {
                    out.push(c);
                }
            }
            return out;
        }
    }
    raw.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_os_release() {
        let release = OsRelease::parse(
            "# yorum\nNAME=\"Ubuntu\"\nID=ubuntu\nID_LIKE=debian\nPRETTY_NAME=\"Ubuntu 24.04 \\\"LTS\\\"\"\nVERSION_ID='24.04'\nbozuk satır\n",
        );
        assert_eq!(release.id, "ubuntu");
        assert_eq!(release.id_like, ["debian"]);
        assert_eq!(release.display_name(), "Ubuntu 24.04 \"LTS\"");
        assert_eq!(release.version_id.as_deref(), Some("24.04"));
        assert!(release.is_like("debian"));
        assert!(!release.is_like("fedora"));
    }

    #[test]
    fn rejects_unknown_os() {
        assert!(Os::from_target("freebsd").is_err());
        assert_eq!(Os::from_target("macos").ok(), Some(Os::Macos));
    }
}
