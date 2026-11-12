//! Yedekleme yardımcıları: dotfiles (gizli anahtarlar hariç) ve tarayıcı yer imleri (çerez ve
//! parolalar hariç). Kopyalama sembolik bağlantıları izlemez ve dosya boyutunu sınırlar.

use std::path::{Path, PathBuf};

use crate::platform::Os;

/// Ev klasörüne göre yedeklenen yapılandırma dosyaları ve klasörleri.
pub const DOTFILES: [&str; 22] = [
    ".bashrc",
    ".bash_profile",
    ".profile",
    ".zshrc",
    ".zprofile",
    ".inputrc",
    ".gitconfig",
    ".gitignore_global",
    ".vimrc",
    ".tmux.conf",
    ".editorconfig",
    ".config/fish/config.fish",
    ".config/starship.toml",
    ".config/nvim",
    ".config/helix",
    ".config/alacritty",
    ".config/kitty",
    ".config/wezterm",
    ".config/git",
    "Documents/PowerShell/Microsoft.PowerShell_profile.ps1",
    "Documents/WindowsPowerShell/Microsoft.PowerShell_profile.ps1",
    ".wezterm.lua",
];

const MAX_FILE_BYTES: u64 = 1024 * 1024;
const MAX_DEPTH: usize = 8;

/// Hiçbir zaman yedeklenmeyen klasör adları ve uzantılar.
const SECRET_DIRS: [&str; 6] = [".ssh", ".gnupg", ".aws", ".kube", ".docker", ".password-store"];
const SECRET_EXTENSIONS: [&str; 6] = ["key", "pem", "p12", "pfx", "kdbx", "gpg"];

/// İçeriğinde gizli bilgi kalıbı olan dosyalar atlanır.
pub fn looks_secret(text: &str) -> bool {
    const PATTERNS: [&str; 12] = [
        "PRIVATE KEY",
        "ghp_",
        "github_pat_",
        "gho_",
        "glpat-",
        "xoxb-",
        "xoxp-",
        "AKIA",
        "_authToken",
        "aws_secret_access_key",
        "-----BEGIN PGP",
        "sk-ant-",
    ];
    let lower = text.to_ascii_lowercase();
    PATTERNS.iter().any(|p| text.contains(p))
        || [
            "password=",
            "password =",
            "token=",
            "token =",
            "secret=",
            "api_key",
            "apikey",
        ]
        .iter()
        .any(|p| lower.contains(p))
}

fn is_secret_path(path: &Path) -> bool {
    path.components()
        .any(|c| SECRET_DIRS.iter().any(|d| c.as_os_str() == *d))
        || path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| SECRET_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct CopyReport {
    pub copied: Vec<PathBuf>,
    /// Gizli bilgi içerdiği ya da çok büyük olduğu için atlananlar.
    pub skipped: Vec<PathBuf>,
}

fn copy_one(from: &Path, to: &Path, check_secrets: bool, report: &mut CopyReport) -> std::io::Result<()> {
    let meta = std::fs::symlink_metadata(from)?;
    if meta.file_type().is_symlink() {
        return Ok(());
    }
    if meta.is_dir() {
        copy_dir(from, to, 0, check_secrets, report)
    } else if meta.is_file() {
        if meta.len() > MAX_FILE_BYTES || (check_secrets && is_secret_path(from)) {
            report.skipped.push(from.to_path_buf());
            return Ok(());
        }
        if check_secrets {
            let bytes = std::fs::read(from)?;
            if looks_secret(&String::from_utf8_lossy(&bytes)) {
                report.skipped.push(from.to_path_buf());
                return Ok(());
            }
        }
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(from, to)?;
        report.copied.push(to.to_path_buf());
        Ok(())
    } else {
        Ok(())
    }
}

fn copy_dir(from: &Path, to: &Path, depth: usize, check_secrets: bool, report: &mut CopyReport) -> std::io::Result<()> {
    if depth > MAX_DEPTH || (check_secrets && is_secret_path(from)) {
        return Ok(());
    }
    for entry in std::fs::read_dir(from)?.flatten() {
        let path = entry.path();
        let target = to.join(entry.file_name());
        let meta = std::fs::symlink_metadata(&path)?;
        if meta.is_dir() && !meta.file_type().is_symlink() {
            copy_dir(&path, &target, depth + 1, check_secrets, report)?;
        } else {
            copy_one(&path, &target, check_secrets, report)?;
        }
    }
    Ok(())
}

/// Dotfiles'ı `dest/dotfiles` altına kopyalar.
pub fn backup_dotfiles(home: &Path, dest: &Path) -> std::io::Result<CopyReport> {
    let mut report = CopyReport::default();
    for relative in DOTFILES {
        let from = home.join(relative);
        if from.exists() {
            copy_one(&from, &dest.join("dotfiles").join(relative), true, &mut report)?;
        }
    }
    Ok(report)
}

/// Yedeği geri yükler. `force` olmadan var olan dosyaların üzerine yazılmaz; yazılırsa eski hali
/// `.skadi-bak` uzantısıyla saklanır.
pub fn restore_dotfiles(source: &Path, home: &Path, force: bool) -> std::io::Result<CopyReport> {
    let root = source.join("dotfiles");
    let mut report = CopyReport::default();
    let mut files = Vec::new();
    collect_files(&root, 0, &mut files)?;
    for file in files {
        let Ok(relative) = file.strip_prefix(&root) else {
            continue;
        };
        let target = home.join(relative);
        if target.exists() {
            if !force {
                report.skipped.push(target);
                continue;
            }
            let mut backup = target.clone().into_os_string();
            backup.push(".skadi-bak");
            std::fs::copy(&target, PathBuf::from(backup))?;
        }
        copy_one(&file, &target, false, &mut report)?;
    }
    Ok(report)
}

fn collect_files(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    if depth > MAX_DEPTH {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)?.flatten() {
        let path = entry.path();
        let meta = std::fs::symlink_metadata(&path)?;
        if meta.is_dir() && !meta.file_type().is_symlink() {
            collect_files(&path, depth + 1, out)?;
        } else if meta.is_file() {
            out.push(path);
        }
    }
    Ok(())
}

/// Chromium tabanlı tarayıcıların kullanıcı veri klasörleri (ev klasörüne göre).
fn chromium_roots(os: Os) -> Vec<(&'static str, &'static str)> {
    match os {
        Os::Windows => vec![
            ("chrome", "AppData/Local/Google/Chrome/User Data"),
            ("edge", "AppData/Local/Microsoft/Edge/User Data"),
            ("brave", "AppData/Local/BraveSoftware/Brave-Browser/User Data"),
            ("vivaldi", "AppData/Local/Vivaldi/User Data"),
            ("chromium", "AppData/Local/Chromium/User Data"),
        ],
        Os::Linux => vec![
            ("chrome", ".config/google-chrome"),
            ("chromium", ".config/chromium"),
            ("brave", ".config/BraveSoftware/Brave-Browser"),
            ("vivaldi", ".config/vivaldi"),
            ("edge", ".config/microsoft-edge"),
        ],
        Os::Macos => vec![
            ("chrome", "Library/Application Support/Google/Chrome"),
            ("brave", "Library/Application Support/BraveSoftware/Brave-Browser"),
            ("edge", "Library/Application Support/Microsoft Edge"),
            ("vivaldi", "Library/Application Support/Vivaldi"),
            ("chromium", "Library/Application Support/Chromium"),
        ],
    }
}

fn firefox_root(os: Os) -> &'static str {
    match os {
        Os::Windows => "AppData/Roaming/Mozilla/Firefox/Profiles",
        Os::Linux => ".mozilla/firefox",
        Os::Macos => "Library/Application Support/Firefox/Profiles",
    }
}

