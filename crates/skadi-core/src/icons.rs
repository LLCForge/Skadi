//! Uygulama simgeleri için yerel önbellek. Simgeler yalnızca kullanıcı `skadi icons fetch`
//! çalıştırdığında, her uygulamanın kendi resmi sitesinden bir kez indirilir; arayüz hiçbir zaman
//! kendiliğinden ağa çıkmaz.

use std::path::PathBuf;

use crate::platform::Os;

pub const MAX_ICON_BYTES: usize = 512 * 1024;

pub fn icon_dir(os: Os) -> Option<PathBuf> {
    crate::paths::data_dir(os).map(|dir| dir.join("icons"))
}

/// Önbellekteki simge dosyası; yoksa `None`.
pub fn icon_path(os: Os, app_id: &str) -> Option<PathBuf> {
    let dir = icon_dir(os)?;
    ["png", "ico"]
        .iter()
        .map(|ext| dir.join(format!("{app_id}.{ext}")))
        .find(|p| p.is_file())
}

/// Bayt dizisi PNG ya da ICO ise uzantısı. Başka bir şey (HTML hata sayfası vb.) kaydedilmez.
pub fn image_kind(bytes: &[u8]) -> Option<&'static str> {
    if bytes.len() > MAX_ICON_BYTES || bytes.len() < 8 {
        None
    } else if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        Some("png")
    } else if bytes.starts_with(&[0, 0, 1, 0]) {
        Some("ico")
    } else {
        None
    }
}

/// Resmi sitenin kökündeki `favicon.ico` adresi; yalnızca HTTPS.
pub fn favicon_url(homepage: &str) -> Option<String> {
    let rest = homepage.strip_prefix("https://")?;
    let host = rest.split('/').next()?;
    let ok = !host.is_empty()
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-'));
    ok.then(|| format!("https://{host}/favicon.ico"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_images_and_urls() {
        assert_eq!(
            image_kind(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0]),
            Some("png")
        );
        assert_eq!(image_kind(&[0, 0, 1, 0, 1, 0, 16, 16]), Some("ico"));
        assert_eq!(image_kind(b"<!doctype html>"), None);
        assert_eq!(
            favicon_url("https://www.mozilla.org/firefox/").as_deref(),
            Some("https://www.mozilla.org/favicon.ico")
        );
        assert!(favicon_url("http://example.com").is_none());
        assert!(favicon_url("https://ex ample.com").is_none());
    }
}
