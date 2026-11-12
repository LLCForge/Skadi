//! Tweak'leri okuyan, uygulayan ve geri alan motor.
//!
//! Uygulama işlem (transaction) gibidir: her eylemden önce eski durum okunur; bir eylem
//! başarısız olursa o ana kadar yapılanlar ters sırayla geri alınır. Başarılı eylemlerin eski
//! durumları `StateStore`'a yazılır ve geri alma yalnızca bu kayıttan yapılır.

use std::{
    cell::{OnceCell, RefCell},
    collections::{HashMap, HashSet},
    ffi::OsString,
};

use super::{
    Action, RegKind, Tweak,
    action::{
        Cmd, Observed, Previous, apply_cmds, normalize_defaults, parse_systemd_state, powershell, ps_literal,
        restore_cmds, service_key, systemd_is_target,
    },
    state::{ActionRecord, StateStore},
};
use crate::{
    error::{Error, Result},
    exec::{Invocation, Privilege, find_program},
    i18n::Lang,
    manager::ManagerId,
    plan::{Step, StepItem},
    system::{Op, System},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TweakStatus {
    Applied,
    NotApplied,
    Partial,
    /// Bu sistemde uygulanabilecek hiçbir eylem yok.
    Unavailable,
}

impl TweakStatus {
    pub const fn label(self, lang: Lang) -> &'static str {
        match self {
            Self::Applied => lang.pick("applied", "uygulandı"),
            Self::NotApplied => lang.pick("not applied", "uygulanmadı"),
            Self::Partial => lang.pick("partial", "kısmen"),
            Self::Unavailable => lang.pick("n/a", "yok"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApplyOutcome {
    pub changed: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RevertOutcome {
    pub restored: usize,
    pub irreversible: usize,
}

pub struct TweakEngine<'a> {
    system: &'a System,
    appx: OnceCell<HashSet<String>>,
    tasks: RefCell<HashMap<String, Option<bool>>>,
    features: RefCell<HashMap<String, Option<String>>>,
}

impl<'a> TweakEngine<'a> {
    pub fn new(system: &'a System) -> Self {
        Self {
            system,
            appx: OnceCell::new(),
            tasks: RefCell::new(HashMap::new()),
            features: RefCell::new(HashMap::new()),
        }
    }

    pub fn status(&self, tweak: &Tweak) -> Result<TweakStatus> {
        let (mut done, mut pending) = (false, false);
        for action in &tweak.actions {
            match self.observe(action)? {
                Observed::Unavailable => {}
                Observed::Done => done = true,
                Observed::Pending(_) => pending = true,
            }
        }
        Ok(match (done, pending) {
            (false, false) => TweakStatus::Unavailable,
            (true, false) => TweakStatus::Applied,
            (false, true) => TweakStatus::NotApplied,
            (true, true) => TweakStatus::Partial,
        })
    }

    /// Uygulanırsa çalışacak komutlar; hiçbir şeyi değiştirmez. Windows'ta yönetici olmayan bir
    /// terminalde de komutlar, yönetici olarak çalışacakları haliyle gösterilir.
    pub fn preview(&self, tweak: &Tweak) -> Result<Vec<Invocation>> {
        self.ensure_platform(tweak)?;
        let mut privileges = self.system.privileges.clone();
        if self.system.os == crate::platform::Os::Windows {
            privileges.is_elevated = true;
        }
        let mut invocations = Vec::new();
        for action in &tweak.actions {
            // Önizleme yalnızca gösterir: yetki olmadan okunamayan bir özellik, uygulanacakmış gibi
            // gösterilir. Gerçek uygulama zaten yönetici gerektirir ve durumu orada doğru okur.
            let observed = match self.observe(action) {
                Err(Error::ElevationRequired) if matches!(action, Action::WindowsFeature { .. }) => {
                    Observed::Pending(Previous::Feature)
                }
                other => other?,
            };
            if let Observed::Pending(previous) = observed {
                invocations.extend(match (action, &previous) {
                    (Action::Package { packages }, Previous::Package { manager }) => {
                        self.package_invocations(packages, *manager, Op::Remove)?
                    }
                    _ => apply_cmds(action)
                        .iter()
                        .map(|cmd| self.invocation_with(cmd, &privileges))
                        .collect::<Result<Vec<_>>>()?,
                });
            }
        }
        Ok(invocations)
    }

    pub fn apply(&mut self, tweak: &Tweak, store: &mut StateStore) -> Result<ApplyOutcome> {
        self.ensure_platform(tweak)?;
        let mut done: Vec<(&Action, Previous)> = Vec::new();
        let mut failure = None;
        for action in &tweak.actions {
            let previous = match self.observe(action) {
                Ok(Observed::Pending(previous)) => previous,
                Ok(_) => continue,
                Err(error) => {
                    failure = Some(error);
                    break;
                }
            };
            if let Err(error) = self
                .apply_invocations(action, &previous)
                .and_then(|list| run_all(&list))
            {
                failure = Some(error);
                break;
            }
            done.push((action, previous));
        }
        self.invalidate();

        if let Some(error) = failure {
            // Yarım kalan tweak'i geri sar; geri sarılamayanlar kayda geçer ki sonra geri alınabilsin.
            let stuck: Vec<ActionRecord> = done
                .into_iter()
                .rev()
                .filter(|(action, previous)| {
                    self.restore_invocations(action, previous)
                        .and_then(|list| run_all(&list))
                        .is_err()
                })
                .map(|(action, previous)| ActionRecord {
                    key: action.key(),
                    previous,
                })
                .collect();
            self.invalidate();
            store.record(&tweak.id, stuck);
            store.save()?;
            return Err(error);
        }

        let changed = done.len();
        store.record(
            &tweak.id,
            done.into_iter()
                .map(|(action, previous)| ActionRecord {
                    key: action.key(),
                    previous,
                })
                .collect(),
        );
        store.save()?;
        Ok(ApplyOutcome { changed })
    }

    pub fn revert(&mut self, tweak: &Tweak, store: &mut StateStore) -> Result<RevertOutcome> {
        self.ensure_platform(tweak)?;
        let record = store
            .get(&tweak.id)
            .cloned()
            .ok_or_else(|| Error::NothingToRevert(tweak.id.clone()))?;
        let mut outcome = RevertOutcome {
            restored: 0,
            irreversible: 0,
        };
        let mut remaining = Vec::new();
        let mut first_error = None;
        for saved in record.actions.iter().rev() {
            // Katalogdan kaldırılmış bir eylemin kaydı güvenle geri alınamaz; kayıtta kalır.
            let Some(action) = tweak.actions.iter().find(|a| a.key() == saved.key) else {
                remaining.push(saved.clone());
                continue;
            };
            match self
                .restore_invocations(action, &saved.previous)
                .and_then(|list| run_all(&list))
            {
                Ok(()) => {
                    // Skadi'nin oluşturduğu anahtar, içinde başka bir şey kalmadıysa kaldırılır.
                    if let Previous::Registry {
                        value: None,
                        created: Some(root),
                    } = &saved.previous
                    {
                        remove_if_empty(root);
                    }
                    outcome.restored += 1;
                }
                Err(Error::NotReversible(_)) => outcome.irreversible += 1,
                Err(error) => {
                    remaining.push(saved.clone());
                    first_error.get_or_insert(error);
                }
            }
        }
        self.invalidate();
        remaining.reverse();
        store.replace(&tweak.id, remaining);
        store.save()?;
        first_error.map_or(Ok(outcome), Err)
    }

    /// Bir eylemin sistemdeki şu anki durumu. Yalnızca okur.
    pub fn observe(&self, action: &Action) -> Result<Observed> {
        let observed = match action {
            Action::Registry { key, name, kind, data } => {
                let current = self.read_registry(key, name, *kind)?;
                if current.as_deref() == Some(canonical(*kind, data).as_str()) {
                    Observed::Done
                } else {
                    let created = if current.is_none() { missing_root(key)? } else { None };
                    Observed::Pending(Previous::Registry {
                        value: current,
                        created,
                    })
                }
            }
            Action::Service { name, start } => match self.read_registry(&service_key(name), "Start", RegKind::Dword)? {
                None => Observed::Unavailable,
                Some(value) if value == start.registry_value().to_string() => Observed::Done,
                Some(value) => Observed::Pending(Previous::Service {
                    start: value
                        .parse()
                        .map_err(|_| Error::State(format!("{name}: unexpected start value")))?,
                }),
            },
            Action::ScheduledTask { task } => match self.task_enabled(task)? {
                None => Observed::Unavailable,
                Some(false) => Observed::Done,
                Some(true) => Observed::Pending(Previous::Task),
            },
            Action::Appx { package } => {
                if self.appx()?.contains(&package.to_ascii_lowercase()) {
                    Observed::Pending(Previous::Appx)
                } else {
                    Observed::Done
                }
            }
            Action::Systemd { unit, mode } => {
                let Some(output) = self.read("systemctl", &["is-enabled", unit], Privilege::Any)? else {
                    return Ok(Observed::Unavailable);
                };
                match parse_systemd_state(&String::from_utf8_lossy(&output.stdout)) {
                    None => Observed::Unavailable,
                    Some(state) if systemd_is_target(*mode, &state) => Observed::Done,
                    // static/indirect/generated birimler devre dışı bırakılamaz; yalnızca maskelenebilir.
                    Some(state) if state != "enabled" && *mode == super::SystemdMode::Disable => Observed::Unavailable,
                    // Maskelenmiş ya da statik birim kullanıcının bilinçli seçimidir; açmaya zorlanmaz.
                    Some(state) if state != "disabled" && *mode == super::SystemdMode::Enable => Observed::Unavailable,
                    Some(state) => Observed::Pending(Previous::Systemd { state }),
                }
            }
            Action::Package { packages } => {
                let Some((manager, package)) = self.package_for(packages) else {
                    return Ok(Observed::Unavailable);
                };
                match self.system.is_installed(manager, package)? {
                    Some(false) => Observed::Done,
                    _ => Observed::Pending(Previous::Package { manager }),
                }
            }
            Action::ManagedFile {
                path,
                content,
                requires,
                ..
            } => {
                let file = std::path::Path::new(path);
                let dir_ok = file.parent().is_some_and(std::path::Path::is_dir);
                let required_ok = requires.iter().any(|r| std::path::Path::new(r).exists());
                // Gereken program kuruluysa klasörü Skadi oluşturur (ör. Chrome'un politika klasörü).
                let available = if requires.is_empty() { dir_ok } else { required_ok };
                if !available || !super::config::drop_in_is_read(path) {
                    return Ok(Observed::Unavailable);
                }
                match std::fs::read_to_string(file) {
                    Ok(text) if text == *content => Observed::Done,
                    // Sabit adlı dosyada (ör. Firefox policies.json) kullanıcının kendi içeriği varsa dokunulmaz.
                    Ok(_) if !super::config::is_skadi_owned(path) => Observed::Unavailable,
                    Ok(text) => Observed::Pending(Previous::ManagedFile { content: Some(text) }),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                        Observed::Pending(Previous::ManagedFile { content: None })
                    }
                    Err(_) => Observed::Unavailable,
                }
            }
            Action::ConfigLine {
                path,
                section,
                key,
                value,
            } => {
                let Ok(text) = std::fs::read_to_string(path) else {
                    return Ok(Observed::Unavailable);
                };
                match super::config::read_line(&text, section.as_deref(), key) {
                    None => Observed::Unavailable,
                    Some(Some(line))
                        if super::config::value_of(&line).is_some_and(|v| v.eq_ignore_ascii_case(value)) =>
                    {
                        Observed::Done
                    }
                    Some(line) => Observed::Pending(Previous::ConfigLine { line }),
                }
            }
            Action::Kconfig {
                file,
                group,
                key,
                value,
            } => {
                let reader = if find_program("kreadconfig6").is_some() {
                    "kreadconfig6"
                } else {
                    "kreadconfig5"
                };
                let Some(output) = self.read(
                    reader,
                    &["--file", file, "--group", group, "--key", key],
                    Privilege::NonRoot,
                )?
                else {
                    return Ok(Observed::Unavailable);
                };
                let current = String::from_utf8_lossy(&output.stdout).trim().to_owned();
                if current == *value {
                    Observed::Done
                } else {
                    Observed::Pending(Previous::Kconfig {
                        value: (!current.is_empty()).then_some(current),
                    })
                }
            }
            Action::WindowsFeature { name } => match self.feature_state(name)? {
                None => Observed::Unavailable,
                Some(state) if state.starts_with("Enable") => Observed::Done,
                Some(_) => Observed::Pending(Previous::Feature),
            },
            Action::Gsettings { schema, key, value } => {
                match self.read("gsettings", &["get", schema, key], Privilege::NonRoot)? {
                    Some(output) if output.status.success() => {
                        let current = String::from_utf8_lossy(&output.stdout).trim().to_owned();
                        if current == *value {
                            Observed::Done
                        } else {
                            Observed::Pending(Previous::Gsettings { value: current })
                        }
                    }
                    _ => Observed::Unavailable,
                }
            }
            Action::Defaults {
                domain,
                key,
                kind,
                value,
                system,
            } => {
                let privilege = if *system { Privilege::Any } else { Privilege::NonRoot };
                let output = self
                    .read("defaults", &["read", domain, key], privilege)?
                    .ok_or(Error::ProgramNotFound("defaults"))?;
                let current = output
                    .status
                    .success()
                    .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned());
                if current.as_deref().map(|c| normalize_defaults(*kind, c)) == Some(normalize_defaults(*kind, value)) {
                    Observed::Done
                } else {
                    Observed::Pending(Previous::Defaults { value: current })
                }
            }
        };
        Ok(observed)
    }

    fn ensure_platform(&self, tweak: &Tweak) -> Result<()> {
        if tweak.os() == Some(self.system.os) {
            Ok(())
        } else {
            Err(Error::WrongPlatform(tweak.id.clone()))
        }
    }

    fn invalidate(&mut self) {
        self.appx.take();
        self.tasks.get_mut().clear();
        self.features.get_mut().clear();
    }

    fn package_for<'p>(
        &self,
        packages: &'p std::collections::BTreeMap<ManagerId, String>,
    ) -> Option<(ManagerId, &'p str)> {
        packages
            .iter()
            .find(|(manager, _)| self.system.manager(**manager).is_some())
            .map(|(manager, package)| (*manager, package.as_str()))
    }

    fn apply_invocations(&self, action: &Action, previous: &Previous) -> Result<Vec<Invocation>> {
        match (action, previous) {
            (Action::Package { packages }, Previous::Package { manager }) => {
                self.package_invocations(packages, *manager, Op::Remove)
            }
            _ => apply_cmds(action).iter().map(|cmd| self.invocation(cmd)).collect(),
        }
    }

    fn restore_invocations(&self, action: &Action, previous: &Previous) -> Result<Vec<Invocation>> {
        match (action, previous) {
            (Action::Package { packages }, Previous::Package { manager }) => {
                self.package_invocations(packages, *manager, Op::Install)
            }
            _ => restore_cmds(action, previous)?
                .iter()
                .map(|cmd| self.invocation(cmd))
                .collect(),
        }
    }

    fn package_invocations(
        &self,
        packages: &std::collections::BTreeMap<ManagerId, String>,
        manager: ManagerId,
        op: Op,
    ) -> Result<Vec<Invocation>> {
        // Kayıt dosyasındaki yönetici katalogdakiyle eşleşmeli; aksi halde kurcalanmıştır.
        let package = packages
            .get(&manager)
            .ok_or_else(|| Error::State(format!("{manager}: package manager not in catalog")))?;
        let step = Step {
            manager,
            items: vec![StepItem {
                app: package.clone(),
                package: package.clone(),
            }],
        };
        self.system.invocations(&step, op)
    }

    fn invocation(&self, cmd: &Cmd) -> Result<Invocation> {
        self.invocation_with(cmd, &self.system.privileges)
    }

    fn invocation_with(&self, cmd: &Cmd, privileges: &crate::exec::Privileges) -> Result<Invocation> {
        let program = if cmd.program == super::action::SELF {
            std::env::current_exe().map_err(|e| Error::State(e.to_string()))?
        } else if cmd.program == super::action::KWRITECONFIG {
            find_program("kwriteconfig6")
                .or_else(|| find_program("kwriteconfig5"))
                .ok_or(Error::ProgramNotFound("kwriteconfig6"))?
        } else {
            find_program(cmd.program).ok_or(Error::ProgramNotFound(cmd.program))?
        };

        Invocation::build(
            program,
            cmd.args.iter().map(OsString::from).collect(),
            &[],
            cmd.privilege,
            privileges,
            self.system.os,
        )
    }

    /// Okuma komutu çalıştırır; program yoksa `None`.
    fn read(&self, program: &'static str, args: &[&str], privilege: Privilege) -> Result<Option<std::process::Output>> {
        let Some(path) = find_program(program) else {
            return Ok(None);
        };
        let args = args.iter().map(OsString::from).collect();
        let invocation = Invocation::build(path, args, &[], privilege, &self.system.privileges, self.system.os)?;
        invocation.output().map(Some)
    }

    fn read_registry(&self, key: &str, name: &str, kind: RegKind) -> Result<Option<String>> {
        read_registry(key, name, kind)
    }

    fn powershell_lines(&self, script: &str) -> Result<Vec<String>> {
        let script = format!("[Console]::OutputEncoding = [Text.Encoding]::UTF8; {script}");
        let output = self.invocation(&powershell(&script, Privilege::Any))?.output()?;
        if !output.status.success() {
            return Err(Error::CommandFailed {
                program: "powershell".into(),
                code: output.status.code(),
                detail: String::from_utf8_lossy(&output.stderr)
                    .lines()
                    .next()
                    .map(str::to_owned),
            });
        }
        Ok(String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(|line| line.trim().to_owned())
            .filter(|line| !line.is_empty())
            .collect())
    }

    fn appx(&self) -> Result<&HashSet<String>> {
        if let Some(cached) = self.appx.get() {
            return Ok(cached);
        }
        let scope = if self.system.privileges.is_elevated {
            " -AllUsers"
        } else {
            ""
        };
        let names = self
            .powershell_lines(&format!("Get-AppxPackage{scope} | ForEach-Object {{ $_.Name }}"))?
            .into_iter()
            .map(|name| name.to_ascii_lowercase())
            .collect();
        Ok(self.appx.get_or_init(|| names))
    }

    /// Görevin etkin olup olmadığı; görev yoksa `None`.
    fn task_enabled(&self, task: &str) -> Result<Option<bool>> {
        let key = task.to_ascii_lowercase();
        if !self.tasks.borrow().contains_key(&key) {
            self.query_tasks(&[task])?;
        }
        Ok(self.tasks.borrow().get(&key).copied().flatten())
    }

    /// Verilen tweak'lerdeki zamanlanmış görevleri tek PowerShell çağrısıyla önceden okur.
    /// Tüm görevleri listelemek (`Get-ScheduledTask`) saniyeler sürdüğü için yalnızca
    /// gerekenler Task Scheduler COM arayüzüyle sorgulanır.
    pub fn prefetch(&self, tweaks: &[&Tweak]) -> Result<()> {
        let features: Vec<&str> = tweaks
            .iter()
            .flat_map(|tweak| &tweak.actions)
            .filter_map(|action| match action {
                Action::WindowsFeature { name } => Some(name.as_str()),
                _ => None,
            })
            .collect();
        if self.system.privileges.is_elevated {
            self.query_features(&features)?;
        }
        let wanted: Vec<&str> = tweaks
            .iter()
            .flat_map(|tweak| &tweak.actions)
            .filter_map(|action| match action {
                Action::ScheduledTask { task } if !self.tasks.borrow().contains_key(&task.to_ascii_lowercase()) => {
                    Some(task.as_str())
                }
                _ => None,
            })
            .collect();
        self.query_tasks(&wanted)
    }

    /// Özelliğin durumu (`Enabled`, `Disabled`, ...); yoksa `None`. Okumak yönetici yetkisi ister.
    fn feature_state(&self, name: &str) -> Result<Option<String>> {
        // Windows özellik durumunu yalnızca yöneticiye okutur; okunamayan durum "yok" sayılmaz.
        if !self.system.privileges.is_elevated {
            return Err(Error::ElevationRequired);
        }
        if !self.features.borrow().contains_key(name) {
            self.query_features(&[name])?;
        }
        Ok(self.features.borrow().get(name).cloned().flatten())
    }

    fn query_features(&self, names: &[&str]) -> Result<()> {
        let missing: Vec<&str> = names
            .iter()
            .copied()
            .filter(|n| !self.features.borrow().contains_key(*n))
            .collect();
        if missing.is_empty() {
            return Ok(());
        }
        let list = missing.iter().map(|n| ps_literal(n)).collect::<Vec<_>>().join(",");
        let script = format!(
            "foreach ($name in @({list})) {{ $f = Get-WindowsOptionalFeature -Online -FeatureName $name -ErrorAction SilentlyContinue; \
             if ($f) {{ \"$name|$($f.State)\" }} else {{ \"$name|missing\" }} }}"
        );
        let lines = self.powershell_lines(&script)?;
        let mut cache = self.features.borrow_mut();
        for line in lines {
            if let Some((name, state)) = line.rsplit_once('|') {
                cache.insert(name.to_owned(), (state != "missing").then(|| state.to_owned()));
            }
        }
        for name in missing {
            cache.entry(name.to_owned()).or_insert(None);
        }
        Ok(())
    }

    fn query_tasks(&self, tasks: &[&str]) -> Result<()> {
        if tasks.is_empty() {
            return Ok(());
        }
        let list = tasks.iter().map(|task| ps_literal(task)).collect::<Vec<_>>().join(",");
        let script = format!(
            "$service = New-Object -ComObject Schedule.Service; $service.Connect(); \
             foreach ($path in @({list})) {{ \
               $index = $path.LastIndexOf('\\'); $folder = $path.Substring(0, [Math]::Max($index, 1)); \
               try {{ $task = $service.GetFolder($folder).GetTask($path.Substring($index + 1)); \"$path|$($task.Enabled)\" }} \
               catch {{ \"$path|missing\" }} }}"
        );
        let mut cache = self.tasks.borrow_mut();
        for line in self.powershell_lines(&script)? {
            if let Some((task, state)) = line.rsplit_once('|') {
                let enabled = match state {
                    "True" => Some(true),
                    "False" => Some(false),
                    _ => None,
                };
                cache.insert(task.to_ascii_lowercase(), enabled);
            }
        }
        // Yanıtta olmayan görevler (beklenmedik çıktı) yok sayılır; tekrar sorgulanmasın.
        for task in tasks {
            cache.entry(task.to_ascii_lowercase()).or_insert(None);
        }
        Ok(())
    }
}

/// Registry değerini süreç içinde okur (her değer için `reg.exe` başlatmaktan çok daha hızlı).
/// Değer yoksa `None`. Değer beklenenden farklı türdeyse hata döner: aksi halde geri alma
/// var olan bir değeri "yoktu" sanıp silebilirdi.
#[cfg(windows)]
fn read_registry(path: &str, name: &str, kind: RegKind) -> Result<Option<String>> {
    use windows_registry::{CURRENT_USER, LOCAL_MACHINE, Type, USERS};

    // HRESULT_FROM_WIN32(ERROR_FILE_NOT_FOUND)
    const NOT_FOUND: i32 = 0x8007_0002_u32 as i32;
    let failure = |e: windows_registry::Result<()>| Error::Registry(format!("{path}\\{name}: {}", e.unwrap_err()));

    let (hive, subkey) = [("HKLM\\", LOCAL_MACHINE), ("HKCU\\", CURRENT_USER), ("HKU\\", USERS)]
        .into_iter()
        .find_map(|(prefix, hive)| path.strip_prefix(prefix).map(|rest| (hive, rest)))
        .ok_or_else(|| Error::Registry(format!("{path}: unsupported hive")))?;
    let key = match hive.open(subkey) {
        Ok(key) => key,
        Err(e) if e.code().0 == NOT_FOUND => return Ok(None),
        Err(e) => return Err(failure(Err(e))),
    };
    let ty = match key.get_type(name) {
        Ok(ty) => ty,
        Err(e) if e.code().0 == NOT_FOUND => return Ok(None),
        Err(e) => return Err(failure(Err(e))),
    };
    let value = match (kind, ty) {
        (RegKind::Dword, Type::U32) => key.get_u32(name).map(|v| v.to_string()),
        (RegKind::Qword, Type::U64) => key.get_u64(name).map(|v| v.to_string()),
        (RegKind::String, Type::String) => key.get_string(name),
        _ => return Err(Error::Registry(format!("{path}\\{name}: unexpected value type {ty:?}"))),
    };
    value.map(Some).map_err(|e| failure(Err(e)))
}

/// Yolun var olmayan en üst anahtarı (Skadi yazınca oluşacak olan); yol tamamen varsa `None`.
#[cfg(windows)]
fn missing_root(path: &str) -> Result<Option<String>> {
    let (hive, subkey) = split_hive(path)?;
    let mut prefix = String::new();
    for part in subkey.split('\\').filter(|p| !p.is_empty()) {
        if !prefix.is_empty() {
            prefix.push('\\');
        }
        prefix.push_str(part);
        if hive.open(&prefix).is_err() {
            let hive_name = &path[..path.len() - subkey.len()];
            return Ok(Some(format!("{hive_name}{prefix}")));
        }
    }
    Ok(None)
}

/// Anahtar ağacında hiç değer yoksa ağacı siler; en iyi çaba ilkesiyle çalışır.
#[cfg(windows)]
fn remove_if_empty(root: &str) {
    fn empty(key: &windows_registry::Key) -> bool {
        let no_values = key.values().is_ok_and(|mut values| values.next().is_none());
        let children_empty = key
            .keys()
            .map(|names| names.collect::<Vec<_>>())
            .is_ok_and(|names| names.iter().all(|name| key.open(name).is_ok_and(|child| empty(&child))));
        no_values && children_empty
    }
    let Ok((hive, subkey)) = split_hive(root) else { return };
    let Some((parent, leaf)) = subkey.rsplit_once('\\') else {
        return;
    };
    let Ok(target) = hive.open(subkey) else { return };
    if empty(&target)
        && let Ok(parent) = hive.options().read().write().open(parent)
    {
        let _ = parent.remove_tree(leaf);
    }
}

#[cfg(windows)]
fn split_hive(path: &str) -> Result<(&'static windows_registry::Key, &str)> {
    use windows_registry::{CURRENT_USER, LOCAL_MACHINE, USERS};
    [("HKLM\\", LOCAL_MACHINE), ("HKCU\\", CURRENT_USER), ("HKU\\", USERS)]
        .into_iter()
        .find_map(|(prefix, hive)| path.strip_prefix(prefix).map(|rest| (hive, rest)))
        .ok_or_else(|| Error::Registry(format!("{path}: unsupported hive")))
}

#[cfg(not(windows))]
fn missing_root(_path: &str) -> Result<Option<String>> {
    Ok(None)
}

#[cfg(not(windows))]
fn remove_if_empty(_root: &str) {}

#[cfg(not(windows))]
fn read_registry(_path: &str, _name: &str, _kind: RegKind) -> Result<Option<String>> {
    Err(Error::ProgramNotFound("reg"))
}

fn canonical(kind: RegKind, data: &str) -> String {
    match kind {
        RegKind::Dword | RegKind::Qword => data.parse::<u64>().map_or_else(|_| data.to_owned(), |n| n.to_string()),
        RegKind::String => data.to_owned(),
    }
}

/// Tweak komutları sessiz çalışır (ör. `reg.exe`'nin başarı mesajları gösterilmez); hata
/// olursa çıktının son satırı hataya eklenir.
fn run_all(invocations: &[Invocation]) -> Result<()> {
    invocations.iter().try_for_each(Invocation::run_captured)
}
