use serde_json::json;
use skadi_core::{
    Error, Lang, Result,
    system::System,
    tweak::{Risk, StateStore, Tweak, TweakCatalog, TweakCategory, TweakEngine},
};

use crate::{commands::confirm, output::Output};

pub struct Selection {
    /// İmzalı tweak paketi ve onu imzalayan açık anahtar.
    pub pack: Option<(std::path::PathBuf, std::path::PathBuf)>,
    pub ids: Vec<String>,
    pub safe: bool,
    pub all: bool,
    pub last: bool,
    pub snapshot: bool,
    pub dry_run: bool,
    pub yes: bool,
}

/// Gömülü katalog ya da imzası doğrulanmış bir tweak paketi. Paket tweak'leri gömülü katalogla
/// aynı denetimlerden geçer ve gömülü kimlikleri kullanamaz.
pub fn load_catalog(pack: Option<&(std::path::PathBuf, std::path::PathBuf)>) -> Result<TweakCatalog> {
    let Some((file, key)) = pack else {
        return TweakCatalog::builtin();
    };
    let data = skadi_core::signing::read_verified(file, key)?;
    // `.wasm` eklentisi korumalı ortamda çalışır ve yalnızca paket metni üretebilir.
    let text = if file.extension().is_some_and(|e| e.eq_ignore_ascii_case("wasm")) {
        skadi_core::plugin::run(&data)?
    } else {
        if data.len() > 512 * 1024 {
            return Err(Error::Catalog("tweak pack is too large".into()));
        }
        String::from_utf8(data).map_err(|_| Error::Catalog("tweak pack is not UTF-8".into()))?
    };
    let pack = TweakCatalog::from_toml(&text)?;
    let builtin = TweakCatalog::builtin()?;
    if let Some(clash) = pack.tweaks().iter().find(|t| builtin.get(&t.id).is_some()) {
        return Err(Error::Catalog(format!(
            "{}: a tweak pack cannot reuse a built-in id",
            clash.id
        )));
    }
    Ok(pack)
}

pub fn list(
    lang: Lang,
    out: &Output,
    category: Option<&str>,
    pack: Option<&(std::path::PathBuf, std::path::PathBuf)>,
) -> Result<()> {
    let t = |en, tr| lang.pick(en, tr);
    let catalog = load_catalog(pack)?;
    let system = System::detect()?;
    let store = StateStore::open(system.os)?;
    let engine = TweakEngine::new(&system);
    let filter = category.map(str::parse::<TweakCategory>).transpose()?;
    let tweaks: Vec<&Tweak> = catalog.for_host(&system).collect();
    engine.prefetch(&tweaks)?;

    if out.json {
        let rows: Vec<_> = tweaks
            .iter()
            .filter(|tweak| filter.is_none_or(|f| f == tweak.category))
            .map(|tweak| {
                json!({
                    "id": tweak.id,
                    "name": tweak.name.get(lang),
                    "description": tweak.description.get(lang),
                    "category": tweak.category.as_str(),
                    "risk": format!("{:?}", tweak.risk).to_lowercase(),
                    "reversible": tweak.is_reversible(),
                    "status": engine.status(tweak).ok().map(|s| format!("{s:?}").to_lowercase()),
                    "saved": store.is_recorded(&tweak.id),
                    "source": tweak.source,
                })
            })
            .collect();
        return out.print_json(&json!({ "tweaks": rows }));
    }

    for category in TweakCategory::ALL
        .into_iter()
        .filter(|c| filter.is_none_or(|f| f == *c))
    {
        let group: Vec<&&Tweak> = tweaks.iter().filter(|tweak| tweak.category == category).collect();
        if group.is_empty() {
            continue;
        }
        println!("\n{} ({})", category.label(lang), category.as_str());
        for tweak in group {
            let status = engine
                .status(tweak)
                .map_or_else(|_| "?".to_owned(), |s| s.label(lang).to_owned());
            let recorded = store.is_recorded(&tweak.id);
            let saved = if recorded { "*" } else { " " };
            // Skadi uyguladı ama artık tam uygulanmış değil: bir güncelleme ayarı geri almış olabilir.
            let drift = recorded
                && matches!(
                    engine.status(tweak),
                    Ok(skadi_core::tweak::TweakStatus::NotApplied | skadi_core::tweak::TweakStatus::Partial)
                );
            let drift = if drift { t(" [drifted]", " [sürüklendi]") } else { "" };
            let irreversible = if tweak.is_reversible() {
                ""
            } else {
                t(" [irreversible]", " [geri alınamaz]")
            };
            println!(
                "  {:<28} {:<12}{saved} {:<9} {}{irreversible}{drift}",
                tweak.id,
                status,
                tweak.risk.label(lang),
                tweak.name.get(lang)
            );
        }
    }
    println!(
        "\n* {}: {}",
        t("saved state", "kayıtlı durum"),
        t(
            "can be undone with `skadi tweak revert`",
            "`skadi tweak revert` ile geri alınabilir"
        )
    );
    Ok(())
}

