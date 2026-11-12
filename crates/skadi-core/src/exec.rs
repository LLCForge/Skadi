//! Güvenli komut çalıştırma. Hiçbir komut kabuk (shell) üzerinden kurulmaz: program mutlak
//! yol olarak çözülür, argümanlar tek tek aktarılır. Böylece komut enjeksiyonu yüzeyi kapanır.

use std::{
    env,
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

use crate::{
    error::{Error, Result},
    platform::Os,
};

/// Bir komutun hangi yetkiyle çalışması gerektiği.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Privilege {
    /// Normal kullanıcı yeterli; araç gerekirse kendisi yetki ister (winget, flatpak).
    Any,
    /// Yönetici/root gerekli.
    Root,
    /// Root olarak çalıştırılmamalı (Homebrew).
    NonRoot,
}

/// Programı `PATH` içinde arar ve mutlak yolunu döndürür.
pub fn find_program(name: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    env::split_paths(&path)
        // SECURITY FIX: CWE-427 göreli PATH girdileri ("." gibi) çalışma dizinindeki sahte
        // bir programın çalıştırılmasına yol açabilir; yalnızca mutlak dizinler aranır.
        .filter(|dir| dir.is_absolute())
        .find_map(|dir| {
            EXTENSIONS
                .iter()
                .map(|ext| dir.join(format!("{name}{ext}")))
                .find(|candidate| is_executable(candidate))
        })
}

// Rust yalnızca .exe ve .bat/.cmd dosyalarını doğrudan başlatabilir.
#[cfg(windows)]
const EXTENSIONS: &[&str] = &[".exe", ".cmd", ".bat"];
#[cfg(not(windows))]
const EXTENSIONS: &[&str] = &[""];

#[cfg(windows)]
pub(crate) fn is_executable(path: &Path) -> bool {
    // WindowsApps altındaki uygulama takma adları (winget.exe) sıfır baytlık yeniden ayrıştırma
    // noktalarıdır; takip edilmeden okunmalıdır.
    path.symlink_metadata().is_ok_and(|meta| !meta.is_dir())
}

#[cfg(unix)]
pub(crate) fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

/// Sürecin yetki durumu ve yetki değiştirmek için kullanılabilecek araçlar.
#[derive(Debug, Clone, Default)]
pub struct Privileges {
    pub is_elevated: bool,
    /// `sudo` ya da `doas` yolu (yalnızca Unix).
    pub elevator: Option<PathBuf>,
    /// Skadi `sudo`/`doas` ile başlatıldıysa asıl kullanıcı.
    pub invoking_user: Option<String>,
    /// Yükseltilmiş komutlara ortam değişkeni aktarmak için `env` yolu.
    pub env_program: Option<PathBuf>,
}

impl Privileges {
    pub fn detect(os: Os) -> Self {
        if os == Os::Windows {
            return Self {
                is_elevated: is_windows_admin(),
                ..Self::default()
            };
        }
        Self {
            is_elevated: is_unix_root(),
            elevator: find_program("sudo").or_else(|| find_program("doas")),
            invoking_user: ["SUDO_USER", "DOAS_USER"]
                .iter()
                .find_map(|var| env::var(var).ok())
                .or_else(pkexec_user)
                .filter(|user| is_valid_username(user) && user != "root"),
            env_program: find_program("env"),
        }
    }
}

