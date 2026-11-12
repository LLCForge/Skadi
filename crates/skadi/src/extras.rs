//! Yedekleme, çevrimdışı paketler, AppImage, uzak makineler, paket kaynakları ve boş alan temizliği.

use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

use skadi_core::{
    Error, Lang, Result, backup,
    catalog::Catalog,
    exec::{Invocation, Privilege, find_program},
    manager::ManagerId,
    platform::Os,
    system::System,
    tweak::{Action, TweakCatalog},
};

use crate::{commands, commands::confirm, net::build};

fn io_error(path: &Path) -> impl Fn(std::io::Error) -> Error + '_ {
    move |e| Error::State(format!("{}: {e}", path.display()))
}

fn home() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .ok_or_else(|| Error::State("no home directory".into()))
}

fn is_token(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 64
        && !text.starts_with('-')
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

fn is_https_url(url: &str) -> bool {
    url.starts_with("https://")
        && url.len() <= 2048
        && !url
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || matches!(c, '"' | '\'' | '`'))
}

fn report(lang: Lang, report: &backup::CopyReport, what: &str) {
    println!("{} {what}", report.copied.len());
    for skipped in &report.skipped {
        println!(
            "  {} {}",
            lang.pick(
                "skipped (may contain secrets, exists or too large):",
                "atlandı (gizli bilgi içerebilir, var ya da çok büyük):"
            ),
            skipped.display()
        );
    }
}

// ------------------------------------------------------------------------------------------------
// Dotfiles ve yer imleri

pub fn dotfiles(lang: Lang, action: &str, dir: &Path, force: bool) -> Result<()> {
    let home = home()?;
    let result = if action == "backup" {
        backup::backup_dotfiles(&home, dir)
    } else {
        backup::restore_dotfiles(dir, &home, force)
    };
    let copied = result.map_err(io_error(dir))?;
    report(lang, &copied, lang.pick("files copied.", "dosya kopyalandı."));
    if action == "backup" {
        println!(
            "{}",
            lang.pick(
                "SSH and GPG keys, cloud credentials and files with tokens or passwords were left out on purpose.",
                "SSH ve GPG anahtarları, bulut kimlik bilgileri ve token ya da parola içeren dosyalar bilerek dışarıda bırakıldı."
            )
        );
    }
    Ok(())
}

pub fn bookmarks(lang: Lang, dir: &Path) -> Result<()> {
    let os = Os::current()?;
    let copied = backup::backup_bookmarks(os, &home()?, dir).map_err(io_error(dir))?;
    report(
        lang,
        &copied,
        lang.pick("bookmark files copied.", "yer imi dosyası kopyalandı."),
    );
    println!(
        "{}",
        lang.pick(
            "Only bookmarks were copied; cookies, history and saved passwords were not. Restore in Firefox with Bookmarks > Manage bookmarks > Import and Backup > Restore; for Chromium browsers copy the Bookmarks file back while the browser is closed.",
            "Yalnızca yer imleri kopyalandı; çerezler, geçmiş ve kayıtlı parolalar kopyalanmadı. Firefox'ta Yer İmleri > Yer imlerini yönet > İçe Aktar ve Yedekle > Geri Yükle ile; Chromium tarayıcılarında tarayıcı kapalıyken Bookmarks dosyasını geri kopyalayarak geri yükleyin."
        )
    );
    Ok(())
}

// ------------------------------------------------------------------------------------------------
// Profil deposu

