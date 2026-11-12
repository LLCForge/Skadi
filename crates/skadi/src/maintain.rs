use std::{ffi::OsString, path::PathBuf};

use serde_json::json;
use skadi_core::{
    Error, Lang, Result,
    catalog::{App, Catalog},
    exec::{Invocation, Privilege, find_program},
    manager::ManagerId,
    platform::Os,
    schedule,
    system::System,
};

use crate::{commands, commands::confirm, output::Output};

/// Katalogdaki uygulamalardan bu sistemde kurulu olanlar.
pub fn installed(lang: Lang, out: &Output) -> Result<()> {
    let catalog = Catalog::builtin()?;
    let system = System::detect()?;
    let found = system.installed_apps(catalog.apps());
    if out.json {
        let rows: Vec<_> = found
            .iter()
            .map(|(app, manager)| json!({ "id": app.id, "name": app.name, "manager": manager.as_str() }))
            .collect();
        return out.print_json(&json!({ "installed": rows }));
    }
    for (app, manager) in &found {
        println!("  {:<24} {:<10} {}", app.id, manager.as_str(), app.name);
    }
    println!(
        "\n{} {}",
        found.len(),
        lang.pick("catalog apps are installed.", "katalog uygulaması kurulu.")
    );
    Ok(())
}

pub struct UpdateOptions {
    pub check: bool,
    pub firmware: bool,
    pub manager: Option<String>,
    pub dry_run: bool,
    pub yes: bool,
    /// Bitince masaüstü bildirimi göster (zamanlanmış güncellemeler).
    pub notify: bool,
}

fn selected_managers(system: &System, manager: Option<&str>) -> Result<Vec<ManagerId>> {
    match manager {
        Some(name) => {
            let id = name.parse::<ManagerId>()?;
            system.manager(id).ok_or(Error::ManagerNotFound(id.as_str()))?;
            Ok(vec![id])
        }
        None => Ok(system.available_ids()),
    }
}

pub fn update(lang: Lang, out: &Output, options: &UpdateOptions) -> Result<()> {
    let t = |en, tr| lang.pick(en, tr);
    let system = System::detect()?;
    let managers = selected_managers(&system, options.manager.as_deref())?;

    if options.check {
        let mut report = Vec::new();
        for id in managers {
            let result = system.outdated(id);
            if out.json {
                report.push(match &result {
                    Ok(Some(list)) => json!({ "manager": id.as_str(), "supported": true, "packages": list }),
                    Ok(None) => json!({ "manager": id.as_str(), "supported": false }),
                    Err(error) => json!({ "manager": id.as_str(), "error": error.message(lang) }),
                });
                continue;
            }
            match result {
                Ok(Some(list)) if list.is_empty() => println!("{id}: {}", t("up to date", "güncel")),
                Ok(Some(list)) => {
                    println!("{id}:");
                    for package in list {
                        println!(
                            "  {:<40} {} -> {}",
                            package.package,
                            package.current.as_deref().unwrap_or("?"),
                            package.latest.as_deref().unwrap_or("?")
                        );
                    }
                }
                Ok(None) => println!(
                    "{id}: {}",
                    t(
                        "cannot list updates; run `skadi update` to update",
                        "güncellemeleri listeleyemiyor; güncellemek için `skadi update` çalıştırın"
                    )
                ),
                Err(error) => eprintln!("{id}: {}", error.message(lang)),
            }
        }
        if out.json {
            out.print_json(&json!({ "updates": report }))?;
        }
        if options.firmware {
            firmware(lang, &system, true)?;
        }
        return Ok(());
    }

    let preview = commands::preview_system(lang, &system, options.dry_run || out.script.is_some());
    let runner = preview.as_ref().unwrap_or(&system);
    let mut plan = Vec::new();
    let managers_for_snapshot = managers.clone();
    for id in managers {
        match runner.upgrade_invocations(id) {
            Ok(list) => plan.extend(list),
            Err(Error::Unsupported(_)) => {}
            Err(error) => return Err(error),
        }
    }
    for invocation in &plan {
        println!("> {}", invocation.display());
    }
    if let Some(path) = &out.script {
        return crate::output::write_script(lang, path, &plan);
    }
    if options.dry_run || plan.is_empty() {
        return Ok(());
    }
    if !confirm(lang, options.yes, t("Update everything?", "Her şey güncellensin mi?"))? {
        return Ok(());
    }
    save_versions(&system, &managers_for_snapshot);
    let result = plan.iter().try_for_each(skadi_core::exec::Invocation::run);
    if options.notify {
        let body = match &result {
            Ok(()) => t("All apps are up to date.", "Tüm uygulamalar güncel."),
            Err(_) => t(
                "Some updates failed; run `skadi history` for details.",
                "Bazı güncellemeler başarısız oldu; ayrıntı için `skadi history`.",
            ),
        };
        schedule::notify(system.os, "Skadi", body);
    }
    result?;
    if options.firmware {
        firmware(lang, &system, false)?;
    }
    if system.os == Os::Linux && skadi_core::schedule::reboot_required() == Some(true) {
        println!(
            "{}",
            t(
                "A restart is needed to finish the updates (for example a new kernel).",
                "Güncellemelerin tamamlanması için yeniden başlatma gerekiyor (örneğin yeni çekirdek)."
            )
        );
    }
    Ok(())
}