fn select<'c>(catalog: &'c TweakCatalog, system: &'c System, selection: &Selection) -> Result<Vec<&'c Tweak>> {
    if selection.all || selection.last {
        let store = StateStore::open(system.os)?;
        let ids = if selection.all {
            store.recorded()
        } else {
            store.last_session()
        };
        return Ok(ids.iter().filter_map(|id| catalog.get(id)).collect());
    }
    if selection.safe {
        return Ok(catalog
            .for_host(system)
            .filter(|tweak| tweak.risk == Risk::Safe)
            .collect());
    }
    selection
        .ids
        .iter()
        .map(|id| catalog.get(id).ok_or_else(|| Error::UnknownTweak(id.clone())))
        .collect()
}

pub fn apply(lang: Lang, out: &Output, selection: &Selection) -> Result<()> {
    let t = |en, tr| lang.pick(en, tr);
    if selection.all || selection.last {
        return Err(Error::InvalidOption("--all / --last"));
    }
    let catalog = load_catalog(selection.pack.as_ref())?;
    let system = System::detect()?;
    let mut engine = TweakEngine::new(&system);
    let tweaks = select(&catalog, &system, selection)?;
    if let Some(policy) = skadi_core::policy::load(system.os)?
        && let Some(blocked) = tweaks.iter().find(|t| !policy.allows_tweak(&t.id))
    {
        return Err(Error::Policy(format!("{} is not on the allowed list", blocked.id)));
    }

    if let Some(path) = &out.script {
        let mut script = Vec::new();
        for tweak in &tweaks {
            script.extend(engine.preview(tweak)?);
        }
        return crate::output::write_script(lang, path, &script);
    }
    if selection.dry_run {
        if system.os == skadi_core::platform::Os::Windows && !system.privileges.is_elevated {
            println!(
                "{}\n",
                t(
                    "Note: applying these changes requires an administrator terminal.",
                    "Not: bu değişiklikleri uygulamak için yönetici terminali gerekir."
                )
            );
        }
        for tweak in &tweaks {
            println!("{} ({})", tweak.name.get(lang), tweak.id);
            let invocations = engine.preview(tweak)?;
            if invocations.is_empty() {
                println!("  {}", t("nothing to do", "yapılacak bir şey yok"));
            }
            for invocation in invocations {
                println!("  > {}", invocation.display());
            }
        }
        return Ok(());
    }

    println!("{}:", t("Tweaks to apply", "Uygulanacak tweak'ler"));
    for tweak in &tweaks {
        println!("  - {} ({})", tweak.name.get(lang), tweak.risk.label(lang));
    }
    let irreversible: Vec<&&Tweak> = tweaks.iter().filter(|tweak| !tweak.is_reversible()).collect();
    if !irreversible.is_empty() {
        println!(
            "\n{}",
            t(
                "WARNING: the following tweaks remove apps and cannot be undone by Skadi:",
                "UYARI: aşağıdaki tweak'ler uygulama kaldırır ve Skadi tarafından geri alınamaz:"
            )
        );
        for tweak in irreversible {
            println!("  - {}", tweak.name.get(lang));
        }
    }
    if !confirm(lang, selection.yes, t("Continue?", "Devam edilsin mi?"))? {
        return Ok(());
    }

    if selection.snapshot && !snapshot(lang, &system, selection.yes)? {
        return Ok(());
    }

    let mut store = StateStore::open(system.os)?;
    let mut first_error = None;
    for tweak in tweaks {
        match engine.apply(tweak, &mut store) {
            Ok(outcome) => println!(
                "[ok] {}: {} {}",
                tweak.id,
                outcome.changed,
                t("change(s)", "değişiklik")
            ),
            Err(error) => {
                eprintln!("[{}] {}: {}", t("failed", "başarısız"), tweak.id, error.message(lang));
                let stop = matches!(error, Error::ElevationRequired | Error::NoElevator);
                first_error.get_or_insert(error);
                if stop {
                    break;
                }
            }
        }
    }
    println!(
        "\n{} {}",
        t("Previous values were saved to", "Eski değerler şuraya kaydedildi:"),
        store.path().display()
    );
    first_error.map_or(Ok(()), Err)
}

