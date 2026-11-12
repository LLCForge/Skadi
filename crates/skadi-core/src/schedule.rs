//! Zamanlanmış haftalık güncelleme, yeniden başlatma bildirimi ve güncelleme geçmişi.
//!
//! Güvenlik: zamanlanmış görev Skadi'yi yetkili olarak çalıştırır. Skadi'nin dosyası kullanıcının
//! yazabildiği bir yerdeyse, kullanıcı hesabındaki bir zararlı onu değiştirip sessizce yetki
//! kazanabilirdi (CWE-732). Bu yüzden görev yalnızca korumalı bir konumdaki Skadi için kurulur.

use std::path::{Path, PathBuf};

use crate::{
    error::{Error, Result},
    platform::Os,
};

pub const WINDOWS_TASK: &str = "Skadi\\Update";
pub const SYSTEMD_SERVICE: &str = "/etc/systemd/system/skadi-update.service";
pub const SYSTEMD_TIMER: &str = "/etc/systemd/system/skadi-update.timer";
pub const LAUNCHD_LABEL: &str = "org.llcforge.skadi.update";

/// Skadi'nin dosyası yetkili bir görevin çalıştırması için güvenli bir yerde mi?
pub fn executable_is_protected(os: Os, exe: &Path) -> bool {
    match os {
        Os::Windows => ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"]
            .iter()
            .filter_map(std::env::var_os)
            .map(PathBuf::from)
            .any(|dir| exe.starts_with(dir)),
        Os::Linux => root_owned_and_locked(exe) && exe.parent().is_some_and(root_owned_and_locked),
        // macOS'ta görev kullanıcı olarak çalışır (Homebrew root istemez); yetki yükseltmesi yoktur.
        Os::Macos => true,
    }
}

#[cfg(unix)]
fn root_owned_and_locked(path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(path).is_ok_and(|meta| meta.uid() == 0 && meta.mode() & 0o022 == 0)
}

#[cfg(not(unix))]
fn root_owned_and_locked(_path: &Path) -> bool {
    false
}

pub fn systemd_service(exe: &Path) -> String {
    format!(
        "[Unit]\nDescription=Skadi weekly update\nWants=network-online.target\nAfter=network-online.target\n\n\
         [Service]\nType=oneshot\nExecStart={} update --yes --notify\n",
        exe.display()
    )
}

pub fn systemd_timer() -> String {
    "[Unit]\nDescription=Run the Skadi update every week\n\n[Timer]\nOnCalendar=weekly\nPersistent=true\n\
     RandomizedDelaySec=1h\n\n[Install]\nWantedBy=timers.target\n"
        .to_owned()
}

pub fn launchd_plist(exe: &Path) -> String {
    let escape = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\"><dict>\n\
         <key>Label</key><string>{LAUNCHD_LABEL}</string>\n\
         <key>ProgramArguments</key><array><string>{}</string><string>update</string><string>--yes</string><string>--notify</string></array>\n\
         <key>StartCalendarInterval</key><dict><key>Weekday</key><integer>0</integer><key>Hour</key><integer>12</integer></dict>\n\
         </dict></plist>\n",
        escape(&exe.to_string_lossy())
    )
}

pub fn launchd_path() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(format!("Library/LaunchAgents/{LAUNCHD_LABEL}.plist")))
}

/// Linux'ta yeniden başlatma gerekiyor mu? Debian/Ubuntu işaret dosyası ya da Fedora/RHEL
/// `needs-restarting -r` (1 = gerekiyor). Bilinemiyorsa `None`.
pub fn reboot_required() -> Option<bool> {
    if Path::new("/var/run/reboot-required").exists() {
        return Some(true);
    }
    let program = crate::exec::find_program("needs-restarting")?;
    let status = std::process::Command::new(program)
        .arg("-r")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .ok()?;
    status.code().map(|code| code == 1)
}

/// Denetim günlüğündeki bir satır.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryEntry {
    pub time: u64,
    pub command: String,
    pub ok: bool,
}

/// Denetim günlüğünü okur; en yeni kayıtlar sonda.
pub fn history(os: Os) -> Result<Vec<HistoryEntry>> {
    let path = crate::paths::data_dir(os)
        .ok_or_else(|| Error::State("no data directory".into()))?
        .join("audit.log");
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(Error::State(e.to_string())),
    };
    Ok(text.lines().filter_map(parse_history_line).collect())
}

fn parse_history_line(line: &str) -> Option<HistoryEntry> {
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    let program = value.get("program")?.as_str()?;
    // Günlük başka bir sistemden gelmiş olabilir; iki yol ayırıcısı da tanınır.
    let file = program.rsplit(['/', '\\']).next()?;
    let name = file.rsplit_once('.').map_or(file, |(stem, _)| stem).to_owned();
    if name.is_empty() {
        return None;
    }
    let args: Vec<&str> = value
        .get("args")?
        .as_array()?
        .iter()
        .filter_map(|a| a.as_str())
        .collect();
    Some(HistoryEntry {
        time: value.get("time")?.as_u64()?,
        command: std::iter::once(name.as_str()).chain(args).collect::<Vec<_>>().join(" "),
        ok: value.get("ok")?.as_bool()?,
    })
}

