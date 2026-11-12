//! Sistem kontrolleri: kök sertifikalar, proxy, Wi-Fi profilleri, Snap'ten Flatpak'e geçiş, hosts
//! engelleme, güvenlik duvarı, sayfa dosyası ve Başlat sabitlemeleri.

use std::path::{Path, PathBuf};

use serde_json::json;
use skadi_core::{
    Error, Lang, Result,
    catalog::Catalog,
    exec::{Invocation, Privilege, find_program},
    hosts,
    manager::ManagerId,
    platform::Os,
    system::System,
};

use crate::{commands, commands::confirm, net::build, output::Output};

fn io_error(path: &Path) -> impl Fn(std::io::Error) -> Error + '_ {
    move |e| Error::State(format!("{}: {e}", path.display()))
}

fn powershell(system: &System, script: &str, privilege: Privilege) -> Result<Invocation> {
    build(
        system,
        "powershell",
        &["-NoProfile", "-NonInteractive", "-Command", script],
        privilege,
    )
}

// ------------------------------------------------------------------------------------------------
// Kök sertifikalar

pub fn certs(lang: Lang, out: &Output) -> Result<()> {
    let system = System::detect()?;
    let roots = skadi_core::certs::roots(system.os);
    // Windows'ta yalnızca Mozilla listesinde olmayanlar; diğerlerinde liste zaten yönetici eklemeleri.
    let shown: Vec<_> = roots
        .iter()
        .filter(|r| system.os != Os::Windows || !r.in_mozilla)
        .collect();
    if out.json {
        return out.print_json(&json!({ "roots": shown }));
    }
    println!(
        "{}",
        match system.os {
            Os::Windows => lang.pick(
                "Trusted roots that Mozilla does not trust (Microsoft's own roots are expected here):",
                "Mozilla'nın güvenmediği güvenilir kökler (Microsoft'un kendi kökleri burada beklenir):"
            ),
            _ => lang.pick(
                "Root certificates added by an administrator:",
                "Bir yöneticinin eklediği kök sertifikalar:"
            ),
        }
    );
    if shown.is_empty() {
        println!("  {}", lang.pick("none", "yok"));
    }
    for root in shown {
        println!("  {:<14} {}", root.store, root.subject);
        if !root.sha256.is_empty() {
            println!("  {:<14} SHA-256 {}", "", root.sha256);
        }
    }
    println!(
        "\n{}",
        lang.pick(
            "A root you do not recognise lets its owner read your encrypted traffic. Remove it if you did not add it.",
            "Tanımadığınız bir kök, sahibinin şifreli trafiğinizi okumasına izin verir. Siz eklemediyseniz kaldırın."
        )
    );
    Ok(())
}

// ------------------------------------------------------------------------------------------------
// Proxy

fn split_address(address: &str) -> Option<(&str, &str)> {
    let (host, port) = address.rsplit_once(':')?;
    let host_ok = !host.is_empty()
        && host.len() <= 253
        && !host.starts_with(['-', '.'])
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-'));
    let port_ok = port.parse::<u16>().is_ok_and(|p| p > 0);
    (host_ok && port_ok).then_some((host, port))
}

fn macos_services(system: &System) -> Result<Vec<String>> {
    let output = build(system, "networksetup", &["-listallnetworkservices"], Privilege::Any)?.output()?;
    // İlk satır açıklamadır; `*` ile başlayanlar kapalı hizmetlerdir.
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .skip(1)
        .filter(|l| !l.is_empty() && !l.starts_with('*'))
        .map(str::to_owned)
        .collect())
}

