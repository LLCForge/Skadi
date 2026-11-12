//! Paket yöneticisi tanımları.
//!
//! Yeni bir yönetici eklemek için `ManagerId`'ye bir varyant ve `SPECS` tablosuna **aynı sırada**
//! bir kayıt eklemek yeterlidir. Tablo sırası aynı zamanda tercih sırasıdır: bir uygulama birden
//! fazla yöneticide varsa, listede önce gelen ve sistemde bulunan yönetici kullanılır.

use std::{env, fmt, path::PathBuf, process::Output, str::FromStr};

use serde::{Deserialize, Serialize};

use crate::{
    error::{Error, Result},
    exec::{Privilege, find_program, is_executable},
    i18n::Lang,
    platform::Os,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ManagerId {
    Winget,
    /// Microsoft Store (winget üzerinden).
    Msstore,
    Choco,
    Scoop,
    Apt,
    Dnf,
    Yum,
    Zypper,
    Pacman,
    /// Arch User Repository, paru ya da yay ile.
    Aur,
    Apk,
    Xbps,
    Emerge,
    Eopkg,
    Urpmi,
    Flatpak,
    Snap,
    Nix,
    BrewCask,
    Brew,
    /// Mac App Store (`mas`).
    Mas,
}

impl ManagerId {
    pub const ALL: [Self; 21] = [
        Self::Winget,
        Self::Msstore,
        Self::Choco,
        Self::Scoop,
        Self::Apt,
        Self::Dnf,
        Self::Yum,
        Self::Zypper,
        Self::Pacman,
        Self::Aur,
        Self::Apk,
        Self::Xbps,
        Self::Emerge,
        Self::Eopkg,
        Self::Urpmi,
        Self::Flatpak,
        Self::Snap,
        Self::Nix,
        Self::BrewCask,
        Self::Brew,
        Self::Mas,
    ];

    pub fn spec(self) -> &'static ManagerSpec {
        &SPECS[self as usize]
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Winget => "winget",
            Self::Msstore => "msstore",
            Self::Choco => "choco",
            Self::Scoop => "scoop",
            Self::Apt => "apt",
            Self::Dnf => "dnf",
            Self::Yum => "yum",
            Self::Zypper => "zypper",
            Self::Pacman => "pacman",
            Self::Aur => "aur",
            Self::Apk => "apk",
            Self::Xbps => "xbps",
            Self::Emerge => "emerge",
            Self::Eopkg => "eopkg",
            Self::Urpmi => "urpmi",
            Self::Flatpak => "flatpak",
            Self::Snap => "snap",
            Self::Nix => "nix",
            Self::BrewCask => "brew-cask",
            Self::Brew => "brew",
            Self::Mas => "mas",
        }
    }
}

impl fmt::Display for ManagerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(self.as_str())
    }
}

impl FromStr for ManagerId {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        Self::ALL
            .into_iter()
            .find(|id| id.as_str().eq_ignore_ascii_case(s))
            .ok_or_else(|| Error::UnknownManager(s.to_owned()))
    }
}

/// Bir şablonun çalıştıracağı program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Program {
    /// Yöneticinin kendi programı (bulunduğu yol tespitte çözülür).
    Main,
    /// Yardımcı bir program (ör. `dpkg-query`, `rpm`).
    Named(&'static str),
}

/// Paket kimlikleri argümanların sonuna eklenir.
#[derive(Debug, Clone, Copy)]
pub struct CmdTemplate {
    pub program: Program,
    pub args: &'static [&'static str],
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

