//! Kurulu paket taraması, toplu güncelleme, güncelleme önizlemesi ve sürüm sabitleme.
//!
//! Her yöneticinin bu işlemler için kullandığı komutlar burada, yönetici başına bir `match`
//! kolunda durur. Bir yönetici bir işlemi desteklemiyorsa `None` döner ve arayüz bunu
//! "desteklenmiyor" olarak gösterir; Skadi o yönetici için tahmin yürütmez.

use std::collections::HashSet;

use serde::Serialize;

use crate::{
    error::{Error, Result},
    exec::{Invocation, Privilege},
    manager::{CmdTemplate, ManagerId, Program, validate_package_id},
    system::{System, resolve, template_args},
};

/// Kurulu paket listesinin çıktı biçimi.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListFormat {
    /// Her satır bir paket adı.
    Lines,
    /// `ad|sürüm` (Chocolatey `--limit-output`).
    PipeFirst,
    /// `ad<TAB>durum` (dpkg-query); yalnızca `installed` olanlar.
    DpkgStatus,
    /// `winget export` JSON dosyası.
    WingetExport,
    /// `scoop export` JSON çıktısı.
    ScoopExport,
    /// Başlık satırı olan tablo; ilk sütun paket adı (snap).
    TableFirstColumn,
    /// Başlıksız; ilk sütun paket kimliği (mas list).
    FirstColumn,
    /// `ii ad-sürüm_rev açıklama` (xbps-query -l).
    XbpsList,
}

#[derive(Debug, Clone, Copy)]
pub struct ListInstalled {
    pub cmd: CmdTemplate,
    pub format: ListFormat,
}

#[derive(Debug, Clone, Copy)]
pub struct Upgrade {
    /// Depo dizinini yenileyen komut (ör. `apt-get update`).
    pub refresh: Option<CmdTemplate>,
    pub all: CmdTemplate,
}

#[derive(Debug, Clone, Copy)]
pub struct Pin {
    pub add: CmdTemplate,
    pub remove: CmdTemplate,
}

/// Güncellenebilir paket listesinin çıktı biçimi.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutdatedFormat {
    /// `ad|kurulu|yeni|sabit` (Chocolatey).
    ChocoPipe,
    /// `brew outdated --json=v2`; `true` ise cask listesi okunur.
    BrewJson { casks: bool },
    /// `ad/depo yeni mimari [upgradable from: eski]` (apt).
    AptUpgradable,
    /// `ad.mimari sürüm depo` (dnf check-update; güncelleme varsa çıkış kodu 100).
    DnfCheck,
    /// `ad eski -> yeni` (pacman -Qu).
    PacmanQu,
    /// `uygulama<TAB>sürüm` (flatpak remote-ls --updates).
    FlatpakUpdates,
    /// Başlık satırı olan tablo; `ad sürüm ...` (snap refresh --list).
    SnapTable,
}

#[derive(Debug, Clone, Copy)]
pub struct Outdated {
    pub cmd: CmdTemplate,
    pub format: OutdatedFormat,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OutdatedPackage {
    pub package: String,
    pub current: Option<String>,
    pub latest: Option<String>,
}

const fn main(args: &'static [&'static str]) -> CmdTemplate {
    CmdTemplate {
        program: Program::Main,
        args,
    }
}

