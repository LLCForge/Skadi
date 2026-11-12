//! Bakım araçları: geçici dosya taraması, klasör boyutları, sistem raporu, disk sağlığı ve güç
//! planlarının ayrıştırılması. Bu modüldeki okuma işlemleri yetki istemez.

use std::{
    fs::Metadata,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use serde::Serialize;

use crate::{audit::stdout_of, platform::Os};

/// Sembolik bağlantılar ve Windows bağlantı noktaları (junction) izlenmez; derinlik sınırlıdır.
const MAX_DEPTH: usize = 64;

fn walk(dir: &Path, depth: usize, visit: &mut impl FnMut(&Path, &Metadata)) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_dir() {
            if depth < MAX_DEPTH {
                walk(&path, depth + 1, visit);
            }
        } else if meta.is_file() {
            visit(&path, &meta);
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Sweep {
    pub files: u64,
    pub bytes: u64,
    /// Silinemeyen (kullanımda ya da izinsiz) dosyalar.
    pub skipped: u64,
}

/// `min_age`'den eski sıradan dosyaları sayar; `delete` ise siler. Klasörler yerinde bırakılır.
pub fn sweep(root: &Path, min_age: Duration, delete: bool) -> Sweep {
    let cutoff = SystemTime::now().checked_sub(min_age).unwrap_or(SystemTime::UNIX_EPOCH);
    let mut result = Sweep::default();
    walk(root, 0, &mut |path, meta| {
        if meta.modified().is_ok_and(|m| m > cutoff) {
            return;
        }
        if delete && std::fs::remove_file(path).is_err() {
            result.skipped += 1;
            return;
        }
        result.files += 1;
        result.bytes += meta.len();
    });
    result
}

/// Kullanıcının geçici dosya klasörleri. Linux'ta küçük resim önbelleği de eklenir (yeniden oluşur).
pub fn temp_roots(os: Os) -> Vec<PathBuf> {
    let mut roots = vec![std::env::temp_dir()];
    if os == Os::Linux {
        let cache = std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")));
        roots.extend(cache.map(|c| c.join("thumbnails")));
    }
    roots.retain(|root| root.is_absolute() && root.is_dir());
    roots
}

/// Bir klasörün doğrudan alt öğelerinin toplam boyutları, büyükten küçüğe.
pub fn dir_sizes(root: &Path) -> std::io::Result<Vec<(PathBuf, u64)>> {
    let mut sizes = Vec::new();
    for entry in std::fs::read_dir(root)?.flatten() {
        let path = entry.path();
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        let size = if meta.is_dir() && !meta.file_type().is_symlink() {
            let mut total = 0;
            walk(&path, 1, &mut |_, m| total += m.len());
            total
        } else if meta.is_file() {
            meta.len()
        } else {
            continue;
        };
        sizes.push((path, size));
    }
    sizes.sort_by(|a, b| b.1.cmp(&a.1));
    Ok(sizes)
}

#[derive(Debug, Clone, Serialize)]
pub struct DiskInfo {
    pub mount: String,
    pub file_system: String,
    pub kind: String,
    pub removable: bool,
    pub total: u64,
    pub available: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiskHealth {
    pub disk: String,
    pub media: Option<String>,
    /// Aracın bildirdiği ham durum.
    pub status: String,
    /// `None`: durum okunamadı (örneğin yetki gerekiyor).
    pub healthy: Option<bool>,
}

/// Donanım raporu. Bilgisayar adı ve kullanıcı adı gizlilik için bilerek dışarıda bırakılır;
/// rapor güvenle paylaşılabilir.
#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub os: Option<String>,
    pub kernel: Option<String>,
    pub arch: String,
    pub cpu: String,
    pub physical_cores: Option<usize>,
    pub logical_cores: usize,
    pub memory_total: u64,
    pub memory_used: u64,
    pub swap_total: u64,
    pub uptime_secs: u64,
    pub disks: Vec<DiskInfo>,
    pub health: Vec<DiskHealth>,
}

pub fn report(os: Os) -> Report {
    use sysinfo::{CpuRefreshKind, Disks, MemoryRefreshKind, RefreshKind, System};
    let sys = System::new_with_specifics(
        RefreshKind::nothing()
            .with_memory(MemoryRefreshKind::everything())
            .with_cpu(CpuRefreshKind::nothing()),
    );
    let disks = Disks::new_with_refreshed_list()
        .iter()
        .map(|d| DiskInfo {
            mount: d.mount_point().display().to_string(),
            file_system: d.file_system().to_string_lossy().into_owned(),
            kind: d.kind().to_string(),
            removable: d.is_removable(),
            total: d.total_space(),
            available: d.available_space(),
        })
        .collect();
    Report {
        os: System::long_os_version(),
        kernel: System::kernel_version(),
        arch: System::cpu_arch(),
        cpu: sys
            .cpus()
            .first()
            .map_or_else(String::new, |c| c.brand().trim().to_owned()),
        physical_cores: System::physical_core_count(),
        logical_cores: sys.cpus().len(),
        memory_total: sys.total_memory(),
        memory_used: sys.used_memory(),
        swap_total: sys.total_swap(),
        uptime_secs: System::uptime(),
        disks,
        health: disk_health(os),
    }
}

/// Disk sağlığı: Windows'ta `Get-PhysicalDisk`, Linux'ta `smartctl` (root gerekir), macOS'ta `diskutil`.
pub fn disk_health(os: Os) -> Vec<DiskHealth> {
    match os {
        Os::Windows => stdout_of(
            "powershell",
            &[
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "[Console]::OutputEncoding = [Text.Encoding]::UTF8; Get-PhysicalDisk | ForEach-Object { \
                 $_.FriendlyName + '|' + $_.MediaType + '|' + $_.HealthStatus }",
            ],
        )
        .map(|text| parse_windows_health(&text))
        .unwrap_or_default(),
        Os::Linux => {
            let Some(scan) = stdout_of("smartctl", &["--scan"]) else {
                return Vec::new();
            };
            scan.lines()
                .filter_map(|line| line.split_whitespace().next())
                .filter(|dev| {
                    dev.starts_with("/dev/") && dev[5..].chars().all(|c| c.is_ascii_alphanumeric() || c == '/')
                })
                .map(|dev| {
                    let text = stdout_of("smartctl", &["-H", dev]).unwrap_or_default();
                    smartctl_health(dev, &text)
                })
                .collect()
        }
        Os::Macos => stdout_of("diskutil", &["info", "disk0"])
            .and_then(|text| {
                let status = text
                    .lines()
                    .find_map(|l| l.trim().strip_prefix("SMART Status:"))?
                    .trim()
                    .to_owned();
                Some(vec![DiskHealth {
                    disk: "disk0".to_owned(),
                    media: None,
                    healthy: match status.as_str() {
                        "Verified" => Some(true),
                        "Failing" => Some(false),
                        _ => None,
                    },
                    status,
                }])
            })
            .unwrap_or_default(),
    }
}

fn parse_windows_health(text: &str) -> Vec<DiskHealth> {
    text.lines()
        .filter_map(|line| {
            let mut parts = line.trim().split('|');
            let (name, media, status) = (parts.next()?, parts.next()?, parts.next()?);
            Some(DiskHealth {
                disk: name.to_owned(),
                media: Some(media.to_owned()).filter(|m| !m.is_empty() && m != "Unspecified"),
                healthy: match status {
                    "Healthy" => Some(true),
                    "Warning" | "Unhealthy" => Some(false),
                    _ => None,
                },
                status: status.to_owned(),
            })
        })
        .collect()
}

fn smartctl_health(dev: &str, text: &str) -> DiskHealth {
    let line = text
        .lines()
        .find(|l| l.contains("self-assessment test result:") || l.contains("SMART Health Status:"));
    let status = line.and_then(|l| l.rsplit(':').next()).map_or("?", str::trim);
    DiskHealth {
        disk: dev.to_owned(),
        media: None,
        healthy: match status {
            "PASSED" | "OK" => Some(true),
            "?" => None,
            _ => Some(false),
        },
        status: status.to_owned(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PowerPlan {
    pub guid: String,
    pub name: String,
    pub active: bool,
}

/// Windows'un yerleşik güç planları ve takma adları.
pub const WINDOWS_PLANS: [(&str, &str); 4] = [
    ("balanced", "381b4222-f694-41f0-9685-ff5bb260df2e"),
    ("high", "8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c"),
    ("saver", "a1841308-3542-4fab-9ba7-1e4d0f6e8a2e"),
    ("ultimate", "e9a42b02-d5df-448d-aa00-03f14749eb61"),
];

pub fn is_guid(text: &str) -> bool {
    text.len() == 36
        && text.char_indices().all(|(i, c)| {
            if matches!(i, 8 | 13 | 18 | 23) {
                c == '-'
            } else {
                c.is_ascii_hexdigit()
            }
        })
}

/// `powercfg /list` çıktısı. Satır biçimi dile göre değiştiği için yalnızca GUID, parantez içindeki
/// ad ve sondaki `*` (etkin plan) aranır.
pub fn parse_powercfg_list(text: &str) -> Vec<PowerPlan> {
    text.lines()
        .filter_map(|line| {
            let guid = line.split_whitespace().find(|word| is_guid(word))?;
            let name = line
                .split_once('(')
                .and_then(|(_, rest)| rest.rsplit_once(')'))
                .map_or("", |(name, _)| name);
            Some(PowerPlan {
                guid: guid.to_ascii_lowercase(),
                name: name.to_owned(),
                active: line.trim_end().ends_with('*'),
            })
        })
        .collect()
}

/// Kişisel veriyi metinden çıkarır: ev klasörü `~`, kullanıcı adı `<user>` olur. Kısa kullanıcı
/// adları (3 karakterden az) sıradan sözcükleri bozmamak için değiştirilmez.
pub fn redact(text: &str, home: Option<&str>, user: Option<&str>) -> String {
    let mut out = text.to_owned();
    if let Some(home) = home.filter(|h| h.len() > 1) {
        out = out.replace(home, "~");
        // Windows'ta yollar iki biçimde de görünebilir.
        out = out.replace(&home.replace('\\', "/"), "~");
    }
    if let Some(user) = user.filter(|u| u.chars().count() >= 3) {
        out = out.replace(user, "<user>");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sweeps_only_old_files_without_following_links() {
        let root = std::env::temp_dir().join(format!("skadi-sweep-test-{}", std::process::id()));
        let nested = root.join("a");
        std::fs::create_dir_all(&nested).unwrap();
        let old = nested.join("old.tmp");
        std::fs::write(&old, b"12345").unwrap();
        std::fs::File::options()
            .write(true)
            .open(&old)
            .unwrap()
            .set_modified(SystemTime::now() - Duration::from_secs(30 * 86_400))
            .unwrap();
        std::fs::write(root.join("new.tmp"), b"1").unwrap();

        let dry = sweep(&root, Duration::from_secs(7 * 86_400), false);
        assert_eq!((dry.files, dry.bytes), (1, 5));
        assert!(old.exists());
        let sizes = dir_sizes(&root).unwrap();
        assert_eq!(sizes[0], (nested.clone(), 5));
        let done = sweep(&root, Duration::from_secs(7 * 86_400), true);
        assert_eq!(done.files, 1);
        assert!(!old.exists() && root.join("new.tmp").exists() && nested.exists());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn parses_tool_output() {
        let list = "Existing Power Schemes (* Active)\n-----------------------------------\n\
            Power Scheme GUID: 381b4222-f694-41f0-9685-ff5bb260df2e  (Balanced) *\n\
            Güç Düzeni GUID'si: 8C5E7FDA-E8BF-4A96-9A85-A6E23A8C635C  (Yüksek performans)\n";
        let plans = parse_powercfg_list(list);
        assert_eq!(plans.len(), 2);
        assert!(plans[0].active && plans[0].name == "Balanced");
        assert_eq!(plans[1].guid, "8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c");
        assert!(!plans[1].active && plans[1].name == "Yüksek performans");
        assert!(!is_guid("381b4222-f694-41f0-9685-ff5bb260df2"));

        let health = parse_windows_health("Samsung SSD|SSD|Healthy\nOld HDD|HDD|Warning\n");
        assert_eq!(health[0].healthy, Some(true));
        assert_eq!(health[1].healthy, Some(false));
        let smart = smartctl_health("/dev/sda", "SMART overall-health self-assessment test result: PASSED\n");
        assert_eq!(smart.healthy, Some(true));
        assert_eq!(smartctl_health("/dev/sda", "Permission denied").healthy, None);
    }

    #[test]
    fn removes_home_and_user() {
        let text = r"C:\Users\alice\AppData\x.exe install --id Foo.Bar by alice";
        assert_eq!(
            redact(text, Some(r"C:\Users\alice"), Some("alice")),
            r"~\AppData\x.exe install --id Foo.Bar by <user>"
        );
        assert_eq!(redact("ab install", None, Some("ab")), "ab install");
    }
}
