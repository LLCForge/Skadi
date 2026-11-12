use std::io::{self, BufRead, Write};

use serde_json::json;
use skadi_core::{
    Error, Lang, Result,
    catalog::{App, Catalog, Category},
    manager::ManagerId,
    plan::plan,
    system::{Op, System},
};

use crate::output::Output;

/// Kurulumdan önce uyarı verilen asgari boş alan.
const MIN_FREE_BYTES: u64 = 5 * 1024 * 1024 * 1024;

pub struct OpOptions {
    pub apps: Vec<String>,
    pub resume: bool,
    pub manager: Option<String>,
    pub dry_run: bool,
    pub yes: bool,
}

pub fn info(lang: Lang, out: &Output) -> Result<()> {
    let t = |en, tr| lang.pick(en, tr);
    let system = System::detect()?;
    if out.json {
        let managers: Vec<_> = ManagerId::ALL
            .into_iter()
            .filter(|id| id.spec().os == system.os)
            .map(|id| {
                json!({
                    "id": id.as_str(),
                    "path": system.manager(id).map(|m| m.path.display().to_string()),
                    "bootstrap": id.spec().bootstrap.is_some(),
                })
            })
            .collect();
        return out.print_json(&json!({
            "version": env!("CARGO_PKG_VERSION"),
            "os": system.os.to_string(),
            "arch": system.arch,
            "distro": system.distro.as_ref().map(|d| d.display_name().to_owned()),
            "elevated": system.privileges.is_elevated,
            "free_space_bytes": system.free_space(),
            "managers": managers,
        }));
    }
    println!(
        "Skadi {} ({} {})",
        env!("CARGO_PKG_VERSION"),
        t("catalog", "katalog"),
        skadi_core::catalog::catalog_version()
    );
    println!(
        "{:<18} {} ({})",
        t("Operating system", "İşletim sistemi"),
        system.os,
        system.arch
    );
    if let Some(distro) = &system.distro {
        println!("{:<18} {}", t("Distribution", "Dağıtım"), distro.display_name());
    }
    let elevated = if system.privileges.is_elevated {
        t("yes", "evet")
    } else {
        t("no", "hayır")
    };
    println!("{:<18} {elevated}", t("Administrator", "Yönetici yetkisi"));
    println!("\n{}:", t("Package managers", "Paket yöneticileri"));
    for id in ManagerId::ALL.into_iter().filter(|id| id.spec().os == system.os) {
        match system.manager(id) {
            Some(manager) => println!("  [+] {:<10} {}", id, manager.path.display()),
            None if id.spec().bootstrap.is_some() => {
                println!(
                    "  [-] {:<10} {}",
                    id,
                    t("(can be installed automatically)", "(otomatik kurulabilir)")
                );
            }
            None => println!("  [-] {id}"),
        }
    }
    Ok(())
}

pub fn list(lang: Lang, out: &Output, category: Option<&str>, tag: Option<&str>, ids_only: bool) -> Result<()> {
    let mut catalog = Catalog::builtin()?;
    if let Some(tag) = tag {
        catalog.retain(|app| skadi_core::catalog::tags(app).contains(&tag));
    }
    let filter = category.map(str::parse::<Category>).transpose()?;
    let system = System::detect()?;
    let available = system.available_ids();
    if ids_only {
        // Betikler için: yalnızca bu sistemde mevcut bir yöneticiyle kurulabilenler.
        catalog
            .apps()
            .iter()
            .filter(|app| filter.is_none_or(|f| f == app.category))
            .filter(|app| available.iter().any(|id| app.packages.contains_key(id)))
            .for_each(|app| println!("{}", app.id));
        return Ok(());
    }
    if out.json {
        let apps: Vec<_> = catalog
            .apps()
            .iter()
            .filter(|app| filter.is_none_or(|f| f == app.category))
            .map(|app| {
                let packages: std::collections::BTreeMap<&str, &String> =
                    app.packages.iter().map(|(m, p)| (m.as_str(), p)).collect();
                json!({
                    "id": app.id,
                    "name": app.name,
                    "category": app.category.as_str(),
                    "description": app.description.get(lang),
                    "homepage": app.homepage,
                    "foss": app.foss,
                    "packages": packages,
                    "installable_via": available.iter().find(|id| app.packages.contains_key(id)).map(|id| id.as_str()),
                })
            })
            .collect();
        return out.print_json(&json!({ "apps": apps }));
    }
    for category in Category::ALL.into_iter().filter(|c| filter.is_none_or(|f| f == *c)) {
        let apps: Vec<&App> = catalog.apps().iter().filter(|app| app.category == category).collect();
        if apps.is_empty() {
            continue;
        }
        println!("\n{} ({category})", category.label(lang));
        for app in apps {
            let via = available
                .iter()
                .find(|id| app.packages.contains_key(id))
                .map_or_else(|| "-".to_owned(), ToString::to_string);
            let foss = if app.foss == Some(true) { "foss" } else { "" };
            println!("  {:<24} {:<10} {:<5} {}", app.id, via, foss, app.description.get(lang));
        }
    }
    Ok(())
}

