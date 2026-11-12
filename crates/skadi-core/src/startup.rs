//! Bilgisayar açılırken ya da oturum açılırken otomatik başlayan öğeler.
//!
//! Kapatma, öğeyi silmeden yapılır ve geri alınabilir: Windows'ta Görev Yöneticisi'nin kullandığı
//! `StartupApproved` değeri, Linux'ta kullanıcı klasöründe `Hidden=true` içeren bir `.desktop` kopyası.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::i18n::Text;

use crate::platform::Os;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StartupItem {
    /// Nereden başlatıldığı (ör. `HKCU Run`, `systemd`, `~/Library/LaunchAgents`).
    pub source: String,
    pub name: String,
    /// Çalıştırılan komut; bilinmiyorsa boş.
    pub command: String,
    /// `None`: Skadi bu öğeyi açıp kapatamaz (ör. systemd hizmetleri, macOS).
    pub enabled: Option<bool>,
}

const ADVICE: &str = include_str!("../../../catalog/startup.toml");

#[derive(Debug, Deserialize)]
struct AdviceFile {
    item: Vec<Advice>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Advice {
    #[serde(rename = "match")]
    pub pattern: String,
    pub advice: Text,
}

/// Gömülü öneri listesi; bozuksa boş döner (öneriler yalnızca bilgi amaçlıdır).
pub fn advice_list() -> Vec<Advice> {
    toml::from_str::<AdviceFile>(ADVICE).map(|f| f.item).unwrap_or_default()
}

/// Öğe için kapatılabilir önerisi; yoksa `None`.
pub fn advice_for<'a>(list: &'a [Advice], item: &StartupItem) -> Option<&'a Text> {
    let name = item.name.to_lowercase();
    let command = item.command.to_lowercase();
    list.iter()
        .find(|a| name.contains(&a.pattern) || command.contains(&a.pattern))
        .map(|a| &a.advice)
}

/// Bir öğeyi açmak ya da kapatmak için yapılacak değişiklik.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Toggle {
    /// `reg add <key> /v <name> /t REG_BINARY /d <data> /f`; `machine` ise yönetici gerekir.
    Registry {
        key: String,
        name: String,
        data: &'static str,
        machine: bool,
    },
    /// Kullanıcının autostart klasörüne yazılacak dosya.
    File { path: PathBuf, contents: String },
    /// Sistem aracıyla açma/kapama: `systemctl enable|disable --now`, `launchctl enable|disable`.
    Command {
        program: &'static str,
        args: Vec<String>,
        root: bool,
    },
}

/// Kapatılması sistemi açılmaz, ağsız ya da girişsiz bırakabilecek systemd birimleri. Bunlar
/// listelenir ama Skadi bunları açıp kapatmaz.
const PROTECTED_UNITS: [&str; 34] = [
    "systemd-",
    "dbus",
    "networkmanager",
    "network",
    "wpa_supplicant",
    "iwd",
    "polkit",
    "udisks",
    "upower",
    "gdm",
    "sddm",
    "lightdm",
    "display-manager",
    "getty",
    "ssh",
    "sshd",
    "cron",
    "crond",
    "rsyslog",
    "auditd",
    "firewalld",
    "ufw",
    "apparmor",
    "accounts-daemon",
    "bluetooth",
    "snapd",
    "lvm2",
    "multipathd",
    "dm-event",
    "cryptsetup",
    "chronyd",
    "nvidia",
    "zram",
    "plymouth",
];

/// systemd biriminin Skadi tarafından açılıp kapatılabilir olup olmadığı.
pub fn is_managed_unit(unit: &str) -> bool {
    let lower = unit.to_ascii_lowercase();
    unit.ends_with(".service")
        && unit.len() <= 128
        && unit
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '@' | ':'))
        && !PROTECTED_UNITS.iter().any(|p| lower.starts_with(p))
}

/// LaunchAgent/LaunchDaemon etiketi: plist adından türetilir (Apple'ın adlandırma kuralı).
fn launchd_label(item: &StartupItem) -> Option<String> {
    let label = item.name.strip_suffix(".plist")?;
    label
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        .then(|| label.to_owned())
}

fn current_uid() -> Option<String> {
    crate::audit::stdout_of("id", &["-u"])
        .map(|s| s.trim().to_owned())
        .filter(|uid| !uid.is_empty() && uid.chars().all(|c| c.is_ascii_digit()))
}