const fn named(program: &'static str, args: &'static [&'static str]) -> CmdTemplate {
    CmdTemplate {
        program: Program::Named(program),
        args,
    }
}

const RPM_ALL: ListInstalled = ListInstalled {
    cmd: named("rpm", &["-qa", "--qf", "%{NAME}\\n"]),
    format: ListFormat::Lines,
};

impl ManagerId {
    /// Kurulu paketleri tek komutla listeleme yöntemi.
    pub const fn list_installed(self) -> Option<ListInstalled> {
        Some(match self {
            Self::Winget | Self::Msstore => ListInstalled {
                cmd: main(&["export", "--accept-source-agreements", "--disable-interactivity", "-o"]),
                format: ListFormat::WingetExport,
            },
            Self::Choco => ListInstalled {
                cmd: main(&["list", "--limit-output"]),
                format: ListFormat::PipeFirst,
            },
            Self::Scoop => ListInstalled {
                cmd: main(&["export"]),
                format: ListFormat::ScoopExport,
            },
            Self::Apt => ListInstalled {
                cmd: named("dpkg-query", &["-W", "-f=${Package}\\t${db:Status-Status}\\n"]),
                format: ListFormat::DpkgStatus,
            },
            Self::Dnf | Self::Yum | Self::Zypper | Self::Urpmi => RPM_ALL,
            Self::Pacman => ListInstalled {
                cmd: main(&["-Qq"]),
                format: ListFormat::Lines,
            },
            Self::Apk => ListInstalled {
                cmd: main(&["info"]),
                format: ListFormat::Lines,
            },
            Self::Xbps => ListInstalled {
                cmd: named("xbps-query", &["-l"]),
                format: ListFormat::XbpsList,
            },
            Self::Flatpak => ListInstalled {
                cmd: main(&["list", "--app", "--columns=application"]),
                format: ListFormat::Lines,
            },
            Self::Snap => ListInstalled {
                cmd: main(&["list"]),
                format: ListFormat::TableFirstColumn,
            },
            Self::Brew => ListInstalled {
                cmd: main(&["list", "--formula", "-1"]),
                format: ListFormat::Lines,
            },
            Self::BrewCask => ListInstalled {
                cmd: main(&["list", "--cask", "-1"]),
                format: ListFormat::Lines,
            },
            // Yalnızca AUR'dan (depo dışından) kurulmuş paketler.
            Self::Aur => ListInstalled {
                cmd: main(&["-Qqm"]),
                format: ListFormat::Lines,
            },
            // `mas list`: "kimlik  Ad (sürüm)".
            Self::Mas => ListInstalled {
                cmd: main(&["list"]),
                format: ListFormat::FirstColumn,
            },
            Self::Emerge | Self::Eopkg | Self::Nix => return None,
        })
    }

    /// Yöneticinin kurduğu her şeyi güncelleyen komutlar.
    pub const fn upgrade(self) -> Option<Upgrade> {
        let (refresh, all) = match self {
            Self::Winget => (
                None,
                main(&[
                    "upgrade",
                    "--all",
                    "--silent",
                    "--accept-package-agreements",
                    "--accept-source-agreements",
                    "--disable-interactivity",
                ]),
            ),
            Self::Choco => (None, main(&["upgrade", "all", "-y", "--no-progress"])),
            Self::Scoop => (Some(main(&["update"])), main(&["update", "*"])),
            Self::Apt => (Some(main(&["update"])), main(&["upgrade", "-y"])),
            Self::Dnf => (None, main(&["upgrade", "-y"])),
            Self::Yum => (None, main(&["update", "-y"])),
            Self::Zypper => (
                Some(main(&["--non-interactive", "refresh"])),
                main(&["--non-interactive", "update"]),
            ),
            Self::Pacman => (None, main(&["-Syu", "--noconfirm"])),
            Self::Apk => (Some(main(&["update"])), main(&["upgrade"])),
            Self::Xbps => (None, main(&["-Syu"])),
            Self::Emerge => (
                Some(main(&["--sync"])),
                main(&["--update", "--deep", "--newuse", "@world"]),
            ),
            Self::Eopkg => (None, main(&["upgrade", "-y"])),
            Self::Urpmi => (None, main(&["--auto-update", "--auto"])),
            Self::Flatpak => (None, main(&["update", "-y", "--noninteractive"])),
            Self::Snap => (None, main(&["refresh"])),
            Self::Nix => (
                None,
                main(&[
                    "--extra-experimental-features",
                    "nix-command flakes",
                    "profile",
                    "upgrade",
                    "--all",
                ]),
            ),
            Self::Brew => (Some(main(&["update"])), main(&["upgrade", "--formula"])),
            Self::BrewCask => (Some(main(&["update"])), main(&["upgrade", "--cask"])),
            Self::Aur => (None, main(&["-Sua", "--noconfirm"])),
            Self::Mas => (None, main(&["upgrade"])),
            // Store uygulamaları `winget upgrade --all` ile birlikte güncellenir.
            Self::Msstore => return None,
        };
        Some(Upgrade { refresh, all })
    }

    /// Yöneticinin kendi sabitleme (pin/hold) mekanizması. Sabitlenen paketler Skadi dışından
    /// yapılan güncellemelerde de korunur.
    pub const fn pin(self) -> Option<Pin> {
        Some(match self {
            Self::Winget => Pin {
                add: main(&["pin", "add", "--exact", "--accept-source-agreements", "--id"]),
                remove: main(&["pin", "remove", "--exact", "--id"]),
            },
            Self::Choco => Pin {
                add: main(&["pin", "add", "--name"]),
                remove: main(&["pin", "remove", "--name"]),
            },
            Self::Scoop => Pin {
                add: main(&["hold"]),
                remove: main(&["unhold"]),
            },
            Self::Apt => Pin {
                add: named("apt-mark", &["hold"]),
                remove: named("apt-mark", &["unhold"]),
            },
            Self::Zypper => Pin {
                add: main(&["--non-interactive", "addlock"]),
                remove: main(&["--non-interactive", "removelock"]),
            },
            Self::Flatpak => Pin {
                add: main(&["mask"]),
                remove: main(&["mask", "--remove"]),
            },
            Self::Snap => Pin {
                add: main(&["refresh", "--hold"]),
                remove: main(&["refresh", "--unhold"]),
            },
            Self::Brew => Pin {
                add: main(&["pin"]),
                remove: main(&["unpin"]),
            },
            _ => return None,
        })
    }

    /// Önbelleği ve artık bağımlılıkları temizleyen komutlar. Yalnızca yöneticinin kendi güvenli
    /// komutları kullanılır; geri alma imkânını yok eden seçenekler (örneğin `nix-collect-garbage -d`)
    /// ve kabuk boru hattı gerektirenler (pacman yetim paketleri) bilerek dışarıda bırakılır.
    pub const fn cleanup(self) -> &'static [CmdTemplate] {
        match self {
            Self::Choco => {
                const L: &[CmdTemplate] = &[main(&["cache", "remove", "-y"])];
                L
            }
            Self::Scoop => {
                const L: &[CmdTemplate] = &[main(&["cleanup", "*"]), main(&["cache", "rm", "*"])];
                L
            }
            Self::Apt => {
                const L: &[CmdTemplate] = &[main(&["autoremove", "-y"]), main(&["clean"])];
                L
            }
            Self::Dnf | Self::Yum => {
                const L: &[CmdTemplate] = &[main(&["autoremove", "-y"]), main(&["clean", "all"])];
                L
            }
            Self::Zypper => {
                const L: &[CmdTemplate] = &[main(&["--non-interactive", "clean", "--all"])];
                L
            }
            Self::Pacman | Self::Aur => {
                const L: &[CmdTemplate] = &[main(&["-Sc", "--noconfirm"])];
                L
            }
            Self::Xbps => {
                const L: &[CmdTemplate] = &[named("xbps-remove", &["-O", "-o", "-y"])];
                L
            }
            Self::Eopkg => {
                const L: &[CmdTemplate] = &[main(&["delete-cache"]), main(&["remove-orphans", "-y"])];
                L
            }
            Self::Flatpak => {
                const L: &[CmdTemplate] = &[main(&["uninstall", "--unused", "-y", "--noninteractive"])];
                L
            }
            Self::Nix => {
                const L: &[CmdTemplate] = &[named("nix-collect-garbage", &[])];
                L
            }
            Self::Brew => {
                const L: &[CmdTemplate] = &[main(&["cleanup"]), main(&["autoremove"])];
                L
            }
            _ => &[],
        }
    }

    /// Yapılandırma dosyalarını da silen kaldırma (`apt purge`, `brew uninstall --zap`). pacman'in
    /// kaldırma komutu zaten `-Rns` kullanır.
    pub const fn purge(self) -> Option<CmdTemplate> {
        Some(match self {
            Self::Apt => main(&["purge", "-y"]),
            Self::BrewCask => main(&["uninstall", "--cask", "--zap"]),
            _ => return None,
        })
    }

    /// Belirli bir sürüme dönme komutu; sonuna paket ve sürüm bağımsız değişkenleri eklenir.
    /// Snap kendi önceki sürümüne döner (`snap revert`), sürüm gerekmez.
    pub const fn rollback(self) -> Option<CmdTemplate> {
        Some(match self {
            Self::Winget => main(&[
                "install",
                "--exact",
                "--force",
                "--silent",
                "--accept-package-agreements",
                "--accept-source-agreements",
                "--disable-interactivity",
            ]),
            Self::Choco => main(&["install", "-y", "--allow-downgrade", "--no-progress"]),
            Self::Apt => main(&["install", "-y", "--allow-downgrades"]),
            Self::Dnf | Self::Yum => main(&["downgrade", "-y"]),
            Self::Zypper => main(&["--non-interactive", "install", "--oldpackage"]),
            Self::Snap => main(&["revert"]),
            _ => return None,
        })
    }

    /// Güncellenebilir paketleri listeleme yöntemi.
    pub const fn outdated(self) -> Option<Outdated> {
        Some(match self {
            Self::Choco => Outdated {
                cmd: main(&["outdated", "--limit-output"]),
                format: OutdatedFormat::ChocoPipe,
            },
            Self::Brew => Outdated {
                cmd: main(&["outdated", "--formula", "--json=v2"]),
                format: OutdatedFormat::BrewJson { casks: false },
            },
            Self::BrewCask => Outdated {
                cmd: main(&["outdated", "--cask", "--json=v2"]),
                format: OutdatedFormat::BrewJson { casks: true },
            },
            Self::Apt => Outdated {
                cmd: named("apt", &["list", "--upgradable"]),
                format: OutdatedFormat::AptUpgradable,
            },
            Self::Dnf => Outdated {
                cmd: main(&["check-update", "-q"]),
                format: OutdatedFormat::DnfCheck,
            },
            Self::Pacman => Outdated {
                cmd: main(&["-Qu"]),
                format: OutdatedFormat::PacmanQu,
            },
            Self::Flatpak => Outdated {
                cmd: main(&["remote-ls", "--updates", "--columns=application,version"]),
                format: OutdatedFormat::FlatpakUpdates,
            },
            Self::Snap => Outdated {
                cmd: main(&["refresh", "--list"]),
                format: OutdatedFormat::SnapTable,
            },
            _ => return None,
        })
    }
}