/// Profili bir Git deposuna dışa aktarır ve değişiklik varsa kaydeder (kullanıcının Git kimliğiyle).
pub fn profile_git(lang: Lang, dir: &Path) -> Result<()> {
    let system = System::detect()?;
    std::fs::create_dir_all(dir).map_err(io_error(dir))?;
    let dir_text = dir.to_string_lossy().into_owned();
    let git = |args: &[&str]| -> Result<std::process::Output> {
        let mut full = vec!["-C", dir_text.as_str()];
        full.extend_from_slice(args);
        build(&system, "git", &full, Privilege::Any)?.output()
    };
    if !dir.join(".git").exists() {
        git(&["init", "--quiet"])?;
    }
    let file = dir.join("skadi-profile.toml");
    crate::profiles::export(lang, &file)?;
    git(&["add", "skadi-profile.toml"])?;
    let changed = !git(&["diff", "--cached", "--quiet"])?.status.success();
    if !changed {
        println!(
            "{}",
            lang.pick(
                "No changes since the last commit.",
                "Son kayıttan bu yana değişiklik yok."
            )
        );
        return Ok(());
    }
    let message = format!("Skadi profile {}", skadi_core::schedule::format_utc(now()));
    let output = git(&["commit", "--quiet", "-m", &message])?;
    if !output.status.success() {
        return Err(Error::State(format!(
            "git commit: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    println!("{} {}", lang.pick("Committed to", "Kaydedildi:"), dir.display());
    Ok(())
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

// ------------------------------------------------------------------------------------------------
// Registry yedeği

fn decode_utf16(bytes: &[u8]) -> String {
    let body = bytes.strip_prefix(&[0xFF, 0xFE]).unwrap_or(bytes);
    let units: Vec<u16> = body.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    String::from_utf16_lossy(&units)
}

/// Bu sistemdeki tweak'lerin dokunduğu registry anahtarlarını tek bir `.reg` dosyasına aktarır.
pub fn registry_backup(lang: Lang, file: &Path) -> Result<()> {
    let system = System::detect()?;
    if system.os != Os::Windows {
        return Err(Error::Unsupported("tweak backup (Windows)"));
    }
    let catalog = TweakCatalog::builtin()?;
    let mut keys: Vec<String> = catalog
        .for_host(&system)
        .flat_map(|t| t.actions.iter())
        .filter_map(|action| match action {
            Action::Registry { key, .. } => Some(key.clone()),
            Action::Service { name, .. } => Some(format!("HKLM\\SYSTEM\\CurrentControlSet\\Services\\{name}")),
            _ => None,
        })
        .collect();
    keys.sort();
    keys.dedup();
    let temp = std::env::temp_dir().join(format!("skadi-reg-{}.reg", std::process::id()));
    let temp_text = temp.to_string_lossy().into_owned();
    let mut combined = String::from("Windows Registry Editor Version 5.00\r\n");
    let mut exported = 0;
    for key in &keys {
        let output = build(&system, "reg", &["export", key, &temp_text, "/y"], Privilege::Any)?.output()?;
        if !output.status.success() {
            continue; // anahtar henüz yok
        }
        let text = decode_utf16(&std::fs::read(&temp).map_err(io_error(&temp))?);
        let body = text.split_once('\n').map_or("", |(_, rest)| rest);
        combined.push_str(&body.replace("\r\n", "\n").replace('\n', "\r\n"));
        exported += 1;
    }
    let _ = std::fs::remove_file(&temp);
    let mut bytes = vec![0xFF, 0xFE];
    bytes.extend(combined.encode_utf16().flat_map(u16::to_le_bytes));
    std::fs::write(file, bytes).map_err(io_error(file))?;
    println!(
        "{exported} {} {}",
        lang.pick("registry keys exported to", "registry anahtarı şuraya aktarıldı:"),
        file.display()
    );
    Ok(())
}

// ------------------------------------------------------------------------------------------------
// Boş alan

/// Boş alandaki silinmiş dosya kalıntılarını temizler: Windows'ta `cipher /w`, Linux'ta SSD'ler için
/// `fstrim` (TRIM, SSD'de silinen bloklar için tek güvenilir yoldur). macOS TRIM'i kendisi yapar.
pub fn wipe_free(lang: Lang, path: Option<&Path>, yes: bool) -> Result<()> {
    let t = |en, tr| lang.pick(en, tr);
    let system = System::detect()?;
    let invocation = match system.os {
        Os::Windows => {
            let target = path.map_or_else(|| "C:\\".to_owned(), |p| p.display().to_string());
            build(&system, "cipher", &[&format!("/w:{target}")], Privilege::Any)?
        }
        Os::Linux => {
            let target = path.map_or_else(|| "/".to_owned(), |p| p.display().to_string());
            build(&system, "fstrim", &["-v", &target], Privilege::Root)?
        }
        Os::Macos => {
            println!(
                "{}",
                t(
                    "macOS trims SSD free space by itself; with FileVault on, deleted data is also encrypted. Nothing to do.",
                    "macOS SSD boş alanını kendisi temizler (TRIM); FileVault açıkken silinen veriler de şifrelidir. Yapılacak bir şey yok."
                )
            );
            return Ok(());
        }
    };
    println!("> {}", invocation.display());
    if !confirm(
        lang,
        yes,
        t(
            "Clean the free space? This can take a long time.",
            "Boş alan temizlensin mi? Uzun sürebilir.",
        ),
    )? {
        return Ok(());
    }
    invocation.run()
}

// ------------------------------------------------------------------------------------------------
// AppImage

fn applications_dir() -> Result<PathBuf> {
    Ok(home()?.join("Applications"))
}

/// Yayıncının bildirdiği SHA-256 ile doğrulanan bir AppImage kurar. Özet verilmeden kurulum yapılmaz.
pub fn appimage_install(lang: Lang, name: &str, url: &str, sha256: &str) -> Result<()> {
    let system = System::detect()?;
    if system.os != Os::Linux {
        return Err(Error::Unsupported("appimage (Linux)"));
    }
    if system.privileges.is_elevated {
        return Err(Error::MustNotBeRoot);
    }
    if !is_token(name) || !is_https_url(url) || sha256.len() != 64 || !sha256.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(Error::InvalidOption("NAME https://... SHA256"));
    }
    let dir = applications_dir()?;
    std::fs::create_dir_all(&dir).map_err(io_error(&dir))?;
    let target = dir.join(format!("{name}.AppImage"));
    let part = dir.join(format!("{name}.AppImage.part"));
    let curl = find_program("curl").ok_or(Error::ProgramNotFound("curl"))?;
    Invocation::build(
        curl,
        vec![
            "-fsSL".into(),
            "--proto".into(),
            "=https".into(),
            "--tlsv1.2".into(),
            "-o".into(),
            part.clone().into_os_string(),
            url.into(),
        ],
        &[],
        Privilege::Any,
        &system.privileges,
        system.os,
    )?
    .run()?;
    let part_text = part.to_string_lossy().into_owned();
    let output = build(&system, "sha256sum", &[&part_text], Privilege::Any)?.output()?;
    let actual = String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    if actual != sha256.to_ascii_lowercase() {
        let _ = std::fs::remove_file(&part);
        return Err(Error::State(
            lang.pick(
                "checksum mismatch; the download was deleted",
                "özet tutmuyor; indirilen dosya silindi",
            )
            .to_owned(),
        ));
    }
    std::fs::rename(&part, &target).map_err(io_error(&target))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755)).map_err(io_error(&target))?;
    }
    let desktop_dir = home()?.join(".local/share/applications");
    std::fs::create_dir_all(&desktop_dir).map_err(io_error(&desktop_dir))?;
    let desktop = desktop_dir.join(format!("skadi-{name}.desktop"));
    std::fs::write(
        &desktop,
        format!(
            "[Desktop Entry]\nType=Application\nName={name}\nExec=\"{}\"\nCategories=Utility;\n",
            target.display()
        ),
    )
    .map_err(io_error(&desktop))?;
    println!("{} {}", lang.pick("Installed:", "Kuruldu:"), target.display());
    Ok(())
}

pub fn appimage_remove(lang: Lang, name: &str) -> Result<()> {
    if !is_token(name) {
        return Err(Error::InvalidOption("NAME"));
    }
    let target = applications_dir()?.join(format!("{name}.AppImage"));
    let desktop = home()?.join(format!(".local/share/applications/skadi-{name}.desktop"));
    if !target.exists() {
        return Err(Error::State(format!(
            "{}: {}",
            target.display(),
            lang.pick("not found", "bulunamadı")
        )));
    }
    std::fs::remove_file(&target).map_err(io_error(&target))?;
    let _ = std::fs::remove_file(desktop);
    println!("{}", lang.pick("Removed.", "Kaldırıldı."));
    Ok(())
}

// ------------------------------------------------------------------------------------------------
// Çevrimdışı paketler

const BUNDLE_MANIFEST: &str = "skadi-bundle.txt";

fn bundle_packages(system: &System, apps: &[String]) -> Result<(ManagerId, Vec<String>)> {
    let manager = system
        .system_manager()
        .map(|m| m.spec.id)
        .filter(|id| {
            matches!(
                id,
                ManagerId::Apt | ManagerId::Dnf | ManagerId::Pacman | ManagerId::Zypper
            )
        })
        .ok_or(Error::Unsupported("bundle (apt, dnf, pacman, zypper)"))?;
    let catalog = Catalog::builtin()?;
    let mut packages = Vec::new();
    for id in apps {
        let app = catalog.get(id).ok_or_else(|| Error::UnknownApp(id.clone()))?;
        let package = app
            .packages
            .get(&manager)
            .ok_or_else(|| Error::State(format!("{id}: no {} package", manager.as_str())))?;
        skadi_core::manager::validate_package_id(package)?;
        packages.push(package.clone());
    }
    Ok((manager, packages))
}

/// Paketleri ve bağımlılıklarını bir klasöre indirir; `bundle_install` bunları ağ olmadan kurar.
pub fn bundle_create(lang: Lang, dir: &Path, apps: &[String]) -> Result<()> {
    let system = System::detect()?;
    let (manager, packages) = bundle_packages(&system, apps)?;
    let dir = std::path::absolute(dir).map_err(io_error(dir))?;
    std::fs::create_dir_all(dir.join("partial")).map_err(io_error(&dir))?;
    let dir_text = dir.to_string_lossy().into_owned();
    let names: Vec<&str> = packages.iter().map(String::as_str).collect();
    let mut args: Vec<String> = match manager {
        ManagerId::Apt => vec![
            "install".into(),
            "--download-only".into(),
            "-y".into(),
            "-o".into(),
            format!("Dir::Cache::archives={dir_text}"),
        ],
        ManagerId::Dnf => vec![
            "install".into(),
            "--downloadonly".into(),
            "-y".into(),
            "--destdir".into(),
            dir_text.clone(),
        ],
        ManagerId::Pacman => vec![
            "-Sw".into(),
            "--noconfirm".into(),
            "--cachedir".into(),
            dir_text.clone(),
        ],
        _ => vec![
            "--non-interactive".into(),
            "--pkg-cache-dir".into(),
            dir_text.clone(),
            "install".into(),
            "--download-only".into(),
        ],
    };
    args.extend(names.iter().map(|n| (*n).to_owned()));
    let program = match manager {
        ManagerId::Apt => "apt-get",
        ManagerId::Dnf => "dnf",
        ManagerId::Pacman => "pacman",
        _ => "zypper",
    };
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let invocation = build(&system, program, &arg_refs, Privilege::Root)?;
    println!("> {}", invocation.display());
    invocation.run()?;
    std::fs::write(
        dir.join(BUNDLE_MANIFEST),
        format!("{}\n{}\n", manager.as_str(), packages.join(" ")),
    )
    .map_err(io_error(&dir))?;
    println!(
        "{} {}",
        lang.pick("Bundle ready in", "Paket klasörü hazır:"),
        dir.display()
    );
    Ok(())
}

fn package_files(dir: &Path, extension: &str) -> Result<Vec<String>> {
    let mut files: Vec<String> = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        for entry in std::fs::read_dir(&current).map_err(io_error(&current))?.flatten() {
            let path = entry.path();
            let meta = std::fs::symlink_metadata(&path).map_err(io_error(&path))?;
            if meta.is_dir() {
                stack.push(path);
            } else if meta.is_file() && path.to_string_lossy().ends_with(extension) {
                files.push(path.to_string_lossy().into_owned());
            }
        }
    }
    files.sort();
    Ok(files)
}