pub fn revert(lang: Lang, selection: &Selection) -> Result<()> {
    let t = |en, tr| lang.pick(en, tr);
    let catalog = load_catalog(selection.pack.as_ref())?;
    let system = System::detect()?;
    let mut engine = TweakEngine::new(&system);
    let mut store = StateStore::open(system.os)?;
    let tweaks: Vec<&Tweak> = select(&catalog, &system, selection)?
        .into_iter()
        .filter(|tweak| !(selection.safe || selection.all || selection.last) || store.is_recorded(&tweak.id))
        .collect();

    if tweaks.is_empty() {
        println!("{}", t("No tweaks with saved state.", "Kayıtlı durumu olan tweak yok."));
        return Ok(());
    }
    if selection.dry_run {
        for tweak in &tweaks {
            let state = if store.is_recorded(&tweak.id) {
                t("will be restored from saved state", "kayıtlı durumdan geri yüklenecek")
            } else {
                t("no saved state", "kayıtlı durum yok")
            };
            println!("{}: {state}", tweak.id);
        }
        return Ok(());
    }
    if !confirm(
        lang,
        selection.yes,
        t("Undo the selected tweaks?", "Seçilen tweak'ler geri alınsın mı?"),
    )? {
        return Ok(());
    }

    let mut first_error = None;
    for tweak in tweaks {
        match engine.revert(tweak, &mut store) {
            Ok(outcome) => {
                print!(
                    "[ok] {}: {} {}",
                    tweak.id,
                    outcome.restored,
                    t("restored", "geri yüklendi")
                );
                if outcome.irreversible > 0 {
                    print!(
                        ", {} {}",
                        outcome.irreversible,
                        t("could not be undone", "geri alınamadı")
                    );
                }
                println!();
            }
            Err(error) => {
                eprintln!("[{}] {}: {}", t("failed", "başarısız"), tweak.id, error.message(lang));
                let stop = matches!(error, Error::ElevationRequired | Error::NoElevator);
                first_error.get_or_insert(error);
                if stop {
                    break;
                }
            }
        }
    }
    first_error.map_or(Ok(()), Err)
}

/// Değişikliklerden önce anlık görüntü alır. Alınamazsa kullanıcıya sorar; `false` vazgeçmek demektir.
fn snapshot(lang: Lang, system: &System, yes: bool) -> Result<bool> {
    let t = |en, tr| lang.pick(en, tr);
    let failure = match skadi_core::snapshot::snapshot_invocation(system)? {
        Some((tool, invocation)) => {
            println!(
                "{} ({tool})...",
                t("Creating a snapshot", "Anlık görüntü oluşturuluyor")
            );
            match invocation.run_captured() {
                Ok(()) => return Ok(true),
                Err(error) => error.message(lang),
            }
        }
        None => t(
            "no snapshot tool found (Timeshift or Snapper)",
            "anlık görüntü aracı bulunamadı (Timeshift ya da Snapper)",
        )
        .to_owned(),
    };
    eprintln!("{}: {failure}", t("Snapshot failed", "Anlık görüntü alınamadı"));
    confirm(
        lang,
        yes,
        t(
            "Continue without a snapshot?",
            "Anlık görüntü olmadan devam edilsin mi?",
        ),
    )
}

