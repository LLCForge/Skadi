//! `skadi catalog add`: yeni bir uygulama kaydını adım adım hazırlar. Hiçbir dosyayı değiştirmez;
//! doğrulanmış TOML metnini yazdırır, katkıcı bunu `catalog/apps.toml` dosyasına ekleyip PR açar.

use std::io::{BufRead, Write};

use skadi_core::{
    Error, Lang, Result,
    catalog::{Catalog, Category},
    manager::{ManagerId, validate_package_id},
};

fn ask(question: &str) -> Result<String> {
    print!("{question}: ");
    let mut answer = String::new();
    std::io::stdout()
        .flush()
        .and_then(|()| std::io::stdin().lock().read_line(&mut answer))
        .map_err(|source| Error::Spawn {
            program: "stdin".to_owned(),
            source,
        })?;
    Ok(answer.trim().to_owned())
}

/// Geçerli bir yanıt gelene kadar sorar; girdi biterse hata verir.
fn ask_until(lang: Lang, question: &str, valid: impl Fn(&str) -> bool) -> Result<String> {
    for _ in 0..5 {
        let answer = ask(question)?;
        if valid(&answer) {
            return Ok(answer);
        }
        println!("{}", lang.pick("Not valid, try again.", "Geçersiz, yeniden deneyin."));
    }
    Err(Error::InvalidOption("catalog add"))
}

fn quote(text: &str) -> String {
    serde_json::Value::from(text).to_string()
}

fn is_app_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id.starts_with(|c: char| c.is_ascii_lowercase())
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn is_text(text: &str) -> bool {
    !text.is_empty() && text.len() <= 300 && !text.chars().any(char::is_control)
}

pub fn add(lang: Lang) -> Result<()> {
    let t = |en, tr| lang.pick(en, tr);
    let catalog = Catalog::builtin()?;
    println!(
        "{}\n",
        t(
            "This prints a new catalog entry; nothing is changed. See CONTRIBUTING.md.",
            "Bu sihirbaz yeni bir katalog kaydı yazdırır; hiçbir şey değiştirilmez. CONTRIBUTING.md'ye bakın."
        )
    );
    let id = ask_until(
        lang,
        t(
            "App id (lowercase, e.g. my-app)",
            "Uygulama kimliği (küçük harf, ör. my-app)",
        ),
        |id| is_app_id(id) && catalog.get(id).is_none(),
    )?;
    let name = ask_until(lang, t("Display name", "Görünen ad"), is_text)?;
    let categories: Vec<&str> = Category::ALL.iter().map(|c| c.as_str()).collect();
    let category = ask_until(
        lang,
        &format!("{} ({})", t("Category", "Kategori"), categories.join(", ")),
        |c| categories.contains(&c),
    )?;
    let en = ask_until(lang, t("Description in English", "İngilizce açıklama"), is_text)?;
    let tr = ask_until(lang, t("Description in Turkish", "Türkçe açıklama"), is_text)?;
    let homepage = ask_until(
        lang,
        t("Official homepage (https://…)", "Resmi site (https://…)"),
        |h| h.starts_with("https://") && is_text(h) && !h.contains(char::is_whitespace),
    )?;
    let foss = ask_until(
        lang,
        t("Free and open source? (y/n)", "Özgür ve açık kaynak mı? (e/h)"),
        |a| matches!(a, "y" | "n" | "e" | "h"),
    )?;
    let names: Vec<&str> = ManagerId::ALL.iter().map(|m| m.as_str()).collect();
    let managers = ask_until(
        lang,
        &format!(
            "{} ({})",
            t("Package managers, comma-separated", "Paket yöneticileri, virgülle"),
            names.join(", ")
        ),
        |list| !list.is_empty() && list.split(',').all(|m| m.trim().parse::<ManagerId>().is_ok()),
    )?;
    let mut packages = String::new();
    for manager in managers.split(',').map(str::trim) {
        let package = ask_until(lang, &format!("{manager} {}", t("package id", "paket kimliği")), |p| {
            validate_package_id(p).is_ok()
        })?;
        packages.push_str(&format!("{manager} = {}\n", quote(&package)));
    }
    let entry = format!(
        "[[app]]\nid = {}\nname = {}\ncategory = {}\ndescription = {{ en = {}, tr = {} }}\nhomepage = {}\nfoss = {}\n[app.packages]\n{packages}",
        quote(&id),
        quote(&name),
        quote(&category),
        quote(&en),
        quote(&tr),
        quote(&homepage),
        matches!(foss.as_str(), "y" | "e"),
    );
    // Katalog ayrıştırıcısı ve doğrulaması bu kayıt için de çalışır.
    Catalog::from_toml(&entry)?;
    println!("\n{entry}");
    println!(
        "{}",
        t(
            "Check every package id with a read-only query and mention how in the pull request.",
            "Her paket kimliğini salt okunur bir sorguyla doğrulayın ve PR'da nasıl doğruladığınızı yazın."
        )
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn validates_ids() {
        assert!(super::is_app_id("my-app2"));
        assert!(!super::is_app_id("My App") && !super::is_app_id("-x") && !super::is_app_id(""));
        assert!(!super::is_text("a\nb"));
    }
}
