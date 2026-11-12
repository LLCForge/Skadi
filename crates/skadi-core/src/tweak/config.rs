//! INI biçimli yapılandırma dosyalarında tek bir `anahtar=değer` satırını okuma ve düzenleme.
//!
//! Düzenleme mantığı saftır ve test edilir. Dosyaya yazma, `/etc` altındaki dosyalar root'a ait
//! olduğu için `skadi internal config-set` komutuyla yetkili olarak yapılır.

use std::path::Path;

use crate::error::{Error, Result};

/// Bir satıra yapılacak değişiklik.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigEdit {
    /// `anahtar=değer` olarak ayarla (varsa değiştir, yoksa bölümün sonuna ekle).
    Value(String),
    /// Kaydedilmiş özgün satırı aynen geri koy.
    Line(String),
    /// Anahtarın satırını kaldır.
    Remove,
}

/// Yalnızca `/etc` altındaki, `..` içermeyen, basit karakterlerden oluşan yollar.
pub fn is_allowed_path(path: &str) -> bool {
    path.starts_with("/etc/")
        && !path.split('/').any(|part| part == "..")
        && path
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | '_' | '-'))
}

fn section_header(line: &str) -> Option<&str> {
    let line = line.trim();
    line.strip_prefix('[')?.strip_suffix(']').map(str::trim)
}

pub(crate) fn key_of_line(line: &str) -> Option<&str> {
    key_of(line)
}

fn key_of(line: &str) -> Option<&str> {
    let line = line.trim();
    if line.starts_with('#') || line.starts_with(';') {
        return None;
    }
    line.split_once('=').map(|(key, _)| key.trim())
}

/// Bölümdeki anahtarın satır numarası; bölüm yoksa `Err`.
fn locate(lines: &[&str], section: Option<&str>, key: &str) -> std::result::Result<(Option<usize>, usize), ()> {
    // (anahtarın satırı, bölümün bittiği satır)
    let mut inside = section.is_none();
    let mut found_section = section.is_none();
    let mut found = None;
    let mut end = lines.len();
    for (i, line) in lines.iter().enumerate() {
        if let Some(name) = section_header(line) {
            if inside && section.is_some() {
                end = i;
                break;
            }
            inside = section.is_some_and(|s| s == name);
            found_section |= inside;
            continue;
        }
        if inside && key_of(line) == Some(key) {
            found = Some(i);
        }
    }
    if found_section { Ok((found, end)) } else { Err(()) }
}

/// Anahtarın mevcut satırı. Bölüm yoksa `None`; bölüm var ama anahtar yoksa `Some(None)`.
pub fn read_line(text: &str, section: Option<&str>, key: &str) -> Option<Option<String>> {
    let lines: Vec<&str> = text.lines().collect();
    let (found, _) = locate(&lines, section, key).ok()?;
    Some(found.map(|i| lines[i].to_owned()))
}

/// Satırdaki değer (`anahtar = değer` → `değer`).
pub fn value_of(line: &str) -> Option<&str> {
    line.split_once('=').map(|(_, value)| value.trim())
}

/// Değişikliği uygulanmış yeni metin; bölüm yoksa `None`.
pub fn apply_edit(text: &str, section: Option<&str>, key: &str, edit: &ConfigEdit) -> Option<String> {
    let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
    let borrowed: Vec<&str> = lines.iter().map(String::as_str).collect();
    let (found, end) = locate(&borrowed, section, key).ok()?;
    let replacement = match edit {
        ConfigEdit::Value(value) => Some(format!("{key}={value}")),
        ConfigEdit::Line(line) => Some(line.clone()),
        ConfigEdit::Remove => None,
    };
    match (found, replacement) {
        (Some(i), Some(line)) => lines[i] = line,
        (Some(i), None) => {
            lines.remove(i);
        }
        (None, Some(line)) => {
            // Bölümün sonundaki boş satırların önüne eklenir.
            let mut at = end;
            while at > 0 && lines[at - 1].trim().is_empty() {
                at -= 1;
            }
            lines.insert(at, line);
        }
        (None, None) => {}
    }
    let mut out = lines.join("\n");
    if text.ends_with('\n') {
        out.push('\n');
    }
    Some(out)
}