/// Yalnızca yer imlerini yedekler: Chromium ailesinde `Bookmarks` dosyası, Firefox'ta kendi
/// `bookmarkbackups` klasörü. Çerezler, geçmiş ve kayıtlı parolalar kopyalanmaz.
pub fn backup_bookmarks(os: Os, home: &Path, dest: &Path) -> std::io::Result<CopyReport> {
    let mut report = CopyReport::default();
    for (name, root) in chromium_roots(os) {
        let Ok(profiles) = std::fs::read_dir(home.join(root)) else {
            continue;
        };
        for profile in profiles.flatten() {
            let bookmarks = profile.path().join("Bookmarks");
            if bookmarks.is_file() {
                let target = dest
                    .join("bookmarks")
                    .join(name)
                    .join(profile.file_name())
                    .join("Bookmarks");
                copy_one(&bookmarks, &target, false, &mut report)?;
            }
        }
    }
    if let Ok(profiles) = std::fs::read_dir(home.join(firefox_root(os))) {
        for profile in profiles.flatten() {
            let backups = profile.path().join("bookmarkbackups");
            if backups.is_dir() {
                let target = dest.join("bookmarks").join("firefox").join(profile.file_name());
                copy_one(&backups, &target, false, &mut report)?;
            }
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_secrets_and_round_trips() {
        let root = std::env::temp_dir().join(format!("skadi-backup-test-{}", std::process::id()));
        let home = root.join("home");
        let dest = root.join("dest");
        std::fs::create_dir_all(home.join(".config/nvim")).unwrap();
        std::fs::create_dir_all(home.join(".ssh")).unwrap();
        std::fs::write(home.join(".bashrc"), "alias ll='ls -l'\n").unwrap();
        std::fs::write(home.join(".gitconfig"), "[user]\n\tname = A\n").unwrap();
        std::fs::write(home.join(".config/nvim/init.lua"), "vim.o.number = true\n").unwrap();
        std::fs::write(home.join(".config/nvim/secret.lua"), "local token = 'ghp_abc'\n").unwrap();
        std::fs::write(home.join(".ssh/id_ed25519"), "-----BEGIN OPENSSH PRIVATE KEY-----\n").unwrap();

        let report = backup_dotfiles(&home, &dest).unwrap();
        assert_eq!(report.copied.len(), 3);
        assert_eq!(report.skipped.len(), 1);
        assert!(!dest.join("dotfiles/.ssh").exists());
        assert!(!dest.join("dotfiles/.config/nvim/secret.lua").exists());

        let fresh = root.join("fresh");
        std::fs::create_dir_all(&fresh).unwrap();
        std::fs::write(fresh.join(".bashrc"), "old\n").unwrap();
        let restored = restore_dotfiles(&dest, &fresh, false).unwrap();
        assert_eq!(restored.skipped, [fresh.join(".bashrc")]);
        assert_eq!(std::fs::read_to_string(fresh.join(".bashrc")).unwrap(), "old\n");
        restore_dotfiles(&dest, &fresh, true).unwrap();
        assert!(
            std::fs::read_to_string(fresh.join(".bashrc"))
                .unwrap()
                .contains("alias")
        );
        assert!(fresh.join(".bashrc.skadi-bak").exists());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn detects_secret_patterns() {
        assert!(looks_secret("//registry.npmjs.org/:_authToken=abc"));
        assert!(looks_secret("api_key = 123"));
        assert!(!looks_secret("set -o vi\nexport EDITOR=nvim\n"));
    }
}