/// Bir uygulamanın ayrıntıları, gizlilik notu ve alternatifleri.
pub fn show(lang: Lang, out: &Output, id: &str) -> Result<()> {
    let t = |en, tr| lang.pick(en, tr);
    let catalog = Catalog::builtin()?;
    let app = catalog.get(id).ok_or_else(|| Error::UnknownApp(id.to_owned()))?;
    let system = System::detect()?;
    let via = system
        .available_ids()
        .into_iter()
        .find(|m| app.packages.contains_key(m));
    let name_of = |id: &String| {
        catalog
            .get(id)
            .map_or_else(|| id.clone(), |a| format!("{} ({id})", a.name))
    };
    if out.json {
        let packages: std::collections::BTreeMap<&str, &String> =
            app.packages.iter().map(|(m, p)| (m.as_str(), p)).collect();
        return out.print_json(&json!({
            "id": app.id,
            "name": app.name,
            "category": app.category.as_str(),
            "description": app.description.get(lang),
            "homepage": app.homepage,
            "foss": app.foss,
            "telemetry": app.telemetry,
            "privacy_source": app.privacy_source,
            "note": app.note.as_ref().map(|n| n.get(lang)),
            "alternatives": app.alternatives,
            "conflicts": app.conflicts,
            "packages": packages,
            "installable_via": via.map(|m| m.as_str()),
        }));
    }
    let yes_no = |value: Option<bool>| match value {
        Some(true) => t("yes", "evet"),
        Some(false) => t("no", "hayır"),
        None => t("unknown", "bilinmiyor"),
    };
    println!("{} ({})", app.name, app.id);
    println!("{}", app.description.get(lang));
    if let Some(homepage) = &app.homepage {
        println!("{:<16} {homepage}", t("Homepage", "Ana sayfa"));
    }
    println!("{:<16} {}", t("Category", "Kategori"), app.category.label(lang));
    println!("{:<16} {}", t("Open source", "Açık kaynak"), yes_no(app.foss));
    println!("{:<16} {}", t("Telemetry", "Telemetri"), yes_no(app.telemetry));
    if let Some(source) = &app.privacy_source {
        println!("{:<16} {source}", t("Source", "Kaynak"));
    }
    if let Some(note) = &app.note {
        println!("{:<16} {}", t("Note", "Not"), note.get(lang));
    }
    if !app.alternatives.is_empty() {
        let names: Vec<String> = app.alternatives.iter().map(name_of).collect();
        println!("{:<16} {}", t("Alternatives", "Alternatifler"), names.join(", "));
    }
    if !app.conflicts.is_empty() {
        let names: Vec<String> = app.conflicts.iter().map(name_of).collect();
        println!("{:<16} {}", t("Conflicts with", "Çakıştığı"), names.join(", "));
    }
    println!(
        "{:<16} {}",
        t("Tags", "Etiketler"),
        skadi_core::catalog::tags(app).join(", ")
    );
    if let Some(url) = skadi_core::catalog::screenshots_url(app) {
        println!("{:<16} {url}", t("Screenshots", "Ekran görüntüleri"));
    }
    for note in skadi_core::catalog::community_notes()?
        .iter()
        .filter(|n| n.app == app.id)
    {
        println!(
            "{:<16} {} ({})",
            t("Community note", "Topluluk notu"),
            note.text.get(lang),
            note.source
        );
    }
    println!(
        "{:<16} {}",
        t("Installs with", "Kurulacağı yer"),
        via.map_or(t("not available here", "burada kurulamaz"), |m| m.as_str())
    );
    for (manager, package) in &app.packages {
        println!("  {:<12} {package}", manager.as_str());
    }
    Ok(())
}

