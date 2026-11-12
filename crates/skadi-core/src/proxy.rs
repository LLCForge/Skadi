//! Sistemde tanımlı proxy ayarları (yalnızca okur). Beklenmeyen bir proxy, trafiğin izlendiği
//! anlamına gelebilir; bu yüzden rapor gizlilik denetiminin parçasıdır. Adreslerdeki kullanıcı
//! adı ve parola her zaman maskelenir.

use serde::Serialize;

use crate::{audit::stdout_of, exec::PROXY_VARS, platform::Os};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProxySetting {
    pub source: &'static str,
    pub name: String,
    pub value: String,
}

/// `kullanıcı:parola@` kısmını `***@` yapar.
pub fn mask_credentials(value: &str) -> String {
    let start = value.find("://").map_or(0, |i| i + 3);
    let authority_end = value[start..].find('/').map_or(value.len(), |i| start + i);
    match value[start..authority_end].rfind('@') {
        Some(at) => format!("{}***{}", &value[..start], &value[start + at..]),
        None => value.to_owned(),
    }
}

pub fn settings(os: Os) -> Vec<ProxySetting> {
    let mut out: Vec<ProxySetting> = PROXY_VARS
        .iter()
        .filter_map(|name| {
            let value = std::env::var(name).ok().filter(|v| !v.is_empty())?;
            Some(ProxySetting {
                source: "environment",
                name: (*name).to_owned(),
                value: mask_credentials(&value),
            })
        })
        .collect();
    match os {
        Os::Windows => out.extend(windows()),
        Os::Linux => out.extend(gnome()),
        Os::Macos => out.extend(
            stdout_of("scutil", &["--proxy"])
                .map(|text| parse_scutil(&text))
                .unwrap_or_default(),
        ),
    }
    out
}

#[cfg(windows)]
fn windows() -> Vec<ProxySetting> {
    use windows_registry::CURRENT_USER;
    let Ok(key) = CURRENT_USER.open("Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let setting = |name: &str, value: String| ProxySetting {
        source: "Windows (WinINET)",
        name: name.to_owned(),
        value: mask_credentials(&value),
    };
    if key.get_u32("ProxyEnable").is_ok_and(|v| v == 1)
        && let Ok(server) = key.get_string("ProxyServer")
    {
        out.push(setting("ProxyServer", server));
    }
    if let Some(url) = key.get_string("AutoConfigURL").ok().filter(|u| !u.is_empty()) {
        out.push(setting("AutoConfigURL", url));
    }
    out
}

#[cfg(not(windows))]
fn windows() -> Vec<ProxySetting> {
    Vec::new()
}

fn gnome() -> Vec<ProxySetting> {
    let get = |schema: &str, key: &str| {
        stdout_of("gsettings", &["get", schema, key]).map(|s| s.trim().trim_matches('\'').to_owned())
    };
    let setting = |name: &str, value: String| ProxySetting {
        source: "GNOME",
        name: name.to_owned(),
        value: mask_credentials(&value),
    };
    match get("org.gnome.system.proxy", "mode").as_deref() {
        Some("manual") => ["http", "https", "socks"]
            .iter()
            .filter_map(|kind| {
                let schema = format!("org.gnome.system.proxy.{kind}");
                let host = get(&schema, "host").filter(|h| !h.is_empty())?;
                let port = get(&schema, "port").unwrap_or_default();
                Some(setting(kind, format!("{host}:{port}")))
            })
            .collect(),
        Some("auto") => get("org.gnome.system.proxy", "autoconfig-url")
            .map(|url| vec![setting("autoconfig-url", url)])
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// `scutil --proxy` çıktısı: `HTTPEnable : 1`, `HTTPProxy : host`, `HTTPPort : 8080` gibi satırlar.
fn parse_scutil(text: &str) -> Vec<ProxySetting> {
    let value = |key: &str| {
        text.lines().find_map(|line| {
            let (k, v) = line.split_once(':')?;
            (k.trim() == key).then(|| v.trim().to_owned())
        })
    };
    let mut out = Vec::new();
    for kind in ["HTTP", "HTTPS", "SOCKS"] {
        if value(&format!("{kind}Enable")).as_deref() == Some("1")
            && let Some(host) = value(&format!("{kind}Proxy"))
        {
            let port = value(&format!("{kind}Port")).unwrap_or_default();
            out.push(ProxySetting {
                source: "macOS",
                name: kind.to_ascii_lowercase(),
                value: mask_credentials(&format!("{host}:{port}")),
            });
        }
    }
    if value("ProxyAutoConfigEnable").as_deref() == Some("1")
        && let Some(url) = value("ProxyAutoConfigURLString")
    {
        out.push(ProxySetting {
            source: "macOS",
            name: "autoconfig".to_owned(),
            value: mask_credentials(&url),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_credentials() {
        assert_eq!(
            mask_credentials("http://bob:s3cret@proxy:3128"),
            "http://***@proxy:3128"
        );
        assert_eq!(mask_credentials("proxy.corp:8080"), "proxy.corp:8080");
        assert_eq!(
            mask_credentials("http://proxy/pac?u=a@b"),
            "http://proxy/pac?u=a@b",
            "an @ after the host is not a credential"
        );
    }

    #[test]
    fn parses_scutil() {
        let text =
            "<dictionary> {\n  HTTPEnable : 1\n  HTTPPort : 3128\n  HTTPProxy : 10.0.0.1\n  HTTPSEnable : 0\n}\n";
        let found = parse_scutil(text);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].value, "10.0.0.1:3128");
    }
}
