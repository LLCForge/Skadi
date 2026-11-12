//! Güvenlik denetimi: yalnızca okur, yönetici yetkisi istemez. Okunamayan bir durum "kaldı"
//! değil "bilinmiyor" sayılır.

use std::process::{Command, Stdio};

use serde::Serialize;

use crate::{exec::find_program, i18n::Lang, platform::Os};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Pass,
    Fail,
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
pub struct Check {
    pub id: &'static str,
    #[serde(skip)]
    pub name: (&'static str, &'static str),
    pub status: Status,
}

impl Check {
    pub fn label(&self, lang: Lang) -> &'static str {
        lang.pick(self.name.0, self.name.1)
    }
}

fn check(id: &'static str, en: &'static str, tr: &'static str, status: Status) -> Check {
    Check {
        id,
        name: (en, tr),
        status,
    }
}

fn from_bool(value: Option<bool>) -> Status {
    match value {
        Some(true) => Status::Pass,
        Some(false) => Status::Fail,
        None => Status::Unknown,
    }
}

/// Programın standart çıktısı (çıkış kodundan bağımsız); program yoksa `None`.
pub(crate) fn stdout_of(program: &str, args: &[&str]) -> Option<String> {
    let path = if program.starts_with('/') {
        std::path::PathBuf::from(program)
    } else {
        find_program(program)?
    };
    let output = Command::new(path)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    Some(String::from_utf8_lossy(&output.stdout).into_owned())
}

pub fn run_checks(os: Os) -> Vec<Check> {
    match os {
        Os::Windows => windows_checks(),
        Os::Linux => linux_checks(),
        Os::Macos => macos_checks(),
    }
}

#[cfg(windows)]
fn windows_checks() -> Vec<Check> {
    use windows_registry::LOCAL_MACHINE;
    let dword = |path: &str, name: &str| LOCAL_MACHINE.open(path).ok().and_then(|k| k.get_u32(name).ok());
    let ps = |script: &str| {
        stdout_of("powershell", &["-NoProfile", "-NonInteractive", "-Command", script]).map(|s| s.trim().to_owned())
    };
    let firewall = ["DomainProfile", "StandardProfile", "PublicProfile"]
        .iter()
        .map(|profile| {
            dword(
                &format!("SYSTEM\\CurrentControlSet\\Services\\SharedAccess\\Parameters\\FirewallPolicy\\{profile}"),
                "EnableFirewall",
            )
        });
    let firewall: Option<Vec<u32>> = firewall.collect();
    // Shell özelliği yönetici olmadan okunabilir: 1 açık, 2 kapalı.
    let bitlocker = ps(
        "(New-Object -ComObject Shell.Application).NameSpace($env:SystemDrive).Self.ExtendedProperty('System.Volume.BitLockerProtection')",
    );
    let defender = ps("(Get-MpComputerStatus -ErrorAction Stop).RealTimeProtectionEnabled");
    // Parolası zorunlu olmayan etkin yerel hesaplar boş parolayla oturum açabilir.
    let passwordless = ps("@(Get-LocalUser | Where-Object { $_.Enabled -and -not $_.PasswordRequired }).Count");
    // Yerleşik Administrator hesabının SID'i her zaman -500 ile biter (ad dile göre değişir).
    let builtin_admin = ps("(Get-LocalUser | Where-Object { $_.SID.Value -like '*-500' }).Enabled");
    vec![
        check(
            "secure-boot",
            "Secure Boot",
            "Güvenli Önyükleme",
            from_bool(
                dword(
                    "SYSTEM\\CurrentControlSet\\Control\\SecureBoot\\State",
                    "UEFISecureBootEnabled",
                )
                .map(|v| v == 1),
            ),
        ),
        check(
            "uac",
            "User Account Control",
            "Kullanıcı Hesabı Denetimi",
            from_bool(
                dword(
                    "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Policies\\System",
                    "EnableLUA",
                )
                .map(|v| v == 1),
            ),
        ),
        check(
            "firewall",
            "Firewall on for all network types",
            "Tüm ağ türlerinde güvenlik duvarı açık",
            from_bool(firewall.map(|values| values.iter().all(|v| *v == 1))),
        ),
        check(
            "disk-encryption",
            "System drive encrypted (BitLocker)",
            "Sistem sürücüsü şifreli (BitLocker)",
            match bitlocker.as_deref() {
                Some("1") => Status::Pass,
                Some("2") => Status::Fail,
                _ => Status::Unknown,
            },
        ),
        check(
            "antivirus",
            "Defender real-time protection",
            "Defender gerçek zamanlı koruma",
            match defender.as_deref() {
                Some("True") => Status::Pass,
                Some("False") => Status::Fail,
                _ => Status::Unknown,
            },
        ),
        check(
            "accounts",
            "No enabled account may have an empty password",
            "Hiçbir etkin hesap boş parolaya izin vermiyor",
            match passwordless.as_deref().and_then(|n| n.parse::<u32>().ok()) {
                Some(0) => Status::Pass,
                Some(_) => Status::Fail,
                None => Status::Unknown,
            },
        ),
        check(
            "smb1",
            "SMBv1 server off",
            "SMBv1 sunucusu kapalı",
            match dword("SYSTEM\\CurrentControlSet\\Services\\LanmanServer\\Parameters", "SMB1") {
                Some(0) => Status::Pass,
                Some(_) => Status::Fail,
                None => Status::Unknown,
            },
        ),
        check(
            "tpm",
            "TPM 2.0 present and ready",
            "TPM 2.0 var ve hazır",
            from_bool(stdout_of("tpmtool", &["getdeviceinformation"]).and_then(|t| parse_tpmtool(&t))),
        ),
        check(
            "builtin-admin",
            "Built-in Administrator account disabled",
            "Yerleşik Administrator hesabı kapalı",
            match builtin_admin.as_deref() {
                Some("False") => Status::Pass,
                Some("True") => Status::Fail,
                _ => Status::Unknown,
            },
        ),
    ]
}