/// Seçilen tweak'lerin kullanıcıya özel (HKCU) registry değerlerini Windows'un varsayılan kullanıcı
/// profiline yazar (`revert` ile siler). Bundan sonra açılan her yeni hesap bu ayarlarla başlar.
/// Makine genelindeki (HKLM) değerler zaten herkes için geçerlidir.
pub fn default_user(lang: Lang, selection: &Selection, revert: bool) -> Result<()> {
    use skadi_core::tweak::Action;
    let t = |en, tr| lang.pick(en, tr);
    let catalog = load_catalog(selection.pack.as_ref())?;
    let system = System::detect()?;
    if system.os != skadi_core::platform::Os::Windows {
        return Err(Error::Unsupported("tweak default-user (Windows)"));
    }
    if !system.privileges.is_elevated && !selection.dry_run {
        return Err(Error::ElevationRequired);
    }
    let tweaks = select(&catalog, &system, selection)?;
    let mount = r"HKU\SkadiDefaultUser";
    let mut values = Vec::new();
    for tweak in &tweaks {
        for action in &tweak.actions {
            if let Action::Registry { key, name, kind, data } = action
                && let Some(rest) = key.strip_prefix("HKCU\\")
            {
                values.push((format!(r"{mount}\{rest}"), name.clone(), *kind, data.clone()));
            }
        }
    }
    if values.is_empty() {
        println!(
            "{}",
            t(
                "These tweaks have no per-user settings.",
                "Bu tweak'lerin kullanıcıya özel ayarı yok."
            )
        );
        return Ok(());
    }
    let hive = format!(
        r"{}\Users\Default\NTUSER.DAT",
        std::env::var("SystemDrive").unwrap_or_else(|_| "C:".to_owned())
    );
    let preview = crate::commands::preview_system(lang, &system, selection.dry_run);
    let runner = preview.as_ref().unwrap_or(&system);
    let reg = |args: &[&str]| crate::net::build(runner, "reg", args, skadi_core::exec::Privilege::Root);
    let mut plan = Vec::new();
    for (key, name, kind, data) in &values {
        let value: Vec<&str> = if name.is_empty() {
            vec!["/ve"]
        } else {
            vec!["/v", name.as_str()]
        };
        let mut args = vec![if revert { "delete" } else { "add" }, key.as_str()];
        args.extend(value);
        if !revert {
            args.extend(["/t", kind.reg_type(), "/d", data.as_str()]);
        }
        args.push("/f");
        plan.push(reg(&args)?);
    }
    let load = reg(&["load", mount, &hive])?;
    let unload = reg(&["unload", mount])?;
    println!("> {}", load.display());
    for invocation in &plan {
        println!("> {}", invocation.display());
    }
    println!("> {}", unload.display());
    if selection.dry_run
        || !confirm(
            lang,
            selection.yes,
            t(
                "Change the default profile that new user accounts start from?",
                "Yeni kullanıcı hesaplarının başladığı varsayılan profil değiştirilsin mi?",
            ),
        )?
    {
        return Ok(());
    }
    load.run_captured()?;
    // Hata olsa da kovan mutlaka bırakılır; aksi halde varsayılan profil kilitli kalır.
    let mut result = Ok(());
    for invocation in &plan {
        if let Err(error) = invocation.run_captured()
            && !revert
        {
            result = Err(error);
            break;
        }
    }
    unload.run_captured()?;
    result?;
    println!(
        "{}",
        t(
            "Done. Accounts created from now on start with these settings.",
            "Tamam. Bundan sonra açılan hesaplar bu ayarlarla başlar."
        )
    );
    Ok(())
}
