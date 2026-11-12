//! Uygulama kataloğu. Uygulamalar Rust kodunda değil, `catalog/apps.toml` dosyasında tanımlanır;
//! katkıda bulunanlar Rust bilmeden uygulama ekleyebilir. Dosya derleme anında ikiliye gömülür,
//! böylece çalışma anında dışarıdan değiştirilemez.

use std::{collections::BTreeMap, collections::HashSet, fmt, str::FromStr};

use serde::Deserialize;

use crate::{
    error::{Error, Result},
    i18n::{Lang, Text},
    manager::{ManagerId, validate_package_id},
};

const BUILTIN: &str = include_str!("../../../catalog/apps.toml");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Category {
    Browsers,
    Communication,
    Development,
    Multimedia,
    Office,
    Security,
    Utilities,
    Gaming,
    System,
    SelfHosted,
}

impl Category {
    pub const ALL: [Self; 10] = [
        Self::Browsers,
        Self::Communication,
        Self::Development,
        Self::Multimedia,
        Self::Office,
        Self::Security,
        Self::Utilities,
        Self::Gaming,
        Self::System,
        Self::SelfHosted,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Browsers => "browsers",
            Self::Communication => "communication",
            Self::Development => "development",
            Self::Multimedia => "multimedia",
            Self::Office => "office",
            Self::Security => "security",
            Self::Utilities => "utilities",
            Self::Gaming => "gaming",
            Self::System => "system",
            Self::SelfHosted => "self-hosted",
        }
    }

    pub const fn label(self, lang: Lang) -> &'static str {
        match self {
            Self::Browsers => lang.pick("Browsers", "Tarayıcılar"),
            Self::Communication => lang.pick("Communication", "İletişim"),
            Self::Development => lang.pick("Development", "Geliştirme"),
            Self::Multimedia => lang.pick("Multimedia", "Multimedya"),
            Self::Office => lang.pick("Office", "Ofis"),
            Self::Security => lang.pick("Security & Privacy", "Güvenlik ve Gizlilik"),
            Self::Utilities => lang.pick("Utilities", "Araçlar"),
            Self::Gaming => lang.pick("Gaming", "Oyun"),
            Self::System => lang.pick("System tools & runtimes", "Sistem araçları ve çalışma ortamları"),
            Self::SelfHosted => lang.pick("Self-hosted", "Kendi sunucunda barındır"),
        }
    }
}

impl fmt::Display for Category {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(self.as_str())
    }
}

impl FromStr for Category {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        Self::ALL
            .into_iter()
            .find(|c| c.as_str().eq_ignore_ascii_case(s))
            .ok_or_else(|| Error::UnknownCategory(s.to_owned()))
    }
}

const NOTES: &str = include_str!("../../../catalog/notes.toml");

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommunityNote {
    pub app: String,
    pub text: Text,
    pub source: String,
}

#[derive(Deserialize)]
struct NotesFile {
    note: Vec<CommunityNote>,
}

/// Topluluk notları (`catalog/notes.toml`); her notun resmi bir HTTPS kaynağı vardır.
pub fn community_notes() -> Result<Vec<CommunityNote>> {
    let file: NotesFile = toml::from_str(NOTES).map_err(|e| Error::Catalog(format!("notes.toml: {e}")))?;
    if let Some(bad) = file
        .note
        .iter()
        .find(|n| !n.source.starts_with("https://") || n.source.chars().any(char::is_whitespace))
    {
        return Err(Error::Catalog(format!("notes.toml: {} needs an https source", bad.app)));
    }
    Ok(file.note)
}