#[cfg(not(windows))]
fn windows_checks() -> Vec<Check> {
    Vec::new()
}

fn linux_checks() -> Vec<Check> {
    let active = |unit: &str| stdout_of("systemctl", &["is-active", unit]).is_some_and(|s| s.trim() == "active");
    let encryption = stdout_of("lsblk", &["-n", "-o", "TYPE"]).map(|s| s.lines().any(|l| l.trim() == "crypt"));
    let secure_boot = stdout_of("mokutil", &["--sb-state"]).and_then(|s| {
        if s.contains("SecureBoot enabled") {
            Some(true)
        } else if s.contains("SecureBoot disabled") {
            Some(false)
        } else {
            None
        }
    });
    let mac = std::fs::read_to_string("/sys/module/apparmor/parameters/enabled")
        .ok()
        .map(|s| s.trim() == "Y")
        .filter(|on| *on)
        .or_else(|| stdout_of("getenforce", &[]).map(|s| s.trim() == "Enforcing"));
    let ssh = std::fs::read_to_string("/etc/ssh/sshd_config").ok().map(|text| {
        text.lines().map(str::trim).any(|line| {
            let mut words = line.split_whitespace();
            words
                .next()
                .is_some_and(|w| w.eq_ignore_ascii_case("PasswordAuthentication"))
                && words.next().is_some_and(|w| w.eq_ignore_ascii_case("no"))
        })
    });
    let auto_updates = if std::path::Path::new("/etc/apt").exists() {
        std::fs::read_to_string("/etc/apt/apt.conf.d/20auto-upgrades")
            .ok()
            .map(|s| s.contains("Unattended-Upgrade \"1\""))
            .or(Some(false))
    } else if find_program("dnf").is_some() {
        Some(active("dnf-automatic.timer") || active("dnf-automatic-install.timer") || active("dnf5-automatic.timer"))
    } else {
        None
    };
    let mut checks = vec![
        check(
            "disk-encryption",
            "Disk encryption (LUKS)",
            "Disk şifreleme (LUKS)",
            from_bool(encryption),
        ),
        check(
            "secure-boot",
            "Secure Boot",
            "Güvenli Önyükleme",
            from_bool(secure_boot),
        ),
        check(
            "firewall",
            "Firewall active (ufw, firewalld or nftables)",
            "Güvenlik duvarı etkin (ufw, firewalld ya da nftables)",
            from_bool(Some(active("ufw") || active("firewalld") || active("nftables"))),
        ),
        check(
            "mac",
            "AppArmor or SELinux enforcing",
            "AppArmor ya da SELinux etkin",
            from_bool(mac),
        ),
        check(
            "auto-updates",
            "Automatic security updates",
            "Otomatik güvenlik güncellemeleri",
            from_bool(auto_updates),
        ),
    ];
    // SSH sunucusu yoksa bu kontrol anlamsızdır ve listelenmez.
    if ssh.is_some() {
        checks.push(check(
            "ssh-password",
            "SSH password login disabled",
            "SSH parola girişi kapalı",
            from_bool(ssh),
        ));
    }
    checks.push(check(
        "tpm",
        "TPM present",
        "TPM var",
        if std::path::Path::new("/sys/class/tpm/tpm0").exists() {
            Status::Pass
        } else {
            Status::Unknown
        },
    ));
    checks.push(check(
        "uid0",
        "Only root has UID 0",
        "UID 0 yalnızca root'ta",
        std::fs::read_to_string("/etc/passwd")
            .ok()
            .map_or(Status::Unknown, |text| {
                if extra_root_accounts(&text).is_empty() {
                    Status::Pass
                } else {
                    Status::Fail
                }
            }),
    ));
    checks.extend(elevation_check());
    checks
}

