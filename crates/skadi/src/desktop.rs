//! Masaüstü ayarları: varsayılan tarayıcı, GNOME eklentileri, Spotlight dizinlemesi ve macOS
//! yapılandırma profili.

use std::path::{Path, PathBuf};

use skadi_core::{
    Error, Lang, Result, catalog::Catalog, exec::Privilege, manager::ManagerId, platform::Os, system::System,
};

use crate::{commands::confirm, net::build};

fn is_desktop_file(name: &str) -> bool {
    name.ends_with(".desktop")
        && name.len() <= 128
        && !name.starts_with('-')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

/// Linux'ta bir uygulamanın `.desktop` dosyası: katalog kimliğinden ya da Flatpak/Snap adından.
fn desktop_file_for(app: &str) -> Option<String> {
    let catalog = Catalog::builtin().ok()?;
    let mut names = vec![format!("{app}.desktop")];
    if let Some(entry) = catalog.get(app) {
        for manager in [
            ManagerId::Flatpak,
            ManagerId::Snap,
            ManagerId::Apt,
            ManagerId::Dnf,
            ManagerId::Pacman,
        ] {
            if let Some(id) = entry.packages.get(&manager) {
                names.push(format!("{id}.desktop"));
                if manager == ManagerId::Snap {
                    names.push(format!("{id}_{id}.desktop"));
                }
            }
        }
    }
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let mut dirs: Vec<PathBuf> = [
        "/usr/share/applications",
        "/usr/local/share/applications",
        "/var/lib/flatpak/exports/share/applications",
        "/var/lib/snapd/desktop/applications",
    ]
    .iter()
    .map(PathBuf::from)
    .collect();
    if let Some(home) = home {
        dirs.push(home.join(".local/share/applications"));
        dirs.push(home.join(".local/share/flatpak/exports/share/applications"));
    }
    names
        .into_iter()
        .filter(|n| is_desktop_file(n))
        .find(|name| dirs.iter().any(|dir| dir.join(name).is_file()))
}

/// Varsayılan tarayıcı. Linux'ta `xdg-settings` ile ayarlar; Windows ve macOS programların bunu
/// değiştirmesine izin vermediği için ilgili Ayarlar sayfasını açar.
pub fn default_browser(lang: Lang, app: Option<&str>) -> Result<()> {
    let t = |en, tr| lang.pick(en, tr);
    let system = System::detect()?;
    match system.os {
        Os::Linux => match app {
            None => build(
                &system,
                "xdg-settings",
                &["get", "default-web-browser"],
                Privilege::NonRoot,
            )?
            .run(),
            Some(app) => {
                let desktop = if is_desktop_file(app) {
                    app.to_owned()
                } else {
                    desktop_file_for(app).ok_or_else(|| {
                        Error::State(format!(
                            "{app}: {}",
                            t(
                                "no .desktop file found; is it installed?",
                                "`.desktop` dosyası bulunamadı; kurulu mu?"
                            )
                        ))
                    })?
                };
                let invocation = build(
                    &system,
                    "xdg-settings",
                    &["set", "default-web-browser", &desktop],
                    Privilege::NonRoot,
                )?;
                println!("> {}", invocation.display());
                invocation.run()
            }
        },
        Os::Windows => {
            println!(
                "{}",
                t(
                    "Windows protects default apps from programs; opening Settings > Apps > Default apps.",
                    "Windows varsayılan uygulamaları programlara karşı korur; Ayarlar > Uygulamalar > Varsayılan uygulamalar açılıyor."
                )
            );
            // explorer başarıyla açsa da 1 ile çıkabilir; sonuç önemli değil.
            let _ = build(&system, "explorer", &["ms-settings:defaultapps"], Privilege::Any)?.output();
            Ok(())
        }
        Os::Macos => {
            println!(
                "{}",
                t(
                    "macOS lets only you change the default browser: System Settings > Desktop & Dock > Default web browser.",
                    "Varsayılan tarayıcıyı macOS'ta yalnızca siz değiştirebilirsiniz: Sistem Ayarları > Masaüstü ve Dock > Varsayılan web tarayıcısı."
                )
            );
            build(&system, "open", &["-b", "com.apple.systempreferences"], Privilege::Any)?.run()
        }
    }
}

fn is_extension_uuid(uuid: &str) -> bool {
    uuid.contains('@')
        && uuid.len() <= 128
        && !uuid.starts_with('-')
        && uuid
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '@' | '.' | '-' | '_'))
}