/// fwupd ile firmware denetimi ya da güncellemesi. fwupdmgr "yapılacak bir şey yok" durumunu
/// 2 çıkış koduyla bildirir; bu bir hata sayılmaz.
fn firmware(lang: Lang, system: &System, check: bool) -> Result<()> {
    if system.os != Os::Linux {
        return Err(Error::InvalidOption("--firmware (Linux)"));
    }
    let fwupd = find_program("fwupdmgr").ok_or(Error::ProgramNotFound("fwupdmgr"))?;
    let steps: Vec<Vec<&str>> = if check {
        vec![vec!["get-updates"]]
    } else {
        vec![vec!["refresh"], vec!["update", "--assume-yes"]]
    };
    for args in steps {
        let invocation = Invocation::build(
            fwupd.clone(),
            args.iter().map(OsString::from).collect(),
            &[],
            Privilege::Any,
            &system.privileges,
            system.os,
        )?;
        println!("> {}", invocation.display());
        match invocation.run() {
            Ok(()) | Err(Error::CommandFailed { code: Some(2), .. }) => {}
            Err(error) => return Err(error),
        }
    }
    let _ = lang;
    Ok(())
}

/// Haftalık otomatik güncellemeyi kurar, kaldırır ya da durumunu gösterir.
pub fn schedule(lang: Lang, action: &str) -> Result<()> {
    let t = |en, tr| lang.pick(en, tr);
    let system = System::detect()?;
    let exe = std::env::current_exe().map_err(|e| Error::State(e.to_string()))?;
    if action == "enable" && !schedule::executable_is_protected(system.os, &exe) {
        return Err(Error::UnprotectedExecutable(exe.display().to_string()));
    }
    let run = |program: PathBuf, args: Vec<String>, privilege: Privilege| -> Result<std::process::Output> {
        let invocation = Invocation::build(
            program,
            args.into_iter().map(OsString::from).collect(),
            &[],
            privilege,
            &system.privileges,
            system.os,
        )?;
        if action == "status" {
            invocation.output()
        } else {
            println!("> {}", invocation.display());
            invocation.run_captured().map(|()| std::process::Output {
                status: Default::default(),
                stdout: Vec::new(),
                stderr: Vec::new(),
            })
        }
    };
    let enabled = match system.os {
        Os::Windows => {
            let schtasks = find_program("schtasks").ok_or(Error::ProgramNotFound("schtasks"))?;
            let task = schedule::WINDOWS_TASK.to_owned();
            let args: Vec<String> = match action {
                "enable" => vec![
                    "/Create".into(),
                    "/TN".into(),
                    task,
                    "/TR".into(),
                    format!("\"{}\" update --yes --notify", exe.display()),
                    "/SC".into(),
                    "WEEKLY".into(),
                    "/D".into(),
                    "SUN".into(),
                    "/ST".into(),
                    "12:00".into(),
                    "/RL".into(),
                    "HIGHEST".into(),
                    "/F".into(),
                ],
                "disable" => vec!["/Delete".into(), "/TN".into(), task, "/F".into()],
                _ => vec!["/Query".into(), "/TN".into(), task],
            };
            let privilege = if action == "status" {
                Privilege::Any
            } else {
                Privilege::Root
            };
            run(schtasks, args, privilege).map(|out| out.status.success())
        }
        Os::Linux => match action {
            "status" => {
                let systemctl = find_program("systemctl").ok_or(Error::ProgramNotFound("systemctl"))?;
                let out = run(
                    systemctl,
                    vec!["is-enabled".into(), "skadi-update.timer".into()],
                    Privilege::Any,
                )?;
                Ok(String::from_utf8_lossy(&out.stdout).trim() == "enabled")
            }
            _ => {
                let verb = if action == "enable" { "install" } else { "remove" };
                run(
                    exe.clone(),
                    vec!["internal".into(), "schedule-units".into(), verb.into()],
                    Privilege::Root,
                )
                .map(|_| action == "enable")
            }
        },
        Os::Macos => {
            let path = schedule::launchd_path().ok_or_else(|| Error::State("no home directory".into()))?;
            let launchctl = find_program("launchctl").ok_or(Error::ProgramNotFound("launchctl"))?;
            match action {
                "enable" => {
                    if let Some(dir) = path.parent() {
                        std::fs::create_dir_all(dir).map_err(|e| Error::State(e.to_string()))?;
                    }
                    std::fs::write(&path, schedule::launchd_plist(&exe)).map_err(|e| Error::State(e.to_string()))?;
                    run(
                        launchctl,
                        vec!["load".into(), "-w".into(), path.display().to_string()],
                        Privilege::NonRoot,
                    )
                    .map(|_| true)
                }
                "disable" => {
                    let _ = run(
                        launchctl,
                        vec!["unload".into(), "-w".into(), path.display().to_string()],
                        Privilege::NonRoot,
                    );
                    let _ = std::fs::remove_file(&path);
                    Ok(false)
                }
                _ => Ok(path.exists()),
            }
        }
    }?;
    let state = if enabled {
        t("Weekly automatic updates are on.", "Haftalık otomatik güncelleme açık.")
    } else {
        t(
            "Weekly automatic updates are off.",
            "Haftalık otomatik güncelleme kapalı.",
        )
    };
    println!("{state}");
    Ok(())
}

