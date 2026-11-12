//! `skadi clean`, `power`, `report`, `du` ve `repair`: bakım komutları.

use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    time::Duration,
};

use serde_json::json;
use skadi_core::{
    Error, Lang, Result,
    care::{self, WINDOWS_PLANS, is_guid, parse_powercfg_list},
    exec::{Invocation, Privilege},
    manager::ManagerId,
    platform::Os,
    system::System,
};

use crate::{commands, commands::confirm, net::build, output::Output};

/// Geçici dosyalar bu süreden eskiyse silinir; açık programların dosyaları korunur.
const TEMP_MIN_AGE: Duration = Duration::from_secs(7 * 86_400);

pub fn size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// Paket yöneticisi önbellekleri, artık bağımlılıklar ve eski geçici dosyalar.
pub fn clean(lang: Lang, out: &Output, dry_run: bool, yes: bool) -> Result<()> {
    let t = |en, tr| lang.pick(en, tr);
    let system = System::detect()?;
    let preview = commands::preview_system(lang, &system, dry_run || out.script.is_some());
    let runner = preview.as_ref().unwrap_or(&system);
    let mut plan = Vec::new();
    for id in system
        .available_ids()
        .into_iter()
        .filter(|id| *id != ManagerId::Msstore)
    {
        plan.extend(runner.cleanup_invocations(id)?);
    }
    for invocation in &plan {
        println!("> {}", invocation.display());
    }
    if let Some(path) = &out.script {
        return crate::output::write_script(lang, path, &plan);
    }

    // sudo altında /tmp gibi paylaşılan klasörlerde başka kullanıcıların dosyaları silinmesin.
    let roots = if system.os != Os::Windows && system.privileges.is_elevated {
        println!(
            "{}",
            t(
                "Temporary files are skipped when running as root; run without sudo to clean them.",
                "Root olarak çalışırken geçici dosyalar atlanır; temizlemek için sudo olmadan çalıştırın."
            )
        );
        Vec::new()
    } else {
        care::temp_roots(system.os)
    };
    for root in &roots {
        let found = care::sweep(root, TEMP_MIN_AGE, false);
        println!(
            "  {}: {} {}, {}",
            root.display(),
            found.files,
            t("files older than 7 days", "dosya 7 günden eski"),
            size(found.bytes)
        );
    }
    if dry_run || (plan.is_empty() && roots.is_empty()) {
        return Ok(());
    }
    if !confirm(lang, yes, t("Clean up now?", "Şimdi temizlensin mi?"))? {
        return Ok(());
    }
    // Bir yöneticinin hatası diğerlerini durdurmaz.
    let mut failed = None;
    for invocation in &plan {
        if let Err(error) = invocation.run() {
            eprintln!("{}", error.message(lang));
            failed = Some(error);
        }
    }
    let mut freed = care::Sweep::default();
    for root in &roots {
        let done = care::sweep(root, TEMP_MIN_AGE, true);
        freed.files += done.files;
        freed.bytes += done.bytes;
        freed.skipped += done.skipped;
    }
    if !roots.is_empty() {
        println!(
            "{}: {} {} ({}), {} {}",
            t("Temporary files removed", "Silinen geçici dosyalar"),
            freed.files,
            t("files", "dosya"),
            size(freed.bytes),
            freed.skipped,
            t("in use or protected", "kullanımda ya da korumalı")
        );
    }
    failed.map_or(Ok(()), Err)
}

/// Güç planları (Windows) ve güç profilleri (Linux, power-profiles-daemon).
pub fn power(lang: Lang, out: &Output, target: Option<&str>) -> Result<()> {
    let system = System::detect()?;
    match system.os {
        Os::Windows => windows_power(lang, out, &system, target),
        // power-profiles-daemon yoksa TLP: `tlp ac`/`tlp bat` elle kipe geçer, `tlp start` otomatiğe döner.
        Os::Linux
            if skadi_core::exec::find_program("powerprofilesctl").is_none()
                && skadi_core::exec::find_program("tlp").is_some() =>
        {
            let invocation = match target {
                None => build(&system, "tlp-stat", &["-s"], Privilege::Any)?,
                Some("ac") => build(&system, "tlp", &["ac"], Privilege::Root)?,
                Some("bat") => build(&system, "tlp", &["bat"], Privilege::Root)?,
                Some("auto") => build(&system, "tlp", &["start"], Privilege::Root)?,
                Some(_) => return Err(Error::InvalidOption("ac | bat | auto")),
            };
            invocation.run()
        }
        Os::Linux => {
            const PROFILES: [&str; 3] = ["performance", "balanced", "power-saver"];
            let invocation = match target {
                None => build(&system, "powerprofilesctl", &["list"], Privilege::Any)?,
                Some(name) if PROFILES.contains(&name) => {
                    build(&system, "powerprofilesctl", &["set", name], Privilege::Any)?
                }
                Some(_) => return Err(Error::InvalidOption("performance | balanced | power-saver")),
            };
            invocation.run()
        }
        Os::Macos => Err(Error::Unsupported("power (macOS)")),
    }
}