/// Sistem proxy'sini ayarlar (`Some("host:port")`) ya da kapatır (`None`). Parola kabul edilmez:
/// proxy parolası sistem ayarlarında düz metin olarak saklanırdı.
pub fn proxy_set(lang: Lang, address: Option<&str>, dry_run: bool, yes: bool) -> Result<()> {
    let detected = System::detect()?;
    let preview = commands::preview_system(lang, &detected, dry_run);
    let system = preview.as_ref().unwrap_or(&detected);
    let target = match address {
        Some(a) => Some(split_address(a).ok_or(Error::InvalidOption("host:port (no user name or password)"))?),
        None => None,
    };
    let settings = "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings";
    let mut plan = Vec::new();
    match (system.os, target) {
        (Os::Windows, Some((host, port))) => {
            let server = format!("{host}:{port}");
            plan.push(build(
                system,
                "reg",
                &[
                    "add",
                    settings,
                    "/v",
                    "ProxyServer",
                    "/t",
                    "REG_SZ",
                    "/d",
                    &server,
                    "/f",
                ],
                Privilege::Any,
            )?);
            plan.push(build(
                system,
                "reg",
                &["add", settings, "/v", "ProxyEnable", "/t", "REG_DWORD", "/d", "1", "/f"],
                Privilege::Any,
            )?);
        }
        (Os::Windows, None) => {
            plan.push(build(
                system,
                "reg",
                &["add", settings, "/v", "ProxyEnable", "/t", "REG_DWORD", "/d", "0", "/f"],
                Privilege::Any,
            )?);
        }
        (Os::Linux, Some((host, port))) => {
            plan.push(build(
                system,
                "gsettings",
                &["set", "org.gnome.system.proxy", "mode", "manual"],
                Privilege::NonRoot,
            )?);
            for kind in ["http", "https"] {
                let schema = format!("org.gnome.system.proxy.{kind}");
                plan.push(build(
                    system,
                    "gsettings",
                    &["set", &schema, "host", host],
                    Privilege::NonRoot,
                )?);
                plan.push(build(
                    system,
                    "gsettings",
                    &["set", &schema, "port", port],
                    Privilege::NonRoot,
                )?);
            }
        }
        (Os::Linux, None) => {
            plan.push(build(
                system,
                "gsettings",
                &["set", "org.gnome.system.proxy", "mode", "none"],
                Privilege::NonRoot,
            )?);
        }
        (Os::Macos, target) => {
            for service in macos_services(system)? {
                match target {
                    Some((host, port)) => {
                        plan.push(build(
                            system,
                            "networksetup",
                            &["-setwebproxy", &service, host, port],
                            Privilege::Root,
                        )?);
                        plan.push(build(
                            system,
                            "networksetup",
                            &["-setsecurewebproxy", &service, host, port],
                            Privilege::Root,
                        )?);
                    }
                    None => {
                        plan.push(build(
                            system,
                            "networksetup",
                            &["-setwebproxystate", &service, "off"],
                            Privilege::Root,
                        )?);
                        plan.push(build(
                            system,
                            "networksetup",
                            &["-setsecurewebproxystate", &service, "off"],
                            Privilege::Root,
                        )?);
                    }
                }
            }
        }
    }
    for invocation in &plan {
        println!("> {}", invocation.display());
    }
    let question = if target.is_some() {
        lang.pick(
            "Send web traffic through this proxy?",
            "Web trafiği bu proxy üzerinden gönderilsin mi?",
        )
    } else {
        lang.pick("Turn off the system proxy?", "Sistem proxy'si kapatılsın mı?")
    };
    if dry_run || !confirm(lang, yes, question)? {
        return Ok(());
    }
    plan.iter().try_for_each(Invocation::run_captured)
}

// ------------------------------------------------------------------------------------------------
// Wi-Fi profilleri

fn strip_secrets(text: &str) -> String {
    text.lines()
        .filter(|l| {
            let key = l.trim_start();
            !(key.starts_with("psk=") || key.starts_with("password=") || key.starts_with("wep-key"))
        })
        .map(|l| format!("{l}\n"))
        .collect()
}

#[cfg(unix)]
fn give_to_invoking_user(path: &Path) {
    let id = |name: &str| std::env::var(name).ok().and_then(|v| v.parse::<u32>().ok());
    if let (Some(uid), Some(gid)) = (id("SUDO_UID"), id("SUDO_GID")) {
        let _ = std::os::unix::fs::chown(path, Some(uid), Some(gid));
    }
}

#[cfg(not(unix))]
fn give_to_invoking_user(_path: &Path) {}