/// `pkexec` ile başlatıldıysa asıl kullanıcının adı (`PKEXEC_UID` sayısal kimlik verir).
fn pkexec_user() -> Option<String> {
    let uid = env::var("PKEXEC_UID")
        .ok()
        .filter(|uid| !uid.is_empty() && uid.bytes().all(|b| b.is_ascii_digit()))?;
    let output = run_quiet("id", &["-nu", &uid])?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn run_quiet(name: &str, args: &[&str]) -> Option<Output> {
    let program = find_program(name)?;
    Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()
}

fn is_unix_root() -> bool {
    run_quiet("id", &["-u"])
        .is_some_and(|out| out.status.success() && String::from_utf8_lossy(&out.stdout).trim() == "0")
}

fn is_windows_admin() -> bool {
    // `net session` yalnızca yönetici olarak çalışan süreçlerde başarılı olur.
    run_quiet("net", &["session"]).is_some_and(|out| out.status.success())
}

// SECURITY FIX: CWE-88 ortamdan gelen kullanıcı adı `sudo -u` argümanına girer; seçenek gibi
// yorumlanabilecek ("-" ile başlayan) ya da beklenmeyen karakterler reddedilir.
fn is_valid_username(user: &str) -> bool {
    let mut chars = user.chars();
    user.len() <= 64
        && chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

/// Kurumsal ağlarda kullanılan ek kök sertifika yolları; proxy değişkenleriyle birlikte korunur.
pub const CERT_VARS: [&str; 5] = [
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
    "REQUESTS_CA_BUNDLE",
    "NODE_EXTRA_CA_CERTS",
    "CURL_CA_BUNDLE",
];

/// Yetki yükseltirken korunan proxy değişkenleri.
pub const PROXY_VARS: [&str; 8] = [
    "http_proxy",
    "https_proxy",
    "no_proxy",
    "all_proxy",
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "NO_PROXY",
    "ALL_PROXY",
];

/// Çalıştırılmaya hazır, tamamen çözülmüş bir komut.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub env: Vec<(String, String)>,
}

impl Invocation {
    /// İstenen yetkiye göre komutu doğrudan ya da `sudo`/`doas` ile sarmalayarak kurar.
    pub fn build(
        program: PathBuf,
        args: Vec<OsString>,
        env: &[(&str, &str)],
        privilege: Privilege,
        privileges: &Privileges,
        os: Os,
    ) -> Result<Self> {
        let unix = os != Os::Windows;
        match privilege {
            Privilege::Root if !privileges.is_elevated && !unix => Err(Error::ElevationRequired),
            Privilege::Root if !privileges.is_elevated => {
                let elevator = privileges.elevator.clone().ok_or(Error::NoElevator)?;
                Self::wrapped(elevator, Vec::new(), program, args, env, privileges)
            }
            Privilege::NonRoot if privileges.is_elevated && unix => {
                let user = privileges.invoking_user.clone().ok_or(Error::MustNotBeRoot)?;
                let elevator = privileges.elevator.clone().ok_or(Error::MustNotBeRoot)?;
                let prefix = vec![OsString::from("-u"), OsString::from(user)];
                Self::wrapped(elevator, prefix, program, args, env, privileges)
            }
            _ => Ok(Self {
                program,
                args,
                env: env.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect(),
            }),
        }
    }

    fn wrapped(
        elevator: PathBuf,
        mut prefix: Vec<OsString>,
        program: PathBuf,
        args: Vec<OsString>,
        env: &[(&str, &str)],
        privileges: &Privileges,
    ) -> Result<Self> {
        // Kurumsal proxy ayarları korunur. Yalnızca adlar aktarılır: değerler (parola içerebilir)
        // komut satırına ve denetim günlüğüne hiç yazılmaz. doas bunu yapılandırmayla yapar.
        if elevator.file_name().is_some_and(|n| n == "sudo") {
            let present: Vec<&str> = PROXY_VARS
                .iter()
                .chain(CERT_VARS.iter())
                .copied()
                .filter(|name| std::env::var_os(name).is_some_and(|v| !v.is_empty()))
                .collect();
            if !present.is_empty() {
                prefix.insert(0, OsString::from(format!("--preserve-env={}", present.join(","))));
            }
        }
        // sudo ortam değişkenlerini temizler; gerekli olanlar `env` ile açıkça aktarılır.
        if !env.is_empty() {
            let env_program = privileges.env_program.clone().ok_or(Error::ProgramNotFound("env"))?;
            prefix.push(env_program.into_os_string());
            prefix.extend(env.iter().map(|(k, v)| OsString::from(format!("{k}={v}"))));
        }
        prefix.push(program.into_os_string());
        prefix.extend(args);
        Ok(Self {
            program: elevator,
            args: prefix,
            env: Vec::new(),
        })
    }

    /// Çıktıyı terminale akıtarak çalıştırır.
    pub fn run(&self) -> Result<()> {
        let status = self.command().status().map_err(|source| self.spawn_error(source))?;
        audit(self, status.code(), status.success());
        if status.success() {
            Ok(())
        } else {
            Err(Error::CommandFailed {
                program: self.program_name(),
                code: status.code(),
                detail: None,
            })
        }
    }

    /// Çıktıyı yakalayarak çalıştırır; başarıda sessizdir, hatada son çıktı satırını döndürür.
    /// Standart girdi açık kalır, böylece `sudo` parola sorabilir.
    pub fn run_captured(&self) -> Result<()> {
        let output = self
            .command()
            .stdin(Stdio::inherit())
            .output()
            .map_err(|source| self.spawn_error(source))?;
        audit(self, output.status.code(), output.status.success());
        if output.status.success() {
            return Ok(());
        }
        let last_line = |bytes: &[u8]| {
            String::from_utf8_lossy(bytes)
                .lines()
                .map(str::trim)
                .rfind(|line| !line.is_empty())
                .map(|line| line.chars().take(300).collect::<String>())
        };
        Err(Error::CommandFailed {
            program: self.program_name(),
            code: output.status.code(),
            detail: last_line(&output.stderr).or_else(|| last_line(&output.stdout)),
        })
    }

    /// Çıktıyı yakalayarak çalıştırır; durum sorguları için.
    pub fn output(&self) -> Result<Output> {
        self.command()
            .stdin(Stdio::null())
            .output()
            .map_err(|source| self.spawn_error(source))
    }

    /// Kullanıcıya gösterilecek, kabukta okunabilir hali.
    pub fn display(&self) -> String {
        let env = self.env.iter().map(|(k, v)| quote(&format!("{k}={v}")));
        let words = std::iter::once(self.program.as_os_str())
            .chain(self.args.iter().map(OsString::as_os_str))
            .map(|word| quote(&word.to_string_lossy()));
        env.chain(words).collect::<Vec<_>>().join(" ")
    }

    fn command(&self) -> Command {
        let mut command = Command::new(&self.program);
        command.args(&self.args).envs(self.env.iter().map(|(k, v)| (k, v)));
        command
    }

    fn program_name(&self) -> String {
        self.program
            .file_name()
            .unwrap_or_else(|| OsStr::new("?"))
            .to_string_lossy()
            .into_owned()
    }

    fn spawn_error(&self, source: std::io::Error) -> Error {
        Error::Spawn {
            program: self.program_name(),
            source,
        }
    }
}

static AUDIT_LOG: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// Değişiklik yapan her komutun kaydedileceği denetim günlüğünü etkinleştirir (bir kez).
pub fn set_audit_log(path: PathBuf) {
    let _ = AUDIT_LOG.set(path);
}

/// Çalıştırılan komutu JSON satırı olarak günlüğe ekler. Günlük en iyi çaba ilkesiyle
/// yazılır: yazılamaması kullanıcının işlemini engellemez.
fn audit(invocation: &Invocation, code: Option<i32>, ok: bool) {
    let Some(path) = AUDIT_LOG.get() else {
        return;
    };
    let time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let entry = serde_json::json!({
        "time": time,
        "program": invocation.program.to_string_lossy(),
        "args": invocation.args.iter().map(|a| a.to_string_lossy()).collect::<Vec<_>>(),
        "exit": code,
        "ok": ok,
    });
    let _ = append_private(path, &format!("{entry}\n"));
}

fn append_private(path: &Path, line: &str) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut options = std::fs::OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)?.write_all(line.as_bytes())
}