/// `skadi internal schedule-units`: systemd birimlerini root olarak yazar ya da kaldırır.
pub fn schedule_units(action: &str) -> Result<()> {
    let exe = std::env::current_exe().map_err(|e| Error::State(e.to_string()))?;
    let systemctl = find_program("systemctl").ok_or(Error::ProgramNotFound("systemctl"))?;
    let run = |args: &[&str]| -> Result<()> {
        let status = std::process::Command::new(&systemctl)
            .args(args)
            .status()
            .map_err(|e| Error::State(e.to_string()))?;
        if status.success() {
            Ok(())
        } else {
            Err(Error::CommandFailed {
                program: "systemctl".into(),
                code: status.code(),
                detail: None,
            })
        }
    };
    if action == "install" {
        // Birim, root olarak çalışacağı için yalnızca korumalı konumdaki Skadi'yi gösterebilir.
        if !schedule::executable_is_protected(Os::Linux, &exe) {
            return Err(Error::UnprotectedExecutable(exe.display().to_string()));
        }
        let write =
            |path: &str, text: String| std::fs::write(path, text).map_err(|e| Error::State(format!("{path}: {e}")));
        write(schedule::SYSTEMD_SERVICE, schedule::systemd_service(&exe))?;
        write(schedule::SYSTEMD_TIMER, schedule::systemd_timer())?;
        run(&["daemon-reload"])?;
        run(&["enable", "--now", "skadi-update.timer"])
    } else {
        let _ = run(&["disable", "--now", "skadi-update.timer"]);
        for path in [schedule::SYSTEMD_SERVICE, schedule::SYSTEMD_TIMER] {
            let _ = std::fs::remove_file(path);
        }
        run(&["daemon-reload"])
    }
}

/// Denetim günlüğünden son komutlar.
pub fn history(lang: Lang, out: &Output, limit: usize) -> Result<()> {
    let system_os = Os::current()?;
    let entries = schedule::history(system_os)?;
    let recent = &entries[entries.len().saturating_sub(limit)..];
    if out.json {
        let rows: Vec<_> = recent
            .iter()
            .map(|e| json!({ "time": e.time, "command": e.command, "ok": e.ok }))
            .collect();
        return out.print_json(&json!({ "history": rows }));
    }
    if recent.is_empty() {
        println!("{}", lang.pick("No commands recorded yet.", "Henüz kayıtlı komut yok."));
    }
    for entry in recent {
        let state = if entry.ok { "ok " } else { lang.pick("ERR", "HATA") };
        println!("{} UTC  {state}  {}", schedule::format_utc(entry.time), entry.command);
    }
    Ok(())
}