/// Wi-Fi profillerini bir klasöre aktarır ya da oradan içe alır. Parolalar yalnızca
/// `--with-passwords` ile ve uyarıdan sonra dosyaya yazılır.
pub fn wifi(lang: Lang, action: &str, dir: &Path, with_passwords: bool, yes: bool) -> Result<()> {
    let t = |en, tr| lang.pick(en, tr);
    let system = System::detect()?;
    if with_passwords
        && action == "export"
        && !confirm(
            lang,
            yes,
            t(
                "Write the Wi-Fi passwords to these files in plain text? Delete them after importing.",
                "Wi-Fi parolaları bu dosyalara düz metin olarak yazılsın mı? İçe aktardıktan sonra silin.",
            ),
        )?
    {
        return Ok(());
    }
    match system.os {
        Os::Windows => {
            if action == "export" {
                std::fs::create_dir_all(dir).map_err(io_error(dir))?;
                let folder = format!("folder={}", dir.display());
                let mut args = vec!["wlan", "export", "profile", folder.as_str()];
                if with_passwords {
                    args.push("key=clear");
                }
                build(&system, "netsh", &args, Privilege::Any)?.run()
            } else {
                for entry in std::fs::read_dir(dir).map_err(io_error(dir))?.flatten() {
                    let path = entry.path();
                    if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("xml")) {
                        let file = format!("filename={}", path.display());
                        build(&system, "netsh", &["wlan", "add", "profile", &file], Privilege::Any)?.run()?;
                    }
                }
                Ok(())
            }
        }
        Os::Linux => {
            if !system.privileges.is_elevated {
                return Err(Error::State(
                    t(
                        "Wi-Fi profiles are readable only by root; run: sudo skadi wifi ...",
                        "Wi-Fi profillerini yalnızca root okuyabilir; şunu çalıştırın: sudo skadi wifi ...",
                    )
                    .to_owned(),
                ));
            }
            let system_dir = Path::new("/etc/NetworkManager/system-connections");
            let (from, to) = if action == "export" {
                (system_dir, dir)
            } else {
                (dir, system_dir)
            };
            std::fs::create_dir_all(to).map_err(io_error(to))?;
            let mut count = 0;
            for entry in std::fs::read_dir(from).map_err(io_error(from))?.flatten() {
                let path = entry.path();
                let Ok(meta) = std::fs::symlink_metadata(&path) else {
                    continue;
                };
                if !meta.is_file() || path.extension().is_none_or(|e| e != "nmconnection") {
                    continue;
                }
                let text = std::fs::read_to_string(&path).map_err(io_error(&path))?;
                if !text.lines().any(|l| l.trim() == "type=wifi") {
                    continue;
                }
                let text = if action == "export" && !with_passwords {
                    strip_secrets(&text)
                } else {
                    text
                };
                let target = to.join(path.file_name().unwrap_or_default());
                std::fs::write(&target, text).map_err(io_error(&target))?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600));
                }
                if action == "export" {
                    give_to_invoking_user(&target);
                }
                count += 1;
            }
            if action == "import" && count > 0 {
                build(&system, "nmcli", &["connection", "reload"], Privilege::Any)?.run()?;
            }
            println!("{count} {}", t("Wi-Fi profiles copied.", "Wi-Fi profili kopyalandı."));
            Ok(())
        }
        Os::Macos => Err(Error::State(
            t(
                "macOS moves Wi-Fi passwords with iCloud Keychain; there is no export.",
                "macOS Wi-Fi parolalarını iCloud Anahtar Zinciri ile taşır; dışa aktarma yok.",
            )
            .to_owned(),
        )),
    }
}

// ------------------------------------------------------------------------------------------------
// Snap'ten Flatpak'e

/// Snap kökenli paketler dışında kalan Snap uygulamaları (temel ve çalışma zamanı paketleri hariç).
fn remaining_snap_apps(system: &System) -> Result<Vec<String>> {
    let output = build(system, "snap", &["list"], Privilege::Any)?.output()?;
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .skip(1)
        .filter_map(|l| l.split_whitespace().next())
        .filter(|name| {
            ![
                "core",
                "snapd",
                "bare",
                "gtk-common-themes",
                "gnome-",
                "kde-",
                "mesa-",
                "gpu-",
            ]
            .iter()
            .any(|p| name.starts_with(p))
        })
        .map(str::to_owned)
        .collect())
}

fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)?.flatten() {
        let path = entry.path();
        let meta = std::fs::symlink_metadata(&path)?;
        let target = to.join(entry.file_name());
        if meta.is_dir() {
            copy_tree(&path, &target)?;
        } else if meta.is_file() {
            std::fs::copy(&path, &target)?;
        }
    }
    Ok(())
}