/// Komutları, ilk hatada duran bir PowerShell betiğine dönüştürür.
pub fn powershell_script(invocations: &[Invocation]) -> String {
    let literal = |s: &str| format!("'{}'", s.replace('\'', "''"));
    let mut out = String::from("# Generated by Skadi. Review before running.\n$ErrorActionPreference = 'Stop'\n\n");
    for invocation in invocations {
        for (key, value) in &invocation.env {
            out.push_str(&format!("$env:{key} = {}\n", literal(value)));
        }
        out.push_str("& ");
        out.push_str(&literal(&invocation.program.to_string_lossy()));
        for arg in &invocation.args {
            out.push(' ');
            out.push_str(&literal(&arg.to_string_lossy()));
        }
        out.push_str("\nif ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }\n");
    }
    out
}

/// Komutları, ilk hatada duran bir POSIX kabuk betiğine dönüştürür.
pub fn posix_script(invocations: &[Invocation]) -> String {
    let literal = |s: &str| format!("'{}'", s.replace('\'', r"'\''"));
    let mut out = String::from("#!/bin/sh\n# Generated by Skadi. Review before running.\nset -eu\n\n");
    for invocation in invocations {
        for (key, value) in &invocation.env {
            out.push_str(&format!("{key}={} ", literal(value)));
        }
        out.push_str(&literal(&invocation.program.to_string_lossy()));
        for arg in &invocation.args {
            out.push(' ');
            out.push_str(&literal(&arg.to_string_lossy()));
        }
        out.push('\n');
    }
    out
}