/// `launchctl print-disabled` çıktısındaki kapalı etiketler. Eski sürümler `=> true`, yenileri
/// `=> disabled` yazar.
pub fn parse_launchd_disabled(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| {
            let (label, state) = line.split_once("=>")?;
            let state = state.trim().trim_end_matches(',');
            matches!(state, "true" | "disabled").then(|| label.trim().trim_matches('"').to_owned())
        })
        .collect()
}

const APPROVED: &str = "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved";
/// İlk bayt 2 açık, 3 kapalı demektir; Windows kalan baytlara kapatılma zamanını yazar, sıfır da geçerlidir.
const APPROVED_ON: &str = "020000000000000000000000";
const APPROVED_OFF: &str = "030000000000000000000000";

/// Kaynağa göre `StartupApproved` alt anahtarı ve kovanı (`true`: HKLM).
fn approval_location(source: &str) -> Option<(bool, &'static str)> {
    Some(match source {
        "HKCU Run" => (false, "Run"),
        "HKLM Run" => (true, "Run"),
        "HKLM Run (32-bit)" => (true, "Run32"),
        "Startup folder" => (false, "StartupFolder"),
        "Startup folder (all users)" => (true, "StartupFolder"),
        _ => return None,
    })
}

/// Bir öğeyi açmak (`enable`) ya da kapatmak için gereken değişiklik; desteklenmiyorsa `None`.
pub fn toggle(item: &StartupItem, enable: bool) -> Option<Toggle> {
    let verb = if enable { "enable" } else { "disable" };
    if item.source == "systemd" {
        return is_managed_unit(&item.name).then(|| Toggle::Command {
            program: "systemctl",
            args: vec![verb.to_owned(), "--now".to_owned(), item.name.clone()],
            root: true,
        });
    }
    if item.source.ends_with("LaunchAgents") || item.source.ends_with("LaunchDaemons") {
        let label = launchd_label(item)?;
        // Ajanlar kullanıcı oturumunda (gui/<uid>), sistem hizmetleri `system` alanında kapatılır.
        let (domain, root) = if item.source.ends_with("LaunchDaemons") {
            ("system".to_owned(), true)
        } else {
            (format!("gui/{}", current_uid()?), false)
        };
        return Some(Toggle::Command {
            program: "launchctl",
            args: vec![verb.to_owned(), format!("{domain}/{label}")],
            root,
        });
    }
    if let Some((machine, sub)) = approval_location(&item.source) {
        return Some(Toggle::Registry {
            key: format!("{}\\{APPROVED}\\{sub}", if machine { "HKLM" } else { "HKCU" }),
            name: item.name.clone(),
            data: if enable { APPROVED_ON } else { APPROVED_OFF },
            machine,
        });
    }
    let user_dir = user_autostart()?;
    let original = match item.source.as_str() {
        "~/.config/autostart" => user_dir.join(&item.name),
        "/etc/xdg/autostart" => {
            // Kullanıcı kopyası varsa o düzenlenir; yoksa sistem dosyasından kopya oluşturulur.
            let own = user_dir.join(&item.name);
            if own.is_file() {
                own
            } else {
                Path::new("/etc/xdg/autostart").join(&item.name)
            }
        }
        _ => return None,
    };
    let text = std::fs::read_to_string(&original).ok()?;
    Some(Toggle::File {
        path: user_dir.join(&item.name),
        contents: with_hidden(&text, !enable),
    })
}

fn user_autostart() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .map(|config| config.join("autostart"))
}

/// `.desktop` metninde `Hidden=` satırını ayarlar; yoksa `[Desktop Entry]` başlığının altına ekler.
pub fn with_hidden(text: &str, hidden: bool) -> String {
    let line = format!("Hidden={hidden}");
    let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
    if let Some(existing) = lines.iter_mut().find(|l| l.trim_start().starts_with("Hidden=")) {
        *existing = line;
    } else if let Some(header) = lines.iter().position(|l| l.trim() == "[Desktop Entry]") {
        lines.insert(header + 1, line);
    } else {
        lines.insert(0, "[Desktop Entry]".to_owned());
        lines.insert(1, line);
    }
    lines.join("\n") + "\n"
}

fn desktop_hidden(path: &Path) -> bool {
    std::fs::read_to_string(path).is_ok_and(|text| text.lines().any(|l| l.trim() == "Hidden=true"))
}