/// Bir uygulamanın Snap sürümünü Flatpak sürümüyle değiştirir. Önce Flatpak kurulur ve doğrulanır,
/// sonra Snap kaldırılır. Firefox profili kopyalanır. `remove_snapd` ile başka Snap uygulaması
/// kalmadıysa snapd de kaldırılır ve yeniden kurulmaması için tutulur.
pub fn snap_to_flatpak(lang: Lang, app_id: &str, remove_snapd: bool, dry_run: bool, yes: bool) -> Result<()> {
    let t = |en, tr| lang.pick(en, tr);
    let system = System::detect()?;
    if system.os != Os::Linux {
        return Err(Error::Unsupported("snap-to-flatpak (Linux)"));
    }
    let catalog = Catalog::builtin()?;
    let app = catalog
        .get(app_id)
        .ok_or_else(|| Error::UnknownApp(app_id.to_owned()))?;
    let (Some(snap), Some(flatpak)) = (
        app.packages.get(&ManagerId::Snap),
        app.packages.get(&ManagerId::Flatpak),
    ) else {
        return Err(Error::State(format!(
            "{app_id}: {}",
            t(
                "needs both a Snap and a Flatpak package",
                "hem Snap hem Flatpak paketi gerekir"
            )
        )));
    };
    if system.manager(ManagerId::Flatpak).is_none() {
        return Err(Error::State(
            t(
                "Flatpak is missing; run: skadi bootstrap flatpak",
                "Flatpak yok; şunu çalıştırın: skadi bootstrap flatpak",
            )
            .to_owned(),
        ));
    }
    if !system.is_installed(ManagerId::Snap, snap)?.unwrap_or(false) {
        return Err(Error::State(format!(
            "{snap}: {}",
            t("not installed as a Snap", "Snap olarak kurulu değil")
        )));
    }
    let install = build(
        &system,
        "@self",
        &["install", app_id, "--manager", "flatpak", "--yes"],
        Privilege::Any,
    )?;
    let remove = build(&system, "snap", &["remove", snap], Privilege::Root)?;
    println!("> {}\n> {}", install.display(), remove.display());
    if dry_run
        || !confirm(
            lang,
            yes,
            t(
                "Replace the Snap with the Flatpak?",
                "Snap, Flatpak sürümüyle değiştirilsin mi?",
            ),
        )?
    {
        return Ok(());
    }
    install.run()?;
    if !system.is_installed(ManagerId::Flatpak, flatpak)?.unwrap_or(false) {
        return Err(Error::State(format!(
            "{flatpak}: {}",
            t(
                "Flatpak install could not be verified; the Snap was kept",
                "Flatpak kurulumu doğrulanamadı; Snap korundu"
            )
        )));
    }
    // Firefox profili: yalnızca Flatpak tarafında henüz profil yoksa kopyalanır.
    if app_id == "firefox"
        && let Some(home) = std::env::var_os("HOME").map(PathBuf::from)
        && !system.privileges.is_elevated
    {
        let from = home.join("snap/firefox/common/.mozilla/firefox");
        let to = home.join(".var/app/org.mozilla.firefox/.mozilla/firefox");
        if from.is_dir() && !to.exists() {
            copy_tree(&from, &to).map_err(io_error(&to))?;
            println!(
                "{}",
                t(
                    "Firefox profile copied to the Flatpak.",
                    "Firefox profili Flatpak'e kopyalandı."
                )
            );
        }
    }
    remove.run()?;
    if remove_snapd {
        let left = remaining_snap_apps(&system)?;
        if !left.is_empty() {
            println!(
                "{}: {}",
                t(
                    "snapd kept; other Snap apps remain",
                    "snapd korundu; başka Snap uygulamaları var"
                ),
                left.join(", ")
            );
            return Ok(());
        }
        if system.manager(ManagerId::Apt).is_none() {
            return Err(Error::Unsupported("remove snapd (apt)"));
        }
        build(&system, "apt-get", &["purge", "-y", "snapd"], Privilege::Root)?.run()?;
        // Ubuntu'nun geçiş paketleri snapd'yi yeniden kurmasın.
        build(&system, "apt-mark", &["hold", "snapd"], Privilege::Root)?.run()?;
    }
    Ok(())
}

// ------------------------------------------------------------------------------------------------
// Hosts engelleme

const BLOCK_LIST_URL: &str = "https://raw.githubusercontent.com/StevenBlack/hosts/master/hosts";

pub fn hosts_path(os: Os) -> PathBuf {
    match os {
        Os::Windows => PathBuf::from(std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into()))
            .join("System32\\drivers\\etc\\hosts"),
        _ => PathBuf::from("/etc/hosts"),
    }
}