/// Unix zamanını `YYYY-MM-DD HH:MM` (UTC) biçimine çevirir.
pub fn format_utc(secs: u64) -> String {
    // Howard Hinnant'ın sivil takvim algoritması.
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}",
        rem / 3_600,
        rem % 3_600 / 60
    )
}

fn running_as_root() -> bool {
    std::fs::read_to_string("/proc/self/status").is_ok_and(|status| {
        status
            .lines()
            .find_map(|l| l.strip_prefix("Uid:"))
            .and_then(|uids| uids.split_whitespace().next())
            == Some("0")
    })
}

/// Root olarak çalışan zamanlayıcı: açık her masaüstü oturumuna (`/run/user/<uid>`) o kullanıcının
/// kimliğiyle bildirim gönderir.
fn notify_sessions(title: &str, body: &str) {
    use std::process::{Command, Stdio};
    let Some(runuser) = crate::exec::find_program("runuser") else {
        return;
    };
    let passwd = std::fs::read_to_string("/etc/passwd").unwrap_or_default();
    let Ok(sessions) = std::fs::read_dir("/run/user") else {
        return;
    };
    for entry in sessions.flatten() {
        let uid = entry.file_name().to_string_lossy().into_owned();
        if !uid.chars().all(|c| c.is_ascii_digit()) || uid.parse::<u32>().is_ok_and(|u| u < 1000) {
            continue;
        }
        let Some(user) = passwd.lines().find_map(|l| {
            let fields: Vec<&str> = l.split(':').collect();
            (fields.len() > 2 && fields[2] == uid).then(|| fields[0].to_owned())
        }) else {
            continue;
        };
        let bus = format!("DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/{uid}/bus");
        let _ = Command::new(&runuser)
            .args([
                "-u",
                &user,
                "--",
                "env",
                &bus,
                "notify-send",
                "--app-name=Skadi",
                "--",
                title,
                body,
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

/// Masaüstü bildirimi gösterir; gösterilemezse sessizce geçer (bildirim hiçbir işi durdurmamalı).
/// Metinler Skadi'nin sabit iletileridir; yine de her biçim için kaçışlanır.
pub fn notify(os: Os, title: &str, body: &str) {
    use std::process::{Command, Stdio};
    let run = |program: &str, args: &[&str]| {
        if let Some(path) = crate::exec::find_program(program) {
            let _ = Command::new(path)
                .args(args)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    };
    match os {
        Os::Linux if running_as_root() => notify_sessions(title, body),
        Os::Linux => run("notify-send", &["--app-name=Skadi", "--", title, body]),
        Os::Macos => {
            let quote = |s: &str| format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""));
            run(
                "osascript",
                &[
                    "-e",
                    &format!("display notification {} with title {}", quote(body), quote(title)),
                ],
            );
        }
        Os::Windows => {
            let quote = |s: &str| format!("'{}'", s.replace('\'', "''"));
            // Ek modül gerekmeden Windows PowerShell'in kendi bildirim kimliğiyle gösterilir.
            let script = format!(
                "$null = [Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime]; \
                 $xml = [Windows.UI.Notifications.ToastNotificationManager]::GetTemplateContent([Windows.UI.Notifications.ToastTemplateType]::ToastText02); \
                 $text = $xml.GetElementsByTagName('text'); \
                 $null = $text.Item(0).AppendChild($xml.CreateTextNode({})); \
                 $null = $text.Item(1).AppendChild($xml.CreateTextNode({})); \
                 $id = '{{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}}\\WindowsPowerShell\\v1.0\\powershell.exe'; \
                 [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier($id).Show([Windows.UI.Notifications.ToastNotification]::new($xml))",
                quote(title),
                quote(body)
            );
            run("powershell", &["-NoProfile", "-NonInteractive", "-Command", &script]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_utc_dates() {
        assert_eq!(format_utc(0), "1970-01-01 00:00");
        assert_eq!(format_utc(951_782_400), "2000-02-29 00:00");
        assert_eq!(format_utc(1_793_484_000), "2026-10-31 22:00");
        assert_eq!(format_utc(1_735_689_599), "2024-12-31 23:59");
        assert_eq!(format_utc(4_102_444_800), "2100-01-01 00:00");
    }

    #[test]
    fn parses_audit_lines() {
        let line =
            r#"{"time":10,"program":"C:\\x\\winget.exe","args":["install","--id","Git.Git"],"exit":0,"ok":true}"#;
        let entry = parse_history_line(line).unwrap();
        assert_eq!(entry.command, "winget install --id Git.Git");
        assert!(entry.ok);
        assert!(parse_history_line("garbage").is_none());
    }

    #[test]
    fn unit_files_run_update() {
        let service = systemd_service(Path::new("/usr/local/bin/skadi"));
        assert!(service.contains("ExecStart=/usr/local/bin/skadi update --yes"));
        assert!(systemd_timer().contains("OnCalendar=weekly"));
        assert!(launchd_plist(Path::new("/a&b/skadi")).contains("/a&amp;b/skadi"));
    }
}
