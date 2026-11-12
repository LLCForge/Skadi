//! TUI ve GUI'nin ortak katmanı: veri modeli, arka plan yüklemesi ve iş çalıştırıcı.
//!
//! Arayüzler sistemde hiçbir komutu doğrudan çalıştırmaz. Her iş, `skadi` CLI'sinin alt süreç
//! olarak başlatılmasıyla yapılır ve CLI'ye yalnızca katalog kimlikleri verilir; komutları CLI
//! gömülü katalogdan yeniden kurar. Yetki gerektiren işler ayrı, yetkili bir alt süreçte çalışır
//! (Linux'ta `pkexec`, macOS'ta yönetici parolası penceresi), arayüzün kendisi yetkisiz kalır.

#[cfg(feature = "gui")]
pub mod gui;
#[cfg(feature = "tui")]
pub mod tui;

use std::{
    collections::{HashMap, HashSet},
    io::{BufRead, BufReader},
    process::{Command, Stdio},
    sync::mpsc,
    thread,
};

use skadi_core::{
    Lang, Result,
    catalog::{App, Catalog},
    exec::{Privilege, find_program},
    manager::ManagerId,
    plan::plan,
    platform::Os,
    profile::{Preset, presets},
    system::System,
    tweak::{Tweak, TweakCatalog, TweakEngine, TweakStatus},
};

pub struct Model {
    pub lang: Lang,
    pub system: System,
    pub available: Vec<ManagerId>,
    pub catalog: Catalog,
    pub tweaks: TweakCatalog,
    pub presets: Vec<Preset>,
    /// `skadi icons fetch` ile önbelleğe alınmış simgeler (uygulama kimliği → dosya).
    pub icons: HashMap<String, std::path::PathBuf>,
}

impl Model {
    pub fn load(lang: Lang) -> Result<Self> {
        let system = System::detect()?;
        let catalog = Catalog::builtin()?;
        let icons = catalog
            .apps()
            .iter()
            .filter_map(|app| skadi_core::icons::icon_path(system.os, &app.id).map(|p| (app.id.clone(), p)))
            .collect();
        Ok(Self {
            lang,
            icons,
            available: system.available_ids(),
            system,
            catalog,
            tweaks: TweakCatalog::builtin()?,
            presets: presets()?,
        })
    }

    /// Bu sistemde anlamlı olan tweak'ler.
    pub fn system_tweaks(&self) -> Vec<&Tweak> {
        self.tweaks.for_host(&self.system).collect()
    }

    /// Uygulamanın bu sistemde kurulacağı yönetici; hiçbiri yoksa `None`.
    pub fn manager_for(&self, app: &App) -> Option<ManagerId> {
        self.available.iter().copied().find(|id| app.packages.contains_key(id))
    }

    /// Bu işletim sistemi için en az bir paketi olan uygulama mı?
    pub fn is_for_this_os(&self, app: &App) -> bool {
        app.packages.keys().any(|m| m.spec().os == self.system.os)
    }