/// Kurulu paket adları (küçük harfe çevrilmiş).
pub fn parse_installed(format: ListFormat, output: &str) -> HashSet<String> {
    let lines = output.lines().map(str::trim).filter(|line| !line.is_empty());
    let names: Vec<String> = match format {
        ListFormat::Lines => lines.map(str::to_owned).collect(),
        ListFormat::PipeFirst => lines.filter_map(|l| l.split('|').next()).map(str::to_owned).collect(),
        ListFormat::DpkgStatus => lines
            .filter_map(|l| l.split_once('\t'))
            .filter(|(_, status)| *status == "installed")
            .map(|(name, _)| name.to_owned())
            .collect(),
        ListFormat::FirstColumn => lines
            .filter_map(|l| l.split_whitespace().next())
            .map(str::to_owned)
            .collect(),
        ListFormat::TableFirstColumn => lines
            .skip(1)
            .filter_map(|l| l.split_whitespace().next())
            .map(str::to_owned)
            .collect(),
        ListFormat::XbpsList => lines
            .filter_map(|l| l.split_whitespace().nth(1))
            .filter_map(|pkgver| pkgver.rsplit_once('-').map(|(name, _)| name.to_owned()))
            .collect(),
        ListFormat::WingetExport => json_strings(output, &["Sources", "*", "Packages", "*", "PackageIdentifier"]),
        ListFormat::ScoopExport => json_strings(output, &["apps", "*", "Name"]),
    };
    names.into_iter().map(|name| name.to_ascii_lowercase()).collect()
}

