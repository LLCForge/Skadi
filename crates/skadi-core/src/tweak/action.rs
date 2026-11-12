//! Eylemlerin komutlara dönüştürülmesi ve sistem çıktılarının ayrıştırılması.
//! Buradaki fonksiyonlar saftır; komutları `engine` çalıştırır.

use serde::{Deserialize, Serialize};

use super::{Action, DefaultsKind, Guard, RegKind, SystemdMode, config, is_plain_value, is_valid_reg_data};
use crate::{
    error::{Error, Result},
    exec::Privilege,
    manager::ManagerId,
};

/// Uygulamadan önce kaydedilen eski durum; geri alma bununla yapılır.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Previous {
    /// `None`: değer yoktu, geri almada silinir. Sayılar onluk tabanda tutulur.
    /// `created`: Skadi'nin oluşturduğu en üst anahtar; geri almada boşsa silinir.
    Registry {
        value: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        created: Option<String>,
    },
    Service {
        start: u32,
    },
    /// Görev etkindi.
    Task,
    /// Uygulama kuruluydu.
    Appx,
    Systemd {
        state: String,
    },
    /// Paket bu yöneticiyle kuruluydu.
    Package {
        manager: ManagerId,
    },
    Gsettings {
        value: String,
    },
    /// Yapılandırma satırının özgün hali; `None` ise satır yoktu.
    ConfigLine {
        line: Option<String>,
    },
    /// Yönetilen dosyanın önceki içeriği; `None` ise dosya yoktu.
    ManagedFile {
        content: Option<String>,
    },
    /// Windows özelliği kapalıydı.
    Feature,
    /// KDE ayarının özgün değeri; `None` ise ayar yoktu.
    Kconfig {
        value: Option<String>,
    },
    /// `None`: anahtar yoktu, geri almada silinir.
    Defaults {
        value: Option<String>,
    },
}

/// Bir eylemin sistemdeki şu anki durumu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Observed {
    /// Bu sistemde uygulanamaz (servis, görev, birim ya da ayar yok).
    Unavailable,
    /// Zaten istenen durumda.
    Done,
    /// Uygulanabilir; mevcut durum kaydedildi.
    Pending(Previous),
}

/// Çalıştırılacak komut; program adı `engine` tarafından mutlak yola çözülür.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Cmd {
    pub program: &'static str,
    pub args: Vec<String>,
    pub privilege: Privilege,
}

impl Cmd {
    fn new(program: &'static str, args: &[&str], privilege: Privilege) -> Self {
        Self {
            program,
            args: args.iter().map(|a| (*a).to_owned()).collect(),
            privilege,
        }
    }
}

/// Skadi'nin kendisi (`@self`) yetkili olarak dosyayı düzenler: `skadi internal config-set`.
fn config_cmd(path: &str, section: Option<&str>, key: &str, mode: &str, text: &str) -> Cmd {
    Cmd::new(
        SELF,
        &["internal", "config-set", path, section.unwrap_or(""), key, mode, text],
        Privilege::Root,
    )
}

fn file_cmd(path: &str, reload: &str, guard: &str, mode: &str, content: &str) -> Cmd {
    Cmd::new(
        SELF,
        &["internal", "file-set", path, reload, guard, mode, content],
        Privilege::Root,
    )
}

/// Plasma 6'da `kwriteconfig6`, Plasma 5'te `kwriteconfig5`; motor hangisi varsa onu kullanır.
pub(crate) const KWRITECONFIG: &str = "@kwriteconfig";

/// `Cmd::program` olarak Skadi'nin kendi yürütülebilir dosyası.
pub(crate) const SELF: &str = "@self";

pub(crate) fn powershell(script: &str, privilege: Privilege) -> Cmd {
    Cmd::new(
        "powershell",
        &[
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ],
        privilege,
    )
}

/// PowerShell tek tırnaklı dize. Katalog doğrulaması tırnağı zaten yasaklar; bu ikinci savunma hattıdır.
pub(crate) fn ps_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

pub(crate) fn split_task(task: &str) -> (&str, &str) {
    // Doğrulama `\` ile başladığını garanti eder; yol kısmı sondaki `\` ile birlikte döner.
    let index = task.rfind('\\').unwrap_or(0);
    (&task[..=index], &task[index + 1..])
}