/// `skadi hosts block|unblock|status`. Liste kullanıcı yetkisiyle indirilir ve doğrulanır; yalnızca
/// dosyaya yazma adımı yetki ister.
pub fn hosts(lang: Lang, action: &str, yes: bool) -> Result<()> {
    let t = |en, tr| lang.pick(en, tr);
    let system = System::detect()?;
    let path = hosts_path(system.os);
    let current = std::fs::read_to_string(&path).map_err(io_error(&path))?;
    match action {
        "status" => {
            match hosts::block_size(&current) {
                Some(n) => println!(
                    "{n} {}",
                    t(
                        "domains are blocked by Skadi.",
                        "alan adı Skadi tarafından engelleniyor."
                    )
                ),
                None => println!(
                    "{}",
                    t(
                        "No Skadi block list in the hosts file.",
                        "Hosts dosyasında Skadi engel listesi yok."
                    )
                ),
            }
            Ok(())
        }
        "unblock" => {
            if hosts::block_size(&current).is_none() {
                println!("{}", t("Nothing to remove.", "Kaldırılacak bir şey yok."));
                return Ok(());
            }
            build(
                &system,
                "@self",
                &["internal", "hosts-set", "unblock", "-"],
                Privilege::Root,
            )?
            .run_captured()
        }
        _ => {
            println!(
                "{}\n{BLOCK_LIST_URL}",
                t(
                    "Downloads the StevenBlack ads and malware list (MIT licence):",
                    "StevenBlack reklam ve zararlı yazılım listesini indirir (MIT lisansı):"
                )
            );
            if !confirm(
                lang,
                yes,
                t(
                    "Block these domains in the hosts file?",
                    "Bu alan adları hosts dosyasında engellensin mi?",
                ),
            )? {
                return Ok(());
            }
            let curl = find_program("curl").ok_or(Error::ProgramNotFound("curl"))?;
            let nonce = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos());
            let list = std::env::temp_dir().join(format!("skadi-hosts-{}-{nonce}.txt", std::process::id()));
            let download = Invocation::build(
                curl,
                vec![
                    "-fsSL".into(),
                    "--proto".into(),
                    "=https".into(),
                    "--tlsv1.2".into(),
                    "-o".into(),
                    list.clone().into_os_string(),
                    BLOCK_LIST_URL.into(),
                ],
                &[],
                Privilege::Any,
                &system.privileges,
                system.os,
            )?;
            download.run_captured()?;
            let domains = hosts::parse_block_list(&std::fs::read_to_string(&list).map_err(io_error(&list))?);
            // Doğrulanmış liste yetkili yazma adımına dosya olarak verilir (komut satırı sınırı).
            let clean: String = domains.iter().map(|d| format!("0.0.0.0 {d}\n")).collect();
            std::fs::write(&list, clean).map_err(io_error(&list))?;
            let list_text = list.to_string_lossy().into_owned();
            let result = build(
                &system,
                "@self",
                &["internal", "hosts-set", "block", &list_text],
                Privilege::Root,
            )?
            .run_captured();
            let _ = std::fs::remove_file(&list);
            result?;
            println!(
                "{} {}",
                domains.len(),
                t(
                    "domains blocked. Undo with: skadi hosts unblock",
                    "alan adı engellendi. Geri almak için: skadi hosts unblock"
                )
            );
            if system.os == Os::Windows {
                println!(
                    "{}",
                    t(
                        "Microsoft Defender may report the hosts change; system domains are never blocked by Skadi.",
                        "Microsoft Defender hosts değişikliğini bildirebilir; Skadi sistem alan adlarını hiçbir zaman engellemez."
                    )
                );
            }
            Ok(())
        }
    }
}

/// Yetkili adım: listeyi yeniden doğrular ve yalnızca Skadi bloğunu değiştirir.
pub fn internal_hosts_set(mode: &str, list: &str) -> Result<()> {
    let os = Os::current()?;
    let path = hosts_path(os);
    let current = std::fs::read_to_string(&path).map_err(io_error(&path))?;
    let updated = match mode {
        "block" => {
            let list_path = Path::new(list);
            let name_ok = list_path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("skadi-hosts-") && n.ends_with(".txt"));
            if !name_ok {
                return Err(Error::InvalidOption("hosts list"));
            }
            let text = std::fs::read_to_string(list_path).map_err(io_error(list_path))?;
            hosts::with_block(&current, &hosts::parse_block_list(&text))
        }
        "unblock" => hosts::without_block(&current),
        _ => return Err(Error::InvalidOption("block | unblock")),
    };
    std::fs::write(&path, updated).map_err(io_error(&path))
}