/// JSON içinde bir yol boyunca dizeleri toplar; `*` dizinin her elemanı demektir.
fn json_strings(text: &str, path: &[&str]) -> Vec<String> {
    fn walk(value: &serde_json::Value, path: &[&str], out: &mut Vec<String>) {
        match path.split_first() {
            None => out.extend(value.as_str().map(str::to_owned)),
            Some((&"*", rest)) => value
                .as_array()
                .into_iter()
                .flatten()
                .for_each(|item| walk(item, rest, out)),
            Some((key, rest)) => {
                if let Some(child) = value.get(*key) {
                    walk(child, rest, out);
                }
            }
        }
    }
    let mut out = Vec::new();
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(text) {
        walk(&value, path, &mut out);
    }
    out
}

pub fn parse_outdated(format: OutdatedFormat, output: &str) -> Vec<OutdatedPackage> {
    let package = |name: &str, current: Option<&str>, latest: Option<&str>| OutdatedPackage {
        package: name.to_owned(),
        current: current.map(str::to_owned),
        latest: latest.map(str::to_owned),
    };
    let lines = output.lines().map(str::trim).filter(|line| !line.is_empty());
    match format {
        OutdatedFormat::ChocoPipe => lines
            .filter_map(|l| {
                let parts: Vec<&str> = l.split('|').collect();
                (parts.len() >= 3).then(|| package(parts[0], Some(parts[1]), Some(parts[2])))
            })
            .collect(),
        OutdatedFormat::AptUpgradable => lines
            .filter(|l| l.contains("[upgradable from:"))
            .filter_map(|l| {
                let (name, rest) = l.split_once('/')?;
                let latest = rest.split_whitespace().nth(1);
                let current = l
                    .rsplit_once("[upgradable from:")?
                    .1
                    .trim()
                    .trim_end_matches(']')
                    .trim();
                Some(package(name, Some(current), latest))
            })
            .collect(),
        OutdatedFormat::DnfCheck => lines
            .filter_map(|l| {
                let mut parts = l.split_whitespace();
                let (name_arch, version, _repo) = (parts.next()?, parts.next()?, parts.next()?);
                let name = name_arch.rsplit_once('.').map_or(name_arch, |(name, _)| name);
                Some(package(name, None, Some(version)))
            })
            .collect(),
        OutdatedFormat::PacmanQu => lines
            .filter_map(|l| {
                let parts: Vec<&str> = l.split_whitespace().collect();
                (parts.len() >= 4 && parts[2] == "->").then(|| package(parts[0], Some(parts[1]), Some(parts[3])))
            })
            .collect(),
        OutdatedFormat::FlatpakUpdates => lines
            .filter_map(|l| {
                let mut parts = l.split('\t');
                let name = parts.next()?;
                name.contains('.')
                    .then(|| package(name, None, parts.next().filter(|v| !v.is_empty())))
            })
            .collect(),
        OutdatedFormat::SnapTable => lines
            .skip(1)
            .filter_map(|l| {
                let mut parts = l.split_whitespace();
                Some(package(parts.next()?, None, parts.next()))
            })
            .collect(),
        OutdatedFormat::BrewJson { casks } => {
            let Ok(value) = serde_json::from_str::<serde_json::Value>(output) else {
                return Vec::new();
            };
            let key = if casks { "casks" } else { "formulae" };
            value
                .get(key)
                .and_then(|v| v.as_array())
                .into_iter()
                .flatten()
                .filter_map(|item| {
                    let name = item.get("name")?.as_str()?;
                    let current = item
                        .get("installed_versions")
                        .and_then(|v| v.as_array())
                        .and_then(|v| v.last())
                        .and_then(|v| v.as_str());
                    let latest = item.get("current_version").and_then(|v| v.as_str());
                    Some(package(name, current, latest))
                })
                .collect()
        }
    }
}