pub(crate) fn registry_privilege(key: &str) -> Privilege {
    if key.starts_with("HKCU\\") {
        Privilege::Any
    } else {
        Privilege::Root
    }
}

pub(crate) fn service_key(name: &str) -> String {
    format!("HKLM\\SYSTEM\\CurrentControlSet\\Services\\{name}")
}

/// Ad boşsa anahtarın varsayılan (adsız) değeri kastedilir: `reg.exe /ve`.
fn reg_value_args(name: &str) -> Vec<&str> {
    if name.is_empty() { vec!["/ve"] } else { vec!["/v", name] }
}

fn reg_add(key: &str, name: &str, kind: RegKind, data: &str) -> Cmd {
    let mut args = vec!["add", key];
    args.extend(reg_value_args(name));
    args.extend(["/t", kind.reg_type(), "/d", data, "/f"]);
    Cmd::new("reg", &args, registry_privilege(key))
}

/// Kayıttaki "oluşturulan anahtar" yolunun güvenle silinebileceğini doğrular: eylemin kendi
/// yolunun öneki olmalı, hive'a fazla yakın olmamalı ve sistemin paylaşılan bir anahtarı olmamalı.
/// Kayıt dosyası kullanıcı tarafından değiştirilebildiği için bu kontrol silmeden önce yapılır.
pub(crate) fn is_safe_created_root(action_key: &str, created: &str) -> bool {
    const SHARED: [&str; 9] = [
        "microsoft",
        "policies",
        "classes",
        "clsid",
        "windows",
        "currentversion",
        "services",
        "wow6432node",
        "currentcontrolset",
    ];
    let inside = action_key == created || action_key.starts_with(&format!("{created}\\"));
    let parts: Vec<&str> = created.split('\\').filter(|part| !part.is_empty()).collect();
    // HKU altında her kullanıcı için fazladan bir seviye vardır (HKU\\<SID>\\Software).
    let min_depth = if created.starts_with("HKU\\") { 4 } else { 3 };
    inside
        && parts.len() >= min_depth
        && parts
            .last()
            .is_some_and(|leaf| !SHARED.contains(&leaf.to_ascii_lowercase().as_str()))
}

/// `systemctl is-enabled` çıktısı. Birim yoksa `None`.
pub(crate) fn parse_systemd_state(stdout: &str) -> Option<String> {
    let state = stdout.lines().next()?.trim();
    let valid =
        !state.is_empty() && state != "not-found" && state.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    valid.then(|| state.to_owned())
}

pub(crate) fn systemd_is_target(mode: SystemdMode, state: &str) -> bool {
    match mode {
        SystemdMode::Disable => matches!(state, "disabled" | "masked"),
        SystemdMode::Mask => state == "masked",
        SystemdMode::Enable => state == "enabled",
    }
}

/// `defaults read` çıktısını katalogdaki değerle karşılaştırılabilir hale getirir.
pub(crate) fn normalize_defaults(kind: DefaultsKind, value: &str) -> String {
    let value = value.trim();
    match (kind, value) {
        (DefaultsKind::Bool, "true" | "YES" | "1") => "1".to_owned(),
        (DefaultsKind::Bool, "false" | "NO" | "0") => "0".to_owned(),
        _ => value.to_owned(),
    }
}

fn defaults_privilege(system: bool) -> Privilege {
    if system { Privilege::Root } else { Privilege::NonRoot }
}

