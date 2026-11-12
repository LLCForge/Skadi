use std::path::Path;

use serde_json::json;
use skadi_core::{
    Lang, Result,
    catalog::Catalog,
    profile::{Profile, preset, presets},
    system::{Op, System},
    tweak::{StateStore, TweakCatalog},
};

use crate::{commands, commands::confirm, output::Output, tweaks};

pub fn list(lang: Lang, out: &Output) -> Result<()> {
    let presets = presets()?;
    if out.json {
        let rows: Vec<_> = presets
            .iter()
            .map(|p| {
                json!({
                    "id": p.id,
                    "name": p.name.get(lang),
                    "description": p.description.get(lang),
                    "apps": p.apps,
                    "tweaks": p.tweaks,
                })
            })
            .collect();
        return out.print_json(&json!({ "presets": rows }));
    }
    for preset in presets {
        println!("  {:<18} {}", preset.id, preset.name.get(lang));
        println!("  {:<18} {}", "", preset.description.get(lang));
    }
    Ok(())
}

/// Bir hazır seti ya da profil dosyasını uygular. Başka sistemlere ait kimlikler atlanır.
pub fn apply(
    lang: Lang,
    out: &Output,
    target: &str,
    public_key: Option<&Path>,
    dry_run: bool,
    yes: bool,
) -> Result<()> {
    let t = |en, tr| lang.pick(en, tr);
    let apps = Catalog::builtin()?;
    let tweak_catalog = TweakCatalog::builtin()?;
    let path = Path::new(target);
    let profile = match (path.exists(), public_key) {
        // İmzalı profil: içerik bir kez okunur, doğrulanır ve aynı baytlar kullanılır.
        (true, Some(key)) => {
            let data = skadi_core::signing::read_verified(path, key)?;
            let text =
                String::from_utf8(data).map_err(|_| skadi_core::Error::Profile("profile is not UTF-8".into()))?;
            println!("{}", t("Signature verified.", "İmza doğrulandı."));
            Profile::parse(&text, &apps, &tweak_catalog)?
        }
        (true, None) => Profile::load(path, &apps, &tweak_catalog)?,
        (false, _) => preset(target)?.profile(),
    };
    let system = System::detect()?;

    let app_ids: Vec<String> = profile
        .apps
        .iter()
        .filter(|id| {
            apps.get(id)
                .is_some_and(|app| app.packages.keys().any(|m| m.spec().os == system.os))
        })
        .cloned()
        .collect();
    let applicable: Vec<&str> = tweak_catalog.for_host(&system).map(|tweak| tweak.id.as_str()).collect();
    let tweak_ids: Vec<String> = profile
        .tweaks
        .iter()
        .filter(|id| applicable.contains(&id.as_str()))
        .cloned()
        .collect();
    let skipped = profile.apps.len() + profile.tweaks.len() - app_ids.len() - tweak_ids.len();

    println!("{}: {}", t("Apps", "Uygulamalar"), app_ids.join(", "));
    println!("{}: {}", t("Tweaks", "Ayarlar"), tweak_ids.join(", "));
    if skipped > 0 {
        println!(
            "{skipped} {}",
            t(
                "items are for other systems and will be skipped.",
                "öğe başka sistemler için, atlanacak."
            )
        );
    }
    let irreversible: Vec<&str> = tweak_ids
        .iter()
        .filter_map(|id| tweak_catalog.get(id))
        .filter(|tweak| !tweak.is_reversible())
        .map(|tweak| tweak.id.as_str())
        .collect();
    if !irreversible.is_empty() {
        println!(
            "{} {}",
            t("WARNING: cannot be undone:", "UYARI: geri alınamaz:"),
            irreversible.join(", ")
        );
    }

    let preview = dry_run || out.script.is_some();
    if !preview && !confirm(lang, yes, t("Apply this profile?", "Bu profil uygulansın mı?"))? {
        return Ok(());
    }
    if !app_ids.is_empty() {
        let options = commands::OpOptions {
            apps: app_ids,
            resume: false,
            manager: None,
            dry_run,
            yes: true,
        };
        commands::apply(lang, out, &options, Op::Install)?;
    }
    if !tweak_ids.is_empty() {
        let selection = tweaks::Selection {
            pack: None,
            ids: tweak_ids,
            safe: false,
            all: false,
            last: false,
            snapshot: false,
            dry_run,
            yes: true,
        };
        // Uygulama betiği zaten yazıldıysa tweak komutları ayrı bir önizleme olarak gösterilir.
        let tweak_out = Output {
            json: out.json,
            script: None,
        };
        tweaks::apply(lang, if out.script.is_some() { &tweak_out } else { out }, &selection)?;
    }
    if !profile.vscode_extensions.is_empty() {
        vscode_extensions(
            lang,
            &system,
            &profile.vscode_extensions,
            dry_run || out.script.is_some(),
        )?;
    }
    Ok(())
}