#[derive(Debug, Clone, Copy)]
pub enum QueryCheck {
    ExitSuccess,
    StdoutContains(&'static str),
    StdoutNonEmpty,
}

impl QueryCheck {
    pub fn matches(self, output: &Output) -> bool {
        let stdout = || String::from_utf8_lossy(&output.stdout);
        output.status.success()
            && match self {
                Self::ExitSuccess => true,
                Self::StdoutContains(needle) => stdout().contains(needle),
                Self::StdoutNonEmpty => !stdout().trim().is_empty(),
            }
    }
}

/// "Bu paket kurulu mu?" sorusunun cevabı.
#[derive(Debug, Clone, Copy)]
pub struct Query {
    pub cmd: CmdTemplate,
    pub check: QueryCheck,
}

/// Yönetici `PATH`'te değilse (ör. kurulumdan hemen sonra) bakılacak bilinen konum.
#[derive(Debug, Clone, Copy)]
pub struct FallbackPath {
    /// Taban dizini veren ortam değişkeni; `None` ise `path` mutlaktır.
    pub base_env: Option<&'static str>,
    pub path: &'static str,
}

impl FallbackPath {
    fn resolve(&self) -> Option<PathBuf> {
        let path = match self.base_env {
            Some(var) => PathBuf::from(env::var_os(var)?).join(self.path),
            None => PathBuf::from(self.path),
        };
        (path.is_absolute() && is_executable(&path)).then_some(path)
    }
}

/// Yönetici eksikse nasıl kurulacağı.
#[derive(Debug, Clone, Copy)]
pub enum Bootstrap {
    /// Resmî PowerShell kurulum betiği (Windows).
    PowerShell { script: &'static str, privilege: Privilege },
    /// Resmî kabuk kurulum betiği (macOS).
    Shell {
        script: &'static str,
        env: &'static [(&'static str, &'static str)],
        privilege: Privilege,
    },
    /// Dağıtımın kendi paket yöneticisiyle kurulan paket ve ardından root olarak çalışacak komutlar.
    SystemPackage {
        package: &'static str,
        post: &'static [CmdTemplate],
    },
}

impl Bootstrap {
    pub fn describe(&self, lang: Lang) -> String {
        match self {
            Self::PowerShell { script, .. } | Self::Shell { script, .. } => format!(
                "{}\n  {script}",
                lang.pick(
                    "The official installer script will be run:",
                    "Resmî kurulum betiği çalıştırılacak:"
                )
            ),
            Self::SystemPackage { package, .. } => format!(
                "{} {package}",
                lang.pick(
                    "Will be installed with the system package manager:",
                    "Sistemin paket yöneticisiyle kurulacak paket:"
                )
            ),
        }
    }
}

#[derive(Debug)]
pub struct ManagerSpec {
    pub id: ManagerId,
    pub display_name: &'static str,
    pub os: Os,
    /// Tespitte aranan program adı.
    pub program: &'static str,
    /// Dağıtımın kendi (yerel) paket yöneticisi mi?
    pub system: bool,
    pub privilege: Privilege,
    /// Tek komutta birden çok paket kurabiliyor mu?
    pub batch: bool,
    pub install: CmdTemplate,
    pub remove: CmdTemplate,
    pub query: Option<Query>,
    pub env: &'static [(&'static str, &'static str)],
    pub fallback_paths: &'static [FallbackPath],
    pub bootstrap: Option<Bootstrap>,
}

impl ManagerSpec {
    /// Yöneticinin bu sistemdeki mutlak yolu.
    pub fn locate(&self) -> Option<PathBuf> {
        find_program(self.program)
            .or_else(|| {
                ALTERNATE_PROGRAMS
                    .iter()
                    .filter(|(id, _)| *id == self.id)
                    .find_map(|(_, program)| find_program(program))
            })
            .or_else(|| self.fallback_paths.iter().find_map(FallbackPath::resolve))
    }
}

/// Aynı komut satırı arayüzünü sunan alternatif programlar (ör. AUR için paru yerine yay).
const ALTERNATE_PROGRAMS: &[(ManagerId, &str)] = &[(ManagerId::Aur, "yay")];

impl ManagerSpec {}

/// Katalogdan gelen paket kimliğini doğrular.
///
/// SECURITY FIX: CWE-88 kimlikler komut satırına argüman olarak eklenir; `-` ile başlayan bir
/// kimlik seçenek olarak yorumlanabilir. Yalnızca paket adlarında görülen karakterlere izin verilir.
pub fn validate_package_id(id: &str) -> Result<()> {
    let valid = !id.is_empty()
        && id.len() <= 128
        && !id.starts_with('-')
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '+' | '/' | '@' | ':' | '#'));
    if valid {
        Ok(())
    } else {
        Err(Error::InvalidPackageId(id.to_owned()))
    }
}