pub fn bundle_install(lang: Lang, dir: &Path, yes: bool) -> Result<()> {
    let system = System::detect()?;
    let manifest = std::fs::read_to_string(dir.join(BUNDLE_MANIFEST)).map_err(io_error(dir))?;
    let manager = manifest.lines().next().unwrap_or("").parse::<ManagerId>()?;
    let (program, mut args, extension): (&str, Vec<String>, &str) = match manager {
        ManagerId::Apt => (
            "apt-get",
            vec!["install".into(), "-y".into(), "--no-download".into()],
            ".deb",
        ),
        ManagerId::Dnf => (
            "dnf",
            vec!["install".into(), "-y".into(), "--disablerepo=*".into()],
            ".rpm",
        ),
        ManagerId::Pacman => ("pacman", vec!["-U".into(), "--noconfirm".into()], ".pkg.tar.zst"),
        ManagerId::Zypper => (
            "zypper",
            vec!["--non-interactive".into(), "--no-refresh".into(), "install".into()],
            ".rpm",
        ),
        _ => return Err(Error::Unsupported("bundle (apt, dnf, pacman, zypper)")),
    };
    let files = package_files(&std::path::absolute(dir).map_err(io_error(dir))?, extension)?;
    if files.is_empty() {
        return Err(Error::State(
            lang.pick("no packages in the bundle", "paket klasöründe paket yok")
                .to_owned(),
        ));
    }
    args.extend(files);
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let invocation = build(&system, program, &arg_refs, Privilege::Root)?;
    println!("> {}", invocation.display());
    if !confirm(
        lang,
        yes,
        lang.pick("Install these packages?", "Bu paketler kurulsun mu?"),
    )? {
        return Ok(());
    }
    invocation.run()
}