// ------------------------------------------------------------------------------------------------
// Güvenlik duvarı

fn service_active(system: &System, unit: &str) -> bool {
    build(system, "systemctl", &["is-active", "--quiet", unit], Privilege::Any)
        .and_then(|i| i.output())
        .is_ok_and(|o| o.status.success())
}

/// Güvenlik duvarını açar ya da durumunu gösterir. Kapatma komutu bilerek yoktur.
pub fn firewall(lang: Lang, action: &str, dry_run: bool, yes: bool) -> Result<()> {
    let t = |en, tr| lang.pick(en, tr);
    let detected = System::detect()?;
    let preview = commands::preview_system(lang, &detected, dry_run);
    let system = preview.as_ref().unwrap_or(&detected);
    let ufw = find_program("ufw").is_some();
    let firewalld = find_program("firewall-cmd").is_some();
    let socketfilterfw = "/usr/libexec/ApplicationFirewall/socketfilterfw";
    if action == "status" {
        let invocation = match system.os {
            Os::Windows => powershell(
                system,
                "Get-NetFirewallProfile | Format-Table Name, Enabled",
                Privilege::Any,
            )?,
            Os::Linux if ufw => build(system, "ufw", &["status", "verbose"], Privilege::Root)?,
            Os::Linux if firewalld => build(system, "firewall-cmd", &["--state"], Privilege::Any)?,
            Os::Linux => return Err(Error::ProgramNotFound("ufw / firewalld")),
            Os::Macos => Invocation::build(
                socketfilterfw.into(),
                vec!["--getglobalstate".into()],
                &[],
                Privilege::Any,
                &system.privileges,
                system.os,
            )?,
        };
        return invocation.run();
    }
    let mut plan = Vec::new();
    match system.os {
        Os::Windows => plan.push(powershell(
            system,
            "Set-NetFirewallProfile -Profile Domain,Public,Private -Enabled True",
            Privilege::Root,
        )?),
        Os::Linux if ufw => {
            // ufw(8): açmak SSH bağlantılarını kesebilir; önce SSH'ye izin verilir.
            if service_active(system, "ssh.service") || service_active(system, "sshd.service") {
                plan.push(build(system, "ufw", &["allow", "22/tcp"], Privilege::Root)?);
            }
            plan.push(build(system, "ufw", &["default", "deny", "incoming"], Privilege::Root)?);
            plan.push(build(system, "ufw", &["--force", "enable"], Privilege::Root)?);
        }
        // firewalld'nin varsayılan bölgesi SSH'ye izin verir.
        Os::Linux if firewalld => plan.push(build(
            system,
            "systemctl",
            &["enable", "--now", "firewalld.service"],
            Privilege::Root,
        )?),
        Os::Linux => return Err(Error::ProgramNotFound("ufw / firewalld")),
        Os::Macos => {
            for flag in ["--setglobalstate", "--setstealthmode"] {
                plan.push(Invocation::build(
                    socketfilterfw.into(),
                    vec![flag.into(), "on".into()],
                    &[],
                    Privilege::Root,
                    &system.privileges,
                    system.os,
                )?);
            }
        }
    }
    for invocation in &plan {
        println!("> {}", invocation.display());
    }
    if dry_run
        || !confirm(
            lang,
            yes,
            t(
                "Turn on the firewall and block unasked incoming connections?",
                "Güvenlik duvarı açılsın ve istenmeyen gelen bağlantılar engellensin mi?",
            ),
        )?
    {
        return Ok(());
    }
    plan.iter().try_for_each(Invocation::run)
}

// ------------------------------------------------------------------------------------------------
// Sayfa dosyası / takas