pub fn apply(lang: Lang, out: &Output, options: &OpOptions, op: Op) -> Result<()> {
    let t = |en, tr| lang.pick(en, tr);
    // Betik istendiğinde hiçbir şey çalıştırılmaz; komutlar yalnızca dosyaya yazılır.
    let dry_run = options.dry_run || out.script.is_some();
    let catalog = Catalog::builtin()?;
    let mut system = System::detect()?;
    // Yarıda kalan kurulum: istenen uygulamalar günlükte durur; kurulu olanlar zaten atlanır.
    let journal = skadi_core::paths::data_dir(system.os).map(|dir| dir.join("pending-install.txt"));
    let requested: Vec<String> = if options.resume {
        if op != Op::Install {
            return Err(Error::InvalidOption("--resume"));
        }
        let ids: Vec<String> = journal
            .as_ref()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .map(|text| {
                text.lines()
                    .map(str::trim)
                    .filter(|l| !l.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        if ids.is_empty() {
            println!(
                "{}",
                t("There is no unfinished install.", "Yarıda kalmış bir kurulum yok.")
            );
            return Ok(());
        }
        ids
    } else {
        options.apps.clone()
    };
    if op == Op::Install
        && let Some(policy) = skadi_core::policy::load(system.os)?
        && let Some(blocked) = requested.iter().find(|id| !policy.allows_app(id))
    {
        return Err(Error::Policy(format!("{blocked} is not on the allowed list")));
    }
    let apps = requested
        .iter()
        .map(|id| catalog.get(id).ok_or_else(|| Error::UnknownApp(id.clone())))
        .collect::<Result<Vec<&App>>>()?;
    let forced = options.manager.as_deref().map(str::parse::<ManagerId>).transpose()?;
    let mut current = plan(&apps, system.os, &system.available_ids(), forced);

    // Birlikte kurulmaması gereken uygulamalar seçildiyse kullanıcı uyarılır.
    if op == Op::Install {
        let selected: Vec<&str> = apps.iter().map(|a| a.id.as_str()).collect();
        let clashes: Vec<String> = apps
            .iter()
            .flat_map(|a| {
                a.conflicts
                    .iter()
                    .filter(|c| selected.contains(&c.as_str()))
                    .map(move |c| (a.id.as_str(), c.as_str()))
            })
            .filter(|(a, b)| a < b)
            .map(|(a, b)| format!("{a} / {b}"))
            .collect();
        if !clashes.is_empty() {
            println!(
                "{}: {}",
                t("These apps conflict", "Bu uygulamalar çakışıyor"),
                clashes.join(", ")
            );
            if !dry_run && !confirm(lang, options.yes, t("Install them anyway?", "Yine de kurulsun mu?"))? {
                return Ok(());
            }
        }
    }

    // Eksik yöneticiler yalnızca kurulumda ve kullanıcı onayıyla kurulur.
    if op == Op::Install && !current.unresolved.is_empty() {
        let mut wanted: Vec<ManagerId> = Vec::new();
        for unresolved in &current.unresolved {
            let candidate = unresolved
                .candidates
                .iter()
                .copied()
                .find(|id| id.spec().bootstrap.is_some());
            if let Some(id) = candidate.filter(|id| !wanted.contains(id)) {
                wanted.push(id);
            }
        }
        for id in wanted {
            if system.manager(id).is_some() {
                continue; // Aynı programı paylaşan yönetici (brew / brew-cask) zaten kuruldu.
            }
            if let Some(bootstrap) = id.spec().bootstrap {
                println!("{id} {}", t("is not installed.", "kurulu değil."));
                println!("{}", bootstrap.describe(lang));
            }
            if dry_run {
                continue;
            }
            if confirm(
                lang,
                options.yes,
                &format!("{id} {}", t("install it now?", "şimdi kurulsun mu?")),
            )? {
                system.bootstrap(id)?;
            }
        }
        if !dry_run {
            current = plan(&apps, system.os, &system.available_ids(), forced);
        }
    }

    for unresolved in &current.unresolved {
        eprintln!(
            "{}: {} {}",
            t("warning", "uyarı"),
            unresolved.app,
            t("cannot be handled on this system", "bu sistemde işlenemiyor")
        );
    }

    let preview = preview_system(lang, &system, dry_run);
    let runner = preview.as_ref().unwrap_or(&system);

    if op == Op::Install && !dry_run && !current.steps.is_empty() {
        if let Some(free) = system.free_space().filter(|free| *free < MIN_FREE_BYTES) {
            println!(
                "{} {:.1} GiB",
                t(
                    "Low disk space on the system drive:",
                    "Sistem sürücüsünde az boş alan var:"
                ),
                free as f64 / 1024.0 / 1024.0 / 1024.0
            );
            if !confirm(lang, options.yes, t("Continue anyway?", "Yine de devam edilsin mi?"))? {
                return Ok(());
            }
        }
    }

    if op == Op::Install && !dry_run {
        if let Some(path) = &journal {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(path, requested.join("\n"));
        }
    }

    let mut script = Vec::new();
    for step in &current.steps {
        let (step, skipped) = if dry_run {
            (step.clone(), Vec::new())
        } else {
            system.prune(step, op)
        };
        for item in skipped {
            let state = match op {
                Op::Install => t("already installed", "zaten kurulu"),
                Op::Remove | Op::Purge => t("not installed", "kurulu değil"),
            };
            println!("{}: {state}", item.app);
        }
        for invocation in runner.invocations(&step, op)? {
            println!("> {}", invocation.display());
            if !dry_run {
                invocation.run()?;
            }
            script.push(invocation);
        }
    }
    // Buraya ancak her adım başarılı olursa gelinir; günlük temizlenir.
    if op == Op::Install && !dry_run {
        if let Some(path) = &journal {
            let _ = std::fs::remove_file(path);
        }
    }
    match &out.script {
        Some(path) => crate::output::write_script(lang, path, &script),
        None => Ok(()),
    }
}

pub fn bootstrap(lang: Lang, manager: &str, yes: bool, dry_run: bool) -> Result<()> {
    let t = |en, tr| lang.pick(en, tr);
    let id = manager.parse::<ManagerId>()?;
    let mut system = System::detect()?;
    if system.manager(id).is_some() {
        println!("{id} {}", t("is already installed.", "zaten kurulu."));
        return Ok(());
    }
    let bootstrap = id
        .spec()
        .bootstrap
        .filter(|_| id.spec().os == system.os)
        .ok_or(Error::BootstrapUnavailable(id))?;
    println!("{}", bootstrap.describe(lang));
    if dry_run || !confirm(lang, yes, t("Continue?", "Devam edilsin mi?"))? {
        return Ok(());
    }
    system.bootstrap(id)?;
    println!("{id} {}", t("is ready.", "hazır."));
    Ok(())
}

/// Önizlemede Windows komutları, yönetici terminalinde çalışacakları haliyle kurulur; yönetici
/// olmayan bir terminalde de kullanıcı ne çalışacağını görebilsin diye. Gerekmiyorsa `None`.
pub(crate) fn preview_system(lang: Lang, system: &System, preview: bool) -> Option<System> {
    if !preview || system.os != skadi_core::platform::Os::Windows || system.privileges.is_elevated {
        return None;
    }
    println!(
        "{}",
        lang.pick(
            "Note: some of these commands need an administrator terminal.",
            "Not: bu komutların bazıları yönetici terminali gerektirir."
        )
    );
    let mut elevated = system.clone();
    elevated.privileges.is_elevated = true;
    Some(elevated)
}

pub(crate) fn confirm(lang: Lang, yes: bool, question: &str) -> Result<bool> {
    if yes {
        return Ok(true);
    }
    let io_error = |source| Error::Spawn {
        program: "stdin".to_owned(),
        source,
    };
    print!("{question} [{}] ", lang.pick("y/N", "e/H"));
    io::stdout().flush().map_err(io_error)?;
    let mut answer = String::new();
    io::stdin().lock().read_line(&mut answer).map_err(io_error)?;
    Ok(matches!(
        answer.trim().to_lowercase().as_str(),
        "y" | "yes" | "e" | "evet"
    ))
}