impl System {
    fn build(
        &self,
        id: ManagerId,
        template: &CmdTemplate,
        packages: &[&str],
        privilege: Privilege,
    ) -> Result<Invocation> {
        let manager = self.manager(id).ok_or(Error::ManagerNotFound(id.as_str()))?;
        let program = resolve(manager.spec, Some(&manager.path), template.program)?;
        Invocation::build(
            program,
            template_args(template, packages),
            manager.spec.env,
            privilege,
            &self.privileges,
            self.os,
        )
    }

    /// Okuma komutları yetki yükseltmeden çalışır (root gerektirenlerde bile).
    fn read_privilege(id: ManagerId) -> Privilege {
        match id.spec().privilege {
            Privilege::Root => Privilege::Any,
            other => other,
        }
    }

    /// Bir yöneticinin kurulu paketleri; yönetici listelemeyi desteklemiyorsa `None`.
    pub fn installed_packages(&self, id: ManagerId) -> Result<Option<HashSet<String>>> {
        let Some(list) = id.list_installed() else {
            return Ok(None);
        };
        if list.format == ListFormat::WingetExport {
            // winget export yalnızca dosyaya yazar; kullanıcıya özel geçici dizinde benzersiz bir ad.
            let nonce = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos());
            let path = std::env::temp_dir().join(format!("skadi-winget-{}-{nonce}.json", std::process::id()));
            let path_text = path.to_string_lossy().into_owned();
            let output = self
                .build(id, &list.cmd, &[&path_text], Self::read_privilege(id))?
                .output()?;
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            let _ = std::fs::remove_file(&path);
            if text.is_empty() && !output.status.success() {
                return Err(Error::CommandFailed {
                    program: id.as_str().to_owned(),
                    code: output.status.code(),
                    detail: None,
                });
            }
            return Ok(Some(parse_installed(list.format, &text)));
        }
        let output = self.build(id, &list.cmd, &[], Self::read_privilege(id))?.output()?;
        if !output.status.success() {
            return Err(Error::CommandFailed {
                program: id.as_str().to_owned(),
                code: output.status.code(),
                detail: None,
            });
        }
        Ok(Some(parse_installed(
            list.format,
            &String::from_utf8_lossy(&output.stdout),
        )))
    }

    /// Toplu güncelleme komutları (önce depo yenileme, sonra güncelleme).
    pub fn upgrade_invocations(&self, id: ManagerId) -> Result<Vec<Invocation>> {
        let upgrade = id.upgrade().ok_or(Error::Unsupported(id.as_str()))?;
        let privilege = id.spec().privilege;
        upgrade
            .refresh
            .iter()
            .chain(std::iter::once(&upgrade.all))
            .map(|template| self.build(id, template, &[], privilege))
            .collect()
    }

    /// Önbellek ve artık bağımlılık temizliği komutları; desteklenmiyorsa boş.
    pub fn cleanup_invocations(&self, id: ManagerId) -> Result<Vec<Invocation>> {
        let privilege = id.spec().privilege;
        id.cleanup()
            .iter()
            .map(|template| self.build(id, template, &[], privilege))
            .collect()
    }

    pub fn pin_invocation(&self, id: ManagerId, package: &str, add: bool) -> Result<Invocation> {
        validate_package_id(package)?;
        let pin = id.pin().ok_or(Error::Unsupported(id.as_str()))?;
        let template = if add { &pin.add } else { &pin.remove };
        self.build(id, template, &[package], id.spec().privilege)
    }

    /// Güncellenebilir paketler; yönetici bunu raporlayamıyorsa `None`.
    pub fn outdated(&self, id: ManagerId) -> Result<Option<Vec<OutdatedPackage>>> {
        let Some(outdated) = id.outdated() else {
            return Ok(None);
        };
        let output = self.build(id, &outdated.cmd, &[], Self::read_privilege(id))?.output()?;
        // dnf check-update güncelleme varsa 100 ile çıkar.
        let ok = output.status.success()
            || (outdated.format == OutdatedFormat::DnfCheck && output.status.code() == Some(100));
        if !ok {
            return Err(Error::CommandFailed {
                program: id.as_str().to_owned(),
                code: output.status.code(),
                detail: None,
            });
        }
        Ok(Some(parse_outdated(
            outdated.format,
            &String::from_utf8_lossy(&output.stdout),
        )))
    }

    /// Katalogdaki uygulamalardan kurulu olanlar ve hangi yöneticiyle kurulu oldukları.
    pub fn installed_apps<'c>(&self, apps: &'c [crate::catalog::App]) -> Vec<(&'c crate::catalog::App, ManagerId)> {
        // Yöneticiler paralel sorgulanır; toplam süre en yavaş yöneticinin süresi kadardır.
        // winget ve msstore aynı listeyi paylaştığı için msstore ayrıca sorgulanmaz.
        let ids: Vec<ManagerId> = self
            .managers()
            .iter()
            .map(|m| m.spec.id)
            .filter(|id| *id != ManagerId::Msstore)
            .collect();
        let mut cache: Vec<(ManagerId, HashSet<String>)> = std::thread::scope(|scope| {
            let handles: Vec<_> = ids
                .iter()
                .map(|&id| scope.spawn(move || (id, self.installed_packages(id))))
                .collect();
            handles
                .into_iter()
                .filter_map(|handle| handle.join().ok())
                .filter_map(|(id, result)| result.ok().flatten().map(|set| (id, set)))
                .collect()
        });
        if self.manager(ManagerId::Msstore).is_some() {
            let shared = cache
                .iter()
                .find(|(m, _)| *m == ManagerId::Winget)
                .map(|(_, s)| s.clone());
            cache.extend(shared.map(|set| (ManagerId::Msstore, set)));
        }
        apps.iter()
            .filter_map(|app| {
                cache.iter().find_map(|(id, set)| {
                    app.packages
                        .get(id)
                        .filter(|package| set.contains(&package.to_ascii_lowercase()))
                        .map(|_| (app, *id))
                })
            })
            .collect()
    }
}

