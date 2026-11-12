//! Çıktı seçenekleri: JSON çıktısı ve dry-run komutlarını betik olarak dışa aktarma.

use std::path::{Path, PathBuf};

use skadi_core::{Error, Lang, Result, exec::Invocation};

#[derive(Debug, Clone, Default)]
pub struct Output {
    pub json: bool,
    /// Çalıştırmak yerine komutları bu betiğe yaz.
    pub script: Option<PathBuf>,
}

impl Output {
    pub fn print_json(&self, value: &serde_json::Value) -> Result<()> {
        let text = serde_json::to_string_pretty(value).map_err(|e| Error::State(e.to_string()))?;
        println!("{text}");
        Ok(())
    }
}

/// Uzantıya göre PowerShell (`.ps1`) ya da POSIX kabuk betiği yazar. Betik yalnızca yazılır,
/// çalıştırılmaz; kullanıcı inceleyip kendisi çalıştırır.
pub fn write_script(lang: Lang, path: &Path, invocations: &[Invocation]) -> Result<()> {
    let powershell = path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("ps1"));
    let text = if powershell {
        skadi_core::exec::powershell_script(invocations)
    } else {
        skadi_core::exec::posix_script(invocations)
    };
    std::fs::write(path, text).map_err(|e| Error::State(format!("{}: {e}", path.display())))?;
    println!(
        "{} {}",
        lang.pick("Script written to", "Betik yazıldı:"),
        path.display()
    );
    Ok(())
}
