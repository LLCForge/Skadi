//! Sistem sadeleştirme (debloat) ve gizlilik ayarları.
//!
//! Tweak'ler `tweaks/*.toml` dosyalarında veri olarak tanımlanır ve ikiliye gömülür. Bir tweak,
//! **tipli** eylemlerden oluşur (registry, servis, zamanlanmış görev, AppX, systemd, paket,
//! gsettings, defaults). Serbest betik çalıştırma bilinçli olarak desteklenmez: her eylem
//! okunabilir, doğrulanabilir ve geri alınabilir olmalıdır.

mod action;
pub mod config;
mod engine;
mod state;

use std::collections::{BTreeMap, HashSet};

use serde::Deserialize;

pub use action::{Observed, Previous};
pub use engine::{ApplyOutcome, RevertOutcome, TweakEngine, TweakStatus};
pub use state::StateStore;

use crate::{
    error::{Error, Result},
    i18n::{Lang, Text},
    manager::{ManagerId, validate_package_id},
    platform::Os,
};

const BUILTIN: [(&str, &str); 3] = [
    ("windows.toml", include_str!("../../../../tweaks/windows.toml")),
    ("linux.toml", include_str!("../../../../tweaks/linux.toml")),
    ("macos.toml", include_str!("../../../../tweaks/macos.toml")),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TweakCategory {
    Privacy,
    Bloat,
    Interface,
    System,
    Security,
}

impl TweakCategory {
    pub const ALL: [Self; 5] = [
        Self::Privacy,
        Self::Security,
        Self::Bloat,
        Self::Interface,
        Self::System,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Privacy => "privacy",
            Self::Bloat => "bloat",
            Self::Interface => "interface",
            Self::System => "system",
            Self::Security => "security",
        }
    }

    pub const fn label(self, lang: Lang) -> &'static str {
        match self {
            Self::Privacy => lang.pick("Privacy & Telemetry", "Gizlilik ve Telemetri"),
            Self::Bloat => lang.pick("Bloatware", "Gereksiz Yazılımlar"),
            Self::Interface => lang.pick("Interface", "Arayüz"),
            Self::System => lang.pick("System", "Sistem"),
            Self::Security => lang.pick("Security", "Güvenlik"),
        }
    }
}