    /// Bir görevi çalıştırılacak alt süreçlere böler.
    pub fn jobs(&self, task: &Task) -> Vec<Job> {
        let lang = self.lang.as_str().to_owned();
        let job = |args: Vec<String>, elevated: bool, label: String| Job {
            args: [vec!["--lang".to_owned(), lang.clone()], args, vec!["--yes".to_owned()]].concat(),
            elevated,
            label,
        };
        // Windows'ta UAC yetkili bir alt sürecin çıktısını aktarmaya izin vermez; orada uygulamanın
        // tamamı yönetici olarak çalışır. Zaten yetkiliysek bölmeye gerek yoktur.
        let split = self.system.os != Os::Windows && !self.system.privileges.is_elevated;
        let (verb, ids): (Vec<&str>, &Vec<String>) = match task {
            Task::Install(ids) => (vec!["install"], ids),
            Task::Remove(ids) => (vec!["remove"], ids),
            Task::ApplyTweaks(ids, _) => (vec!["tweak", "apply"], ids),
            Task::RevertTweaks(ids) => (vec!["tweak", "revert"], ids),
            Task::Update => {
                return self
                    .available
                    .iter()
                    .filter(|id| id.upgrade().is_some())
                    .map(|id| {
                        let root = split && id.spec().privilege == Privilege::Root;
                        job(
                            vec!["update".into(), "--manager".into(), id.as_str().into()],
                            root,
                            id.as_str().into(),
                        )
                    })
                    .collect();
            }
        };
        let needs_root = |id: &String| -> bool {
            if !split {
                return false;
            }
            match task {
                Task::Install(_) | Task::Remove(_) => {
                    let Some(app) = self.catalog.get(id) else { return false };
                    let planned = plan(&[app], self.system.os, &self.available, None);
                    // Eksik bir yöneticinin kurulumu da yetki gerektirebilir.
                    !planned.unresolved.is_empty()
                        || planned
                            .steps
                            .iter()
                            .any(|s| s.manager.spec().privilege == Privilege::Root)
                }
                _ => self.tweaks.get(id).is_some_and(Tweak::needs_root),
            }
        };
        let (root, user): (Vec<&String>, Vec<&String>) = ids.iter().partition(|id| needs_root(id));
        let verb: Vec<String> = verb.into_iter().map(str::to_owned).collect();
        // Yetkisiz işler öğe başına ayrı süreçtir: ilerleme gösterilir ve iptal öğeler arasında
        // güvenle yapılır. Yetkili işler tek süreçte toplanır ki parola bir kez sorulsun.
        let mut jobs: Vec<Job> = user
            .into_iter()
            .map(|id| job([verb.clone(), vec![id.clone()]].concat(), false, id.clone()))
            .collect();
        if !root.is_empty() {
            let label = root.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ");
            let ids: Vec<String> = root.into_iter().cloned().collect();
            jobs.push(job([verb, ids].concat(), true, label));
        }
        // Anlık görüntü yalnızca bir kez ve root yetkisi olan süreçte alınır.
        let target = jobs
            .iter()
            .rposition(|j| j.elevated)
            .or((!jobs.is_empty()).then_some(0));
        if matches!(task, Task::ApplyTweaks(_, true))
            && let Some(job) = target.map(|i| &mut jobs[i])
        {
            // `--yes` her zaman son argümandır; `--snapshot` ondan önce eklenir.
            let at = job.args.len() - 1;
            job.args.insert(at, "--snapshot".to_owned());
        }
        jobs
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Task {
    Install(Vec<String>),
    Remove(Vec<String>),
    /// Uygulanacak tweak'ler ve önce anlık görüntü alınıp alınmayacağı.
    ApplyTweaks(Vec<String>, bool),
    RevertTweaks(Vec<String>),
    Update,
}

/// Tek bir alt süreç: `skadi` argümanları ve yetkili çalışıp çalışmayacağı.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    pub args: Vec<String>,
    pub elevated: bool,
    pub label: String,
}

pub enum Loaded {
    Installed(HashSet<String>),
    Statuses(HashMap<String, Option<TweakStatus>>),
    /// Skadi'nin uyguladığı ama artık tam uygulanmış olmayan tweak'ler (ör. bir güncelleme geri aldı).
    Drifted(HashSet<String>),
}

/// Kurulu uygulamaları ve tweak durumlarını arka planda okur; arayüz bu sırada açık kalır.
pub fn load_in_background(model: &Model) -> mpsc::Receiver<Loaded> {
    let (tx, rx) = mpsc::channel();
    let system = model.system.clone();
    let apps = model.catalog.apps().to_vec();
    let installed_tx = tx.clone();
    thread::spawn(move || {
        let ids = system
            .installed_apps(&apps)
            .into_iter()
            .map(|(app, _)| app.id.clone())
            .collect();
        let _ = installed_tx.send(Loaded::Installed(ids));
    });
    let system = model.system.clone();
    let tweaks: Vec<Tweak> = model.system_tweaks().into_iter().cloned().collect();
    thread::spawn(move || {
        let engine = TweakEngine::new(&system);
        let refs: Vec<&Tweak> = tweaks.iter().collect();
        let _ = engine.prefetch(&refs);
        let statuses: HashMap<String, Option<TweakStatus>> = tweaks
            .iter()
            .map(|tweak| (tweak.id.clone(), engine.status(tweak).ok()))
            .collect();
        let recorded: HashSet<String> = skadi_core::tweak::StateStore::open(system.os)
            .map(|store| store.recorded().into_iter().collect())
            .unwrap_or_default();
        let drifted = statuses
            .iter()
            .filter(|(id, status)| {
                recorded.contains(*id) && matches!(status, Some(TweakStatus::NotApplied | TweakStatus::Partial))
            })
            .map(|(id, _)| id.clone())
            .collect();
        let _ = tx.send(Loaded::Statuses(statuses));
        let _ = tx.send(Loaded::Drifted(drifted));
    });
    rx
}

pub enum RunEvent {
    Line(String),
    Finished(std::result::Result<(), String>),
}

/// İşi alt süreç olarak başlatır; çıktı satırları ve bitiş olayı kanaldan gelir.
pub fn spawn(job: &Job) -> std::result::Result<mpsc::Receiver<RunEvent>, String> {
    let mut command = command_for(job)?;
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    hide_console(&mut command);
    let mut child = command.spawn().map_err(|e| e.to_string())?;
    let (tx, rx) = mpsc::channel();
    let readers: Vec<_> = [
        child
            .stdout
            .take()
            .map(|s| Box::new(s) as Box<dyn std::io::Read + Send>),
        child
            .stderr
            .take()
            .map(|s| Box::new(s) as Box<dyn std::io::Read + Send>),
    ]
    .into_iter()
    .flatten()
    .map(|stream| {
        let tx = tx.clone();
        thread::spawn(move || {
            for line in BufReader::new(stream).lines().map_while(std::result::Result::ok) {
                let _ = tx.send(RunEvent::Line(line));
            }
        })
    })
    .collect();
    thread::spawn(move || {
        let status = child.wait();
        for reader in readers {
            let _ = reader.join();
        }
        let result = match status {
            Ok(status) if status.success() => Ok(()),
            Ok(status) => Err(format!(
                "exit code {}",
                status.code().map_or("-".into(), |c| c.to_string())
            )),
            Err(error) => Err(error.to_string()),
        };
        let _ = tx.send(RunEvent::Finished(result));
    });
    Ok(rx)
}

fn command_for(job: &Job) -> std::result::Result<Command, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    if !job.elevated {
        let mut command = Command::new(exe);
        command.args(&job.args);
        return Ok(command);
    }
    match Os::current().map_err(|e| e.to_string())? {
        Os::Linux => {
            let pkexec = find_program("pkexec").ok_or("pkexec not found")?;
            let mut command = Command::new(pkexec);
            command.arg(exe).args(&job.args);
            Ok(command)
        }
        Os::Macos => {
            // Kabuk dizesindeki her kelime tek tırnakla korunur; AppleScript dizesi için de ters
            // eğik çizgi ve çift tırnak kaçırılır. Argümanlar zaten yalnızca kimliklerdir.
            let quote = |s: &str| format!("'{}'", s.replace('\'', r"'\''"));
            let shell: Vec<String> = std::iter::once(exe.to_string_lossy().into_owned())
                .chain(job.args.iter().cloned())
                .map(|w| quote(&w))
                .collect();
            let script = format!(
                "do shell script \"{}\" with administrator privileges",
                shell.join(" ").replace('\\', "\\\\").replace('"', "\\\"")
            );
            let mut command = Command::new(find_program("osascript").ok_or("osascript not found")?);
            command.arg("-e").arg(script);
            Ok(command)
        }
        Os::Windows => Err("administrator rights are required".to_owned()),
    }
}