/// Uygulamayı yöneticisinin kendi sabitleme mekanizmasıyla güncellemelere kapatır ya da açar.
pub fn pin(lang: Lang, apps: &[String], add: bool, dry_run: bool) -> Result<()> {
    let catalog = Catalog::builtin()?;
    let system = System::detect()?;
    let selected = apps
        .iter()
        .map(|id| catalog.get(id).ok_or_else(|| Error::UnknownApp(id.clone())))
        .collect::<Result<Vec<&App>>>()?;
    let installed = system.installed_apps(catalog.apps());
    for app in selected {
        // Önce uygulamanın gerçekten kurulu olduğu yönetici, yoksa sabitlemeyi destekleyen ilk yönetici.
        let manager = installed
            .iter()
            .find(|(a, _)| a.id == app.id)
            .map(|(_, m)| *m)
            .filter(|m| m.pin().is_some())
            .or_else(|| {
                system
                    .available_ids()
                    .into_iter()
                    .find(|m| m.pin().is_some() && app.packages.contains_key(m))
            })
            .ok_or(Error::Unsupported("pin"))?;
        let invocation = system.pin_invocation(manager, &app.packages[&manager], add)?;
        println!("> {}", invocation.display());
        if !dry_run {
            invocation.run_captured()?;
            let state = if add {
                lang.pick("pinned", "sabitlendi")
            } else {
                lang.pick("unpinned", "sabitleme kaldırıldı")
            };
            println!("{}: {state} ({manager})", app.id);
        }
    }
    Ok(())
}

/// Otomatik başlayan öğeler (yalnızca listeler; hiçbir şeyi değiştirmez).
pub fn startup(lang: Lang, out: &Output) -> Result<()> {
    let items = skadi_core::startup::startup_items(Os::current()?);
    let advice = skadi_core::startup::advice_list();
    if out.json {
        return out.print_json(&json!({ "startup": items }));
    }
    let mut source = "";
    for item in &items {
        if item.source != source {
            source = &item.source;
            println!("\n{source}");
        }
        let state = match item.enabled {
            Some(true) => lang.pick("on ", "açık "),
            Some(false) => lang.pick("off", "kapalı"),
            None => "",
        };
        println!("  {state:<6} {:<32} {}", item.name, item.command);
        if item.enabled != Some(false)
            && let Some(text) = skadi_core::startup::advice_for(&advice, item)
        {
            println!("         {} {}", lang.pick("tip:", "öneri:"), text.get(lang));
        }
    }
    println!(
        "\n{} {}",
        items.iter().filter(|i| i.enabled != Some(false)).count(),
        lang.pick("items start automatically.", "öğe otomatik başlıyor.")
    );
    Ok(())
}

/// Güvenlik denetimi ve gizlilik puanı. Hiçbir şeyi değiştirmez.
pub fn audit(lang: Lang, out: &Output, html: Option<&std::path::Path>) -> Result<()> {
    use skadi_core::{
        audit::{Status, run_checks, score},
        tweak::{TweakCatalog, TweakCategory, TweakEngine, TweakStatus},
    };
    let t = |en, tr| lang.pick(en, tr);
    let system = System::detect()?;
    let checks = run_checks(system.os);
    let count = |status: Status| checks.iter().filter(|c| c.status == status).count();
    let security = score(count(Status::Pass), count(Status::Fail));

    // Gizlilik puanı: bu sistemde uygulanabilen gizlilik tweak'lerinden uygulanmış olanlar.
    let catalog = TweakCatalog::builtin()?;
    let engine = TweakEngine::new(&system);
    let privacy: Vec<_> = catalog
        .for_host(&system)
        .filter(|tweak| tweak.category == TweakCategory::Privacy)
        .collect();
    engine.prefetch(&privacy)?;
    let rows: Vec<(&str, TweakStatus)> = privacy
        .iter()
        .filter_map(|tweak| engine.status(tweak).ok().map(|s| (tweak.name.get(lang), s)))
        .collect();
    let statuses: Vec<TweakStatus> = rows.iter().map(|(_, s)| *s).collect();
    let applied = statuses.iter().filter(|s| **s == TweakStatus::Applied).count();
    let open = statuses
        .iter()
        .filter(|s| matches!(s, TweakStatus::NotApplied | TweakStatus::Partial))
        .count();
    let privacy_score = score(applied, open);

    if let Some(path) = html {
        let page = audit_html(lang, &system, &checks, security, privacy_score, &rows);
        std::fs::write(path, page).map_err(|e| Error::State(format!("{}: {e}", path.display())))?;
        println!("{} {}", t("Report written to", "Rapor yazıldı:"), path.display());
        return Ok(());
    }
    if out.json {
        return out.print_json(&json!({
            "checks": checks,
            "security_score": security,
            "privacy_score": privacy_score,
            "privacy_tweaks_applied": applied,
            "privacy_tweaks_open": open,
        }));
    }
    println!("{}", t("Security checks", "Güvenlik denetimi"));
    for check in &checks {
        let (mark, label) = match check.status {
            Status::Pass => ("+", t("pass", "geçti")),
            Status::Fail => ("x", t("FAIL", "KALDI")),
            Status::Unknown => ("?", t("unknown", "bilinmiyor")),
        };
        println!("  [{mark}] {:<44} {label}", check.label(lang));
    }
    let percent = |value: Option<u32>| value.map_or_else(|| "-".to_owned(), |v| format!("{v}%"));
    println!("\n{:<26} {}", t("Security score", "Güvenlik puanı"), percent(security));
    println!(
        "{:<26} {} ({applied}/{} {})",
        t("Privacy score", "Gizlilik puanı"),
        percent(privacy_score),
        applied + open,
        t("privacy tweaks applied", "gizlilik ayarı uygulanmış")
    );
    if open > 0 {
        println!(
            "\n{}",
            t(
                "Open privacy tweaks: skadi tweak list --category privacy",
                "Uygulanmamış gizlilik ayarları: skadi tweak list --category privacy"
            )
        );
    }
    Ok(())
}