/// Sürüm metni: harf, rakam ve `. + ~ : _ -`; `-` ile başlayamaz.
pub fn validate_version(version: &str) -> Result<()> {
    let ok = !version.is_empty()
        && version.len() <= 64
        && !version.starts_with('-')
        && version
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '+' | '~' | ':' | '_' | '-'));
    if ok {
        Ok(())
    } else {
        Err(Error::InvalidOption("version"))
    }
}

/// Kurulu paketler ve sürümleri. `winget export --include-versions`, `choco list`, dpkg ve rpm
/// çıktılarını okur.
pub fn parse_versions(id: ManagerId, text: &str) -> std::collections::HashMap<String, String> {
    let mut out = std::collections::HashMap::new();
    match id {
        ManagerId::Winget => {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(text) {
                for source in value["Sources"].as_array().into_iter().flatten() {
                    for package in source["Packages"].as_array().into_iter().flatten() {
                        if let (Some(p), Some(v)) = (package["PackageIdentifier"].as_str(), package["Version"].as_str())
                        {
                            out.insert(p.to_owned(), v.to_owned());
                        }
                    }
                }
            }
        }
        _ => {
            let separator = if id == ManagerId::Choco { '|' } else { '\t' };
            for line in text.lines() {
                if let Some((p, v)) = line.trim().split_once(separator)
                    && !p.is_empty()
                    && !v.is_empty()
                {
                    out.insert(p.to_owned(), v.trim().to_owned());
                }
            }
        }
    }
    out
}