const WINGET_BOOTSTRAP: &str = "$ErrorActionPreference = 'Stop'; $ProgressPreference = 'SilentlyContinue'; \
Install-PackageProvider -Name NuGet -Force | Out-Null; \
Install-Module -Name Microsoft.WinGet.Client -Force -Repository PSGallery | Out-Null; \
Repair-WinGetPackageManager -AllUsers";

const CHOCO_BOOTSTRAP: &str = "$ErrorActionPreference = 'Stop'; \
[System.Net.ServicePointManager]::SecurityProtocol = [System.Net.ServicePointManager]::SecurityProtocol -bor 3072; \
Invoke-Expression ((New-Object System.Net.WebClient).DownloadString('https://community.chocolatey.org/install.ps1'))";

const SCOOP_BOOTSTRAP: &str = "$ErrorActionPreference = 'Stop'; \
$installer = [scriptblock]::Create((Invoke-RestMethod -Uri 'https://get.scoop.sh')); \
$admin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator); \
if ($admin) { & $installer -RunAsAdmin } else { & $installer }";

const BREW_BOOTSTRAP: &str = r#"set -euo pipefail; installer="$(curl --proto '=https' --tlsv1.2 -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"; /bin/bash -c "$installer""#;

// Homebrew'un analitik verisi gönderimi kapatılır.
const BREW_ENV: &[(&str, &str)] = &[("HOMEBREW_NO_ANALYTICS", "1"), ("HOMEBREW_NO_ENV_HINTS", "1")];

const BREW_PATHS: &[FallbackPath] = &[
    FallbackPath {
        base_env: None,
        path: "/opt/homebrew/bin/brew",
    },
    FallbackPath {
        base_env: None,
        path: "/usr/local/bin/brew",
    },
];

const BREW_BOOT: Bootstrap = Bootstrap::Shell {
    script: BREW_BOOTSTRAP,
    env: &[("NONINTERACTIVE", "1")],
    privilege: Privilege::NonRoot,
};

const RPM_QUERY: Query = Query {
    cmd: named("rpm", &["-q"]),
    check: QueryCheck::ExitSuccess,
};

