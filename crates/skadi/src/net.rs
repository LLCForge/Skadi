//! `skadi dns` ve `skadi network`: DNS sağlayıcıları, port ve bağlantı raporları, ağ sıfırlama.

use std::ffi::OsString;

use serde_json::json;
use skadi_core::{
    Error, Lang, Result,
    dns::{PROVIDERS, Provider, identify, measure, provider, resolved_dns_value, windows_set_script},
    exec::{Invocation, Privilege, find_program},
    platform::Os,
    system::System,
};

use crate::{commands::confirm, output::Output};

pub(crate) fn build(system: &System, program: &'static str, args: &[&str], privilege: Privilege) -> Result<Invocation> {
    let path = if program == "@self" {
        std::env::current_exe().map_err(|e| Error::State(e.to_string()))?
    } else {
        find_program(program).ok_or(Error::ProgramNotFound(program))?
    };
    Invocation::build(
        path,
        args.iter().map(OsString::from).collect(),
        &[],
        privilege,
        &system.privileges,
        system.os,
    )
}

fn stdout(system: &System, program: &'static str, args: &[&str]) -> Result<String> {
    let output = build(system, program, args, Privilege::Any)?.output()?;
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

pub fn dns_test(lang: Lang, out: &Output) -> Result<()> {
    let mut results: Vec<(&Provider, Option<u128>)> = PROVIDERS
        .iter()
        .map(|p| (p, measure(p.ipv4[0]).map(|d| d.as_millis())))
        .collect();
    results.sort_by_key(|(_, ms)| ms.unwrap_or(u128::MAX));
    if out.json {
        let rows: Vec<_> = results
            .iter()
            .map(|(p, ms)| json!({ "provider": p.id, "ms": ms }))
            .collect();
        return out.print_json(&json!({ "dns": rows }));
    }
    for (i, (provider, ms)) in results.iter().enumerate() {
        let time = ms.map_or_else(
            || lang.pick("no answer", "yanıt yok").to_owned(),
            |ms| format!("{ms} ms"),
        );
        let best = if i == 0 && ms.is_some() {
            lang.pick("  fastest", "  en hızlı")
        } else {
            ""
        };
        println!("  {:<12} {:<16} {time}{best}", provider.name, provider.ipv4[0]);
    }
    Ok(())
}

pub fn dns_status(lang: Lang, system: &System) -> Result<()> {
    let label = |address: &str| identify(address).map_or_else(String::new, |p| format!(" ({})", p.name));
    match system.os {
        Os::Windows => {
            let text = stdout(
                system,
                "powershell",
                &[
                    "-NoProfile",
                    "-NonInteractive",
                    "-Command",
                    skadi_core::dns::WINDOWS_STATUS_SCRIPT,
                ],
            )?;
            for line in text.lines().filter_map(|l| l.split_once('|')) {
                // fec0:0:0:ffff::1-3 Windows'un kullanılmayan bağdaştırıcılara koyduğu yer tutuculardır.
                let servers: Vec<String> = line
                    .1
                    .split_whitespace()
                    .filter(|s| !s.starts_with("fec0:0:0:ffff::"))
                    .map(|s| format!("{s}{}", label(s)))
                    .collect();
                if servers.is_empty() {
                    continue;
                }
                println!("  {:<28} {}", line.0, servers.join(", "));
            }
        }
        Os::Linux => {
            let text = stdout(system, "resolvectl", &["status"])?;
            for line in text
                .lines()
                .filter(|l| l.contains("DNS Server") || l.contains("DNSOverTLS") || l.contains("Protocols"))
            {
                println!("  {}", line.trim());
            }
        }
        Os::Macos => {
            let text = stdout(system, "scutil", &["--dns"])?;
            let mut seen = Vec::new();
            for server in text
                .lines()
                .filter_map(|l| l.trim().strip_prefix("nameserver[").and_then(|r| r.split(':').nth(1)))
            {
                let server = server.trim().to_owned();
                if !seen.contains(&server) {
                    println!("  {server}{}", label(&server));
                    seen.push(server);
                }
            }
        }
    }
    let _ = lang;
    Ok(())
}

/// macOS'taki etkin ağ servisleri (devre dışı olanlar `*` ile başlar).
fn mac_services(system: &System) -> Result<Vec<String>> {
    let text = stdout(system, "networksetup", &["-listallnetworkservices"])?;
    Ok(text
        .lines()
        .skip(1)
        .filter(|l| !l.starts_with('*') && !l.trim().is_empty())
        .map(str::to_owned)
        .collect())
}

/// DNS'i bir sağlayıcıya ayarlar (`Some`) ya da varsayılana döndürür (`None`).
pub fn dns_apply(lang: Lang, system: &System, target: Option<&str>, dry_run: bool, yes: bool) -> Result<()> {
    let t = |en, tr| lang.pick(en, tr);
    let preview = crate::commands::preview_system(lang, system, dry_run);
    let system = preview.as_ref().unwrap_or(system);
    let chosen = match target {
        Some(id) => Some(provider(id).ok_or_else(|| Error::Profile(format!("unknown DNS provider: {id}")))?),
        None => None,
    };
    let mut plan: Vec<Invocation> = Vec::new();
    let mut note = None;
    match system.os {
        Os::Windows => {
            let doh = system.windows.as_ref().is_some_and(|w| w.build >= 22000);
            let script = match chosen {
                Some(p) => windows_set_script(p, doh),
                None => skadi_core::dns::WINDOWS_RESET_SCRIPT.to_owned(),
            };
            if chosen.is_some() && !doh {
                note = Some(t(
                    "DNS over HTTPS needs Windows 11; plain DNS will be used.",
                    "DNS-over-HTTPS Windows 11 gerektirir; düz DNS kullanılacak.",
                ));
            }
            plan.push(build(
                system,
                "powershell",
                &["-NoProfile", "-NonInteractive", "-Command", &script],
                Privilege::Root,
            )?);
        }
        Os::Linux => {
            if !std::path::Path::new("/etc/systemd/resolved.conf").exists() {
                return Err(Error::ProgramNotFound("systemd-resolved"));
            }
            let path = "/etc/systemd/resolved.conf";
            match chosen {
                Some(p) => {
                    let value = resolved_dns_value(p);
                    plan.push(build(
                        system,
                        "@self",
                        &["internal", "config-set", path, "Resolve", "DNS", "value", &value],
                        Privilege::Root,
                    )?);
                    plan.push(build(
                        system,
                        "@self",
                        &["internal", "config-set", path, "Resolve", "DNSOverTLS", "value", "yes"],
                        Privilege::Root,
                    )?);
                }
                None => {
                    for key in ["DNS", "DNSOverTLS"] {
                        plan.push(build(
                            system,
                            "@self",
                            &["internal", "config-set", path, "Resolve", key, "remove", ""],
                            Privilege::Root,
                        )?);
                    }
                }
            }
            plan.push(build(
                system,
                "systemctl",
                &["restart", "systemd-resolved"],
                Privilege::Root,
            )?);
        }
        Os::Macos => {
            note = Some(t(
                "macOS applies plain DNS here; encrypted DNS needs a configuration profile.",
                "macOS'ta burada düz DNS uygulanır; şifreli DNS için yapılandırma profili gerekir.",
            ));
            for service in mac_services(system)? {
                let mut args = vec!["-setdnsservers".to_owned(), service];
                match chosen {
                    Some(p) => args.extend(p.ipv4.iter().chain(p.ipv6).map(|s| (*s).to_owned())),
                    None => args.push("Empty".to_owned()),
                }
                let refs: Vec<&str> = args.iter().map(String::as_str).collect();
                plan.push(build(system, "networksetup", &refs, Privilege::Root)?);
            }
        }
    }
    if let Some(note) = note {
        println!("{note}");
    }
    for invocation in &plan {
        println!("> {}", invocation.display());
    }
    if dry_run {
        return Ok(());
    }
    let question = match chosen {
        Some(p) => format!("{} {}?", t("Switch DNS to", "DNS şuna geçirilsin mi:"), p.name),
        None => t(
            "Reset DNS to the network default?",
            "DNS ağın varsayılanına döndürülsün mü?",
        )
        .to_owned(),
    };
    if !confirm(lang, yes, &question)? {
        return Ok(());
    }
    plan.iter().try_for_each(Invocation::run_captured)?;
    println!("{}", t("Done.", "Tamamlandı."));
    Ok(())
}

#[derive(Debug, Clone)]
pub struct Socket {
    pub protocol: String,
    pub local: String,
    pub remote: String,
    pub process: String,
}

/// Dinleyen portlar (`listening`) ya da kurulu bağlantılar.
fn sockets(system: &System, listening: bool) -> Result<Vec<Socket>> {
    let mut rows = Vec::new();
    match system.os {
        Os::Windows => {
            // PID -> süreç adı.
            let tasks = stdout(system, "tasklist", &["/fo", "csv", "/nh"])?;
            let names: std::collections::HashMap<String, String> = tasks
                .lines()
                .filter_map(|l| {
                    let parts: Vec<&str> = l.split("\",\"").collect();
                    (parts.len() >= 2).then(|| {
                        (
                            parts[1].trim_matches('"').to_owned(),
                            parts[0].trim_matches('"').to_owned(),
                        )
                    })
                })
                .collect();
            for line in stdout(system, "netstat", &["-ano"])?.lines() {
                let cols: Vec<&str> = line.split_whitespace().collect();
                let (protocol, local, remote, state, pid) = match cols.as_slice() {
                    [p @ "TCP", l, r, s, pid] => (*p, *l, *r, *s, *pid),
                    [p @ "UDP", l, r, pid] => (*p, *l, *r, "", *pid),
                    _ => continue,
                };
                let wanted = if listening {
                    state == "LISTENING" || protocol == "UDP"
                } else {
                    state == "ESTABLISHED"
                };
                if wanted {
                    rows.push(Socket {
                        protocol: protocol.to_owned(),
                        local: local.to_owned(),
                        remote: if listening { String::new() } else { remote.to_owned() },
                        process: names.get(pid).cloned().unwrap_or_else(|| format!("pid {pid}")),
                    });
                }
            }
        }
        Os::Linux => {
            let args: &[&str] = if listening {
                &["-tulpnH"]
            } else {
                &["-tupnH", "state", "established"]
            };
            for line in stdout(system, "ss", args)?.lines() {
                let cols: Vec<&str> = line.split_whitespace().collect();
                // ss: Netid State Recv-Q Send-Q Local Peer [Process]; "state established" State sütununu kaldırır.
                let (protocol, local, remote, process) = if listening {
                    (cols.first(), cols.get(4), cols.get(5), cols.get(6))
                } else {
                    (cols.first(), cols.get(3), cols.get(4), cols.get(5))
                };
                let (Some(protocol), Some(local)) = (protocol, local) else {
                    continue;
                };
                let process = process.and_then(|p| p.split('"').nth(1)).unwrap_or("-").to_owned();
                rows.push(Socket {
                    protocol: (*protocol).to_owned(),
                    local: (*local).to_owned(),
                    remote: if listening {
                        String::new()
                    } else {
                        remote.map(|r| (*r).to_owned()).unwrap_or_default()
                    },
                    process,
                });
            }
        }
        Os::Macos => {
            let args: &[&str] = if listening {
                &["-nP", "-iTCP", "-sTCP:LISTEN"]
            } else {
                &["-nP", "-iTCP", "-sTCP:ESTABLISHED"]
            };
            for line in stdout(system, "lsof", args)?.lines().skip(1) {
                let cols: Vec<&str> = line.split_whitespace().collect();
                let (Some(command), Some(name)) = (cols.first(), cols.get(8)) else {
                    continue;
                };
                let (local, remote) = name.split_once("->").unwrap_or((name, ""));
                rows.push(Socket {
                    protocol: "TCP".to_owned(),
                    local: local.to_owned(),
                    remote: remote.to_owned(),
                    process: (*command).to_owned(),
                });
            }
        }
    }
    rows.sort_by(|a, b| a.process.to_lowercase().cmp(&b.process.to_lowercase()));
    Ok(rows)
}

pub fn network_sockets(lang: Lang, out: &Output, listening: bool) -> Result<()> {
    let system = System::detect()?;
    let rows = sockets(&system, listening)?;
    if out.json {
        let rows: Vec<_> = rows
            .iter()
            .map(|r| json!({ "protocol": r.protocol, "local": r.local, "remote": r.remote, "process": r.process }))
            .collect();
        return out.print_json(&json!({ "sockets": rows, "listening": listening }));
    }
    for row in &rows {
        if listening {
            println!("  {:<5} {:<28} {}", row.protocol, row.local, row.process);
        } else {
            println!(
                "  {:<5} {:<28} {:<28} {}",
                row.protocol, row.local, row.remote, row.process
            );
        }
    }
    let what = if listening {
        lang.pick("listening sockets", "dinleyen soket")
    } else {
        lang.pick("open connections", "açık bağlantı")
    };
    println!("\n{} {what}", rows.len());
    Ok(())
}

/// Ağ yığınını sıfırlar: Windows'ta Winsock ve TCP/IP, Linux'ta NetworkManager'ı yeniden başlatır.
pub fn network_reset(lang: Lang, dry_run: bool, yes: bool) -> Result<()> {
    let t = |en, tr| lang.pick(en, tr);
    let detected = System::detect()?;
    let preview = crate::commands::preview_system(lang, &detected, dry_run);
    let system = preview.as_ref().unwrap_or(&detected);
    let plan = match system.os {
        Os::Windows => vec![
            build(system, "netsh", &["winsock", "reset"], Privilege::Root)?,
            build(system, "netsh", &["int", "ip", "reset"], Privilege::Root)?,
            build(system, "ipconfig", &["/flushdns"], Privilege::Root)?,
        ],
        Os::Linux => vec![build(
            system,
            "systemctl",
            &["restart", "NetworkManager"],
            Privilege::Root,
        )?],
        Os::Macos => return Err(Error::Unsupported("network reset (macOS)")),
    };
    for invocation in &plan {
        println!("> {}", invocation.display());
    }
    if dry_run
        || !confirm(
            lang,
            yes,
            t(
                "Reset the network stack? A restart may be needed (Windows).",
                "Ağ yığını sıfırlansın mı? Yeniden başlatma gerekebilir (Windows).",
            ),
        )?
    {
        return Ok(());
    }
    plan.iter().try_for_each(Invocation::run)
}

/// Basit bağlantı tanılaması: DNS çözümleme, şifreli DNS sağlayıcısına ulaşma ve HTTPS bağlantısı.
pub fn network_diagnose(lang: Lang) -> Result<()> {
    use std::net::{TcpStream, ToSocketAddrs};
    let t = |en, tr| lang.pick(en, tr);
    let mark = |ok: bool| if ok { "[+]" } else { "[x]" };
    let start = std::time::Instant::now();
    let resolved = ("example.com", 443).to_socket_addrs().ok().and_then(|mut a| a.next());
    println!(
        "  {} {:<36} {}",
        mark(resolved.is_some()),
        t("System DNS resolves names", "Sistem DNS'i adları çözüyor"),
        resolved.map_or_else(String::new, |_| format!("{} ms", start.elapsed().as_millis()))
    );
    let dns = measure("9.9.9.9");
    println!(
        "  {} {:<36} {}",
        mark(dns.is_some()),
        t("Public DNS reachable (Quad9)", "Genel DNS erişilebilir (Quad9)"),
        dns.map_or_else(String::new, |d| format!("{} ms", d.as_millis()))
    );
    let https = resolved.and_then(|addr| {
        let start = std::time::Instant::now();
        TcpStream::connect_timeout(&addr, std::time::Duration::from_secs(3))
            .ok()
            .map(|_| start.elapsed())
    });
    println!(
        "  {} {:<36} {}",
        mark(https.is_some()),
        t(
            "HTTPS port reachable (example.com)",
            "HTTPS portu erişilebilir (example.com)"
        ),
        https.map_or_else(String::new, |d| format!("{} ms", d.as_millis()))
    );
    Ok(())
}

/// Sistemde tanımlı proxy ayarları; tanımlı değilse doğrudan bağlantı.
pub fn proxy(lang: Lang, out: &Output) -> Result<()> {
    let system = System::detect()?;
    let settings = skadi_core::proxy::settings(system.os);
    if out.json {
        return out.print_json(&json!({ "proxy": settings }));
    }
    if settings.is_empty() {
        println!(
            "{}",
            lang.pick(
                "No proxy is configured; connections go out directly.",
                "Tanımlı proxy yok; bağlantılar doğrudan çıkıyor."
            )
        );
    }
    for setting in &settings {
        println!("  {:<20} {:<16} {}", setting.source, setting.name, setting.value);
    }
    Ok(())
}