/// VS Code ya da VSCodium eklentilerini kullanıcı olarak kurar.
fn vscode_extensions(lang: Lang, system: &System, extensions: &[String], dry_run: bool) -> Result<()> {
    let Some(code) = ["code", "codium"].into_iter().find_map(skadi_core::exec::find_program) else {
        println!(
            "{}",
            lang.pick(
                "VS Code or VSCodium was not found; skipping editor extensions.",
                "VS Code ya da VSCodium bulunamadı; editör eklentileri atlanıyor."
            )
        );
        return Ok(());
    };
    for extension in extensions {
        let invocation = skadi_core::exec::Invocation::build(
            code.clone(),
            vec!["--install-extension".into(), extension.into()],
            &[],
            skadi_core::exec::Privilege::NonRoot,
            &system.privileges,
            system.os,
        )?;
        println!("> {}", invocation.display());
        if !dry_run {
            invocation.run_captured()?;
        }
    }
    Ok(())
}

/// Kurulu katalog uygulamalarını ve Skadi'nin uyguladığı tweak'leri profile yazar.
pub fn export(lang: Lang, path: &Path) -> Result<()> {
    let apps = Catalog::builtin()?;
    let tweaks = TweakCatalog::builtin()?;
    let system = System::detect()?;
    let store = StateStore::open(system.os)?;
    let profile = Profile {
        version: 1,
        vscode_extensions: Vec::new(),
        apps: system
            .installed_apps(apps.apps())
            .into_iter()
            .map(|(app, _)| app.id.clone())
            .collect(),
        tweaks: store
            .recorded()
            .into_iter()
            .filter(|id| tweaks.get(id).is_some())
            .collect(),
    };
    profile.save(path)?;
    println!(
        "{} {} ({} {}, {} {})",
        lang.pick("Profile saved to", "Profil kaydedildi:"),
        path.display(),
        profile.apps.len(),
        lang.pick("apps", "uygulama"),
        profile.tweaks.len(),
        lang.pick("tweaks", "ayar")
    );
    Ok(())
}

/// Numaralı bir seçim ister; boş yanıt, `0` ya da girdi sonu atlamak demektir.
fn choose(lang: Lang, question: &str, options: &[(String, String)]) -> Result<Option<String>> {
    use std::io::{BufRead, Write};
    println!("\n{question}");
    println!("  0) {}", lang.pick("Skip", "Atla"));
    for (index, (_, label)) in options.iter().enumerate() {
        println!("  {}) {label}", index + 1);
    }
    loop {
        print!("> ");
        let mut answer = String::new();
        let read = std::io::stdout()
            .flush()
            .and_then(|()| std::io::stdin().lock().read_line(&mut answer))
            .map_err(|source| skadi_core::Error::Spawn {
                program: "stdin".to_owned(),
                source,
            })?;
        let answer = answer.trim();
        if read == 0 || answer.is_empty() || answer == "0" {
            return Ok(None);
        }
        match answer.parse::<usize>() {
            Ok(n) if (1..=options.len()).contains(&n) => return Ok(Some(options[n - 1].0.clone())),
            _ => println!(
                "{}",
                lang.pick("Enter a number from the list.", "Listeden bir numara girin.")
            ),
        }
    }
}