/// Dosyayı düzenler: aynı dizinde geçici dosyaya yazar, izinleri korur ve yerine taşır.
pub fn edit_file(path: &str, section: Option<&str>, key: &str, edit: &ConfigEdit) -> Result<()> {
    let fail = |reason: String| Error::State(format!("{path}: {reason}"));
    if !is_allowed_path(path) {
        return Err(fail("path is not allowed".into()));
    }
    let text = std::fs::read_to_string(path).map_err(|e| fail(e.to_string()))?;
    let updated = apply_edit(&text, section, key, edit).ok_or_else(|| fail("section not found".into()))?;
    let target = Path::new(path);
    let temp = target.with_extension("skadi-tmp");
    let permissions = std::fs::metadata(target)
        .map_err(|e| fail(e.to_string()))?
        .permissions();
    std::fs::write(&temp, updated).map_err(|e| fail(e.to_string()))?;
    std::fs::set_permissions(&temp, permissions).map_err(|e| fail(e.to_string()))?;
    std::fs::rename(&temp, target).map_err(|e| fail(e.to_string()))
}

/// Skadi'nin kendi dosyalarını yazabildiği klasörler (doğrudan içleri; alt klasör yok).
const MANAGED_DIRS: [&str; 9] = [
    "/etc/opt/chrome/policies/managed/",
    "/etc/chromium/policies/managed/",
    "/etc/brave/policies/managed/",
    "/etc/firefox/policies/",
    "/etc/apt/apt.conf.d/",
    "/etc/dnf/automatic.conf.d/",
    "/etc/sysctl.d/",
    "/etc/NetworkManager/conf.d/",
    "/etc/ssh/sshd_config.d/",
];

/// Firefox yalnızca bu adı okur; Skadi bu dosyaya yalnızca yoksa ya da kendi içeriğiyse yazar.
const FIXED_NAMES: [&str; 1] = ["/etc/firefox/policies/policies.json"];

pub fn is_managed_path(path: &str) -> bool {
    is_allowed_path(path)
        && (FIXED_NAMES.contains(&path)
            || MANAGED_DIRS.iter().any(|dir| {
                path.strip_prefix(dir)
                    .is_some_and(|name| !name.contains('/') && name.contains("skadi") && !name.starts_with('.'))
            }))
}

/// Dosya adı Skadi'ye ait mi (değilse yalnızca Skadi'nin yazdığı içerik değiştirilebilir).
pub fn is_skadi_owned(path: &str) -> bool {
    path.rsplit('/').next().is_some_and(|name| name.contains("skadi"))
}

pub fn is_managed_content(content: &str) -> bool {
    !content.is_empty() && content.len() <= 4096 && content.chars().all(|c| c == '\n' || c == '\t' || !c.is_control())
}

/// sshd yalnızca ana dosyada `Include` satırı varsa `sshd_config.d` klasörünü okur.
pub fn drop_in_is_read(path: &str) -> bool {
    if !path.starts_with("/etc/ssh/sshd_config.d/") {
        return true;
    }
    std::fs::read_to_string("/etc/ssh/sshd_config").is_ok_and(|text| {
        text.lines()
            .map(str::trim)
            .any(|line| line.starts_with("Include") && line.contains("/etc/ssh/sshd_config.d/"))
    })
}

/// Yetkili kullanıcının (sudo/doas/pkexec ile çalıştıran) `authorized_keys` dosyası dolu mu.
fn invoking_user_has_ssh_key() -> bool {
    let privileges = crate::exec::Privileges::detect(crate::platform::Os::Linux);
    let Some(user) = privileges.invoking_user else {
        return false;
    };
    let Ok(passwd) = std::fs::read_to_string("/etc/passwd") else {
        return false;
    };
    passwd
        .lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split(':').collect();
            (fields.len() >= 7 && fields[0] == user).then(|| fields[5].to_owned())
        })
        .next()
        .and_then(|home| std::fs::read_to_string(Path::new(&home).join(".ssh/authorized_keys")).ok())
        .is_some_and(|keys| {
            keys.lines()
                .map(str::trim)
                .any(|l| !l.is_empty() && !l.starts_with('#'))
        })
}

fn run_quiet(program: &str, args: &[&str]) -> bool {
    crate::exec::find_program(program).is_some_and(|path| {
        std::process::Command::new(path)
            .args(args)
            .stdin(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    })
}

fn put(path: &Path, content: Option<&str>) -> std::io::Result<()> {
    match content {
        Some(text) => {
            let temp = path.with_extension("skadi-tmp");
            std::fs::write(&temp, text)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&temp, std::fs::Permissions::from_mode(0o644))?;
            }
            std::fs::rename(&temp, path)
        }
        None => match std::fs::remove_file(path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        },
    }
}