/// GNOME Shell eklentilerini listeler, kapatır ya da açar (`gnome-extensions`).
pub fn gnome_extensions(action: Option<&str>, uuid: Option<&str>) -> Result<()> {
    let system = System::detect()?;
    if system.os != Os::Linux {
        return Err(Error::Unsupported("gnome-extensions (Linux)"));
    }
    let invocation = match (action, uuid) {
        (Some(action @ ("enable" | "disable")), Some(uuid)) if is_extension_uuid(uuid) => {
            build(&system, "gnome-extensions", &[action, uuid], Privilege::NonRoot)?
        }
        (Some(_), _) => return Err(Error::InvalidOption("enable|disable <uuid>")),
        (None, _) => build(&system, "gnome-extensions", &["list", "--enabled"], Privilege::NonRoot)?,
    };
    invocation.run()
}

/// macOS Spotlight dizinlemesi (`mdutil`).
pub fn spotlight(lang: Lang, state: Option<&str>, yes: bool) -> Result<()> {
    let system = System::detect()?;
    if system.os != Os::Macos {
        return Err(Error::Unsupported("spotlight (macOS)"));
    }
    let invocation = match state {
        None => build(&system, "mdutil", &["-s", "/"], Privilege::Any)?,
        Some(flag @ ("on" | "off")) => build(&system, "mdutil", &["-i", flag, "/"], Privilege::Root)?,
        Some(_) => return Err(Error::InvalidOption("on | off")),
    };
    println!("> {}", invocation.display());
    if state == Some("off")
        && !confirm(
            lang,
            yes,
            lang.pick(
                "Turn off Spotlight indexing? Spotlight and Mail search stop finding new files.",
                "Spotlight dizinlemesi kapatılsın mı? Spotlight ve Mail araması yeni dosyaları bulamaz.",
            ),
        )?
    {
        return Ok(());
    }
    invocation.run()
}

fn plist_escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// Rastgele görünümlü UUID (RFC 4122 biçiminde). Profil kimlikleri yalnızca benzersiz olmalıdır.
fn uuid(seed: u64) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let mut value =
        nanos ^ (u128::from(std::process::id()) << 64) ^ u128::from(seed).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    // splitmix tarzı karıştırma
    value ^= value >> 31;
    value = value.wrapping_mul(0xBF58_476D_1CE4_E5B9_94D0_49BB_1331_11EB);
    let hex = format!("{value:032X}");
    format!(
        "{}-{}-4{}-A{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[13..16],
        &hex[17..20],
        &hex[20..32]
    )
}

struct Payload {
    kind: &'static str,
    name: &'static str,
    keys: &'static [(&'static str, &'static str)],
}

/// Apple'ın Device Management belgelerindeki yük türleri ve anahtarları.
const PAYLOADS: [Payload; 5] = [
    Payload {
        kind: "com.apple.applicationaccess",
        name: "Spotlight",
        keys: &[("allowSpotlightInternetResults", "<false/>")],
    },
    Payload {
        kind: "com.apple.screensaver",
        name: "Screen lock",
        keys: &[
            ("askForPassword", "<true/>"),
            ("askForPasswordDelay", "<integer>0</integer>"),
        ],
    },
    Payload {
        kind: "com.apple.security.firewall",
        name: "Firewall",
        keys: &[("EnableFirewall", "<true/>"), ("EnableStealthMode", "<true/>")],
    },
    // Chrome Enterprise: macOS'ta ilkeler `com.google.Chrome` alanından okunur.
    Payload {
        kind: "com.google.Chrome",
        name: "Chrome",
        keys: &[
            ("MetricsReportingEnabled", "<false/>"),
            ("SearchSuggestEnabled", "<false/>"),
            ("UrlKeyedAnonymizedDataCollectionEnabled", "<false/>"),
            ("SafeBrowsingExtendedReportingEnabled", "<false/>"),
            (
                "ExtensionInstallForcelist",
                "<array><string>ddkjiahejlhfcafbddmgiahcphecmpfh;https://clients2.google.com/service/update2/crx</string></array>",
            ),
        ],
    },
    // Mozilla: `org.mozilla.firefox` alanı ve EnterprisePoliciesEnabled.
    Payload {
        kind: "org.mozilla.firefox",
        name: "Firefox",
        keys: &[
            ("EnterprisePoliciesEnabled", "<true/>"),
            ("DisableTelemetry", "<true/>"),
            ("DisableFirefoxStudies", "<true/>"),
            (
                "ExtensionSettings",
                "<dict><key>uBlock0@raymondhill.net</key><dict><key>installation_mode</key><string>normal_installed</string><key>install_url</key><string>https://addons.mozilla.org/firefox/downloads/latest/ublock-origin/latest.xpi</string></dict></dict>",
            ),
        ],
    },
];