// ------------------------------------------------------------------------------------------------
// Uzak makineler

fn is_ssh_host(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 255
        && !host.starts_with('-')
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '@'))
}

/// Bir hazır seti ya da profil dosyasını SSH ile başka makinelere uygular. Kullanıcının kendi SSH
/// yapılandırması ve anahtarları kullanılır; Skadi parola ya da anahtar saklamaz. Uzak komut
/// yalnızca doğrulanmış parçalardan oluşur.
pub fn remote_apply(lang: Lang, target: &str, hosts: &[String], dry_run: bool, yes: bool) -> Result<()> {
    let system = System::detect()?;
    if let Some(bad) = hosts.iter().find(|h| !is_ssh_host(h)) {
        return Err(Error::State(format!("invalid host: {bad}")));
    }
    let local = Path::new(target);
    let remote_target = if local.is_file() {
        // Profil yerelde doğrulanır; uzağa yalnızca geçerli bir profil gider.
        skadi_core::profile::Profile::load(local, &Catalog::builtin()?, &TweakCatalog::builtin()?)?;
        format!("/tmp/skadi-profile-{}.toml", now())
    } else if is_token(target) {
        skadi_core::profile::preset(target)?;
        target.to_owned()
    } else {
        return Err(Error::InvalidOption("preset id or profile file"));
    };
    let mut remote = format!(
        "curl -fsSL --proto '=https' --tlsv1.2 https://raw.githubusercontent.com/LLCForge/Skadi/main/install.sh | sh -s -- profile apply {remote_target} --yes"
    );
    if dry_run {
        remote.push_str(" --dry-run");
    }
    for host in hosts {
        println!("{host}: {remote}");
    }
    if !confirm(
        lang,
        yes,
        lang.pick("Run this on every host?", "Bu, her makinede çalıştırılsın mı?"),
    )? {
        return Ok(());
    }
    for host in hosts {
        if local.is_file() {
            let destination = format!("{host}:{remote_target}");
            let local_text = local.to_string_lossy().into_owned();
            build(&system, "scp", &["-q", "--", &local_text, &destination], Privilege::Any)?.run()?;
        }
        build(&system, "ssh", &["-t", "--", host, &remote], Privilege::Any)?.run()?;
    }
    Ok(())
}