impl std::str::FromStr for TweakCategory {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        Self::ALL
            .into_iter()
            .find(|c| c.as_str().eq_ignore_ascii_case(s))
            .ok_or_else(|| Error::UnknownCategory(s.to_owned()))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Risk {
    /// Varsayılan olarak önerilebilir.
    Safe,
    /// Bazı kullanıcıların ihtiyaç duyduğu bir özelliği kapatabilir.
    Moderate,
    /// Ne yaptığını bilen kullanıcılar için.
    Advanced,
}

impl Risk {
    pub const fn label(self, lang: Lang) -> &'static str {
        match self {
            Self::Safe => lang.pick("safe", "güvenli"),
            Self::Moderate => lang.pick("moderate", "orta"),
            Self::Advanced => lang.pick("advanced", "ileri"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RegKind {
    Dword,
    Qword,
    String,
}

impl RegKind {
    pub const fn reg_type(self) -> &'static str {
        match self {
            Self::Dword => "REG_DWORD",
            Self::Qword => "REG_QWORD",
            Self::String => "REG_SZ",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ServiceStart {
    Automatic,
    Manual,
    Disabled,
}

impl ServiceStart {
    /// `HKLM\SYSTEM\CurrentControlSet\Services\<ad>\Start` değeri.
    pub const fn registry_value(self) -> u32 {
        match self {
            Self::Automatic => 2,
            Self::Manual => 3,
            Self::Disabled => 4,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SystemdMode {
    #[default]
    Disable,
    Mask,
    /// Birimi etkinleştirir (ör. otomatik güvenlik güncellemesi zamanlayıcısı).
    Enable,
}

/// Yönetilen dosya yazıldıktan sonra değişikliği etkinleştiren adım.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Reload {
    #[default]
    None,
    Sysctl,
    NetworkManager,
    /// `sshd -t` ile doğrular; geçersizse dosyayı hemen geri alır.
    Sshd,
}

impl Reload {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Sysctl => "sysctl",
            Self::NetworkManager => "network-manager",
            Self::Sshd => "sshd",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        [Self::None, Self::Sysctl, Self::NetworkManager, Self::Sshd]
            .into_iter()
            .find(|r| r.as_str() == text)
    }
}

/// Yazmadan önce denetlenen güvenlik koşulu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Guard {
    #[default]
    None,
    /// Parola girişini kapatmadan önce kullanıcının bir SSH anahtarı olmalı (dışarıda kalmayı önler).
    AuthorizedKeys,
    /// Skadi sudo/doas/pkexec ile normal bir kullanıcıdan çalıştırılmalı (root girişi kapatılınca
    /// o kullanıcı hâlâ bağlanabilir).
    SudoUser,
}

impl Guard {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::AuthorizedKeys => "authorized-keys",
            Self::SudoUser => "sudo-user",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        [Self::None, Self::AuthorizedKeys, Self::SudoUser]
            .into_iter()
            .find(|g| g.as_str() == text)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DefaultsKind {
    Bool,
    Int,
    String,
}

impl DefaultsKind {
    pub const fn flag(self) -> &'static str {
        match self {
            Self::Bool => "-bool",
            Self::Int => "-int",
            Self::String => "-string",
        }
    }
}

/// Tek bir sistem değişikliği.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Action {
    /// Windows registry değeri. `key` `HKLM\`, `HKCU\` ya da `HKU\` ile başlar.
    Registry {
        key: String,
        name: String,
        kind: RegKind,
        data: String,
    },
    /// Windows servisinin başlangıç türü.
    Service { name: String, start: ServiceStart },
    /// Windows zamanlanmış görevini devre dışı bırakır. Örn. `\Microsoft\Windows\X\Y`.
    ScheduledTask { task: String },
    /// Windows hazır uygulamasını (AppX) kaldırır. **Geri alınamaz.**
    Appx { package: String },
    /// Linux systemd birimini devre dışı bırakır ya da maskeler.
    Systemd {
        unit: String,
        #[serde(default)]
        mode: SystemdMode,
    },
    /// Linux'ta dağıtım paketini kaldırır; geri alma yeniden kurar.
    Package { packages: BTreeMap<ManagerId, String> },
    /// GNOME ayarı.
    Gsettings { schema: String, key: String, value: String },
    /// Linux yapılandırma dosyasında `anahtar=değer` satırı (yalnızca `/etc` altı).
    ConfigLine {
        path: String,
        #[serde(default)]
        section: Option<String>,
        key: String,
        value: String,
    },
    /// Skadi'ye ait, içeriği katalogda sabit bir yapılandırma dosyası (yalnızca izinli `/etc`
    /// klasörlerinde ve adında `skadi` geçen dosyalar). Geri alma önceki içeriği geri yazar ya da
    /// dosyayı siler.
    ManagedFile {
        path: String,
        content: String,
        #[serde(default)]
        reload: Reload,
        /// Bu yollardan hiçbiri yoksa tweak bu sistemde uygulanamaz (ör. tarayıcı kurulu değil).
        #[serde(default)]
        requires: Vec<String>,
        #[serde(default)]
        guard: Guard,
    },
    /// KDE ayarı (`kwriteconfig6` ya da `kwriteconfig5`).
    Kconfig {
        file: String,
        group: String,
        key: String,
        value: String,
    },
    /// Windows isteğe bağlı özelliğini etkinleştirir (WSL, Hyper-V, .NET 3.5 ...).
    WindowsFeature { name: String },
    /// macOS `defaults` ayarı. `system = true` ise alan mutlak bir plist yoludur ve root gerekir.
    Defaults {
        domain: String,
        key: String,
        kind: DefaultsKind,
        value: String,
        #[serde(default)]
        system: bool,
    },
}

impl Action {
    pub const fn os(&self) -> Os {
        match self {
            Self::Registry { .. }
            | Self::Service { .. }
            | Self::ScheduledTask { .. }
            | Self::Appx { .. }
            | Self::WindowsFeature { .. } => Os::Windows,
            Self::Systemd { .. }
            | Self::Package { .. }
            | Self::Gsettings { .. }
            | Self::ConfigLine { .. }
            | Self::ManagedFile { .. }
            | Self::Kconfig { .. } => Os::Linux,
            Self::Defaults { .. } => Os::Macos,
        }
    }

    pub const fn is_reversible(&self) -> bool {
        !matches!(self, Self::Appx { .. })
    }

    /// Eylem yönetici/root yetkisi gerektiriyor mu? Arayüzler işleri buna göre yetkili ve
    /// yetkisiz alt süreçlere böler.
    pub fn needs_root(&self) -> bool {
        match self {
            Self::Registry { key, .. } => !key.starts_with("HKCU\\"),
            Self::Service { .. } | Self::ScheduledTask { .. } | Self::Appx { .. } => true,
            Self::Systemd { .. }
            | Self::Package { .. }
            | Self::ConfigLine { .. }
            | Self::ManagedFile { .. }
            | Self::WindowsFeature { .. } => true,
            Self::Gsettings { .. } | Self::Kconfig { .. } => false,
            Self::Defaults { system, .. } => *system,
        }
    }

    /// Durum kaydında eylemi tanımlayan kararlı anahtar. Katalogdaki sıra değişse de geri
    /// alma doğru eylemi bulur.
    pub fn key(&self) -> String {
        match self {
            Self::Registry { key, name, .. } => format!("registry:{key}\\{name}"),
            Self::Service { name, .. } => format!("service:{name}"),
            Self::ScheduledTask { task } => format!("task:{task}"),
            Self::Appx { package } => format!("appx:{package}"),
            Self::Systemd { unit, .. } => format!("systemd:{unit}"),
            Self::Package { packages } => {
                let ids: Vec<String> = packages.iter().map(|(m, p)| format!("{m}={p}")).collect();
                format!("package:{}", ids.join(","))
            }
            Self::Gsettings { schema, key, .. } => format!("gsettings:{schema}:{key}"),
            Self::ConfigLine { path, section, key, .. } => {
                format!("config:{path}:{}:{key}", section.as_deref().unwrap_or(""))
            }
            Self::ManagedFile { path, .. } => format!("file:{path}"),
            Self::WindowsFeature { name } => format!("feature:{name}"),
            Self::Kconfig { file, group, key, .. } => format!("kconfig:{file}:{group}:{key}"),
            Self::Defaults { domain, key, .. } => format!("defaults:{domain}:{key}"),
        }
    }

    fn validate(&self) -> std::result::Result<(), String> {
        match self {
            Self::Registry { key, name, kind, data } => {
                let hive_ok = ["HKLM\\", "HKCU\\", "HKU\\"].iter().any(|h| key.starts_with(h));
                check(hive_ok && is_reg_text(key, 255), "registry key")?;
                // Boş ad, anahtarın varsayılan değeridir.
                check(name.is_empty() || is_reg_text(name, 255), "registry value name")?;
                check(is_valid_reg_data(*kind, data), "registry data")
            }
            Self::Service { name, .. } => check(
                !name.is_empty()
                    && name.len() <= 256
                    && name.chars().all(|c| c.is_ascii_alphanumeric() || "_-.".contains(c)),
                "service name",
            ),
            Self::ScheduledTask { task } => check(
                task.starts_with('\\')
                    && !task.ends_with('\\')
                    && task.len() <= 260
                    && task
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || " \\._-()".contains(c)),
                "scheduled task path",
            ),
            Self::Appx { package } => check(
                !package.is_empty()
                    && package.len() <= 128
                    && package.chars().all(|c| c.is_ascii_alphanumeric() || c == '.'),
                "appx package",
            ),
            Self::Systemd { unit, .. } => {
                let suffix_ok = [".service", ".timer", ".socket", ".path"]
                    .iter()
                    .any(|s| unit.ends_with(s));
                check(suffix_ok && is_token(unit, "@._:-"), "systemd unit")
            }
            Self::Package { packages } => {
                check(!packages.is_empty(), "package list")?;
                for (manager, package) in packages {
                    let spec = manager.spec();
                    check(
                        spec.system && spec.os == Os::Linux,
                        "package manager (must be a Linux system manager)",
                    )?;
                    validate_package_id(package).map_err(|_| "package id".to_owned())?;
                }
                Ok(())
            }
            Self::ConfigLine {
                path,
                section,
                key,
                value,
            } => {
                check(config::is_allowed_path(path), "config path (must be under /etc)")?;
                check(section.as_deref().is_none_or(|s| is_token(s, "._-")), "config section")?;
                check(is_token(key, "._-"), "config key")?;
                check(is_plain_value(value) && !value.contains('#'), "config value")
            }
            Self::ManagedFile {
                path,
                content,
                requires,
                ..
            } => {
                check(config::is_managed_path(path), "managed file path")?;
                check(config::is_managed_content(content), "managed file content")?;
                check(
                    requires.len() <= 8 && requires.iter().all(|r| r.starts_with('/') && is_token(r, "/._-")),
                    "managed file requirement",
                )
            }
            Self::WindowsFeature { name } => check(is_token(name, ".-"), "windows feature name"),
            Self::Kconfig {
                file,
                group,
                key,
                value,
            } => {
                check(is_token(file, "._-"), "kconfig file")?;
                check(
                    is_plain_value(group) && !group.contains(['[', ']']) && !group.is_empty(),
                    "kconfig group",
                )?;
                check(is_token(key, "._-"), "kconfig key")?;
                check(is_plain_value(value), "kconfig value")
            }
            Self::Gsettings { schema, key, value } => {
                check(is_token(schema, ".-"), "gsettings schema")?;
                check(is_token(key, "-"), "gsettings key")?;
                check(is_plain_value(value), "gsettings value")
            }
            Self::Defaults {
                domain,
                key,
                kind,
                value,
                system,
            } => {
                let extra = if *system { "._-/ " } else { "._-" };
                let domain_ok = is_token(domain, extra) && (!*system || domain.starts_with('/'));
                check(domain_ok, "defaults domain")?;
                check(is_plain_value(key) && key.len() <= 128, "defaults key")?;
                let value_ok = match kind {
                    DefaultsKind::Bool => value == "true" || value == "false",
                    DefaultsKind::Int => value.parse::<i64>().is_ok(),
                    DefaultsKind::String => is_plain_value(value),
                };
                check(value_ok, "defaults value")
            }
        }
    }
}

fn check(ok: bool, what: &str) -> std::result::Result<(), String> {
    if ok { Ok(()) } else { Err(format!("invalid {what}")) }
}

/// Harf/rakam ve izin verilen ek karakterlerden oluşan, `-` ile başlamayan kısa metin.
fn is_token(value: &str, extra: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && !value.starts_with('-')
        && value.chars().all(|c| c.is_ascii_alphanumeric() || extra.contains(c))
}

/// Kontrol karakteri içermeyen, makul uzunlukta değer. Komutlar kabuksuz çalıştığı için
/// yalnızca seçenek gibi görünmesi (`-` ile başlaması) ve kontrol karakterleri engellenir.
pub(crate) fn is_plain_value(value: &str) -> bool {
    value.len() <= 1024 && !value.starts_with('-') && !value.chars().any(char::is_control)
}

fn is_reg_text(value: &str, max: usize) -> bool {
    !value.is_empty() && value.len() <= max && !value.contains('"') && !value.chars().any(char::is_control)
}

pub(crate) fn is_valid_reg_data(kind: RegKind, data: &str) -> bool {
    match kind {
        RegKind::Dword => data.parse::<u32>().is_ok(),
        RegKind::Qword => data.parse::<u64>().is_ok(),
        RegKind::String => is_plain_value(data),
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tweak {
    pub id: String,
    pub name: Text,
    pub description: Text,
    pub category: TweakCategory,
    pub risk: Risk,
    /// Yalnızca bu dağıtım ailelerinde gösterilir (Linux). Boşsa her yerde.
    #[serde(default)]
    pub distros: Vec<String>,
    /// Ayarı açıklayan resmî belge (https).
    #[serde(default)]
    pub source: Option<String>,
    /// Windows Home'da çalışmaz (ör. Grup İlkesi tabanlı Windows Update ayarları).
    #[serde(default)]
    pub requires_pro: bool,
    /// Gereken en düşük Windows yapı numarası (ör. Windows 11 için 22000).
    #[serde(default)]
    pub windows_min_build: Option<u32>,
    /// Yalnızca bu dağıtım sürümlerinde (`VERSION_ID`) gösterilir. Boşsa her sürümde.
    #[serde(default)]
    pub distro_versions: Vec<String>,
    #[serde(rename = "action")]
    pub actions: Vec<Action>,
}

impl Tweak {
    pub fn os(&self) -> Option<Os> {
        self.actions.first().map(Action::os)
    }

    pub fn is_reversible(&self) -> bool {
        self.actions.iter().all(Action::is_reversible)
    }

    pub fn needs_root(&self) -> bool {
        self.actions.iter().any(Action::needs_root)
    }

    fn fits(&self, system: &crate::system::System) -> bool {
        let distro = system.distro.as_ref();
        let family_ok = self.distros.is_empty() || distro.is_some_and(|d| self.distros.iter().any(|f| d.is_like(f)));
        let version_ok = self.distro_versions.is_empty()
            || distro
                .and_then(|d| d.version_id.as_deref())
                .is_some_and(|v| self.distro_versions.iter().any(|want| want == v));
        // Windows bilgisi okunamadıysa tweak gizlenmez; uygulama sırasında komut yine denetlenir.
        let windows_ok = system.windows.as_ref().is_none_or(|w| {
            (!self.requires_pro || !w.is_home()) && self.windows_min_build.is_none_or(|min| w.build >= min)
        });
        family_ok && version_ok && windows_ok
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTweaks {
    #[serde(rename = "tweak", default)]
    tweaks: Vec<Tweak>,
}

#[derive(Debug, Clone)]
pub struct TweakCatalog {
    tweaks: Vec<Tweak>,
}

impl TweakCatalog {
    pub fn builtin() -> Result<Self> {
        let mut tweaks = Vec::new();
        for (file, source) in BUILTIN {
            let raw: RawTweaks = toml::from_str(source).map_err(|e| Error::Catalog(format!("{file}: {e}")))?;
            tweaks.extend(raw.tweaks);
        }
        validate(&tweaks)?;
        Ok(Self { tweaks })
    }

    pub fn from_toml(source: &str) -> Result<Self> {
        let raw: RawTweaks = toml::from_str(source).map_err(|e| Error::Catalog(e.to_string()))?;
        validate(&raw.tweaks)?;
        Ok(Self { tweaks: raw.tweaks })
    }

    pub fn tweaks(&self) -> &[Tweak] {
        &self.tweaks
    }

    pub fn get(&self, id: &str) -> Option<&Tweak> {
        self.tweaks.iter().find(|t| t.id == id)
    }

    /// Bu işletim sisteminde (ve Linux'ta bu dağıtımda) anlamlı olan tweak'ler.
    /// Bu sistemde anlamlı olan tweak'ler: işletim sistemi, dağıtım ailesi ve sürümü, Windows
    /// yapı numarası ve sürümü (Home/Pro) dikkate alınır.
    pub fn for_host<'a>(&'a self, system: &'a crate::system::System) -> impl Iterator<Item = &'a Tweak> {
        self.tweaks
            .iter()
            .filter(move |t| t.os() == Some(system.os) && t.fits(system))
    }
}

fn validate(tweaks: &[Tweak]) -> Result<()> {
    let mut ids = HashSet::new();
    // Aynı ayara dokunan eylemler: anahtar -> (tweak, eylemin tamamı).
    let mut targets: std::collections::HashMap<String, (&str, &Action)> = std::collections::HashMap::new();
    for tweak in tweaks {
        let fail = |reason: String| Err(Error::Catalog(format!("{}: {reason}", tweak.id)));
        let id_ok = !tweak.id.is_empty()
            && tweak.id.len() <= 64
            && tweak
                .id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        if !id_ok || !ids.insert(tweak.id.as_str()) {
            return fail("id must be unique and contain only lowercase letters, digits and '-'".into());
        }
        if tweak.name.all().iter().any(|n| n.trim().is_empty() || n.len() > 80) {
            return fail("name must be 1-80 characters in every language".into());
        }
        if tweak
            .description
            .all()
            .iter()
            .any(|d| d.trim().is_empty() || d.len() > 300)
        {
            return fail("description must be 1-300 characters in every language".into());
        }
        let Some(os) = tweak.os() else {
            return fail("at least one action is required".into());
        };
        let mut keys = HashSet::new();
        for action in &tweak.actions {
            if action.os() != os {
                return fail("all actions of a tweak must target the same OS".into());
            }
            if !keys.insert(action.key()) {
                return fail(format!("duplicate action {}", action.key()));
            }
            if let Err(reason) = action.validate() {
                return fail(format!("{reason} in {}", action.key()));
            }
        }
        if tweak.distros.iter().any(|d| !is_token(d, "-")) {
            return fail("invalid distro family".into());
        }
        if tweak
            .source
            .as_deref()
            .is_some_and(|url| !url.starts_with("https://") || url.len() > 300)
        {
            return fail("source must be an https link".into());
        }
        // İki tweak aynı ayara farklı değer yazarsa biri diğerini bozar; katalog bunu kabul etmez.
        for action in &tweak.actions {
            match targets.get(&action.key()) {
                Some((other, previous)) if *previous != action => {
                    return fail(format!("conflicts with {other} on {}", action.key()));
                }
                Some(_) => {}
                None => {
                    targets.insert(action.key(), (tweak.id.as_str(), action));
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_tweaks_are_valid() {
        let catalog = TweakCatalog::builtin().expect("gömülü tweak kataloğu geçerli olmalı");
        for os in [Os::Windows, Os::Linux, Os::Macos] {
            assert!(
                catalog.tweaks().iter().any(|t| t.os() == Some(os)),
                "{os} için tweak yok"
            );
        }
    }

    fn one(action: &str) -> Result<TweakCatalog> {
        TweakCatalog::from_toml(&format!(
            "[[tweak]]\nid = \"t\"\ncategory = \"privacy\"\nrisk = \"safe\"\n\
             name = {{ en = \"x\", tr = \"x\" }}\ndescription = {{ en = \"x\", tr = \"x\" }}\n\
             [[tweak.action]]\n{action}\n"
        ))
    }

    /// İnceleme kuralı (CONTRIBUTING.md): her tweak bir HTTPS belge bağlantısı göstermeli.
    #[test]
    fn every_tweak_cites_a_source() {
        let catalog = TweakCatalog::builtin().unwrap();
        for tweak in catalog.tweaks() {
            let source = tweak
                .source
                .as_deref()
                .unwrap_or_else(|| panic!("{}: needs a `source` link to documentation", tweak.id));
            assert!(source.starts_with("https://"), "{}: source must be https", tweak.id);
        }
    }

    #[test]
    fn rejects_conflicting_tweaks() {
        let tweak = |id: &str, data: &str| {
            format!(
                "[[tweak]]\nid = \"{id}\"\ncategory = \"privacy\"\nrisk = \"safe\"\n\
                 name = {{ en = \"x\", tr = \"x\" }}\ndescription = {{ en = \"x\", tr = \"x\" }}\n\
                 [[tweak.action]]\ntype = \"registry\"\nkey = 'HKCU\\X'\nname = \"V\"\nkind = \"dword\"\ndata = \"{data}\"\n"
            )
        };
        assert!(
            TweakCatalog::from_toml(&(tweak("a", "0") + &tweak("b", "0"))).is_ok(),
            "aynı değer çakışma değildir"
        );
        assert!(TweakCatalog::from_toml(&(tweak("a", "0") + &tweak("b", "1"))).is_err());
    }

    #[test]
    fn accepts_valid_actions() {
        for action in [
            "type = \"registry\"\nkey = 'HKLM\\SOFTWARE\\X'\nname = \"V\"\nkind = \"dword\"\ndata = \"0\"",
            "type = \"service\"\nname = \"DiagTrack\"\nstart = \"disabled\"",
            "type = \"scheduled-task\"\ntask = '\\Microsoft\\Windows\\Feedback\\Siuf\\DmClient'",
            "type = \"appx\"\npackage = \"Microsoft.BingNews\"",
            "type = \"systemd\"\nunit = \"apport.service\"",
            "type = \"package\"\npackages = { apt = \"popularity-contest\" }",
            "type = \"gsettings\"\nschema = \"org.gnome.desktop.privacy\"\nkey = \"report-technical-problems\"\nvalue = \"false\"",
            "type = \"defaults\"\ndomain = \"com.apple.AdLib\"\nkey = \"allowApplePersonalizedAdvertising\"\nkind = \"bool\"\nvalue = \"false\"",
        ] {
            assert!(one(action).is_ok(), "{action}");
        }
    }

    #[test]
    fn rejects_hostile_actions() {
        for action in [
            "type = \"registry\"\nkey = 'HKCR\\X'\nname = \"V\"\nkind = \"dword\"\ndata = \"0\"",
            "type = \"registry\"\nkey = 'HKLM\\X'\nname = \"V\"\nkind = \"dword\"\ndata = \"-1\"",
            "type = \"service\"\nname = \"a b\"\nstart = \"disabled\"",
            "type = \"scheduled-task\"\ntask = \"\\\\x'; Remove-Item C:\\\\\"",
            "type = \"appx\"\npackage = \"*\"",
            "type = \"systemd\"\nunit = \"--root=/ x.service\"",
            "type = \"package\"\npackages = { flatpak = \"x\" }",
            "type = \"defaults\"\ndomain = \"x\"\nkey = \"k\"\nkind = \"bool\"\nvalue = \"yes\"",
            "type = \"script\"\nrun = \"rm -rf /\"",
        ] {
            assert!(one(action).is_err(), "{action}");
        }
    }
}