fn windows_power(lang: Lang, out: &Output, system: &System, target: Option<&str>) -> Result<()> {
    let list = || -> Result<Vec<care::PowerPlan>> {
        let output = build(system, "powercfg", &["/list"], Privilege::Any)?.output()?;
        Ok(parse_powercfg_list(&String::from_utf8_lossy(&output.stdout)))
    };
    let plans = list()?;
    let Some(target) = target else {
        if out.json {
            return out.print_json(&json!({ "plans": plans }));
        }
        for plan in &plans {
            let alias = WINDOWS_PLANS
                .iter()
                .find(|(_, g)| *g == plan.guid)
                .map_or("", |(a, _)| a);
            println!(
                "{} {} {:<10} {}",
                if plan.active { "*" } else { " " },
                plan.guid,
                alias,
                plan.name
            );
        }
        return Ok(());
    };
    let wanted = target.to_ascii_lowercase();
    let guid = WINDOWS_PLANS
        .iter()
        .find(|(alias, _)| *alias == wanted)
        .map(|(_, guid)| (*guid).to_owned())
        .or_else(|| is_guid(&wanted).then(|| wanted.clone()))
        .ok_or(Error::InvalidOption("balanced | high | saver | ultimate | GUID"))?;
    let guid = if plans.iter().any(|p| p.guid == guid) {
        guid
    } else if wanted == "ultimate" {
        // Nihai Performans çoğu sürümde gizlidir; bir kopyası oluşturulur ve yeni GUID kullanılır.
        let copy = build(system, "powercfg", &["-duplicatescheme", &guid], Privilege::Any)?;
        println!("> {}", copy.display());
        let text = String::from_utf8_lossy(&copy.output()?.stdout).into_owned();
        text.split_whitespace()
            .find(|w| is_guid(w))
            .map(str::to_ascii_lowercase)
            .ok_or(Error::Unsupported("Ultimate Performance"))?
    } else {
        return Err(Error::InvalidOption("powercfg /list"));
    };
    let activate = build(system, "powercfg", &["/setactive", &guid], Privilege::Any)?;
    println!("> {}", activate.display());
    activate.run_captured()?;
    println!("{}", lang.pick("Power plan changed.", "Güç planı değiştirildi."));
    Ok(())
}

/// Donanım ve disk sağlığı raporu (bilgisayar ve kullanıcı adı içermez).
pub fn report(lang: Lang, out: &Output) -> Result<()> {
    let t = |en, tr| lang.pick(en, tr);
    let system = System::detect()?;
    let report = care::report(system.os);
    if out.json {
        return out.print_json(&json!({ "report": report }));
    }
    let unknown = || "?".to_owned();
    println!(
        "{:<14} {}",
        t("System", "Sistem"),
        report.os.clone().unwrap_or_else(unknown)
    );
    println!(
        "{:<14} {}",
        t("Kernel", "Çekirdek"),
        report.kernel.clone().unwrap_or_else(unknown)
    );
    println!(
        "{:<14} {} ({} {}, {} {}, {})",
        t("Processor", "İşlemci"),
        report.cpu,
        report.physical_cores.map_or_else(unknown, |n| n.to_string()),
        t("cores", "çekirdek"),
        report.logical_cores,
        t("threads", "iş parçacığı"),
        report.arch
    );
    println!(
        "{:<14} {} / {} ({} {})",
        t("Memory", "Bellek"),
        size(report.memory_used),
        size(report.memory_total),
        t("swap", "takas"),
        size(report.swap_total)
    );
    println!(
        "{:<14} {} {} {} {}",
        t("Uptime", "Açık kalma"),
        report.uptime_secs / 86_400,
        t("days", "gün"),
        report.uptime_secs % 86_400 / 3_600,
        t("hours", "saat")
    );
    println!("\n{}", t("Disks", "Diskler"));
    for disk in &report.disks {
        println!(
            "  {:<20} {:<6} {:<4} {} / {} {}{}",
            disk.mount,
            disk.file_system,
            disk.kind,
            size(disk.available),
            size(disk.total),
            t("free", "boş"),
            if disk.removable {
                t(" (removable)", " (çıkarılabilir)")
            } else {
                ""
            }
        );
    }
    println!("\n{}", t("Disk health", "Disk sağlığı"));
    if report.health.is_empty() {
        println!(
            "  {}",
            t(
                "Not available (Linux needs smartmontools and root).",
                "Okunamadı (Linux'ta smartmontools ve root gerekir)."
            )
        );
    }
    for health in &report.health {
        let mark = match health.healthy {
            Some(true) => "[+]",
            Some(false) => "[x]",
            None => "[?]",
        };
        println!(
            "  {mark} {:<40} {:<6} {}",
            health.disk,
            health.media.as_deref().unwrap_or(""),
            health.status
        );
    }
    Ok(())
}