pub fn startup_items(os: Os) -> Vec<StartupItem> {
    match os {
        Os::Windows => windows_items(),
        Os::Linux => linux_items(),
        Os::Macos => macos_items(),
    }
}

fn files_in(dir: &Path, source: &str, extension: Option<&str>) -> Vec<StartupItem> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut items: Vec<StartupItem> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && extension.is_none_or(|ext| path.extension().is_some_and(|e| e == ext)))
        .map(|path| StartupItem {
            source: source.to_owned(),
            name: path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            command: desktop_exec(&path).unwrap_or_default(),
            enabled: None,
        })
        .collect();
    items.sort_by(|a, b| a.name.cmp(&b.name));
    items
}

/// `.desktop` dosyasındaki `Exec=` satırı.
fn desktop_exec(path: &Path) -> Option<String> {
    if path.extension()? != "desktop" {
        return None;
    }
    let text = std::fs::read_to_string(path).ok()?;
    text.lines()
        .find_map(|line| line.strip_prefix("Exec="))
        .map(str::to_owned)
}

#[cfg(windows)]
fn windows_items() -> Vec<StartupItem> {
    use windows_registry::{CURRENT_USER, LOCAL_MACHINE};
    let run = "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Run";
    let mut items = Vec::new();
    for (source, hive, path) in [
        ("HKCU Run", CURRENT_USER, run.to_owned()),
        ("HKLM Run", LOCAL_MACHINE, run.to_owned()),
        (
            "HKLM Run (32-bit)",
            LOCAL_MACHINE,
            run.replace("SOFTWARE\\", "SOFTWARE\\WOW6432Node\\"),
        ),
    ] {
        let Ok(key) = hive.open(&path) else { continue };
        let Ok(values) = key.values() else { continue };
        for (name, _) in values {
            items.push(StartupItem {
                source: source.to_owned(),
                command: key.get_string(&name).unwrap_or_default(),
                enabled: None,
                name,
            });
        }
    }
    let folders = [
        (
            "APPDATA",
            "Microsoft\\Windows\\Start Menu\\Programs\\Startup",
            "Startup folder",
        ),
        (
            "ProgramData",
            "Microsoft\\Windows\\Start Menu\\Programs\\StartUp",
            "Startup folder (all users)",
        ),
    ];
    for (var, sub, source) in folders {
        if let Some(base) = std::env::var_os(var) {
            items.extend(
                files_in(&Path::new(&base).join(sub), source, None)
                    .into_iter()
                    .filter(|i| i.name != "desktop.ini"),
            );
        }
    }
    for item in &mut items {
        if let Some((machine, sub)) = approval_location(&item.source) {
            let hive = if machine { LOCAL_MACHINE } else { CURRENT_USER };
            let state = hive
                .open(format!("{APPROVED}\\{sub}"))
                .and_then(|key| key.get_value(&item.name))
                .ok()
                .and_then(|value| value.first().copied());
            item.enabled = Some(state.is_none_or(|byte| byte & 1 == 0));
        }
    }
    items
}

#[cfg(not(windows))]
fn windows_items() -> Vec<StartupItem> {
    Vec::new()
}

fn linux_items() -> Vec<StartupItem> {
    let mut items = Vec::new();
    if let Some(systemctl) = crate::exec::find_program("systemctl")
        && let Ok(output) = std::process::Command::new(systemctl)
            .args([
                "list-unit-files",
                "--type=service",
                "--state=enabled",
                "--no-legend",
                "--no-pager",
            ])
            .output()
    {
        items.extend(String::from_utf8_lossy(&output.stdout).lines().filter_map(|line| {
            line.split_whitespace().next().map(|unit| StartupItem {
                source: "systemd".to_owned(),
                name: unit.to_owned(),
                command: String::new(),
                enabled: is_managed_unit(unit).then_some(true),
            })
        }));
    }
    if let Some(home) = std::env::var_os("HOME") {
        items.extend(files_in(
            &Path::new(&home).join(".config/autostart"),
            "~/.config/autostart",
            Some("desktop"),
        ));
    }
    items.extend(files_in(
        Path::new("/etc/xdg/autostart"),
        "/etc/xdg/autostart",
        Some("desktop"),
    ));
    let own = user_autostart();
    for item in items.iter_mut().filter(|i| i.source != "systemd") {
        let user_copy = own.as_ref().map(|dir| dir.join(&item.name)).filter(|p| p.is_file());
        let path = user_copy.unwrap_or_else(|| Path::new(&item.source).join(&item.name));
        item.enabled = Some(!desktop_hidden(&path));
    }
    items
}

