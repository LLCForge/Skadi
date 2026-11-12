use std::fmt;

use crate::{i18n::Lang, manager::ManagerId};

#[derive(Debug)]
pub enum Error {
    UnsupportedOs(String),
    UnknownManager(String),
    UnknownApp(String),
    UnknownCategory(String),
    ManagerNotFound(&'static str),
    ProgramNotFound(&'static str),
    InvalidPackageId(String),
    Catalog(String),
    ElevationRequired,
    NoElevator,
    MustNotBeRoot,
    NoSystemManager,
    BootstrapUnavailable(ManagerId),
    BootstrapIncomplete(ManagerId),
    UnknownTweak(String),
    UnknownStartupItem(String),
    Signature(String),
    Policy(String),
    Profile(String),
    Media(String),
    UnprotectedExecutable(String),
    InvalidOption(&'static str),
    Unsupported(&'static str),
    WrongPlatform(String),
    NotReversible(String),
    NothingToRevert(String),
    State(String),
    Registry(String),
    Spawn {
        program: String,
        source: std::io::Error,
    },
    CommandFailed {
        program: String,
        code: Option<i32>,
        /// Komutun hata çıktısının son satırı (yakalandıysa).
        detail: Option<String>,
    },
}

impl Error {
    /// Kullanıcıya gösterilecek, seçili dildeki mesaj.
    pub fn message(&self, lang: Lang) -> String {
        let t = |en, tr| lang.pick(en, tr);
        match self {
            Self::UnsupportedOs(os) => format!(
                "{}: {os}",
                t("unsupported operating system", "desteklenmeyen işletim sistemi")
            ),
            Self::UnknownManager(m) => format!("{}: {m}", t("unknown package manager", "bilinmeyen paket yöneticisi")),
            Self::UnknownApp(a) => format!(
                "{}: {a}",
                t("no such app in the catalog", "katalogda böyle bir uygulama yok")
            ),
            Self::UnknownCategory(c) => format!("{}: {c}", t("unknown category", "bilinmeyen kategori")),
            Self::ManagerNotFound(m) => {
                format!(
                    "{}: {m}",
                    t(
                        "package manager not found on this system",
                        "paket yöneticisi bu sistemde bulunamadı"
                    )
                )
            }
            Self::ProgramNotFound(p) => {
                format!("{}: {p}", t("required program not found", "gerekli program bulunamadı"))
            }
            Self::InvalidPackageId(id) => format!("{}: {id:?}", t("invalid package id", "geçersiz paket kimliği")),
            Self::Catalog(reason) => format!("{}: {reason}", t("invalid catalog", "katalog geçersiz")),
            Self::ElevationRequired => t(
                "this action requires administrator rights; reopen the terminal as administrator and try again",
                "bu işlem yönetici yetkisi gerektiriyor; terminali yönetici olarak açıp tekrar deneyin",
            )
            .to_owned(),
            Self::NoElevator => t(
                "neither sudo nor doas was found to elevate privileges",
                "yetki yükseltmek için sudo veya doas bulunamadı",
            )
            .to_owned(),
            Self::MustNotBeRoot => t(
                "this package manager must not run as root; start Skadi as a regular user",
                "bu paket yöneticisi root olarak çalıştırılamaz; Skadi'yi normal kullanıcıyla başlatın",
            )
            .to_owned(),
            Self::NoSystemManager => t(
                "the system's own package manager was not found",
                "sistemin kendi paket yöneticisi bulunamadı",
            )
            .to_owned(),
            Self::BootstrapUnavailable(m) => {
                format!(
                    "{m}: {}",
                    t(
                        "automatic installation is not supported",
                        "otomatik kurulum desteklenmiyor"
                    )
                )
            }
            Self::BootstrapIncomplete(m) => format!(
                "{m}: {}",
                t(
                    "installed but not found; reopen the terminal and try again",
                    "kuruldu ancak bulunamadı; terminali yeniden açıp tekrar deneyin",
                )
            ),
            Self::Unsupported(manager) => format!("{manager}: {}", t("not supported here", "burada desteklenmiyor")),
            Self::InvalidOption(option) => format!(
                "{}: {option}",
                t("option not valid here", "bu seçenek burada geçerli değil")
            ),
            Self::UnprotectedExecutable(path) => format!(
                "{path}: {}",
                t(
                    "scheduled updates run Skadi with administrator rights, so it must be installed in a protected location first (Program Files on Windows, a root-owned directory such as /usr/local/bin on Linux)",
                    "zamanlanmış güncelleme Skadi'yi yönetici yetkisiyle çalıştırır; bu yüzden önce korumalı bir konuma kurulmalıdır (Windows'ta Program Files, Linux'ta /usr/local/bin gibi root'a ait bir dizin)"
                )
            ),
            Self::Media(reason) => format!("{}: {reason}", t("cannot process file", "dosya işlenemedi")),
            Self::Profile(reason) => format!("{}: {reason}", t("profile error", "profil hatası")),
            Self::UnknownTweak(id) => format!("{}: {id}", t("no such tweak", "böyle bir tweak yok")),
            Self::UnknownStartupItem(name) => {
                format!("{}: {name}", t("no such startup item", "böyle bir açılış öğesi yok"))
            }
            Self::Signature(detail) => format!("{}: {detail}", t("signature check failed", "imza doğrulanamadı")),
            Self::Policy(detail) => format!(
                "{}: {detail}",
                t("blocked by the organisation policy", "kurum politikası engelledi")
            ),
            Self::WrongPlatform(id) => format!(
                "{id}: {}",
                t(
                    "this tweak is not for this operating system",
                    "bu tweak bu işletim sistemi için değil"
                )
            ),
            Self::NotReversible(what) => format!("{what}: {}", t("cannot be undone", "geri alınamaz")),
            Self::NothingToRevert(id) => format!(
                "{id}: {}",
                t(
                    "no saved state; it was not applied by Skadi",
                    "kayıtlı durum yok; Skadi tarafından uygulanmamış"
                )
            ),
            Self::Registry(reason) => format!("{}: {reason}", t("registry error", "registry hatası")),
            Self::State(reason) => format!("{}: {reason}", t("state file error", "durum kaydı hatası")),
            Self::Spawn { program, source } => {
                format!(
                    "{} ({program}): {source}",
                    t("failed to start command", "komut başlatılamadı")
                )
            }
            Self::CommandFailed { program, code, detail } => format!(
                "{} ({program}), {}: {}{}",
                t("command failed", "komut başarısız oldu"),
                t("exit code", "çıkış kodu"),
                code.map_or_else(|| "-".to_owned(), |c| c.to_string()),
                detail.as_deref().map(|d| format!(" - {d}")).unwrap_or_default()
            ),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(&self.message(Lang::En))
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Spawn { source, .. } => Some(source),
            _ => None,
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;
