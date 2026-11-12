//! Dil desteği (Türkçe / İngilizce).
//!
//! Metinler derleme anında sabit çiftler olarak tutulur; eksik çeviri derlenmez. Yeni bir dil
//! eklemek için `Lang`'a varyant ve `Text`'e alan eklenir; derleyici eksik kalan her yeri gösterir.

use std::{env, fmt, str::FromStr};

use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Lang {
    #[default]
    En,
    Tr,
}

impl Lang {
    pub const ALL: [Self; 2] = [Self::En, Self::Tr];

    /// Öncelik: `SKADI_LANG` ortam değişkeni → sistem dili → İngilizce.
    pub fn detect() -> Self {
        env::var("SKADI_LANG")
            .ok()
            .and_then(|tag| Self::from_tag(&tag))
            .or_else(|| sys_locale::get_locale().and_then(|tag| Self::from_tag(&tag)))
            .unwrap_or_default()
    }

    /// `tr`, `tr-TR`, `tr_TR.UTF-8` gibi etiketleri tanır.
    pub fn from_tag(tag: &str) -> Option<Self> {
        let primary = tag.split(['-', '_', '.', '@']).next()?.to_ascii_lowercase();
        match primary.as_str() {
            "tr" => Some(Self::Tr),
            "en" => Some(Self::En),
            _ => None,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::Tr => "tr",
        }
    }

    /// İki dildeki sabit metinden bu dile ait olanı seçer.
    pub const fn pick<'a>(self, en: &'a str, tr: &'a str) -> &'a str {
        match self {
            Self::En => en,
            Self::Tr => tr,
        }
    }
}

impl fmt::Display for Lang {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(self.as_str())
    }
}

impl FromStr for Lang {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_tag(s).ok_or_else(|| format!("unsupported language / desteklenmeyen dil: {s}"))
    }
}

/// Katalog gibi veri dosyalarındaki çevrilmiş metin. İki dil de zorunludur.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Text {
    pub en: String,
    pub tr: String,
}

impl Text {
    pub fn get(&self, lang: Lang) -> &str {
        lang.pick(&self.en, &self.tr)
    }

    pub fn all(&self) -> [&str; 2] {
        [&self.en, &self.tr]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_locale_tags() {
        assert_eq!(Lang::from_tag("tr_TR.UTF-8"), Some(Lang::Tr));
        assert_eq!(Lang::from_tag("tr-TR"), Some(Lang::Tr));
        assert_eq!(Lang::from_tag("EN-us"), Some(Lang::En));
        assert_eq!(Lang::from_tag("de-DE"), None);
        assert_eq!(Lang::from_tag(""), None);
    }
}