#[cfg(windows)]
fn hide_console(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    // CREATE_NO_WINDOW: arayüzden başlatılan CLI için konsol penceresi açılmasın.
    command.creation_flags(0x0800_0000);
}

#[cfg(not(windows))]
fn hide_console(_command: &mut Command) {}

/// Windows'ta uygulamayı yönetici olarak yeniden başlatır (UAC penceresi).
#[cfg(windows)]
pub fn relaunch_elevated(args: &[&str]) -> std::result::Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let literal = |s: &str| format!("'{}'", s.replace('\'', "''"));
    let arguments = args.iter().map(|a| literal(a)).collect::<Vec<_>>().join(",");
    let script = format!(
        "Start-Process -FilePath {} -ArgumentList {} -Verb RunAs",
        literal(&exe.to_string_lossy()),
        if arguments.is_empty() {
            "@()".to_owned()
        } else {
            arguments
        }
    );
    let powershell = find_program("powershell").ok_or("powershell not found")?;
    let mut command = Command::new(powershell);
    command.args(["-NoProfile", "-NonInteractive", "-Command", &script]);
    hide_console(&mut command);
    let status = command.status().map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err("elevation was cancelled".to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jobs_pass_only_ids_and_split_by_privilege() {
        let model = Model::load(Lang::En).unwrap();
        let jobs = model.jobs(&Task::Install(vec!["git".into(), "vlc".into()]));
        assert!(!jobs.is_empty());
        for job in &jobs {
            assert_eq!(&job.args[..2], ["--lang", "en"]);
            assert_eq!(job.args.last().map(String::as_str), Some("--yes"));
            assert!(job.args.contains(&"install".to_owned()));
        }
        let ids: Vec<&String> = jobs
            .iter()
            .flat_map(|j| &j.args)
            .filter(|a| *a == "git" || *a == "vlc")
            .collect();
        assert_eq!(ids.len(), 2, "her kimlik tam bir kez");
        if model.system.os == Os::Windows {
            assert!(jobs.iter().all(|j| !j.elevated), "Windows'ta bölme yapılmaz");
        }
    }
}