/// Yönetilen dosyayı yazar (`Some`) ya da siler (`None`), ardından değişikliği etkinleştirir.
/// Doğrulama başarısız olursa önceki durum geri yüklenir.
pub fn write_managed(path: &str, content: Option<&str>, reload: super::Reload, guard: super::Guard) -> Result<()> {
    use super::{Guard, Reload};
    let fail = |reason: String| Error::State(format!("{path}: {reason}"));
    if !is_managed_path(path) || content.is_some_and(|c| !is_managed_content(c)) {
        return Err(fail("not allowed".into()));
    }
    if content.is_some() && guard == Guard::AuthorizedKeys && !invoking_user_has_ssh_key() {
        return Err(fail(
            "no SSH key found in your ~/.ssh/authorized_keys; refusing to turn off password login".into(),
        ));
    }
    if content.is_some()
        && guard == Guard::SudoUser
        && crate::exec::Privileges::detect(crate::platform::Os::Linux)
            .invoking_user
            .is_none()
    {
        return Err(fail(
            "run Skadi with sudo from your own account; refusing to turn off root login for a root-only session".into(),
        ));
    }
    let target = Path::new(path);
    let previous = std::fs::read_to_string(target).ok();
    // Tarayıcı politika klasörleri tarayıcı kurulunca bile var olmayabilir.
    if content.is_some()
        && let Some(dir) = target.parent()
        && !dir.exists()
    {
        std::fs::create_dir_all(dir).map_err(|e| fail(e.to_string()))?;
    }
    put(target, content).map_err(|e| fail(e.to_string()))?;
    let restore = |reason: &str| {
        let _ = put(target, previous.as_deref());
        fail(reason.to_owned())
    };
    match reload {
        Reload::None => {}
        Reload::Sysctl => {
            if !run_quiet("sysctl", &["--system"]) {
                let _ = run_quiet("sysctl", &["--system"]);
                return Err(restore("sysctl rejected the settings; the previous state was restored"));
            }
        }
        Reload::NetworkManager => {
            // NetworkManager.conf(5): yapılandırma `nmcli general reload conf` ile yeniden okunur.
            if !run_quiet("nmcli", &["general", "reload", "conf"]) {
                let _ = run_quiet("systemctl", &["reload", "NetworkManager.service"]);
            }
        }
        Reload::Sshd => {
            if !run_quiet("sshd", &["-t"]) {
                return Err(restore(
                    "sshd rejected the configuration; the previous state was restored",
                ));
            }
            for unit in ["ssh.service", "sshd.service"] {
                let _ = run_quiet("systemctl", &["try-reload-or-restart", unit]);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const DNF: &str = "[main]\ngpgcheck=True\ninstallonly_limit=3\n\n[other]\ncountme=True\n";

    #[test]
    fn sets_replaces_and_removes_within_a_section() {
        let set = apply_edit(DNF, Some("main"), "countme", &ConfigEdit::Value("False".into())).unwrap();
        assert_eq!(
            set,
            "[main]\ngpgcheck=True\ninstallonly_limit=3\ncountme=False\n\n[other]\ncountme=True\n"
        );
        assert_eq!(
            read_line(&set, Some("main"), "countme"),
            Some(Some("countme=False".into()))
        );
        assert_eq!(read_line(DNF, Some("main"), "countme"), Some(None));
        assert_eq!(read_line(DNF, Some("missing"), "countme"), None);

        let removed = apply_edit(&set, Some("main"), "countme", &ConfigEdit::Remove).unwrap();
        assert_eq!(removed, DNF, "kaldırma özgün metne döner");

        let original = "[main]\n countme = True # yorum\n";
        let changed = apply_edit(original, Some("main"), "countme", &ConfigEdit::Value("False".into())).unwrap();
        let restored = apply_edit(
            &changed,
            Some("main"),
            "countme",
            &ConfigEdit::Line(" countme = True # yorum".into()),
        )
        .unwrap();
        assert_eq!(restored, original, "özgün satır birebir geri gelir");
    }

    #[test]
    fn only_simple_etc_paths_are_allowed() {
        assert!(is_allowed_path("/etc/dnf/dnf.conf"));
        assert!(!is_allowed_path("/etc/../root/.ssh/authorized_keys"));
        assert!(is_managed_path("/etc/sysctl.d/90-skadi-privacy.conf"));
        assert!(!is_managed_path("/etc/sysctl.d/90-other.conf"));
        assert!(!is_managed_path("/etc/sysctl.d/sub/90-skadi.conf"));
        assert!(!is_managed_path("/etc/passwd-skadi"));
        assert!(is_managed_path("/etc/firefox/policies/policies.json"));
        assert!(!is_managed_path("/etc/firefox/policies/other.json"));
        assert!(is_managed_path("/etc/opt/chrome/policies/managed/skadi-privacy.json"));
        assert!(!is_skadi_owned("/etc/firefox/policies/policies.json"));
        assert!(is_managed_content("[main]\nkey=value\n"));
        assert!(!is_managed_content("a\u{0}b"));
        assert!(!is_allowed_path("/home/user/x.conf"));
        assert!(!is_allowed_path("/etc/a b.conf"));
    }
}