fn macos_checks() -> Vec<Check> {
    let contains = |program: &str, args: &[&str], needle: &str| stdout_of(program, args).map(|s| s.contains(needle));
    let mut checks = vec![
        check(
            "disk-encryption",
            "FileVault",
            "FileVault",
            from_bool(contains("fdesetup", &["status"], "FileVault is On")),
        ),
        check(
            "sip",
            "System Integrity Protection",
            "Sistem Bütünlüğü Koruması",
            from_bool(contains("csrutil", &["status"], "enabled")),
        ),
        check(
            "gatekeeper",
            "Gatekeeper",
            "Gatekeeper",
            from_bool(contains("spctl", &["--status"], "assessments enabled")),
        ),
        check(
            "firewall",
            "Firewall",
            "Güvenlik duvarı",
            from_bool(contains(
                "/usr/libexec/ApplicationFirewall/socketfilterfw",
                &["--getglobalstate"],
                "enabled",
            )),
        ),
    ];
    checks.extend(elevation_check());
    checks
}

/// `tpmtool getdeviceinformation` çıktısı: TPM var mı ve sürümü 2.0 mı. Araç İngilizce yazar.
pub fn parse_tpmtool(text: &str) -> Option<bool> {
    let field = |name: &str| {
        text.lines()
            .find_map(|l| l.trim().trim_start_matches('-').strip_prefix(name))
            .map(|v| v.trim_start_matches(':').trim().to_owned())
    };
    match field("TPM Present")?.as_str() {
        "False" => Some(false),
        "True" => Some(field("TPM Version").is_some_and(|v| v.starts_with("2."))),
        _ => None,
    }
}

/// `/etc/passwd` içinde root dışında UID 0 olan hesaplar (gizli bir root hesabı olabilir).
pub fn extra_root_accounts(passwd: &str) -> Vec<String> {
    passwd
        .lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| {
            let mut fields = l.split(':');
            let name = fields.next()?;
            let uid = fields.nth(1)?;
            (uid == "0" && name != "root").then(|| name.to_owned())
        })
        .collect()
}

