//! Çalışan sistemin anlık görüntüsü ve sistem üzerinde iş yapan tek katman.
//! Planlama (`plan`) saftır; komutları burada kurulan `Invocation`'lar çalıştırır.

use std::{ffi::OsString, path::PathBuf};

use crate::{
    error::{Error, Result},
    exec::{Invocation, Privilege, Privileges, find_program},
    manager::{Bootstrap, CmdTemplate, ManagerId, ManagerSpec, Program, validate_package_id},
    plan::{Step, StepItem},
    platform::{Os, OsRelease},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Install,
    Remove,
    /// Kaldırır ve yöneticinin kendi temizliğiyle yapılandırma dosyalarını da siler
    /// (apt `purge`, Homebrew `--zap`). Kullanıcı belgelerine dokunulmaz.
    Purge,
}

/// Sistemde bulunan bir paket yöneticisi.
#[derive(Debug, Clone)]
pub struct Manager {
    pub spec: &'static ManagerSpec,
    pub path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct System {
    pub os: Os,
    pub arch: &'static str,
    pub distro: Option<OsRelease>,
    /// Windows yapı numarası ve sürümü; yalnızca Windows'ta.
    pub windows: Option<WindowsVersion>,
    pub privileges: Privileges,
    pub settings: crate::settings::Settings,
    managers: Vec<Manager>,
}

/// Windows yapı numarası ve sürüm kimliği (`EditionID`: Core, Professional, Enterprise ...).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsVersion {
    pub build: u32,
    pub edition: String,
}

impl WindowsVersion {
    /// Home sürümlerinin `EditionID` değeri "Core" ile başlar (Core, CoreN, CoreSingleLanguage ...).
    pub fn is_home(&self) -> bool {
        self.edition.starts_with("Core")
    }

    #[cfg(windows)]
    fn detect() -> Option<Self> {
        let key = windows_registry::LOCAL_MACHINE
            .open("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion")
            .ok()?;
        Some(Self {
            build: key.get_string("CurrentBuildNumber").ok()?.trim().parse().ok()?,
            edition: key.get_string("EditionID").ok()?,
        })
    }

    #[cfg(not(windows))]
    fn detect() -> Option<Self> {
        None
    }
}

/// Dağıtım ailesi → yerel paket yöneticisi. `ID_LIKE` sayesinde türevler de eşleşir.
const DISTRO_MANAGERS: &[(&str, ManagerId)] = &[
    ("debian", ManagerId::Apt),
    ("ubuntu", ManagerId::Apt),
    ("fedora", ManagerId::Dnf),
    ("rhel", ManagerId::Dnf),
    ("centos", ManagerId::Dnf),
    ("suse", ManagerId::Zypper),
    ("opensuse", ManagerId::Zypper),
    ("arch", ManagerId::Pacman),
    ("alpine", ManagerId::Apk),
    ("void", ManagerId::Xbps),
    ("gentoo", ManagerId::Emerge),
    ("solus", ManagerId::Eopkg),
    ("mageia", ManagerId::Urpmi),
];

fn distro_manager(release: &OsRelease) -> Option<ManagerId> {
    DISTRO_MANAGERS
        .iter()
        .find(|(family, _)| release.is_like(family))
        .map(|(_, id)| *id)
}

impl System {
    pub fn detect() -> Result<Self> {
        let os = Os::current()?;
        let mut system = Self {
            os,
            arch: std::env::consts::ARCH,
            distro: if os == Os::Linux { OsRelease::load() } else { None },
            windows: if os == Os::Windows {
                WindowsVersion::detect()
            } else {
                None
            },
            privileges: Privileges::detect(os),
            settings: crate::settings::Settings::load(os)?,
            managers: Vec::new(),
        };
        system.refresh();
        Ok(system)
    }

    /// Paket yöneticilerini yeniden tarar (ör. bir yönetici kurulduktan sonra).
    pub fn refresh(&mut self) {
        let preferred = self.distro.as_ref().and_then(distro_manager);
        let mut managers: Vec<Manager> = ManagerId::ALL
            .into_iter()
            .map(ManagerId::spec)
            .filter(|spec| spec.os == self.os)
            .filter_map(|spec| spec.locate().map(|path| Manager { spec, path }))
            .collect();
        // Dağıtımın kendi yöneticisi öne alınır; kararlı sıralama diğerlerinin sırasını korur.
        managers.sort_by_key(|m| Some(m.spec.id) != preferred);
        // Kullanıcının tercih sırası her şeyin önüne geçer.
        let prefer = &self.settings.install.prefer;
        managers.sort_by_key(|m| prefer.iter().position(|id| *id == m.spec.id).unwrap_or(usize::MAX));
        self.managers = managers;
    }

    /// Bulunan yöneticiler, tercih sırasıyla.
    pub fn managers(&self) -> &[Manager] {
        &self.managers
    }

    pub fn manager(&self, id: ManagerId) -> Option<&Manager> {
        self.managers.iter().find(|m| m.spec.id == id)
    }