impl System {
    /// Kurulu paketlerin sürümleri; yönetici desteklemiyorsa `None`.
    pub fn installed_versions(&self, id: ManagerId) -> Result<Option<std::collections::HashMap<String, String>>> {
        let template = match id {
            ManagerId::Choco => main(&["list", "--limit-output"]),
            ManagerId::Apt => named("dpkg-query", &["-W", "-f=${Package}\\t${Version}\\n"]),
            ManagerId::Dnf | ManagerId::Yum | ManagerId::Zypper => {
                named("rpm", &["-qa", "--qf", "%{NAME}\\t%{VERSION}-%{RELEASE}\\n"])
            }
            ManagerId::Winget => {
                let nonce = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_nanos());
                let path = std::env::temp_dir().join(format!("skadi-versions-{}-{nonce}.json", std::process::id()));
                let path_text = path.to_string_lossy().into_owned();
                let template = main(&[
                    "export",
                    "--include-versions",
                    "--accept-source-agreements",
                    "--disable-interactivity",
                    "-o",
                ]);
                let _ = self.build(id, &template, &[&path_text], Privilege::Any)?.output()?;
                let text = std::fs::read_to_string(&path).unwrap_or_default();
                let _ = std::fs::remove_file(&path);
                return Ok(Some(parse_versions(id, &text)));
            }
            _ => return Ok(None),
        };
        let output = self.build(id, &template, &[], Self::read_privilege(id))?.output()?;
        Ok(Some(parse_versions(id, &String::from_utf8_lossy(&output.stdout))))
    }

    /// Bir paketi belirli bir sürüme döndüren komut.
    pub fn rollback_invocation(&self, id: ManagerId, package: &str, version: Option<&str>) -> Result<Invocation> {
        validate_package_id(package)?;
        if let Some(v) = version {
            validate_version(v)?;
        }
        let template = id.rollback().ok_or(Error::Unsupported(id.as_str()))?;
        let pair_eq;
        let pair_dash;
        let args: Vec<&str> = match (id, version) {
            (ManagerId::Snap, _) => vec![package],
            (_, None) => return Err(Error::InvalidOption("version")),
            (ManagerId::Winget, Some(v)) => vec!["--id", package, "--version", v],
            (ManagerId::Choco, Some(v)) => vec![package, "--version", v],
            (ManagerId::Dnf | ManagerId::Yum, Some(v)) => {
                pair_dash = format!("{package}-{v}");
                vec![pair_dash.as_str()]
            }
            (_, Some(v)) => {
                pair_eq = format!("{package}={v}");
                vec![pair_eq.as_str()]
            }
        };
        self.build(id, &template, &args, id.spec().privilege)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_versions_and_validates() {
        let choco = parse_versions(ManagerId::Choco, "git|2.45.1\nvlc|3.0.21\n");
        assert_eq!(choco["git"], "2.45.1");
        let dpkg = parse_versions(ManagerId::Apt, "git\t1:2.43.0-1ubuntu7\n");
        assert_eq!(dpkg["git"], "1:2.43.0-1ubuntu7");
        let winget = parse_versions(
            ManagerId::Winget,
            r#"{"Sources":[{"Packages":[{"PackageIdentifier":"Git.Git","Version":"2.45.1"}]}]}"#,
        );
        assert_eq!(winget["Git.Git"], "2.45.1");
        assert!(validate_version("1:2.43.0-1ubuntu7").is_ok());
        assert!(validate_version("--force").is_err() && validate_version("1;rm").is_err());
        assert!(ManagerId::Brew.rollback().is_none());
    }

    #[test]
    fn parses_installed_formats() {
        let set = parse_installed(ListFormat::PipeFirst, "Firefox|160.0\ngit|2.50\n");
        assert!(set.contains("firefox") && set.contains("git"));
        let set = parse_installed(ListFormat::DpkgStatus, "git\tinstalled\nvim\tconfig-files\n");
        assert_eq!(set.len(), 1);
        let set = parse_installed(
            ListFormat::TableFirstColumn,
            "Name Version Rev\ncore22 2024 1\nfirefox 130 2\n",
        );
        assert!(set.contains("firefox") && !set.contains("name"));
        let set = parse_installed(ListFormat::XbpsList, "ii git-2.45.0_1   Git\nii xz-5.4_1 desc\n");
        assert!(set.contains("git") && set.contains("xz"));
        let export = r#"{"Sources":[{"Packages":[{"PackageIdentifier":"Mozilla.Firefox"}]},{"Packages":[{"PackageIdentifier":"9NKSQGP7F2NH"}]}]}"#;
        let set = parse_installed(ListFormat::WingetExport, export);
        assert!(set.contains("mozilla.firefox") && set.contains("9nksqgp7f2nh"));
        let set = parse_installed(ListFormat::ScoopExport, r#"{"apps":[{"Name":"7zip"}]}"#);
        assert!(set.contains("7zip"));
        assert!(parse_installed(ListFormat::WingetExport, "not json").is_empty());
    }

    #[test]
    fn parses_outdated_formats() {
        let apt = "Listing...\nfirefox/noble-updates 131.0 amd64 [upgradable from: 130.0]\n";
        assert_eq!(
            parse_outdated(OutdatedFormat::AptUpgradable, apt),
            [OutdatedPackage {
                package: "firefox".into(),
                current: Some("130.0".into()),
                latest: Some("131.0".into())
            }]
        );
        let pacman = parse_outdated(OutdatedFormat::PacmanQu, "git 2.45-1 -> 2.46-1\nbad line\n");
        assert_eq!(pacman.len(), 1);
        assert_eq!(pacman[0].latest.as_deref(), Some("2.46-1"));
        let choco = parse_outdated(OutdatedFormat::ChocoPipe, "git|2.45|2.46|false\n");
        assert_eq!(choco[0].current.as_deref(), Some("2.45"));
        let dnf = parse_outdated(OutdatedFormat::DnfCheck, "firefox.x86_64   131.0-1.fc40   updates\n");
        assert_eq!(dnf[0].package, "firefox");
        let brew = r#"{"formulae":[{"name":"git","installed_versions":["2.45"],"current_version":"2.46"}],"casks":[]}"#;
        assert_eq!(
            parse_outdated(OutdatedFormat::BrewJson { casks: false }, brew)[0]
                .latest
                .as_deref(),
            Some("2.46")
        );
        assert!(parse_outdated(OutdatedFormat::BrewJson { casks: true }, brew).is_empty());
        let flatpak = parse_outdated(OutdatedFormat::FlatpakUpdates, "org.mozilla.firefox\t131.0\n");
        assert_eq!(flatpak[0].package, "org.mozilla.firefox");
    }

    #[test]
    fn every_manager_with_pin_also_upgrades() {
        for id in ManagerId::ALL {
            if id.pin().is_some() {
                assert!(id.upgrade().is_some(), "{id}");
            }
        }
    }
}