/// Temiz kurulum sihirbazı: uygulama seti, tweak seviyesi ve DNS sağlayıcısı. Her adım kendi
/// planını gösterir ve ayrıca onay ister.
pub fn setup(lang: Lang, out: &Output, dry_run: bool) -> Result<()> {
    let t = |en, tr| lang.pick(en, tr);
    let all = presets()?;
    let (tweak_sets, app_sets): (Vec<_>, Vec<_>) = all.iter().partition(|p| p.id.starts_with("tweaks-"));
    let labels = |list: &[&skadi_core::profile::Preset]| -> Vec<(String, String)> {
        list.iter()
            .map(|p| {
                (
                    p.id.clone(),
                    format!("{} - {}", p.name.get(lang), p.description.get(lang)),
                )
            })
            .collect()
    };
    let apps = choose(
        lang,
        t("1/3 Which apps should be installed?", "1/3 Hangi uygulamalar kurulsun?"),
        &labels(&app_sets),
    )?;
    let tweaks = choose(
        lang,
        t(
            "2/3 How far should Windows/Linux/macOS be cleaned up?",
            "2/3 Sistem ne kadar sadeleştirilsin?",
        ),
        &labels(&tweak_sets),
    )?;
    let providers: Vec<(String, String)> = skadi_core::dns::PROVIDERS
        .iter()
        .map(|p| (p.id.to_owned(), p.name.to_owned()))
        .collect();
    let dns = choose(
        lang,
        t(
            "3/3 Which private DNS provider?",
            "3/3 Hangi gizlilik odaklı DNS sağlayıcısı?",
        ),
        &providers,
    )?;

    for target in apps.iter().chain(&tweaks) {
        println!();
        apply(lang, out, target, None, dry_run, false)?;
    }
    if let Some(dns) = dns {
        println!();
        crate::net::dns_apply(lang, &System::detect()?, Some(&dns), dry_run, false)?;
    }
    println!("\n{}", t("Setup finished.", "Kurulum tamamlandı."));
    Ok(())
}

/// Bir hazır setin ya da profil dosyasının uygulamalarını başka bir aracın biçimine çevirir.
/// Tweak'ler dönüştürülmez: onları yalnızca Skadi geri alınabilir biçimde uygulayabilir.
pub fn convert(lang: Lang, format: &str, target: &str, output: Option<&Path>) -> Result<()> {
    use skadi_core::export::{ExportFormat, render};
    let format = ExportFormat::parse(format).ok_or(skadi_core::Error::InvalidOption(
        "ansible | brewfile | winget | nix | intune",
    ))?;
    let apps = Catalog::builtin()?;
    let path = Path::new(target);
    let profile = if path.exists() {
        Profile::load(path, &apps, &TweakCatalog::builtin()?)?
    } else {
        preset(target)?.profile()
    };
    let selected: Vec<_> = profile.apps.iter().filter_map(|id| apps.get(id)).collect();
    let text = render(format, &selected);
    match output {
        Some(file) => {
            std::fs::write(file, text).map_err(|e| skadi_core::Error::State(format!("{}: {e}", file.display())))?;
            eprintln!("{} {}", lang.pick("Written to", "Yazıldı:"), file.display());
        }
        None => print!("{text}"),
    }
    if !profile.tweaks.is_empty() {
        eprintln!(
            "{}",
            lang.pick(
                "Tweaks are not converted; apply them with `skadi profile apply`.",
                "Tweak'ler dönüştürülmez; `skadi profile apply` ile uygulayın."
            )
        );
    }
    Ok(())
}