pub fn mobileconfig_text() -> String {
    let mut content = String::new();
    for (index, payload) in PAYLOADS.iter().enumerate() {
        content.push_str("    <dict>\n");
        for (key, value) in payload.keys {
            content.push_str(&format!("      <key>{key}</key>{value}\n"));
        }
        content.push_str(&format!(
            "      <key>PayloadType</key><string>{}</string>\n      <key>PayloadIdentifier</key><string>org.llcforge.skadi.{index}</string>\n      <key>PayloadUUID</key><string>{}</string>\n      <key>PayloadVersion</key><integer>1</integer>\n      <key>PayloadDisplayName</key><string>{}</string>\n    </dict>\n",
            payload.kind,
            uuid(index as u64 + 1),
            plist_escape(payload.name)
        ));
    }
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n<dict>\n  <key>PayloadContent</key>\n  <array>\n{content}  </array>\n  <key>PayloadDisplayName</key><string>Skadi privacy and security</string>\n  <key>PayloadDescription</key><string>Turns off Spotlight internet suggestions, asks for the password right after sleep or the screen saver, turns on the firewall with stealth mode, and reduces Chrome and Firefox data collection with uBlock Origin installed. Remove it any time in System Settings.</string>\n  <key>PayloadIdentifier</key><string>org.llcforge.skadi</string>\n  <key>PayloadType</key><string>Configuration</string>\n  <key>PayloadUUID</key><string>{}</string>\n  <key>PayloadVersion</key><integer>1</integer>\n  <key>PayloadRemovalDisallowed</key><false/>\n</dict>\n</plist>\n",
        uuid(0)
    )
}

/// macOS yapılandırma profili yazar. Kurulum kullanıcının onayıyla Sistem Ayarları'ndan yapılır ve
/// istendiğinde aynı yerden kaldırılır.
pub fn macos_profile(lang: Lang, path: &Path) -> Result<()> {
    std::fs::write(path, mobileconfig_text()).map_err(|e| Error::State(format!("{}: {e}", path.display())))?;
    println!(
        "{} {}\n{}",
        lang.pick("Profile written to", "Profil yazıldı:"),
        path.display(),
        lang.pick(
            "Open it on the Mac, then approve it in System Settings > General > Device Management (Profiles). Remove it there to undo.",
            "Dosyayı Mac'te açın, ardından Sistem Ayarları > Genel > Aygıt Yönetimi (Profiller) bölümünden onaylayın. Geri almak için oradan kaldırın."
        )
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_names_and_builds_profile() {
        assert!(is_desktop_file("org.mozilla.firefox.desktop"));
        assert!(!is_desktop_file("--evil.desktop") && !is_desktop_file("a b.desktop"));
        assert!(is_extension_uuid("dash-to-dock@micxgx.gmail.com"));
        assert!(!is_extension_uuid("no-at-sign") && !is_extension_uuid("-x@y"));
        let text = mobileconfig_text();
        assert!(text.contains("<string>Configuration</string>"));
        assert!(text.contains("allowSpotlightInternetResults"));
        assert_eq!(text.matches("<key>PayloadUUID</key>").count(), 6);
        let id = uuid(7);
        assert_eq!(id.len(), 36);
        assert_eq!(&id[14..15], "4");
    }
}