/// Katalog verisinden türetilen etiketler (elle tutulan etiket listesi eskimesin diye).
pub fn tags(app: &App) -> Vec<&'static str> {
    use crate::platform::Os;
    let mut tags = Vec::new();
    match app.foss {
        Some(true) => tags.push("open-source"),
        Some(false) => tags.push("proprietary"),
        None => {}
    }
    match app.telemetry {
        Some(true) => tags.push("telemetry"),
        Some(false) => tags.push("no-telemetry"),
        None => {}
    }
    let mut systems = Vec::new();
    for (os, tag) in [(Os::Windows, "windows"), (Os::Linux, "linux"), (Os::Macos, "macos")] {
        if app.packages.keys().any(|m| m.spec().os == os) {
            systems.push(tag);
        }
    }
    if systems.len() == 3 {
        tags.push("cross-platform");
    }
    tags.extend(systems);
    if app.packages.contains_key(&crate::manager::ManagerId::Flatpak) {
        tags.push("flatpak");
    }
    tags
}

/// Uygulamanın ekran görüntülerinin bulunduğu sayfa: Flathub sayfası, yoksa resmi site.
pub fn screenshots_url(app: &App) -> Option<String> {
    app.packages
        .get(&crate::manager::ManagerId::Flatpak)
        .map(|id| format!("https://flathub.org/apps/{id}"))
        .or_else(|| app.homepage.clone())
}

/// Gömülü katalogların sürümü (`catalog/VERSION`, YYYY.AA.GG); katalog her değiştiğinde güncellenir.
pub fn catalog_version() -> &'static str {
    include_str!("../../../catalog/VERSION").trim()
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct App {
    pub id: String,
    pub name: String,
    pub category: Category,
    pub description: Text,
    #[serde(default)]
    pub homepage: Option<String>,
    /// Özgür/açık kaynak yazılım mı? Bilinmiyorsa belirtilmez.
    #[serde(default)]
    pub foss: Option<bool>,
    /// Uygulama varsayılan olarak kullanım verisi gönderiyor mu? Bilinmiyorsa belirtilmez.
    #[serde(default)]
    pub telemetry: Option<bool>,
    /// `telemetry` bilgisinin dayandığı resmi sayfa (gizlilik politikası ya da belgeler).
    #[serde(default)]
    pub privacy_source: Option<String>,
    /// Gizlilik dostu alternatiflerin kimlikleri.
    #[serde(default)]
    pub alternatives: Vec<String>,
    /// Birlikte kurulmaması gereken uygulamaların kimlikleri.
    #[serde(default)]
    pub conflicts: Vec<String>,
    /// Kısa gizlilik notu.
    #[serde(default)]
    pub note: Option<Text>,
    /// Paket yöneticisi → o yöneticideki paket kimliği.
    pub packages: BTreeMap<ManagerId, String>,
}

#[derive(Debug, Clone)]
pub struct Catalog {
    apps: Vec<App>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCatalog {
    #[serde(rename = "app", default)]
    apps: Vec<App>,
}

impl Catalog {
    pub fn builtin() -> Result<Self> {
        Self::from_toml(BUILTIN)
    }

    pub fn from_toml(source: &str) -> Result<Self> {
        let raw: RawCatalog = toml::from_str(source).map_err(|e| Error::Catalog(e.to_string()))?;
        validate(&raw.apps)?;
        Ok(Self { apps: raw.apps })
    }

    pub fn apps(&self) -> &[App] {
        &self.apps
    }

    /// Yalnızca koşulu sağlayan uygulamaları tutar (ör. etiket filtresi).
    pub fn retain(&mut self, keep: impl Fn(&App) -> bool) {
        self.apps.retain(|app| keep(app));
    }