// ------------------------------------------------------------------------------------------------
// Paket kaynakları (yerel ayna)

/// Kurum içi ayna ya da ek paket kaynağı ekler ya da listeler. Yalnızca HTTPS kabul edilir; dağıtımın
/// kendi depo dosyalarına dokunulmaz.
pub fn source(lang: Lang, action: &str, manager: &str, name: Option<&str>, url: Option<&str>, yes: bool) -> Result<()> {
    let detected = System::detect()?;
    let id = manager.parse::<ManagerId>()?;
    detected.manager(id).ok_or(Error::ManagerNotFound(id.as_str()))?;
    let list = action == "list";
    let args: Vec<String> = match (id, list) {
        (ManagerId::Winget, true) => vec!["source".into(), "list".into()],
        (ManagerId::Choco, true) => vec!["source".into(), "list".into()],
        (ManagerId::Scoop, true) => vec!["bucket".into(), "list".into()],
        (ManagerId::Flatpak, true) => vec!["remotes".into()],
        (_, true) => return Err(Error::Unsupported("source (winget, choco, scoop, flatpak)")),
        (_, false) => {
            let (Some(name), Some(url)) = (name, url) else {
                return Err(Error::InvalidOption("NAME https://URL"));
            };
            if !is_token(name) || !is_https_url(url) {
                return Err(Error::InvalidOption("NAME https://URL"));
            }
            match id {
                ManagerId::Winget => vec![
                    "source".into(),
                    "add".into(),
                    "--name".into(),
                    name.into(),
                    "--arg".into(),
                    url.into(),
                    "--accept-source-agreements".into(),
                ],
                ManagerId::Choco => vec![
                    "source".into(),
                    "add".into(),
                    format!("--name={name}"),
                    format!("--source={url}"),
                ],
                ManagerId::Scoop => vec!["bucket".into(), "add".into(), name.into(), url.into()],
                ManagerId::Flatpak => vec!["remote-add".into(), "--if-not-exists".into(), name.into(), url.into()],
                _ => return Err(Error::Unsupported("source (winget, choco, scoop, flatpak)")),
            }
        }
    };
    let program = detected
        .manager(id)
        .map(|m| m.path.clone())
        .ok_or(Error::ManagerNotFound(id.as_str()))?;
    let privilege = match (id, list) {
        (_, true) | (ManagerId::Scoop, _) => Privilege::Any,
        _ => Privilege::Root,
    };
    let invocation = Invocation::build(
        program,
        args.into_iter().map(OsString::from).collect(),
        &[],
        privilege,
        &detected.privileges,
        detected.os,
    )?;
    println!("> {}", invocation.display());
    if !list
        && !confirm(
            lang,
            yes,
            lang.pick(
                "Add this package source? Only add sources you trust; they can deliver any software.",
                "Bu paket kaynağı eklensin mi? Yalnızca güvendiğiniz kaynakları ekleyin; her türlü yazılımı gönderebilirler.",
            ),
        )?
    {
        return Ok(());
    }
    let _ = commands::preview_system(lang, &detected, false);
    invocation.run()
}