/// Bir klasörde en çok yer kaplayan öğeler.
pub fn du(lang: Lang, out: &Output, path: &Path, top: usize) -> Result<()> {
    let sizes = care::dir_sizes(path).map_err(|e| Error::State(format!("{}: {e}", path.display())))?;
    if out.json {
        let items: Vec<_> = sizes
            .iter()
            .take(top)
            .map(|(p, s)| json!({ "path": p.display().to_string(), "bytes": s }))
            .collect();
        return out.print_json(&json!({ "items": items }));
    }
    let total: u64 = sizes.iter().map(|(_, s)| s).sum();
    println!("{} {}: {}", path.display(), lang.pick("total", "toplam"), size(total));
    for (item, bytes) in sizes.iter().take(top) {
        println!("  {:>10}  {}", size(*bytes), item.display());
    }
    Ok(())
}

/// Sistem dosyası ve paket veritabanı onarımı.
pub fn repair(lang: Lang, dry_run: bool, yes: bool) -> Result<()> {
    let t = |en, tr| lang.pick(en, tr);
    let detected = System::detect()?;
    let preview = commands::preview_system(lang, &detected, dry_run);
    let system = preview.as_ref().unwrap_or(&detected);
    let plan: Vec<Invocation> = match system.os {
        Os::Windows => vec![
            build(
                system,
                "DISM",
                &["/Online", "/Cleanup-Image", "/RestoreHealth"],
                Privilege::Root,
            )?,
            build(system, "sfc", &["/scannow"], Privilege::Root)?,
        ],
        Os::Linux if system.manager(ManagerId::Apt).is_some() => vec![
            build(system, "dpkg", &["--configure", "-a"], Privilege::Root)?,
            build(system, "apt-get", &["install", "-f", "-y"], Privilege::Root)?,
        ],
        // `zypper verify` eksik bağımlılıkları bulur ve kurar.
        Os::Linux if system.manager(ManagerId::Zypper).is_some() => vec![build(
            system,
            "zypper",
            &["--non-interactive", "verify"],
            Privilege::Root,
        )?],
        // dnf ve pacman'in resmi bir onarım komutu yok; belgelenmiş denetimler çalıştırılır ve sonuç
        // raporlanır, sistem değiştirilmez.
        Os::Linux if system.manager(ManagerId::Dnf).is_some() => {
            println!(
                "{}",
                t(
                    "dnf has no automatic repair; checking the package database instead.",
                    "dnf'in otomatik onarımı yok; bunun yerine paket veritabanı denetleniyor."
                )
            );
            vec![build(system, "dnf", &["check"], Privilege::Any)?]
        }
        Os::Linux if system.manager(ManagerId::Pacman).is_some() => {
            println!(
                "{}",
                t(
                    "pacman has no automatic repair; checking the database and package files instead.",
                    "pacman'in otomatik onarımı yok; bunun yerine veritabanı ve paket dosyaları denetleniyor."
                )
            );
            vec![
                build(system, "pacman", &["-Dk"], Privilege::Any)?,
                build(system, "pacman", &["-Qk"], Privilege::Any)?,
            ]
        }
        Os::Linux => return Err(Error::Unsupported("repair (apt, zypper, dnf, pacman)")),
        Os::Macos => vec![build(system, "diskutil", &["verifyVolume", "/"], Privilege::Any)?],
    };
    for invocation in &plan {
        println!("> {}", invocation.display());
    }
    if dry_run
        || !confirm(
            lang,
            yes,
            t(
                "Run the repair? This can take a long time.",
                "Onarım çalıştırılsın mı? Uzun sürebilir.",
            ),
        )?
    {
        return Ok(());
    }
    plan.iter().try_for_each(Invocation::run)
}