/// `ManagerId` sırasıyla birebir aynı olmalıdır (testle doğrulanır).
pub static SPECS: [ManagerSpec; 21] = [
    ManagerSpec {
        id: ManagerId::Winget,
        display_name: "WinGet",
        os: Os::Windows,
        program: "winget",
        system: false,
        privilege: Privilege::Any,
        batch: false,
        install: main(&[
            "install",
            "--exact",
            "--silent",
            "--accept-package-agreements",
            "--accept-source-agreements",
            "--disable-interactivity",
            "--id",
        ]),
        remove: main(&[
            "uninstall",
            "--exact",
            "--silent",
            "--accept-source-agreements",
            "--disable-interactivity",
            "--id",
        ]),
        query: Some(Query {
            cmd: main(&[
                "list",
                "--exact",
                "--accept-source-agreements",
                "--disable-interactivity",
                "--id",
            ]),
            check: QueryCheck::ExitSuccess,
        }),
        env: &[],
        fallback_paths: &[FallbackPath {
            base_env: Some("LOCALAPPDATA"),
            path: "Microsoft/WindowsApps/winget.exe",
        }],
        bootstrap: Some(Bootstrap::PowerShell {
            script: WINGET_BOOTSTRAP,
            privilege: Privilege::Root,
        }),
    },
    ManagerSpec {
        id: ManagerId::Msstore,
        display_name: "Microsoft Store",
        os: Os::Windows,
        program: "winget",
        system: false,
        privilege: Privilege::Any,
        batch: false,
        install: main(&[
            "install",
            "--exact",
            "--silent",
            "--source",
            "msstore",
            "--accept-package-agreements",
            "--accept-source-agreements",
            "--disable-interactivity",
            "--id",
        ]),
        remove: main(&[
            "uninstall",
            "--exact",
            "--silent",
            "--source",
            "msstore",
            "--accept-source-agreements",
            "--disable-interactivity",
            "--id",
        ]),
        // Store paketleri için kurulu-mu sorgusu doğrulanmadı; durum bilinmiyor sayılır.
        query: None,
        env: &[],
        fallback_paths: &[FallbackPath {
            base_env: Some("LOCALAPPDATA"),
            path: "Microsoft/WindowsApps/winget.exe",
        }],
        bootstrap: Some(Bootstrap::PowerShell {
            script: WINGET_BOOTSTRAP,
            privilege: Privilege::Root,
        }),
    },
    ManagerSpec {
        id: ManagerId::Choco,
        display_name: "Chocolatey",
        os: Os::Windows,
        program: "choco",
        system: false,
        privilege: Privilege::Root,
        batch: true,
        install: main(&["install", "-y", "--no-progress"]),
        remove: main(&["uninstall", "-y", "--no-progress"]),
        query: Some(Query {
            cmd: main(&["list", "--exact", "--limit-output"]),
            check: QueryCheck::StdoutNonEmpty,
        }),
        env: &[],
        fallback_paths: &[
            FallbackPath {
                base_env: Some("ChocolateyInstall"),
                path: "bin/choco.exe",
            },
            FallbackPath {
                base_env: Some("ProgramData"),
                path: "chocolatey/bin/choco.exe",
            },
        ],
        bootstrap: Some(Bootstrap::PowerShell {
            script: CHOCO_BOOTSTRAP,
            privilege: Privilege::Root,
        }),
    },
    ManagerSpec {
        id: ManagerId::Scoop,
        display_name: "Scoop",
        os: Os::Windows,
        program: "scoop",
        system: false,
        privilege: Privilege::Any,
        batch: true,
        install: main(&["install"]),
        remove: main(&["uninstall"]),
        query: None,
        env: &[],
        fallback_paths: &[FallbackPath {
            base_env: Some("USERPROFILE"),
            path: "scoop/shims/scoop.cmd",
        }],
        bootstrap: Some(Bootstrap::PowerShell {
            script: SCOOP_BOOTSTRAP,
            privilege: Privilege::Any,
        }),
    },
    ManagerSpec {
        id: ManagerId::Apt,
        display_name: "APT",
        os: Os::Linux,
        program: "apt-get",
        system: true,
        privilege: Privilege::Root,
        batch: true,
        install: main(&["install", "-y"]),
        remove: main(&["remove", "-y"]),
        query: Some(Query {
            cmd: named("dpkg-query", &["-W", "-f=${Status}"]),
            check: QueryCheck::StdoutContains("install ok installed"),
        }),
        env: &[("DEBIAN_FRONTEND", "noninteractive")],
        fallback_paths: &[],
        bootstrap: None,
    },
    ManagerSpec {
        id: ManagerId::Dnf,
        display_name: "DNF",
        os: Os::Linux,
        program: "dnf",
        system: true,
        privilege: Privilege::Root,
        batch: true,
        install: main(&["install", "-y"]),
        remove: main(&["remove", "-y"]),
        query: Some(RPM_QUERY),
        env: &[],
        fallback_paths: &[],
        bootstrap: None,
    },
    ManagerSpec {
        id: ManagerId::Yum,
        display_name: "YUM",
        os: Os::Linux,
        program: "yum",
        system: true,
        privilege: Privilege::Root,
        batch: true,
        install: main(&["install", "-y"]),
        remove: main(&["remove", "-y"]),
        query: Some(RPM_QUERY),
        env: &[],
        fallback_paths: &[],
        bootstrap: None,
    },
    ManagerSpec {
        id: ManagerId::Zypper,
        display_name: "Zypper",
        os: Os::Linux,
        program: "zypper",
        system: true,
        privilege: Privilege::Root,
        batch: true,
        install: main(&["--non-interactive", "install"]),
        remove: main(&["--non-interactive", "remove"]),
        query: Some(RPM_QUERY),
        env: &[],
        fallback_paths: &[],
        bootstrap: None,
    },
    ManagerSpec {
        id: ManagerId::Pacman,
        display_name: "Pacman",
        os: Os::Linux,
        program: "pacman",
        system: true,
        privilege: Privilege::Root,
        batch: true,
        install: main(&["-S", "--needed", "--noconfirm"]),
        remove: main(&["-Rns", "--noconfirm"]),
        query: Some(Query {
            cmd: main(&["-Q"]),
            check: QueryCheck::ExitSuccess,
        }),
        env: &[],
        fallback_paths: &[],
        bootstrap: None,
    },
    ManagerSpec {
        id: ManagerId::Aur,
        display_name: "AUR (paru/yay)",
        os: Os::Linux,
        program: "paru",
        system: false,
        // paru ve yay root olarak çalışmayı reddeder; gerektiğinde sudo'yu kendileri çağırır.
        privilege: Privilege::NonRoot,
        batch: true,
        install: main(&["-S", "--needed", "--noconfirm"]),
        remove: main(&["-Rns", "--noconfirm"]),
        query: Some(Query {
            cmd: main(&["-Q"]),
            check: QueryCheck::ExitSuccess,
        }),
        env: &[],
        fallback_paths: &[],
        bootstrap: None,
    },
    ManagerSpec {
        id: ManagerId::Apk,
        display_name: "APK",
        os: Os::Linux,
        program: "apk",
        system: true,
        privilege: Privilege::Root,
        batch: true,
        install: main(&["add"]),
        remove: main(&["del"]),
        query: Some(Query {
            cmd: main(&["info", "-e"]),
            check: QueryCheck::ExitSuccess,
        }),
        env: &[],
        fallback_paths: &[],
        bootstrap: None,
    },
    ManagerSpec {
        id: ManagerId::Xbps,
        display_name: "XBPS",
        os: Os::Linux,
        program: "xbps-install",
        system: true,
        privilege: Privilege::Root,
        batch: true,
        install: main(&["-Sy"]),
        remove: named("xbps-remove", &["-Ry"]),
        query: Some(Query {
            cmd: named("xbps-query", &["-S"]),
            check: QueryCheck::ExitSuccess,
        }),
        env: &[],
        fallback_paths: &[],
        bootstrap: None,
    },
    ManagerSpec {
        id: ManagerId::Emerge,
        display_name: "Portage",
        os: Os::Linux,
        program: "emerge",
        system: true,
        privilege: Privilege::Root,
        batch: true,
        install: main(&[]),
        remove: main(&["--depclean"]),
        query: None,
        env: &[],
        fallback_paths: &[],
        bootstrap: None,
    },
    ManagerSpec {
        id: ManagerId::Eopkg,
        display_name: "eopkg",
        os: Os::Linux,
        program: "eopkg",
        system: true,
        privilege: Privilege::Root,
        batch: true,
        install: main(&["install", "-y"]),
        remove: main(&["remove", "-y"]),
        query: None,
        env: &[],
        fallback_paths: &[],
        bootstrap: None,
    },
    ManagerSpec {
        id: ManagerId::Urpmi,
        display_name: "urpmi",
        os: Os::Linux,
        program: "urpmi",
        system: true,
        privilege: Privilege::Root,
        batch: true,
        install: main(&["--auto"]),
        remove: named("urpme", &["--auto"]),
        query: Some(RPM_QUERY),
        env: &[],
        fallback_paths: &[],
        bootstrap: None,
    },
    ManagerSpec {
        id: ManagerId::Flatpak,
        display_name: "Flatpak",
        os: Os::Linux,
        program: "flatpak",
        system: false,
        privilege: Privilege::Any,
        batch: true,
        install: main(&["install", "-y", "--noninteractive", "flathub"]),
        remove: main(&["uninstall", "-y", "--noninteractive"]),
        query: Some(Query {
            cmd: main(&["info"]),
            check: QueryCheck::ExitSuccess,
        }),
        env: &[],
        fallback_paths: &[],
        bootstrap: Some(Bootstrap::SystemPackage {
            package: "flatpak",
            post: &[main(&[
                "remote-add",
                "--if-not-exists",
                "flathub",
                "https://dl.flathub.org/repo/flathub.flatpakrepo",
            ])],
        }),
    },
    ManagerSpec {
        id: ManagerId::Snap,
        display_name: "Snap",
        os: Os::Linux,
        program: "snap",
        system: false,
        privilege: Privilege::Root,
        batch: true,
        install: main(&["install"]),
        remove: main(&["remove"]),
        query: Some(Query {
            cmd: main(&["list"]),
            check: QueryCheck::ExitSuccess,
        }),
        env: &[],
        fallback_paths: &[],
        bootstrap: Some(Bootstrap::SystemPackage {
            package: "snapd",
            post: &[named("systemctl", &["enable", "--now", "snapd.socket"])],
        }),
    },
    ManagerSpec {
        id: ManagerId::Nix,
        display_name: "Nix",
        os: Os::Linux,
        program: "nix",
        system: false,
        privilege: Privilege::Any,
        batch: true,
        install: main(&[
            "--extra-experimental-features",
            "nix-command flakes",
            "profile",
            "install",
        ]),
        remove: main(&[
            "--extra-experimental-features",
            "nix-command flakes",
            "profile",
            "remove",
        ]),
        query: None,
        env: &[],
        fallback_paths: &[
            FallbackPath {
                base_env: Some("HOME"),
                path: ".nix-profile/bin/nix",
            },
            FallbackPath {
                base_env: None,
                path: "/nix/var/nix/profiles/default/bin/nix",
            },
        ],
        bootstrap: None,
    },
    ManagerSpec {
        id: ManagerId::BrewCask,
        display_name: "Homebrew Cask",
        os: Os::Macos,
        program: "brew",
        system: false,
        privilege: Privilege::NonRoot,
        batch: true,
        install: main(&["install", "--cask"]),
        remove: main(&["uninstall", "--cask"]),
        query: Some(Query {
            cmd: main(&["list", "--cask"]),
            check: QueryCheck::ExitSuccess,
        }),
        env: BREW_ENV,
        fallback_paths: BREW_PATHS,
        bootstrap: Some(BREW_BOOT),
    },
    ManagerSpec {
        id: ManagerId::Brew,
        display_name: "Homebrew",
        os: Os::Macos,
        program: "brew",
        system: false,
        privilege: Privilege::NonRoot,
        batch: true,
        install: main(&["install", "--formula"]),
        remove: main(&["uninstall", "--formula"]),
        query: Some(Query {
            cmd: main(&["list", "--formula"]),
            check: QueryCheck::ExitSuccess,
        }),
        env: BREW_ENV,
        fallback_paths: BREW_PATHS,
        bootstrap: Some(BREW_BOOT),
    },
    ManagerSpec {
        id: ManagerId::Mas,
        display_name: "Mac App Store",
        os: Os::Macos,
        program: "mas",
        system: false,
        privilege: Privilege::NonRoot,
        batch: true,
        install: main(&["install"]),
        remove: main(&["uninstall"]),
        query: None,
        env: &[],
        fallback_paths: &[
            FallbackPath {
                base_env: None,
                path: "/opt/homebrew/bin/mas",
            },
            FallbackPath {
                base_env: None,
                path: "/usr/local/bin/mas",
            },
        ],
        bootstrap: None,
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_table_matches_enum_order() {
        for id in ManagerId::ALL {
            assert_eq!(id.spec().id, id, "SPECS sırası ManagerId ile uyuşmuyor");
        }
    }

    #[test]
    fn manager_ids_round_trip() {
        for id in ManagerId::ALL {
            assert_eq!(id.as_str().parse::<ManagerId>().unwrap(), id);
        }
        assert!("npm".parse::<ManagerId>().is_err());
    }

    #[test]
    fn validates_package_ids() {
        for ok in [
            "Mozilla.Firefox",
            "org.mozilla.firefox",
            "www-client/firefox",
            "nixpkgs#git",
            "python@3.13",
            "g++",
        ] {
            assert!(validate_package_id(ok).is_ok(), "{ok}");
        }
        for bad in ["", "-rf", "a b", "x;rm", "$(id)", "a\nb", &"a".repeat(129)] {
            assert!(validate_package_id(bad).is_err(), "{bad:?}");
        }
    }
}