pub fn pagefile(lang: Lang, auto: bool, dry_run: bool, yes: bool) -> Result<()> {
    let system = System::detect()?;
    if auto {
        if system.os != Os::Windows {
            return Err(Error::Unsupported("pagefile --auto (Windows)"));
        }
        let invocation = powershell(
            &system,
            "Get-CimInstance Win32_ComputerSystem | Set-CimInstance -Property @{ AutomaticManagedPagefile = $true }",
            Privilege::Root,
        )?;
        println!("> {}", invocation.display());
        if dry_run
            || !confirm(
                lang,
                yes,
                lang.pick(
                    "Let Windows manage the page file size? A restart applies it.",
                    "Sayfa dosyası boyutunu Windows yönetsin mi? Yeniden başlatınca geçerli olur.",
                ),
            )?
        {
            return Ok(());
        }
        return invocation.run();
    }
    let invocation = match system.os {
        Os::Windows => powershell(
            &system,
            "'Automatic: ' + (Get-CimInstance Win32_ComputerSystem).AutomaticManagedPagefile; \
             Get-CimInstance Win32_PageFileUsage | ForEach-Object { $_.Name + ': ' + $_.AllocatedBaseSize + ' MB, in use ' + $_.CurrentUsage + ' MB, peak ' + $_.PeakUsage + ' MB' }",
            Privilege::Any,
        )?,
        Os::Linux => build(&system, "swapon", &["--show"], Privilege::Any)?,
        Os::Macos => build(&system, "sysctl", &["vm.swapusage"], Privilege::Any)?,
    };
    invocation.run()
}

// ------------------------------------------------------------------------------------------------
// Başlat sabitlemeleri (Windows 11)

const START_PINS: &str = r#"{"pinnedList":[{"packagedAppId":"windows.immersivecontrolpanel_cw5n1h2txyewy!microsoft.windows.immersivecontrolpanel"},{"desktopAppLink":"%APPDATA%\\Microsoft\\Windows\\Start Menu\\Programs\\File Explorer.lnk"}]}"#;

/// Başlat menüsünde yalnızca Ayarlar ve Dosya Gezgini sabitli kalsın diye "Configure Start Pins"
/// ilkesini ayarlar (`apply`) ya da kaldırır (`revert`). Windows 11 Pro, Enterprise, Education.
pub fn start_pins(lang: Lang, action: &str, dry_run: bool, yes: bool) -> Result<()> {
    let detected = System::detect()?;
    if detected.os != Os::Windows {
        return Err(Error::Unsupported("start-pins (Windows 11)"));
    }
    if detected
        .windows
        .as_ref()
        .is_some_and(|w| w.is_home() || w.build < 22000)
    {
        return Err(Error::Unsupported("start-pins (Windows 11 Pro, Enterprise, Education)"));
    }
    let preview = commands::preview_system(lang, &detected, dry_run);
    let system = preview.as_ref().unwrap_or(&detected);
    let key = "HKLM\\SOFTWARE\\Policies\\Microsoft\\Windows\\Explorer";
    let file = PathBuf::from(std::env::var_os("ProgramData").unwrap_or_else(|| "C:\\ProgramData".into()))
        .join("Skadi")
        .join("start-pins.json");
    let file_text = file.to_string_lossy().into_owned();
    let invocation = if action == "revert" {
        build(
            system,
            "reg",
            &["delete", key, "/v", "ConfigureStartPins", "/f"],
            Privilege::Root,
        )?
    } else {
        build(
            system,
            "reg",
            &[
                "add",
                key,
                "/v",
                "ConfigureStartPins",
                "/t",
                "REG_SZ",
                "/d",
                &file_text,
                "/f",
            ],
            Privilege::Root,
        )?
    };
    println!("> {}", invocation.display());
    if dry_run
        || !confirm(
            lang,
            yes,
            lang.pick(
                "Change the Start menu pins policy? Signing out applies it.",
                "Başlat menüsü sabitleme ilkesi değiştirilsin mi? Oturumu kapatıp açınca geçerli olur.",
            ),
        )?
    {
        return Ok(());
    }
    if action != "revert" {
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir).map_err(io_error(dir))?;
        }
        std::fs::write(&file, START_PINS).map_err(io_error(&file))?;
    }
    invocation.run_captured()?;
    if action == "revert" {
        let _ = std::fs::remove_file(&file);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_proxy_and_strips_wifi_secrets() {
        assert_eq!(split_address("proxy.corp:8080"), Some(("proxy.corp", "8080")));
        assert!(split_address("user:pass@proxy:80").is_none());
        assert!(split_address("-x:80").is_none() && split_address("host:0").is_none());
        let text = "[wifi-security]\nkey-mgmt=wpa-psk\npsk=secret\n[connection]\ntype=wifi\n";
        let clean = strip_secrets(text);
        assert!(!clean.contains("secret") && clean.contains("type=wifi"));
        assert!(serde_json::from_str::<serde_json::Value>(START_PINS).is_ok());
    }
}