/// Görüntülerden meta verileri siler. Varsayılan olarak özgün dosyaya dokunmaz.
pub fn clean_metadata(lang: Lang, files: &[PathBuf], in_place: bool) -> Result<()> {
    use skadi_core::metadata::{MAX_BYTES, strip};
    let mut first_error = None;
    for path in files {
        let result = (|| -> Result<(PathBuf, u64)> {
            let fail = |e: String| Error::Media(format!("{}: {e}", path.display()));
            let size = std::fs::metadata(path).map_err(|e| fail(e.to_string()))?.len();
            if size > MAX_BYTES {
                return Err(fail("file is too large".into()));
            }
            let bytes = std::fs::read(path).map_err(|e| fail(e.to_string()))?;
            let clean = strip(&bytes).map_err(|e| match e {
                Error::Media(reason) => fail(reason),
                other => other,
            })?;
            let removed = (bytes.len() - clean.len()) as u64;
            let target = if in_place {
                path.clone()
            } else {
                let stem = path
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let ext = path
                    .extension()
                    .map(|e| e.to_string_lossy().into_owned())
                    .unwrap_or_default();
                path.with_file_name(format!("{stem}.clean.{ext}"))
            };
            // Önce geçici dosyaya yazılır, sonra yerine taşınır: yarım yazma özgün dosyayı bozmaz.
            let temp = target.with_extension("skadi-tmp");
            std::fs::write(&temp, &clean).map_err(|e| fail(e.to_string()))?;
            std::fs::rename(&temp, &target).map_err(|e| fail(e.to_string()))?;
            Ok((target, removed))
        })();
        match result {
            Ok((target, removed)) => println!(
                "[ok] {} -> {} ({removed} {})",
                path.display(),
                target.display(),
                lang.pick("bytes of metadata removed", "bayt meta veri silindi")
            ),
            Err(error) => {
                eprintln!("[{}] {}", lang.pick("failed", "başarısız"), error.message(lang));
                first_error.get_or_insert(error);
            }
        }
    }
    first_error.map_or(Ok(()), Err)
}

/// Kurulu ve telemetri gönderen uygulamalar için katalogdaki alternatifler.
pub fn suggest(lang: Lang, out: &Output) -> Result<()> {
    let catalog = Catalog::builtin()?;
    let system = System::detect()?;
    let installed: Vec<&App> = system
        .installed_apps(catalog.apps())
        .into_iter()
        .map(|(app, _)| app)
        .filter(|app| app.telemetry == Some(true) && !app.alternatives.is_empty())
        .collect();
    if out.json {
        let rows: Vec<_> = installed
            .iter()
            .map(|app| json!({ "id": app.id, "alternatives": app.alternatives, "note": app.note.as_ref().map(|n| n.get(lang)) }))
            .collect();
        return out.print_json(&json!({ "suggestions": rows }));
    }
    if installed.is_empty() {
        println!(
            "{}",
            lang.pick(
                "No installed app with known telemetry has a listed alternative.",
                "Telemetrisi bilinen ve alternatifi listelenmiş kurulu bir uygulama yok."
            )
        );
    }
    for app in installed {
        let names: Vec<String> = app
            .alternatives
            .iter()
            .filter_map(|id| catalog.get(id))
            .map(|alt| format!("{} ({})", alt.name, alt.id))
            .collect();
        println!("{} ({})", app.name, app.id);
        if let Some(note) = &app.note {
            println!("  {}", note.get(lang));
        }
        println!("  {}: {}", lang.pick("Alternatives", "Alternatifler"), names.join(", "));
    }
    Ok(())
}