/// Eylemi uygulayan komutlar. `Package` motor tarafından paket yöneticisiyle işlenir.
pub(crate) fn apply_cmds(action: &Action) -> Vec<Cmd> {
    match action {
        Action::Registry { key, name, kind, data } => vec![reg_add(key, name, *kind, data)],
        Action::Service { name, start } => {
            vec![reg_add(
                &service_key(name),
                "Start",
                RegKind::Dword,
                &start.registry_value().to_string(),
            )]
        }
        Action::ScheduledTask { task } => {
            let (path, name) = split_task(task);
            vec![powershell(
                &format!(
                    "Disable-ScheduledTask -TaskPath {} -TaskName {} -ErrorAction Stop | Out-Null",
                    ps_literal(path),
                    ps_literal(name)
                ),
                Privilege::Root,
            )]
        }
        Action::Appx { package } => vec![powershell(
            &format!(
                "$ErrorActionPreference = 'Stop'; $name = {}; \
                 Get-AppxPackage -AllUsers -Name $name | Remove-AppxPackage -AllUsers; \
                 Get-AppxProvisionedPackage -Online | Where-Object DisplayName -EQ $name | Remove-AppxProvisionedPackage -Online | Out-Null",
                ps_literal(package)
            ),
            Privilege::Root,
        )],
        Action::Systemd { unit, mode } => {
            let verb = match mode {
                SystemdMode::Disable => "disable",
                SystemdMode::Mask => "mask",
                SystemdMode::Enable => "enable",
            };
            vec![Cmd::new("systemctl", &[verb, "--now", unit], Privilege::Root)]
        }
        Action::Package { .. } => Vec::new(),
        Action::ConfigLine {
            path,
            section,
            key,
            value,
        } => vec![config_cmd(path, section.as_deref(), key, "value", value)],
        Action::ManagedFile {
            path,
            content,
            reload,
            guard,
            ..
        } => vec![file_cmd(path, reload.as_str(), guard.as_str(), "write", content)],
        Action::Kconfig {
            file,
            group,
            key,
            value,
        } => vec![Cmd::new(
            KWRITECONFIG,
            &["--file", file, "--group", group, "--key", key, value],
            Privilege::NonRoot,
        )],
        Action::WindowsFeature { name } => vec![powershell(
            &format!(
                "Enable-WindowsOptionalFeature -Online -FeatureName {} -All -NoRestart -ErrorAction Stop | Out-Null",
                ps_literal(name)
            ),
            Privilege::Root,
        )],
        Action::Gsettings { schema, key, value } => {
            vec![Cmd::new("gsettings", &["set", schema, key, value], Privilege::NonRoot)]
        }
        Action::Defaults {
            domain,
            key,
            kind,
            value,
            system,
        } => vec![Cmd::new(
            "defaults",
            &["write", domain, key, kind.flag(), value],
            defaults_privilege(*system),
        )],
    }
}

