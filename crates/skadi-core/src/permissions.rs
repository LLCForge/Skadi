//! Kamera, mikrofon ve konum iznine sahip uygulamalar (Windows; yalnızca okur).

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Grant {
    pub capability: &'static str,
    pub app: String,
    pub allowed: bool,
    /// Son kullanım (Unix saniyesi); hiç kullanılmadıysa `None`.
    pub last_used: Option<u64>,
}

pub const CAPABILITIES: [&str; 3] = ["webcam", "microphone", "location"];

/// Windows FILETIME (1601'den beri 100 ns) → Unix saniyesi.
pub fn filetime_to_unix(filetime: u64) -> Option<u64> {
    (filetime / 10_000_000)
        .checked_sub(11_644_473_600)
        .filter(|secs| *secs > 0)
}

#[cfg(windows)]
pub fn grants() -> Vec<Grant> {
    use windows_registry::CURRENT_USER;
    let root = "Software\\Microsoft\\Windows\\CurrentVersion\\CapabilityAccessManager\\ConsentStore";
    let mut out = Vec::new();
    for capability in CAPABILITIES {
        let Ok(store) = CURRENT_USER.open(format!("{root}\\{capability}")) else {
            continue;
        };
        let mut read = |key: &windows_registry::Key, app: String| {
            out.push(Grant {
                capability,
                allowed: key.get_string("Value").is_ok_and(|v| v == "Allow"),
                last_used: key.get_u64("LastUsedTimeStop").ok().and_then(filetime_to_unix),
                app,
            });
        };
        if let Ok(names) = store.keys() {
            for name in names.filter(|n| n != "NonPackaged") {
                if let Ok(key) = store.open(&name) {
                    read(&key, name);
                }
            }
        }
        // Masaüstü uygulamaları: yoldaki `\` karakterleri `#` olarak saklanır.
        if let Ok(desktop) = store.open("NonPackaged")
            && let Ok(names) = desktop.keys()
        {
            for name in names {
                if let Ok(key) = desktop.open(&name) {
                    read(&key, name.replace('#', "\\"));
                }
            }
        }
    }
    out
}

#[cfg(not(windows))]
pub fn grants() -> Vec<Grant> {
    Vec::new()
}

/// Windows dışındaki sistemlerde bir uygulamaya verilmiş izin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AppGrant {
    /// İzin türü (ör. `devices/camera`, `location`, `kTCCServiceCamera`).
    pub kind: String,
    pub app: String,
    /// Aracın bildirdiği değer.
    pub value: String,
}

/// `flatpak permissions` çıktısı: `Tablo Nesne Uygulama İzinler Veri` sütunları.
pub fn parse_flatpak_permissions(text: &str) -> Vec<AppGrant> {
    text.lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            if fields.len() < 3 || fields[0] == "Table" {
                return None;
            }
            Some(AppGrant {
                kind: format!("{}/{}", fields[0], fields[1]),
                app: fields[2].to_owned(),
                value: fields.get(3).copied().unwrap_or("").to_owned(),
            })
        })
        .collect()
}

/// `sqlite3 -separator '|'` ile okunan TCC satırları: `servis|uygulama|izin`. 2 izin verildi demektir.
pub fn parse_tcc(text: &str) -> Vec<AppGrant> {
    text.lines()
        .filter_map(|line| {
            let mut parts = line.split('|');
            let (service, client, auth) = (parts.next()?, parts.next()?, parts.next()?);
            Some(AppGrant {
                kind: service.trim_start_matches("kTCCService").to_owned(),
                app: client.to_owned(),
                value: if auth.trim() == "2" { "allowed" } else { "denied" }.to_owned(),
            })
        })
        .collect()
}

/// Linux'ta Flatpak portal izinleri, macOS'ta kullanıcının TCC veritabanı. macOS'ta veritabanını
/// okumak için terminale Tam Disk Erişimi verilmesi gerekir; verilmemişse hata metni döner.
pub fn app_grants(os: crate::platform::Os) -> std::result::Result<Vec<AppGrant>, String> {
    use crate::platform::Os;
    let run = |program: &str, args: &[&str]| {
        let path = crate::exec::find_program(program).ok_or_else(|| format!("{program} not found"))?;
        let output = std::process::Command::new(path)
            .args(args)
            .stdin(std::process::Stdio::null())
            .output()
            .map_err(|e| e.to_string())?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).into_owned())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
        }
    };
    match os {
        Os::Linux => run("flatpak", &["permissions"]).map(|t| parse_flatpak_permissions(&t)),
        Os::Macos => {
            let home = std::env::var("HOME").map_err(|e| e.to_string())?;
            let db = format!("{home}/Library/Application Support/com.apple.TCC/TCC.db");
            run(
                "sqlite3",
                &[
                    "-readonly",
                    "-separator",
                    "|",
                    &db,
                    "SELECT service, client, auth_value FROM access",
                ],
            )
            .map(|t| parse_tcc(&t))
        }
        Os::Windows => Ok(Vec::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_flatpak_and_tcc() {
        let flatpak = "Table    Object   App              Permissions Data\n\
                       devices  camera   org.gnome.Snapshot yes        0x00\n\
                       location location org.gnome.Maps   EXACT,0\n";
        let grants = parse_flatpak_permissions(flatpak);
        assert_eq!(grants.len(), 2);
        assert_eq!(grants[0].kind, "devices/camera");
        assert_eq!(grants[1].app, "org.gnome.Maps");
        let tcc = parse_tcc("kTCCServiceCamera|us.zoom.xos|2\nkTCCServiceMicrophone|com.x|0\n");
        assert_eq!(tcc[0].kind, "Camera");
        assert_eq!(tcc[0].value, "allowed");
        assert_eq!(tcc[1].value, "denied");
    }

    #[test]
    fn converts_filetime() {
        assert_eq!(filetime_to_unix(0), None);
        // 2024-01-01 00:00:00 UTC
        assert_eq!(filetime_to_unix(133_485_408_000_000_000), Some(1_704_067_200));
    }
}