/// Kamera, mikrofon ve konum izni olan uygulamalar.
pub fn permissions(lang: Lang, out: &Output) -> Result<()> {
    use skadi_core::permissions::{CAPABILITIES, grants};
    let system = System::detect()?;
    if system.os != Os::Windows {
        let grants = skadi_core::permissions::app_grants(system.os).map_err(|detail| {
            if system.os == Os::Macos {
                Error::State(format!(
                    "{} ({detail})",
                    lang.pick(
                        "give your terminal Full Disk Access in System Settings > Privacy & Security, then try again",
                        "Sistem Ayarları > Gizlilik ve Güvenlik'ten terminalinize Tam Disk Erişimi verip yeniden deneyin"
                    )
                ))
            } else {
                Error::State(detail)
            }
        })?;
        if out.json {
            return out.print_json(&json!({ "permissions": grants }));
        }
        if grants.is_empty() {
            println!(
                "{}",
                lang.pick("No app permissions recorded.", "Kayıtlı uygulama izni yok.")
            );
        }
        for grant in grants {
            println!("  {:<24} {:<40} {}", grant.kind, grant.app, grant.value);
        }
        return Ok(());
    }
    let all = grants();
    if out.json {
        return out.print_json(&json!({ "permissions": all }));
    }
    for capability in CAPABILITIES {
        let title = match capability {
            "webcam" => lang.pick("Camera", "Kamera"),
            "microphone" => lang.pick("Microphone", "Mikrofon"),
            _ => lang.pick("Location", "Konum"),
        };
        let allowed: Vec<_> = all.iter().filter(|g| g.capability == capability && g.allowed).collect();
        println!("\n{title} ({})", allowed.len());
        for grant in allowed {
            let used = grant.last_used.map_or_else(
                || lang.pick("never used", "hiç kullanılmadı").to_owned(),
                |t| {
                    format!(
                        "{} {} UTC",
                        lang.pick("last used", "son kullanım"),
                        schedule::format_utc(t)
                    )
                },
            );
            println!("  {:<60} {used}", grant.app);
        }
    }
    Ok(())
}