/// Hata bildirimi taslağı: sürüm, sistem ve son komutlar. Bilgisayar adı, kullanıcı adı ve ev
/// klasörü yer almaz; yine de paylaşmadan önce gözden geçirilmesi istenir.
pub fn bug_report(lang: Lang) -> Result<()> {
    let system = System::detect()?;
    let home = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).ok();
    let user = std::env::var("USERNAME").or_else(|_| std::env::var("USER")).ok();
    let clean = |text: &str| care::redact(text, home.as_deref(), user.as_deref());
    let report = care::report(system.os);
    let managers: Vec<&str> = system.managers().iter().map(|m| m.spec.id.as_str()).collect();
    let portable = std::env::current_exe()
        .ok()
        .and_then(|exe| skadi_core::paths::portable_dir(&exe))
        .is_some();
    let history = skadi_core::schedule::history(system.os).unwrap_or_default();

    eprintln!(
        "{}\n",
        lang.pick(
            "Review this text, then paste it into a new issue: https://github.com/LLCForge/Skadi/issues/new",
            "Bu metni gözden geçirip yeni bir kayda yapıştırın: https://github.com/LLCForge/Skadi/issues/new"
        )
    );
    println!("### What happened\n\n<!-- Describe the problem and the steps to reproduce it. -->\n");
    println!("### Environment\n");
    println!("- Skadi: {}", env!("CARGO_PKG_VERSION"));
    println!("- System: {}", clean(report.os.as_deref().unwrap_or("?")));
    println!("- Architecture: {}", report.arch);
    println!("- Language: {}", lang.pick("en", "tr"));
    println!("- Elevated: {}", system.privileges.is_elevated);
    println!("- Portable mode: {portable}");
    println!("- Package managers: {}", managers.join(", "));
    println!("\n### Last commands\n\n```text");
    for entry in &history[history.len().saturating_sub(10)..] {
        println!(
            "{} {} {}",
            skadi_core::schedule::format_utc(entry.time),
            if entry.ok { "ok " } else { "ERR" },
            clean(&entry.command)
        );
    }
    println!("```");
    Ok(())
}

/// Zararlı yazılım taraması: Windows'ta Microsoft Defender, Linux ve macOS'ta ClamAV; Linux'ta
/// isteğe bağlı rkhunter. Yol kanonik (mutlak) hale getirilir, böylece hiçbir zaman seçenek gibi okunmaz.
pub fn scan(lang: Lang, path: Option<&Path>, rootkit: bool) -> Result<()> {
    let t = |en, tr| lang.pick(en, tr);
    let system = System::detect()?;
    let target = match path {
        Some(p) => Some(std::fs::canonicalize(p).map_err(|e| Error::State(format!("{}: {e}", p.display())))?),
        None => None,
    };
    let invocation = if rootkit {
        if system.os != Os::Linux {
            return Err(Error::Unsupported("scan --rootkit (Linux)"));
        }
        build(&system, "rkhunter", &["--check", "--sk", "--rwo"], Privilege::Root)?
    } else if system.os == Os::Windows {
        let defender = std::env::var_os("ProgramFiles")
            .map(|p| PathBuf::from(p).join("Windows Defender").join("MpCmdRun.exe"))
            .filter(|p| p.is_file())
            .ok_or(Error::ProgramNotFound("MpCmdRun.exe"))?;
        let mut args: Vec<OsString> = vec!["-Scan".into()];
        match &target {
            Some(path) => args.extend([
                "-ScanType".into(),
                "3".into(),
                "-File".into(),
                path.clone().into_os_string(),
            ]),
            None => args.extend(["-ScanType".into(), "1".into()]),
        }
        Invocation::build(defender, args, &[], Privilege::Any, &system.privileges, system.os)?
    } else {
        let folder = match target {
            Some(path) => path,
            None => std::env::var_os("HOME")
                .map(PathBuf::from)
                .ok_or_else(|| Error::State("no home directory".into()))?,
        };
        let clamscan = skadi_core::exec::find_program("clamscan").ok_or(Error::ProgramNotFound("clamscan"))?;
        Invocation::build(
            clamscan,
            vec!["-r".into(), "-i".into(), folder.into_os_string()],
            &[],
            Privilege::Any,
            &system.privileges,
            system.os,
        )?
    };
    println!("> {}", invocation.display());
    let result = invocation.run();
    // clamscan 1, MpCmdRun 2 ile çıkar: tehdit bulundu.
    if let Err(Error::CommandFailed { code: Some(1 | 2), .. }) = &result
        && !rootkit
    {
        eprintln!(
            "{}",
            t(
                "Threats were found; see the list above.",
                "Tehdit bulundu; yukarıdaki listeye bakın."
            )
        );
    }
    result
}

#[cfg(test)]
mod tests {
    #[test]
    fn formats_sizes() {
        assert_eq!(super::size(512), "512 B");
        assert_eq!(super::size(1536), "1.5 KiB");
        assert_eq!(super::size(3 * 1024 * 1024 * 1024), "3.0 GiB");
    }
}