/// Kaydedilen eski durumu geri yükleyen komutlar. Kayıt dosyası kullanıcı tarafından
/// değiştirilebileceği için eski değerler burada yeniden doğrulanır.
pub(crate) fn restore_cmds(action: &Action, previous: &Previous) -> Result<Vec<Cmd>> {
    let tampered = || Error::State(format!("{}: invalid saved state", action.key()));
    let cmds = match (action, previous) {
        (Action::Registry { key, name, kind, .. }, Previous::Registry { value, created }) => {
            if created.as_deref().is_some_and(|root| !is_safe_created_root(key, root)) {
                return Err(tampered());
            }
            match value {
                Some(value) if is_valid_reg_data(*kind, value) => vec![reg_add(key, name, *kind, value)],
                Some(_) => return Err(tampered()),
                None => {
                    let mut args = vec!["delete", key.as_str()];
                    args.extend(reg_value_args(name));
                    args.push("/f");
                    vec![Cmd::new("reg", &args, registry_privilege(key))]
                }
            }
        }
        (Action::Service { name, .. }, Previous::Service { start }) if *start <= 4 => {
            vec![reg_add(&service_key(name), "Start", RegKind::Dword, &start.to_string())]
        }
        (Action::ScheduledTask { task }, Previous::Task) => {
            let (path, name) = split_task(task);
            vec![powershell(
                &format!(
                    "Enable-ScheduledTask -TaskPath {} -TaskName {} -ErrorAction Stop | Out-Null",
                    ps_literal(path),
                    ps_literal(name)
                ),
                Privilege::Root,
            )]
        }
        (Action::Appx { .. }, Previous::Appx) => return Err(Error::NotReversible(action.key())),
        (Action::Systemd { unit, mode }, Previous::Systemd { state }) => {
            if parse_systemd_state(state).is_none() {
                return Err(tampered());
            }
            let mut cmds = Vec::new();
            if *mode == SystemdMode::Mask && state != "masked" {
                cmds.push(Cmd::new("systemctl", &["unmask", unit], Privilege::Root));
            }
            if *mode == SystemdMode::Enable {
                match state.as_str() {
                    "disabled" => cmds.push(Cmd::new("systemctl", &["disable", "--now", unit], Privilege::Root)),
                    "masked" => cmds.push(Cmd::new("systemctl", &["mask", "--now", unit], Privilege::Root)),
                    _ => {}
                }
            } else if state == "enabled" {
                cmds.push(Cmd::new("systemctl", &["enable", "--now", unit], Privilege::Root));
            }
            cmds
        }
        (Action::ManagedFile { path, reload, .. }, Previous::ManagedFile { content }) => match content {
            // Geri yazmada güvenlik koşulu aranmaz: önceki duruma dönmek hiç kimseyi dışarıda bırakmaz.
            Some(old) if config::is_managed_content(old) => {
                vec![file_cmd(path, reload.as_str(), Guard::None.as_str(), "write", old)]
            }
            Some(_) => return Err(tampered()),
            None => vec![file_cmd(path, reload.as_str(), Guard::None.as_str(), "remove", "")],
        },
        (Action::Gsettings { schema, key, .. }, Previous::Gsettings { value }) if is_plain_value(value) => {
            vec![Cmd::new("gsettings", &["set", schema, key, value], Privilege::NonRoot)]
        }
        (
            Action::Defaults {
                domain,
                key,
                kind,
                system,
                ..
            },
            Previous::Defaults { value },
        ) => {
            let privilege = defaults_privilege(*system);
            match value {
                None => vec![Cmd::new("defaults", &["delete", domain, key], privilege)],
                Some(value) => {
                    let value = match (kind, normalize_defaults(*kind, value).as_str()) {
                        (DefaultsKind::Bool, "1") => "true".to_owned(),
                        (DefaultsKind::Bool, "0") => "false".to_owned(),
                        (DefaultsKind::Bool, _) => return Err(tampered()),
                        (DefaultsKind::Int, v) if v.parse::<i64>().is_ok() => v.to_owned(),
                        (DefaultsKind::String, v) if is_plain_value(v) => v.to_owned(),
                        _ => return Err(tampered()),
                    };
                    vec![Cmd::new(
                        "defaults",
                        &["write", domain, key, kind.flag(), &value],
                        privilege,
                    )]
                }
            }
        }
        (Action::Package { .. }, Previous::Package { .. }) => Vec::new(),
        (Action::ConfigLine { path, section, key, .. }, Previous::ConfigLine { line }) => match line {
            // Kayıttaki satır aynı anahtara ait olmalı ve satır sonu içermemeli.
            Some(line) if config::key_of_line(line) == Some(key.as_str()) && !line.contains('\n') => {
                vec![config_cmd(path, section.as_deref(), key, "line", line)]
            }
            Some(_) => return Err(tampered()),
            None => vec![config_cmd(path, section.as_deref(), key, "remove", "")],
        },
        (Action::Kconfig { file, group, key, .. }, Previous::Kconfig { value }) => match value {
            Some(value) if is_plain_value(value) => vec![Cmd::new(
                KWRITECONFIG,
                &["--file", file, "--group", group, "--key", key, value],
                Privilege::NonRoot,
            )],
            Some(_) => return Err(tampered()),
            None => vec![Cmd::new(
                KWRITECONFIG,
                &["--file", file, "--group", group, "--key", key, "--delete"],
                Privilege::NonRoot,
            )],
        },
        (Action::WindowsFeature { name }, Previous::Feature) => vec![powershell(
            &format!(
                "Disable-WindowsOptionalFeature -Online -FeatureName {} -NoRestart -ErrorAction Stop | Out-Null",
                ps_literal(name)
            ),
            Privilege::Root,
        )],
        _ => return Err(tampered()),
    };
    Ok(cmds)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tweak::ServiceStart;

    #[test]
    fn parses_systemd_states() {
        assert_eq!(parse_systemd_state("enabled\n").as_deref(), Some("enabled"));
        assert_eq!(parse_systemd_state(""), None);
        assert_eq!(parse_systemd_state("not-found\n"), None);
        assert!(systemd_is_target(SystemdMode::Disable, "masked"));
        assert!(!systemd_is_target(SystemdMode::Mask, "disabled"));
    }

    #[test]
    fn splits_task_paths() {
        assert_eq!(
            split_task("\\Microsoft\\Windows\\Feedback\\Siuf\\DmClient"),
            ("\\Microsoft\\Windows\\Feedback\\Siuf\\", "DmClient")
        );
        assert_eq!(ps_literal("a'b"), "'a''b'");
    }

    #[test]
    fn registry_restore_deletes_or_restores() {
        let action = Action::Registry {
            key: "HKCU\\Software\\X".into(),
            name: "V".into(),
            kind: RegKind::Dword,
            data: "0".into(),
        };
        let delete = restore_cmds(
            &action,
            &Previous::Registry {
                value: None,
                created: None,
            },
        )
        .unwrap();
        assert_eq!(delete[0].args, ["delete", "HKCU\\Software\\X", "/v", "V", "/f"]);
        assert_eq!(delete[0].privilege, Privilege::Any);
        let restore = restore_cmds(
            &action,
            &Previous::Registry {
                value: Some("1".into()),
                created: None,
            },
        )
        .unwrap();
        assert_eq!(restore[0].args[7], "1");
        assert!(
            restore_cmds(
                &action,
                &Previous::Registry {
                    value: Some("x".into()),
                    created: None
                }
            )
            .is_err()
        );
        assert!(restore_cmds(&action, &Previous::Task).is_err());
    }

    #[test]
    fn default_value_and_created_key_rules() {
        let action = Action::Registry {
            key: "HKCU\\Software\\Classes\\CLSID\\{x}\\InprocServer32".into(),
            name: String::new(),
            kind: RegKind::String,
            data: String::new(),
        };
        assert!(apply_cmds(&action)[0].args.contains(&"/ve".to_owned()));
        let restore = restore_cmds(
            &action,
            &Previous::Registry {
                value: None,
                created: None,
            },
        )
        .unwrap();
        assert_eq!(
            restore[0].args,
            [
                "delete",
                "HKCU\\Software\\Classes\\CLSID\\{x}\\InprocServer32",
                "/ve",
                "/f"
            ]
        );

        let key = "HKCU\\Software\\Classes\\CLSID\\{x}\\InprocServer32";
        assert!(is_safe_created_root(key, "HKCU\\Software\\Classes\\CLSID\\{x}"));
        assert!(
            !is_safe_created_root(key, "HKCU\\Software"),
            "hive'a yakın anahtar silinmemeli"
        );
        assert!(
            !is_safe_created_root(key, "HKCU\\Software\\Other\\Key"),
            "başka yol silinmemeli"
        );
        assert!(
            !is_safe_created_root(key, "HKCU\\Software\\Classes\\CLS"),
            "yarım bileşen önek değildir"
        );
        let tampered = Previous::Registry {
            value: None,
            created: Some("HKCU\\Software".into()),
        };
        assert!(restore_cmds(&action, &tampered).is_err());
    }

    #[test]
    fn service_and_systemd_commands() {
        let service = Action::Service {
            name: "DiagTrack".into(),
            start: ServiceStart::Disabled,
        };
        let cmd = &apply_cmds(&service)[0];
        assert_eq!(cmd.args[1], "HKLM\\SYSTEM\\CurrentControlSet\\Services\\DiagTrack");
        assert_eq!(cmd.args[7], "4");
        assert_eq!(cmd.privilege, Privilege::Root);

        let unit = Action::Systemd {
            unit: "apport.service".into(),
            mode: SystemdMode::Mask,
        };
        let restore = restore_cmds(
            &unit,
            &Previous::Systemd {
                state: "enabled".into(),
            },
        )
        .unwrap();
        assert_eq!(restore[0].args, ["unmask", "apport.service"]);
        assert_eq!(restore[1].args, ["enable", "--now", "apport.service"]);
        assert!(restore_cmds(&Action::Appx { package: "X".into() }, &Previous::Appx).is_err());
    }

    #[test]
    fn defaults_round_trip() {
        let action = Action::Defaults {
            domain: "com.apple.AdLib".into(),
            key: "allowApplePersonalizedAdvertising".into(),
            kind: DefaultsKind::Bool,
            value: "false".into(),
            system: false,
        };
        assert_eq!(
            normalize_defaults(DefaultsKind::Bool, "0\n"),
            normalize_defaults(DefaultsKind::Bool, "false")
        );
        let restore = restore_cmds(
            &action,
            &Previous::Defaults {
                value: Some("1".into()),
            },
        )
        .unwrap();
        assert_eq!(restore[0].args.last().map(String::as_str), Some("true"));
        assert_eq!(restore[0].privilege, Privilege::NonRoot);
    }
}