/// Bir açılış öğesini siler gibi değil, geri alınabilir biçimde kapatır ya da yeniden açar.
pub fn startup_toggle(lang: Lang, name: &str, enable: bool, dry_run: bool) -> Result<()> {
    use skadi_core::startup::{Toggle, startup_items, toggle};
    let detected = System::detect()?;
    let mut matches: Vec<_> = startup_items(detected.os)
        .into_iter()
        .filter(|item| item.name.eq_ignore_ascii_case(name))
        .collect();
    // Kapatılmış bir systemd birimi listede görünmez; yeniden açmak için adıyla seçilebilir.
    if matches.is_empty() && enable && detected.os == Os::Linux && skadi_core::startup::is_managed_unit(name) {
        matches.push(skadi_core::startup::StartupItem {
            source: "systemd".to_owned(),
            name: name.to_owned(),
            command: String::new(),
            enabled: Some(false),
        });
    }
    if matches.is_empty() {
        return Err(Error::UnknownStartupItem(name.to_owned()));
    }
    let preview = commands::preview_system(lang, &detected, dry_run);
    let system = preview.as_ref().unwrap_or(&detected);
    for item in &matches {
        let change = item
            .enabled
            .and_then(|_| toggle(item, enable))
            .ok_or(Error::Unsupported("startup (protected system item)"))?;
        match change {
            Toggle::Registry {
                key,
                name,
                data,
                machine,
            } => {
                let privilege = if machine { Privilege::Root } else { Privilege::Any };
                let invocation = crate::net::build(
                    system,
                    "reg",
                    &["add", &key, "/v", &name, "/t", "REG_BINARY", "/d", data, "/f"],
                    privilege,
                )?;
                println!("> {}", invocation.display());
                if !dry_run {
                    invocation.run_captured()?;
                }
            }
            Toggle::Command { program, args, root } => {
                let args: Vec<&str> = args.iter().map(String::as_str).collect();
                let privilege = if root { Privilege::Root } else { Privilege::Any };
                let invocation = crate::net::build(system, program, &args, privilege)?;
                println!("> {}", invocation.display());
                if !dry_run {
                    invocation.run_captured()?;
                }
            }
            Toggle::File { path, contents } => {
                // sudo altında HOME root'a ait olabilir; kullanıcının dosyası kendi yetkisiyle yazılır.
                if system.privileges.is_elevated {
                    return Err(Error::MustNotBeRoot);
                }
                println!("> {} (Hidden={})", path.display(), !enable);
                if !dry_run {
                    let write = |dir: &std::path::Path| {
                        std::fs::create_dir_all(dir).and_then(|()| std::fs::write(&path, &contents))
                    };
                    path.parent()
                        .map_or(Ok(()), write)
                        .map_err(|e| Error::State(format!("{}: {e}", path.display())))?;
                }
            }
        }
    }
    if !dry_run {
        println!(
            "{}",
            if enable {
                lang.pick("Turned on.", "Açıldı.")
            } else {
                lang.pick(
                    "Turned off; it can be turned on again any time.",
                    "Kapatıldı; istediğiniz zaman yeniden açabilirsiniz.",
                )
            }
        );
    }
    Ok(())
}

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// Yazdırılabilir (tarayıcıdan PDF'e) tek dosyalık uyumluluk raporu. Betik ve dış kaynak içermez;
/// bilgisayar ve kullanıcı adı yer almaz.
fn audit_html(
    lang: Lang,
    system: &System,
    checks: &[skadi_core::audit::Check],
    security: Option<u32>,
    privacy: Option<u32>,
    tweaks: &[(&str, skadi_core::tweak::TweakStatus)],
) -> String {
    use skadi_core::{audit::Status, tweak::TweakStatus};
    let t = |en, tr| lang.pick(en, tr);
    let report = skadi_core::care::report(system.os);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let percent = |value: Option<u32>| value.map_or_else(|| "-".to_owned(), |v| format!("{v}%"));
    let mut rows = String::new();
    for check in checks {
        let (class, label) = match check.status {
            Status::Pass => ("pass", t("Pass", "Geçti")),
            Status::Fail => ("fail", t("Fail", "Kaldı")),
            Status::Unknown => ("unknown", t("Unknown", "Bilinmiyor")),
        };
        rows.push_str(&format!(
            "<tr><td>{}</td><td class=\"{class}\">{label}</td></tr>\n",
            escape_html(check.label(lang))
        ));
    }
    let mut tweak_rows = String::new();
    for (name, status) in tweaks {
        let (class, label) = match status {
            TweakStatus::Applied => ("pass", t("Applied", "Uygulandı")),
            TweakStatus::Partial => ("unknown", t("Partly applied", "Kısmen")),
            _ => ("fail", t("Not applied", "Uygulanmadı")),
        };
        tweak_rows.push_str(&format!(
            "<tr><td>{}</td><td class=\"{class}\">{label}</td></tr>\n",
            escape_html(name)
        ));
    }
    format!(
        "<!doctype html>\n<html lang=\"{code}\"><head><meta charset=\"utf-8\">\n\
         <meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; style-src 'unsafe-inline'\">\n\
         <title>{title}</title>\n<style>\n\
         body{{font-family:system-ui,sans-serif;max-width:52rem;margin:2rem auto;padding:0 1rem;color:#1a1a1a;line-height:1.5}}\n\
         table{{width:100%;border-collapse:collapse;margin:1rem 0}}td,th{{text-align:left;padding:.4rem .6rem;border-bottom:1px solid #ddd}}\n\
         .pass{{color:#11663a;font-weight:600}}.fail{{color:#a3121a;font-weight:600}}.unknown{{color:#6b6b6b}}\n\
         .scores{{display:flex;gap:2rem}}.scores strong{{display:block;font-size:1.6rem}}\n\
         </style></head><body>\n\
         <h1>{title}</h1>\n<p>{generated} {time} UTC &middot; {os} &middot; {arch} &middot; Skadi {version}</p>\n\
         <section class=\"scores\"><div><span>{sec_label}</span><strong>{sec}</strong></div><div><span>{priv_label}</span><strong>{priv}</strong></div></section>\n\
         <h2>{checks_title}</h2>\n<table><tr><th>{check_col}</th><th>{result_col}</th></tr>\n{rows}</table>\n\
         <h2>{tweaks_title}</h2>\n<table><tr><th>{tweak_col}</th><th>{result_col}</th></tr>\n{tweak_rows}</table>\n\
         </body></html>\n",
        code = lang.pick("en", "tr"),
        title = t("Security and privacy report", "Güvenlik ve gizlilik raporu"),
        generated = t("Generated", "Oluşturulma"),
        time = schedule::format_utc(now),
        os = escape_html(report.os.as_deref().unwrap_or("?")),
        arch = escape_html(&report.arch),
        version = env!("CARGO_PKG_VERSION"),
        sec_label = t("Security score", "Güvenlik puanı"),
        sec = percent(security),
        priv_label = t("Privacy score", "Gizlilik puanı"),
        priv = percent(privacy),
        checks_title = t("Security checks", "Güvenlik denetimleri"),
        check_col = t("Check", "Denetim"),
        result_col = t("Result", "Sonuç"),
        tweaks_title = t("Privacy settings", "Gizlilik ayarları"),
        tweak_col = t("Setting", "Ayar"),
    )
}