    pub fn available_ids(&self) -> Vec<ManagerId> {
        self.managers.iter().map(|m| m.spec.id).collect()
    }

    /// Sistem sürücüsündeki boş alan (bayt); ölçülemezse `None`.
    pub fn free_space(&self) -> Option<u64> {
        let root = match self.os {
            Os::Windows => PathBuf::from(format!(
                "{}\\",
                std::env::var("SystemDrive").unwrap_or_else(|_| "C:".to_owned())
            )),
            _ => PathBuf::from("/"),
        };
        let disks = sysinfo::Disks::new_with_refreshed_list();
        disks
            .list()
            .iter()
            .filter(|disk| root.starts_with(disk.mount_point()))
            .max_by_key(|disk| disk.mount_point().as_os_str().len())
            .map(sysinfo::Disk::available_space)
    }

    pub fn system_manager(&self) -> Option<&Manager> {
        self.managers.iter().find(|m| m.spec.system)
    }

    /// Bir adımı çalıştırmak için gereken komutlar. Toplu kurulum desteklemeyen yöneticilerde
    /// her paket ayrı komut olur.
    pub fn invocations(&self, step: &Step, op: Op) -> Result<Vec<Invocation>> {
        let manager = self
            .manager(step.manager)
            .ok_or(Error::ManagerNotFound(step.manager.as_str()))?;
        let spec = manager.spec;
        let purge_template;
        let template = match op {
            Op::Install => &spec.install,
            Op::Remove => &spec.remove,
            Op::Purge => match spec.id.purge() {
                Some(t) => {
                    purge_template = t;
                    &purge_template
                }
                None => &spec.remove,
            },
        };
        let program = resolve(spec, Some(&manager.path), template.program)?;
        let packages: Vec<&str> = step.items.iter().map(|item| item.package.as_str()).collect();
        for package in &packages {
            validate_package_id(package)?;
        }
        let chunks: Vec<&[&str]> = if spec.batch {
            vec![packages.as_slice()]
        } else {
            packages.chunks(1).collect()
        };
        let flatpak_user = spec.id == ManagerId::Flatpak && self.settings.install.flatpak_user;
        let build = |args: Vec<OsString>| {
            Invocation::build(
                program.clone(),
                args,
                spec.env,
                spec.privilege,
                &self.privileges,
                self.os,
            )
        };
        let mut invocations = Vec::new();
        if op == Op::Install {
            if spec.id == ManagerId::Scoop {
                for bucket in missing_scoop_buckets(&packages)? {
                    invocations.push(build(os_args(&["bucket", "add", bucket]))?);
                }
            }
            if flatpak_user {
                // Kullanıcı kurulumunun kendi Flathub deposu olmalıdır.
                invocations.push(build(os_args(&[
                    "remote-add",
                    "--user",
                    "--if-not-exists",
                    "flathub",
                    "https://dl.flathub.org/repo/flathub.flatpakrepo",
                ]))?);
            }
        }
        for chunk in chunks.into_iter().filter(|chunk| !chunk.is_empty()) {
            let mut args = template_args(template, chunk);
            if flatpak_user {
                args.insert(1, OsString::from("--user"));
            }
            invocations.push(build(args)?);
        }
        Ok(invocations)
    }

    /// Paketin kurulu olup olmadığı; yönetici bunu sorgulayamıyorsa `None`.
    pub fn is_installed(&self, id: ManagerId, package: &str) -> Result<Option<bool>> {
        let manager = self.manager(id).ok_or(Error::ManagerNotFound(id.as_str()))?;
        let spec = manager.spec;
        let Some(query) = &spec.query else {
            return Ok(None);
        };
        validate_package_id(package)?;
        let program = resolve(spec, Some(&manager.path), query.cmd.program)?;
        // Sorgular yalnızca okur; root gerektiren yöneticilerde bile yetki yükseltilmez.
        let privilege = match spec.privilege {
            Privilege::Root => Privilege::Any,
            other => other,
        };
        let output = Invocation::build(
            program,
            template_args(&query.cmd, &[package]),
            spec.env,
            privilege,
            &self.privileges,
            self.os,
        )?
        .output()?;
        Ok(Some(query.check.matches(&output)))
    }

    /// Yapılmasına gerek olmayan öğeleri ayıklar (kurulu olanı kurmak, olmayanı kaldırmak).
    /// Durumu sorgulanamayan öğeler planda kalır.
    pub fn prune(&self, step: &Step, op: Op) -> (Step, Vec<StepItem>) {
        let (skip, keep): (Vec<StepItem>, Vec<StepItem>) = step.items.iter().cloned().partition(|item| {
            let installed = self.is_installed(step.manager, &item.package).ok().flatten();
            matches!(
                (op, installed),
                (Op::Install, Some(true)) | (Op::Remove | Op::Purge, Some(false))
            )
        });
        (
            Step {
                manager: step.manager,
                items: keep,
            },
            skip,
        )
    }

