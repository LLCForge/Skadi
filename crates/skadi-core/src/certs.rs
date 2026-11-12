//! Güvenilir kök sertifikaların denetimi (yalnızca okur). Kötü amaçlı yazılımlar ve bazı "güvenlik"
//! ürünleri trafiği okumak için kök sertifika ekler; burada listelenen bir sertifika bunun işareti
//! olabilir.

use std::collections::HashSet;

use serde::Serialize;

use crate::{audit::stdout_of, platform::Os};

const MOZILLA_ROOTS: &str = include_str!("../../../catalog/mozilla-roots.txt");

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RootCert {
    pub store: String,
    pub subject: String,
    /// Bilinmiyorsa boş (ör. Linux'ta yalnızca dosya adı okunur).
    pub sha256: String,
    /// Mozilla'nın güvendiği köklerden biri mi.
    pub in_mozilla: bool,
}

pub fn mozilla_roots() -> HashSet<String> {
    MOZILLA_ROOTS
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_owned)
        .collect()
}

/// `depo|parmakizi|konu` satırları (Windows PowerShell çıktısı).
pub fn parse_windows_roots(text: &str, known: &HashSet<String>) -> Vec<RootCert> {
    let mut seen = HashSet::new();
    text.lines()
        .filter_map(|line| {
            let mut parts = line.trim().splitn(3, '|');
            let (store, hash, subject) = (parts.next()?, parts.next()?, parts.next()?);
            let hash = hash.to_ascii_uppercase();
            (hash.len() == 64 && seen.insert(hash.clone())).then(|| RootCert {
                store: store.to_owned(),
                subject: subject.to_owned(),
                in_mozilla: known.contains(&hash),
                sha256: hash,
            })
        })
        .collect()
}

/// `security find-certificate -a -Z` çıktısı: `SHA-256 hash:` ve `"labl"<blob>="ad"` satırları.
pub fn parse_macos_keychain(text: &str, known: &HashSet<String>) -> Vec<RootCert> {
    let mut out = Vec::new();
    let mut hash = String::new();
    for line in text.lines().map(str::trim) {
        if let Some(h) = line.strip_prefix("SHA-256 hash:") {
            hash = h.trim().to_ascii_uppercase();
        } else if let Some(label) = line.strip_prefix("\"labl\"<blob>=") {
            out.push(RootCert {
                store: "System.keychain".to_owned(),
                subject: label.trim_matches('"').to_owned(),
                in_mozilla: known.contains(&hash),
                sha256: std::mem::take(&mut hash),
            });
        }
    }
    out
}

/// Windows'ta makine ve kullanıcı kök depoları; Linux'ta yöneticinin eklediği güven çapaları;
/// macOS'ta yöneticinin Sistem anahtar zincirine eklediği sertifikalar.
pub fn roots(os: Os) -> Vec<RootCert> {
    let known = mozilla_roots();
    match os {
        Os::Windows => stdout_of(
            "powershell",
            &[
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "[Console]::OutputEncoding = [Text.Encoding]::UTF8; $sha = [Security.Cryptography.SHA256]::Create(); \
                 foreach ($s in 'LocalMachine','CurrentUser') { Get-ChildItem \"Cert:\\$s\\Root\" | ForEach-Object { \
                 $s + '|' + [BitConverter]::ToString($sha.ComputeHash($_.RawData)).Replace('-','') + '|' + $_.Subject } }",
            ],
        )
        .map(|t| parse_windows_roots(&t, &known))
        .unwrap_or_default(),
        Os::Linux => [
            "/usr/local/share/ca-certificates",
            "/etc/pki/ca-trust/source/anchors",
            "/etc/ca-certificates/trust-source/anchors",
        ]
        .iter()
        .flat_map(|dir| {
            std::fs::read_dir(dir)
                .into_iter()
                .flatten()
                .flatten()
                .filter(|e| e.path().is_file())
                .map(move |e| RootCert {
                    store: (*dir).to_owned(),
                    subject: e.file_name().to_string_lossy().into_owned(),
                    sha256: String::new(),
                    in_mozilla: false,
                })
        })
        .collect(),
        Os::Macos => stdout_of(
            "security",
            &["find-certificate", "-a", "-Z", "/Library/Keychains/System.keychain"],
        )
        .map(|t| parse_macos_keychain(&t, &known))
        .unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_stores_against_mozilla_list() {
        let known = mozilla_roots();
        assert!(known.len() > 100);
        assert!(known.iter().all(|h| h.len() == 64));
        let any = known.iter().next().unwrap().clone();
        let text = format!(
            "LocalMachine|{any}|CN=Known\nLocalMachine|{}|CN=Strange Proxy CA\nCurrentUser|{any}|CN=Known\n",
            "A".repeat(64)
        );
        let roots = parse_windows_roots(&text, &known);
        assert_eq!(roots.len(), 2, "duplicates from the inherited user store are dropped");
        assert!(roots[0].in_mozilla && !roots[1].in_mozilla);
        let mac = "SHA-256 hash: ABCD\n    \"labl\"<blob>=\"Corp Root\"\n";
        let found = parse_macos_keychain(mac, &known);
        assert_eq!(found[0].subject, "Corp Root");
        assert!(!found[0].in_mozilla);
    }
}