// ------------------------------------------------------------------------------------------------
// Uygulama simgeleri

/// Her uygulamanın resmi sitesinden simgesini indirir (`fetch`) ya da önbelleği siler (`clear`).
/// Yalnızca kullanıcı çalıştırınca ağa çıkılır; doğrulanmamış içerik kaydedilmez.
pub fn icons(lang: Lang, action: &str, yes: bool) -> Result<()> {
    let system = System::detect()?;
    let dir = skadi_core::icons::icon_dir(system.os).ok_or_else(|| Error::State("no data directory".into()))?;
    if action == "clear" {
        if dir.exists() {
            std::fs::remove_dir_all(&dir).map_err(io_error(&dir))?;
        }
        println!("{}", lang.pick("Icon cache removed.", "Simge önbelleği silindi."));
        return Ok(());
    }
    let catalog = Catalog::builtin()?;
    if !confirm(
        lang,
        yes,
        lang.pick(
            "Download each app's icon from its official website once? Each site sees one request from you.",
            "Her uygulamanın simgesi resmi sitesinden bir kez indirilsin mi? Her site sizden tek bir istek görür.",
        ),
    )? {
        return Ok(());
    }
    std::fs::create_dir_all(&dir).map_err(io_error(&dir))?;
    let curl = find_program("curl").ok_or(Error::ProgramNotFound("curl"))?;
    let temp = dir.join("download.tmp");
    let mut saved = 0;
    for app in catalog.apps() {
        let Some(url) = app.homepage.as_deref().and_then(skadi_core::icons::favicon_url) else {
            continue;
        };
        if skadi_core::icons::icon_path(system.os, &app.id).is_some() {
            continue;
        }
        let ok = std::process::Command::new(&curl)
            .args([
                "-fsSL",
                "--proto",
                "=https",
                "--tlsv1.2",
                "--max-time",
                "10",
                "--max-filesize",
                "524288",
                "-o",
            ])
            .arg(&temp)
            .arg(&url)
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        if !ok {
            continue;
        }
        let bytes = std::fs::read(&temp).unwrap_or_default();
        if let Some(ext) = skadi_core::icons::image_kind(&bytes) {
            let target = dir.join(format!("{}.{ext}", app.id));
            std::fs::write(&target, bytes).map_err(io_error(&target))?;
            saved += 1;
        }
    }
    let _ = std::fs::remove_file(&temp);
    println!("{saved} {}", lang.pick("icons saved.", "simge kaydedildi."));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_inputs_and_decodes() {
        assert!(is_ssh_host("admin@server-1.lan") && !is_ssh_host("-oProxyCommand=x") && !is_ssh_host("a b"));
        assert!(
            is_https_url("https://mirror.corp/choco/") && !is_https_url("http://x") && !is_https_url("https://a b")
        );
        assert!(is_token("my-app") && !is_token("../x"));
        let mut bytes = vec![0xFF, 0xFE];
        bytes.extend(
            "Windows Registry Editor Version 5.00\r\n\r\n[HKEY_CURRENT_USER\\X]\r\n"
                .encode_utf16()
                .flat_map(u16::to_le_bytes),
        );
        assert!(decode_utf16(&bytes).contains("[HKEY_CURRENT_USER\\X]"));
    }
}