    /// Eksik bir paket yöneticisini resmî yöntemiyle kurar ve sistemi yeniden tarar.
    pub fn bootstrap(&mut self, id: ManagerId) -> Result<()> {
        let spec = id.spec();
        let bootstrap = spec
            .bootstrap
            .filter(|_| spec.os == self.os)
            .ok_or(Error::BootstrapUnavailable(id))?;
        match bootstrap {
            Bootstrap::PowerShell { script, privilege } => {
                let powershell = find_program("powershell").ok_or(Error::ProgramNotFound("powershell"))?;
                let args = [
                    "-NoProfile",
                    "-NonInteractive",
                    "-ExecutionPolicy",
                    "Bypass",
                    "-Command",
                    script,
                ];
                Invocation::build(powershell, os_args(&args), &[], privilege, &self.privileges, self.os)?.run()?;
            }
            Bootstrap::Shell { script, env, privilege } => {
                let bash = find_program("bash").ok_or(Error::ProgramNotFound("bash"))?;
                Invocation::build(
                    bash,
                    os_args(&["-c", script]),
                    env,
                    privilege,
                    &self.privileges,
                    self.os,
                )?
                .run()?;
            }
            Bootstrap::SystemPackage { package, post } => {
                let native = self.system_manager().ok_or(Error::NoSystemManager)?.spec.id;
                let step = Step {
                    manager: native,
                    items: vec![StepItem {
                        app: id.as_str().to_owned(),
                        package: package.to_owned(),
                    }],
                };
                for invocation in self.invocations(&step, Op::Install)? {
                    invocation.run()?;
                }
                for template in post {
                    let program = resolve(spec, None, template.program)?;
                    Invocation::build(
                        program,
                        template_args(template, &[]),
                        &[],
                        Privilege::Root,
                        &self.privileges,
                        self.os,
                    )?
                    .run()?;
                }
            }
        }
        self.refresh();
        self.manager(id).map(|_| ()).ok_or(Error::BootstrapIncomplete(id))
    }
}

/// Kurulabilecek Scoop bucket'ları; başka bucket'a izin verilmez.
const SCOOP_BUCKETS: [&str; 6] = ["extras", "games", "nerd-fonts", "versions", "java", "nonportable"];

/// `bucket/uygulama` biçimindeki paketler için henüz eklenmemiş bucket'lar.
fn missing_scoop_buckets<'a>(packages: &[&'a str]) -> Result<Vec<&'a str>> {
    let root = std::env::var_os("SCOOP")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(|home| PathBuf::from(home).join("scoop")));
    let mut missing = Vec::new();
    for package in packages {
        let Some((bucket, _)) = package.split_once('/') else {
            continue;
        };
        if !SCOOP_BUCKETS.contains(&bucket) {
            return Err(Error::InvalidPackageId((*package).to_owned()));
        }
        let present = root
            .as_ref()
            .is_some_and(|root| root.join("buckets").join(bucket).is_dir());
        if !present && !missing.contains(&bucket) {
            missing.push(bucket);
        }
    }
    Ok(missing)
}

pub(crate) fn resolve(spec: &ManagerSpec, main: Option<&PathBuf>, program: Program) -> Result<PathBuf> {
    match program {
        Program::Main => main
            .cloned()
            .or_else(|| spec.locate())
            .ok_or(Error::ManagerNotFound(spec.program)),
        Program::Named(name) => find_program(name).ok_or(Error::ProgramNotFound(name)),
    }
}

pub(crate) fn template_args(template: &CmdTemplate, packages: &[&str]) -> Vec<OsString> {
    template.args.iter().chain(packages).map(OsString::from).collect()
}

fn os_args(args: &[&str]) -> Vec<OsString> {
    args.iter().map(OsString::from).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scoop_buckets_are_allowlisted() {
        assert!(missing_scoop_buckets(&["git"]).unwrap().is_empty());
        assert!(missing_scoop_buckets(&["evil/x"]).is_err());
        assert!(missing_scoop_buckets(&["extras/vlc", "extras/obs"]).unwrap().len() <= 1);
    }

    #[test]
    fn maps_distro_families() {
        let mint = OsRelease::parse("ID=linuxmint\nID_LIKE=\"ubuntu debian\"\n");
        assert_eq!(distro_manager(&mint), Some(ManagerId::Apt));
        let tumbleweed = OsRelease::parse("ID=opensuse-tumbleweed\nID_LIKE=\"opensuse suse\"\n");
        assert_eq!(distro_manager(&tumbleweed), Some(ManagerId::Zypper));
        let unknown = OsRelease::parse("ID=mystery\n");
        assert_eq!(distro_manager(&unknown), None);
    }

    #[test]
    fn appends_packages_after_template_args() {
        let args = template_args(&ManagerId::Winget.spec().install, &["Mozilla.Firefox"]);
        assert_eq!(
            args.iter().rev().take(2).collect::<Vec<_>>(),
            ["Mozilla.Firefox", "--id"]
        );
    }
}