/// Komutları ve tweak değişikliklerini güne göre birleştiren zaman çizelgesi.
pub fn timeline(lang: Lang, limit: usize) -> Result<()> {
    let os = Os::current()?;
    let mut events: Vec<(u64, String)> = schedule::history(os)?
        .into_iter()
        .map(|e| {
            let state = if e.ok { "ok " } else { lang.pick("ERR", "HATA") };
            (e.time, format!("{state}  {}", e.command))
        })
        .collect();
    if let Ok(store) = skadi_core::tweak::StateStore::open(os) {
        events.extend(
            store
                .applied_times()
                .into_iter()
                .map(|(id, time)| (time, format!("{}  {id}", lang.pick("tweak", "ayar")))),
        );
    }
    events.sort_by_key(|(time, _)| *time);
    let recent = &events[events.len().saturating_sub(limit)..];
    if recent.is_empty() {
        println!(
            "{}",
            lang.pick("No changes recorded yet.", "Henüz kayıtlı değişiklik yok.")
        );
    }
    let mut day = String::new();
    for (time, text) in recent {
        let stamp = schedule::format_utc(*time);
        let (date, clock) = stamp.split_once(' ').unwrap_or((stamp.as_str(), ""));
        if date != day {
            day = date.to_owned();
            println!("\n{date}");
        }
        println!("  {clock} UTC  {text}");
    }
    Ok(())
}

const VERSION_SNAPSHOTS: &str = "versions-before-update.json";
const MAX_SNAPSHOTS: usize = 5;

/// Güncellemeden önce kurulu sürümleri kaydeder (son 5 güncelleme); geri alma bunu kullanır.
fn save_versions(system: &System, managers: &[ManagerId]) {
    let Some(dir) = skadi_core::paths::data_dir(system.os) else {
        return;
    };
    let mut snapshot = serde_json::Map::new();
    for id in managers {
        if let Ok(Some(versions)) = system.installed_versions(*id) {
            snapshot.insert(id.as_str().to_owned(), json!(versions));
        }
    }
    if snapshot.is_empty() {
        return;
    }
    let path = dir.join(VERSION_SNAPSHOTS);
    let mut all: Vec<serde_json::Value> = std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    all.push(json!({ "time": now, "versions": snapshot }));
    let keep = all.len().saturating_sub(MAX_SNAPSHOTS);
    let all: Vec<_> = all.into_iter().skip(keep).collect();
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(&path, serde_json::to_string(&all).unwrap_or_default());
}

/// Bir paketi son güncellemeden önceki sürümüne döndürür.
pub fn rollback(lang: Lang, package: &str, manager: Option<&str>, dry_run: bool, yes: bool) -> Result<()> {
    let t = |en, tr| lang.pick(en, tr);
    let system = System::detect()?;
    let snapshots: Vec<serde_json::Value> = skadi_core::paths::data_dir(system.os)
        .map(|dir| dir.join(VERSION_SNAPSHOTS))
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    let preview = commands::preview_system(lang, &system, dry_run);
    let runner = preview.as_ref().unwrap_or(&system);
    for id in selected_managers(&system, manager)? {
        if id.rollback().is_none() {
            continue;
        }
        let invocation = if id == ManagerId::Snap {
            if !system.is_installed(id, package)?.unwrap_or(false) {
                continue;
            }
            runner.rollback_invocation(id, package, None)?
        } else {
            let Some(current) = system.installed_versions(id)?.and_then(|v| v.get(package).cloned()) else {
                continue;
            };
            // En yeni kayıttan geriye doğru, şu ankinden farklı ilk sürüm.
            let previous = snapshots.iter().rev().find_map(|snap| {
                snap["versions"][id.as_str()][package]
                    .as_str()
                    .filter(|v| *v != current)
                    .map(str::to_owned)
            });
            let Some(previous) = previous else {
                return Err(Error::State(format!(
                    "{package}: {}",
                    t(
                        "no earlier version was recorded; Skadi records versions when it runs an update",
                        "önceki sürüm kaydı yok; Skadi sürümleri bir güncelleme çalıştırırken kaydeder"
                    )
                )));
            };
            println!("{id}: {package} {current} -> {previous}");
            runner.rollback_invocation(id, package, Some(&previous))?
        };
        println!("> {}", invocation.display());
        if dry_run || !confirm(lang, yes, t("Go back to this version?", "Bu sürüme dönülsün mü?"))? {
            return Ok(());
        }
        return invocation.run();
    }
    Err(Error::State(format!(
        "{package}: {}",
        t(
            "not installed with a package manager that can go back to an earlier version",
            "önceki sürüme dönebilen bir paket yöneticisiyle kurulu değil"
        )
    )))
}
