//! `skadi share`: bir uygulama seçimini başka bir bilgisayarda tek satırla kurulabilecek komutlara
//! ve QR koduna çevirir. Yalnızca katalogdaki kimlikler kabul edilir; komut metnine başka bir şey
//! girmez.

use qrcode::{QrCode, render::unicode};
use skadi_core::{Error, Lang, Result, catalog::Catalog, platform::Os};

const PS1: &str = "https://raw.githubusercontent.com/LLCForge/Skadi/main/install.ps1";
const SH: &str = "https://raw.githubusercontent.com/LLCForge/Skadi/main/install.sh";

/// Windows (PowerShell) ve Linux/macOS için tek satırlık kurulum komutları.
pub fn launch_lines(ids: &[String]) -> (String, String) {
    let list = ids.join(" ");
    (
        format!("& ([scriptblock]::Create((irm {PS1}))) install {list}"),
        format!("curl -fsSL {SH} | sh -s -- install {list}"),
    )
}

/// Koyu terminallerde okunabilen (renkleri ters) Unicode QR kodu.
pub fn qr_text(text: &str) -> Result<String> {
    let code = QrCode::new(text.as_bytes()).map_err(|e| Error::State(format!("QR: {e}")))?;
    Ok(code
        .render::<unicode::Dense1x2>()
        .dark_color(unicode::Dense1x2::Light)
        .light_color(unicode::Dense1x2::Dark)
        .quiet_zone(true)
        .build())
}

/// Arayüzlerde çizim için QR modülleri: genişlik ve satır satır koyu/açık.
pub fn qr_modules(text: &str) -> Option<(usize, Vec<bool>)> {
    let code = QrCode::new(text.as_bytes()).ok()?;
    let dark = code.to_colors().into_iter().map(|c| c == qrcode::Color::Dark).collect();
    Some((code.width(), dark))
}

pub fn share(lang: Lang, apps: &[String]) -> Result<()> {
    let catalog = Catalog::builtin()?;
    if let Some(unknown) = apps.iter().find(|id| catalog.get(id).is_none()) {
        return Err(Error::UnknownApp(unknown.clone()));
    }
    let (windows, unix) = launch_lines(apps);
    println!("Windows (PowerShell):\n  {windows}\n\nLinux, macOS:\n  {unix}\n");
    let current = if Os::current()? == Os::Windows { &windows } else { &unix };
    println!(
        "{}",
        lang.pick(
            "Scan to copy the command for this kind of system:",
            "Bu tür sistem için komutu kopyalamak üzere tarayın:"
        )
    );
    println!("{}", qr_text(current)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_lines_and_qr() {
        let (windows, unix) = launch_lines(&["git".to_owned(), "vlc".to_owned()]);
        assert!(windows.ends_with("install git vlc") && windows.contains("install.ps1"));
        assert!(unix.ends_with("sh -s -- install git vlc"));
        let (width, modules) = qr_modules(&unix).unwrap();
        assert_eq!(modules.len(), width * width);
        assert!(qr_text(&unix).unwrap().lines().count() > 10);
    }
}