/// sudo ya da doas kuralında parolasız yetki (`NOPASSWD`, `!authenticate`, `nopass`) var mı.
/// Yorum satırları sayılmaz.
pub fn passwordless_rule(text: &str) -> bool {
    text.lines()
        .map(|line| line.split('#').next().unwrap_or("").trim())
        .filter(|line| !line.is_empty())
        .any(|line| {
            line.contains("NOPASSWD")
                || line.contains("!authenticate")
                || line.split_whitespace().any(|w| w == "nopass")
        })
}

/// sudo/doas yapılandırma denetimi. Dosyalar genelde yalnızca root tarafından okunur; okunamazsa
/// sonuç "bilinmiyor" olur. Hiçbiri yoksa denetim listelenmez.
fn elevation_check() -> Option<Check> {
    use std::path::Path;
    let mut files = vec![
        Path::new("/etc/sudoers").to_path_buf(),
        Path::new("/etc/doas.conf").to_path_buf(),
    ];
    if let Ok(entries) = std::fs::read_dir("/etc/sudoers.d") {
        // sudo, adında nokta olan ya da `~` ile biten dosyaları okumaz.
        files.extend(entries.flatten().map(|e| e.path()).filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| !n.contains('.') && !n.ends_with('~'))
        }));
    }
    let existing: Vec<_> = files.into_iter().filter(|p| p.is_file()).collect();
    if existing.is_empty() {
        return None;
    }
    let mut status = Status::Pass;
    for path in existing {
        match std::fs::read_to_string(&path) {
            Ok(text) if passwordless_rule(&text) => return Some(elevation(Status::Fail)),
            Ok(_) => {}
            Err(_) => status = Status::Unknown,
        }
    }
    Some(elevation(status))
}

fn elevation(status: Status) -> Check {
    check(
        "sudo-password",
        "sudo/doas always asks for a password",
        "sudo/doas her zaman parola soruyor",
        status,
    )
}

/// Geçen / (geçen + kalan) yüzde olarak; hesaplanamıyorsa `None`.
pub fn score(passed: usize, failed: usize) -> Option<u32> {
    let total = passed + failed;
    (total > 0).then(|| ((passed * 100 + total / 2) / total) as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tpm_and_uid0() {
        let text = "-TPM Present: True\n-TPM Version: 2.0\n-Is Initialized: True\n";
        assert_eq!(parse_tpmtool(text), Some(true));
        assert_eq!(parse_tpmtool("-TPM Present: False\n"), Some(false));
        assert_eq!(parse_tpmtool("garbage"), None);
        let passwd =
            "root:x:0:0:root:/root:/bin/bash\ntoor:x:0:0::/root:/bin/sh\nalice:x:1000:1000::/home/alice:/bin/bash\n";
        assert_eq!(extra_root_accounts(passwd), ["toor"]);
    }

    #[test]
    fn finds_passwordless_rules() {
        assert!(passwordless_rule("%wheel ALL=(ALL) NOPASSWD: ALL\n"));
        assert!(passwordless_rule("permit nopass alice as root\n"));
        assert!(passwordless_rule("Defaults !authenticate\n"));
        assert!(!passwordless_rule(
            "# %wheel ALL=(ALL) NOPASSWD: ALL\n%sudo ALL=(ALL:ALL) ALL\n"
        ));
        assert!(!passwordless_rule("permit persist :wheel\n"));
    }

    #[test]
    fn scores_round_and_ignore_unknowns() {
        assert_eq!(score(0, 0), None);
        assert_eq!(score(3, 1), Some(75));
        assert_eq!(score(2, 1), Some(67));
        assert_eq!(from_bool(None), Status::Unknown);
    }
}