fn quote(word: &str) -> String {
    let plain = !word.is_empty()
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./=:@#+,%\\".contains(c));
    if plain {
        word.to_owned()
    } else {
        format!("'{}'", word.replace('\'', r"'\''"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unix_user() -> Privileges {
        Privileges {
            is_elevated: false,
            elevator: Some(PathBuf::from("/usr/bin/sudo")),
            invoking_user: None,
            env_program: Some(PathBuf::from("/usr/bin/env")),
        }
    }

    fn args(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    #[test]
    fn root_command_is_wrapped_with_sudo_and_env() {
        let inv = Invocation::build(
            PathBuf::from("/usr/bin/apt-get"),
            args(&["install", "-y", "git"]),
            &[("DEBIAN_FRONTEND", "noninteractive")],
            Privilege::Root,
            &unix_user(),
            Os::Linux,
        )
        .unwrap();
        assert_eq!(inv.program, PathBuf::from("/usr/bin/sudo"));
        // Ortamda proxy tanımlıysa yalnızca adları taşıyan bir `--preserve-env=` öneki eklenir.
        let preserve = |a: &OsString| a.to_string_lossy().starts_with("--preserve-env=");
        if let Some(flag) = inv.args.first().filter(|a| preserve(a)) {
            let names = flag.to_string_lossy().trim_start_matches("--preserve-env=").to_owned();
            assert!(
                names
                    .split(',')
                    .all(|n| PROXY_VARS.contains(&n) || CERT_VARS.contains(&n)),
                "{names}"
            );
        }
        let rest: Vec<OsString> = inv.args.iter().filter(|a| !preserve(a)).cloned().collect();
        assert_eq!(
            rest,
            args(&[
                "/usr/bin/env",
                "DEBIAN_FRONTEND=noninteractive",
                "/usr/bin/apt-get",
                "install",
                "-y",
                "git"
            ])
        );
        assert!(inv.env.is_empty());
    }

    #[test]
    fn elevated_root_command_runs_directly() {
        let privileges = Privileges {
            is_elevated: true,
            ..unix_user()
        };
        let inv = Invocation::build(
            PathBuf::from("/usr/bin/pacman"),
            args(&["-S", "git"]),
            &[],
            Privilege::Root,
            &privileges,
            Os::Linux,
        )
        .unwrap();
        assert_eq!(inv.program, PathBuf::from("/usr/bin/pacman"));
    }

    #[test]
    fn non_root_tool_drops_privileges_or_fails() {
        let mut privileges = Privileges {
            is_elevated: true,
            ..unix_user()
        };
        let build = |p: &Privileges| {
            Invocation::build(
                PathBuf::from("/opt/homebrew/bin/brew"),
                args(&["install", "git"]),
                &[],
                Privilege::NonRoot,
                p,
                Os::Macos,
            )
        };
        assert!(matches!(build(&privileges), Err(Error::MustNotBeRoot)));
        privileges.invoking_user = Some("ayse".to_owned());
        let inv = build(&privileges).unwrap();
        // Ortamda proxy ya da sertifika değişkeni varsa başa `--preserve-env=` eklenir.
        let rest: Vec<OsString> = inv
            .args
            .iter()
            .filter(|a| !a.to_string_lossy().starts_with("--preserve-env="))
            .cloned()
            .collect();
        assert_eq!(rest, args(&["-u", "ayse", "/opt/homebrew/bin/brew", "install", "git"]));
    }

    #[test]
    fn windows_root_without_admin_fails() {
        let result = Invocation::build(
            PathBuf::from("C:/choco.exe"),
            Vec::new(),
            &[],
            Privilege::Root,
            &Privileges::default(),
            Os::Windows,
        );
        assert!(matches!(result, Err(Error::ElevationRequired)));
    }

    #[test]
    fn rejects_hostile_usernames() {
        assert!(is_valid_username("ayse"));
        assert!(!is_valid_username("-oProxyCommand"));
        assert!(!is_valid_username("a b"));
        assert!(!is_valid_username(""));
    }

    #[test]
    fn scripts_quote_every_argument() {
        let inv = Invocation {
            program: PathBuf::from("/usr/bin/apt-get"),
            args: args(&["install", "it's; rm -rf /"]),
            env: vec![("DEBIAN_FRONTEND".to_owned(), "noninteractive".to_owned())],
        };
        let sh = posix_script(std::slice::from_ref(&inv));
        assert!(sh.contains(r"DEBIAN_FRONTEND='noninteractive' '/usr/bin/apt-get' 'install' 'it'\''s; rm -rf /'"));
        assert!(sh.contains("set -eu"));
        let ps = powershell_script(&[inv]);
        assert!(ps.contains("'it''s; rm -rf /'"));
        assert!(ps.contains("$LASTEXITCODE"));
    }

    #[test]
    fn display_quotes_unsafe_words() {
        let inv = Invocation {
            program: PathBuf::from("/bin/echo"),
            args: args(&["a b", "it's"]),
            env: vec![("K".to_owned(), "v".to_owned())],
        };
        assert_eq!(inv.display(), r"K=v /bin/echo 'a b' 'it'\''s'");
    }
}