fn macos_items() -> Vec<StartupItem> {
    let mut items = Vec::new();
    if let Some(home) = std::env::var_os("HOME") {
        items.extend(files_in(
            &Path::new(&home).join("Library/LaunchAgents"),
            "~/Library/LaunchAgents",
            Some("plist"),
        ));
    }
    items.extend(files_in(
        Path::new("/Library/LaunchAgents"),
        "/Library/LaunchAgents",
        Some("plist"),
    ));
    items.extend(files_in(
        Path::new("/Library/LaunchDaemons"),
        "/Library/LaunchDaemons",
        Some("plist"),
    ));
    let user_disabled = current_uid()
        .and_then(|uid| crate::audit::stdout_of("launchctl", &["print-disabled", &format!("gui/{uid}")]))
        .map(|t| parse_launchd_disabled(&t))
        .unwrap_or_default();
    let system_disabled = crate::audit::stdout_of("launchctl", &["print-disabled", "system"])
        .map(|t| parse_launchd_disabled(&t))
        .unwrap_or_default();
    for item in &mut items {
        if let Some(label) = launchd_label(item) {
            let disabled = if item.source.ends_with("LaunchDaemons") {
                &system_disabled
            } else {
                &user_disabled
            };
            item.enabled = Some(!disabled.contains(&label));
        }
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_desktop_exec_lines() {
        let dir = std::env::temp_dir().join(format!("skadi-autostart-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("app.desktop"),
            "[Desktop Entry]\nName=App\nExec=/usr/bin/app --tray\n",
        )
        .unwrap();
        std::fs::write(dir.join("notes.txt"), "x").unwrap();
        let items = files_in(&dir, "test", Some("desktop"));
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].command, "/usr/bin/app --tray");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn advice_matches_names_and_commands() {
        let list = advice_list();
        assert!(list.len() >= 15, "advice list must parse");
        assert!(list.iter().all(|a| a.pattern == a.pattern.to_lowercase()));
        let item = StartupItem {
            source: "HKCU Run".to_owned(),
            name: "OneDrive".to_owned(),
            command: "C:\\x\\OneDrive.exe /background".to_owned(),
            enabled: Some(true),
        };
        assert!(advice_for(&list, &item).is_some());
        let unknown = StartupItem {
            name: "SecurityHealth".to_owned(),
            command: String::new(),
            ..item
        };
        assert!(advice_for(&list, &unknown).is_none());
    }

    #[test]
    fn protects_core_units_and_parses_launchd() {
        assert!(is_managed_unit("cups.service"));
        assert!(is_managed_unit("teamviewerd.service"));
        assert!(!is_managed_unit("NetworkManager.service"));
        assert!(!is_managed_unit("systemd-resolved.service"));
        assert!(!is_managed_unit("gdm.service"));
        assert!(!is_managed_unit("evil;rm.service"));
        let text = "disabled services = {\n\t\"com.adobe.agent\" => disabled\n\t\"org.x\" => enabled\n\t\"com.old\" => true\n}\n";
        assert_eq!(parse_launchd_disabled(text), ["com.adobe.agent", "com.old"]);
    }

    #[test]
    fn toggles_hidden_and_registry() {
        let text = "[Desktop Entry]\nName=App\nExec=app\n";
        let off = with_hidden(text, true);
        assert_eq!(off, "[Desktop Entry]\nHidden=true\nName=App\nExec=app\n");
        assert_eq!(
            with_hidden(&off, false),
            "[Desktop Entry]\nHidden=false\nName=App\nExec=app\n"
        );
        let item = StartupItem {
            source: "HKLM Run (32-bit)".to_owned(),
            name: "Tool".to_owned(),
            command: String::new(),
            enabled: Some(true),
        };
        let Some(Toggle::Registry { key, data, machine, .. }) = toggle(&item, false) else {
            panic!("registry toggle expected");
        };
        assert!(machine && key.ends_with("StartupApproved\\Run32") && key.starts_with("HKLM\\SOFTWARE"));
        assert!(data.starts_with("03"));
    }
}