    pub fn get(&self, id: &str) -> Option<&App> {
        self.apps.iter().find(|app| app.id == id)
    }
}

fn validate(apps: &[App]) -> Result<()> {
    let all: HashSet<&str> = apps.iter().map(|a| a.id.as_str()).collect();
    for app in apps {
        if let Some(source) = &app.privacy_source
            && (!source.starts_with("https://") || source.chars().any(char::is_whitespace))
        {
            return Err(Error::Catalog(format!(
                "{}: privacy_source must be an https URL",
                app.id
            )));
        }
        let references = app.alternatives.iter().chain(&app.conflicts);
        if let Some(bad) = references
            .clone()
            .find(|id| !all.contains(id.as_str()) || **id == app.id)
        {
            return Err(Error::Catalog(format!("{}: unknown or self reference {bad}", app.id)));
        }
        if app
            .note
            .as_ref()
            .is_some_and(|n| n.all().iter().any(|t| t.trim().is_empty() || t.len() > 300))
        {
            return Err(Error::Catalog(format!(
                "{}: note must be 1-300 characters in every language",
                app.id
            )));
        }
    }
    let mut seen = HashSet::new();
    for app in apps {
        let fail = |reason: &str| Err(Error::Catalog(format!("{}: {reason}", app.id)));
        let id_ok = !app.id.is_empty()
            && app.id.len() <= 64
            && app
                .id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        if !id_ok {
            return fail("id may only contain lowercase letters, digits and '-'");
        }
        if !seen.insert(app.id.as_str()) {
            return fail("id is defined more than once");
        }
        if app.name.trim().is_empty() || app.name.len() > 64 {
            return fail("name is empty or too long");
        }
        if app
            .description
            .all()
            .iter()
            .any(|d| d.trim().is_empty() || d.len() > 200)
        {
            return fail("description must be 1-200 characters in every language");
        }
        if app.homepage.as_deref().is_some_and(|url| !url.starts_with("https://")) {
            return fail("homepage must start with https://");
        }
        if app.packages.is_empty() {
            return fail("at least one package is required");
        }
        for package in app.packages.values() {
            validate_package_id(package)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_catalog_is_valid() {
        let catalog = Catalog::builtin().expect("gömülü katalog geçerli olmalı");
        assert!(!catalog.apps().is_empty());
        assert!(catalog.get("git").is_some());
    }

    #[test]
    fn parses_manager_keys() {
        let catalog = Catalog::from_toml(
            r#"
            [[app]]
            id = "demo"
            name = "Demo"
            category = "utilities"
            description = { en = "Demo", tr = "Deneme" }
            [app.packages]
            winget = "Demo.Demo"
            brew-cask = "demo"
            "#,
        )
        .unwrap();
        let app = catalog.get("demo").unwrap();
        assert_eq!(app.packages[&ManagerId::BrewCask], "demo");
        assert_eq!(app.packages[&ManagerId::Winget], "Demo.Demo");
    }

    #[test]
    fn rejects_bad_entries() {
        let entry = |id: &str, manager: &str, package: &str| {
            format!(
                "[[app]]\nid = \"{id}\"\nname = \"X\"\ncategory = \"utilities\"\ndescription = {{ en = \"x\", tr = \"x\" }}\n[app.packages]\n{manager} = \"{package}\"\n"
            )
        };
        assert!(Catalog::from_toml(&entry("Kötü", "apt", "x")).is_err());
        assert!(Catalog::from_toml(&entry("ok", "apt", "--force")).is_err());
        assert!(Catalog::from_toml(&entry("ok", "npm", "x")).is_err());
        let duplicate = entry("ok", "apt", "x") + &entry("ok", "apt", "y");
        assert!(Catalog::from_toml(&duplicate).is_err());
    }

    #[test]
    fn notes_tags_and_screenshots() {
        let catalog = Catalog::builtin().unwrap();
        for note in community_notes().unwrap() {
            assert!(catalog.get(&note.app).is_some(), "note for unknown app {}", note.app);
        }
        let firefox = catalog.get("firefox").unwrap();
        let tags = tags(firefox);
        assert!(tags.contains(&"open-source") && tags.contains(&"telemetry") && tags.contains(&"cross-platform"));
        assert!(
            screenshots_url(firefox)
                .unwrap()
                .starts_with("https://flathub.org/apps/")
        );
    }
}
