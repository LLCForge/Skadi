//! Skadi komut satırı. Tüm iş `skadi-core` içindedir; burası yalnızca argümanları ayrıştırır,
//! planı gösterir ve onay alır. TUI ve GUI aynı çekirdeği kullanır.

mod care;
mod commands;
mod contribute;
mod controls;
mod desktop;
mod extras;
mod maintain;
mod net;
mod output;
mod profiles;
mod share;
mod tray;
mod trust;
mod tweaks;
#[cfg(any(feature = "tui", feature = "gui"))]
mod ui;

use std::{path::PathBuf, process::ExitCode};

use clap::{Args, CommandFactory, FromArgMatches, Parser, Subcommand};
use skadi_core::{Lang, platform::Os, system::Op};

use crate::output::Output;

#[derive(Parser)]
#[command(name = "skadi", version)]
struct Cli {
    #[arg(long, global = true, value_name = "LANG")]
    lang: Option<Lang>,

    #[arg(long, global = true)]
    json: bool,

    #[arg(long, global = true, value_name = "FILE")]
    script: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    Info,
    List {
        #[arg(long)]
        category: Option<String>,
        #[arg(long)]
        tag: Option<String>,
        #[arg(long)]
        ids: bool,
    },
    Installed,
    Show {
        app: String,
    },
    Install(OpArgs),
    Remove(OpArgs),
    Update {
        #[arg(long)]
        check: bool,
        #[arg(long)]
        firmware: bool,
        #[arg(long)]
        notify: bool,
        #[arg(long, value_name = "PACKAGE", conflicts_with_all = ["check", "firmware"])]
        rollback: Option<String>,
        #[arg(short, long)]
        manager: Option<String>,
        #[arg(long)]
        dry_run: bool,
        #[arg(short, long)]
        yes: bool,
    },
    Pin(PinArgs),
    Unpin(PinArgs),
    Schedule {
        #[arg(value_parser = ["enable", "disable", "status"])]
        action: String,
    },
    History {
        #[arg(long, default_value_t = 20)]
        limit: usize,
        #[arg(long)]
        timeline: bool,
    },
    Startup {
        #[arg(value_parser = ["disable", "enable"], requires = "name")]
        action: Option<String>,
        name: Option<String>,
        #[arg(long)]
        dry_run: bool,
    },
    Audit {
        #[arg(long, value_name = "FILE")]
        html: Option<PathBuf>,
    },
    CleanMetadata {
        #[arg(required = true, value_name = "FILE")]
        files: Vec<PathBuf>,
        #[arg(long)]
        in_place: bool,
    },
    Suggest,
    Permissions,
    Clean {
        #[arg(long)]
        dry_run: bool,
        #[arg(short, long)]
        yes: bool,
    },
    Power {
        target: Option<String>,
    },
    Report,
    Tray,
    Sign {
        #[command(subcommand)]
        command: SignCommand,
    },
    Policy {
        #[arg(value_parser = ["show", "install", "remove"], default_value = "show")]
        action: String,
        file: Option<PathBuf>,
        #[arg(long, value_name = "FILE")]
        public_key: Option<PathBuf>,
        #[arg(short, long)]
        yes: bool,
    },
    Icons {
        #[arg(value_parser = ["fetch", "clear"])]
        action: String,
        #[arg(short, long)]
        yes: bool,
    },
    Dotfiles {
        #[arg(value_parser = ["backup", "restore"])]
        action: String,
        dir: PathBuf,
        #[arg(long)]
        force: bool,
    },
    Bookmarks {
        dir: PathBuf,
    },
    WipeFree {
        path: Option<PathBuf>,
        #[arg(short, long)]
        yes: bool,
    },
    Appimage {
        #[arg(value_parser = ["install", "remove"])]
        action: String,
        name: String,
        url: Option<String>,
        #[arg(long)]
        sha256: Option<String>,
    },
    Bundle {
        #[arg(value_parser = ["create", "install"])]
        action: String,
        dir: PathBuf,
        #[arg(value_name = "APP")]
        apps: Vec<String>,
        #[arg(short, long)]
        yes: bool,
    },
    Remote {
        #[arg(value_parser = ["apply"])]
        action: String,
        target: String,
        #[arg(long = "host", required = true)]
        hosts: Vec<String>,
        #[arg(long)]
        dry_run: bool,
        #[arg(short, long)]
        yes: bool,
    },
    Source {
        #[arg(value_parser = ["list", "add"])]
        action: String,
        manager: String,
        name: Option<String>,
        url: Option<String>,
        #[arg(short, long)]
        yes: bool,
    },
    Certs,
    Wifi {
        #[arg(value_parser = ["export", "import"])]
        action: String,
        dir: PathBuf,
        #[arg(long)]
        with_passwords: bool,
        #[arg(short, long)]
        yes: bool,
    },
    SnapToFlatpak {
        app: String,
        #[arg(long)]
        remove_snapd: bool,
        #[arg(long)]
        dry_run: bool,
        #[arg(short, long)]
        yes: bool,
    },
    Hosts {
        #[arg(value_parser = ["block", "unblock", "status"], default_value = "status")]
        action: String,
        #[arg(short, long)]
        yes: bool,
    },
    Firewall {
        #[arg(value_parser = ["on", "status"], default_value = "status")]
        action: String,
        #[arg(long)]
        dry_run: bool,
        #[arg(short, long)]
        yes: bool,
    },
    Pagefile {
        #[arg(long)]
        auto: bool,
        #[arg(long)]
        dry_run: bool,
        #[arg(short, long)]
        yes: bool,
    },
    StartPins {
        #[arg(value_parser = ["apply", "revert"])]
        action: String,
        #[arg(long)]
        dry_run: bool,
        #[arg(short, long)]
        yes: bool,
    },
    DefaultBrowser {
        app: Option<String>,
    },
    GnomeExtensions {
        #[arg(value_parser = ["enable", "disable"], requires = "uuid")]
        action: Option<String>,
        uuid: Option<String>,
    },
    Spotlight {
        #[arg(value_parser = ["on", "off"])]
        state: Option<String>,
        #[arg(short, long)]
        yes: bool,
    },
    MacosProfile {
        path: PathBuf,
    },
    Share {
        #[arg(required = true, value_name = "APP")]
        apps: Vec<String>,
    },
    Scan {
        path: Option<PathBuf>,
        #[arg(long)]
        rootkit: bool,
    },
    Catalog {
        #[command(subcommand)]
        command: CatalogCommand,
    },
    BugReport,
    Setup {
        #[arg(long)]
        dry_run: bool,
    },
    Du {
        path: Option<PathBuf>,
        #[arg(long, default_value_t = 15)]
        top: usize,
    },
    Repair {
        #[arg(long)]
        dry_run: bool,
        #[arg(short, long)]
        yes: bool,
    },
    Dns {
        #[command(subcommand)]
        command: DnsCommand,
    },
    Network {
        #[command(subcommand)]
        command: NetworkCommand,
    },
    Bootstrap {
        manager: String,
        #[arg(short, long)]
        yes: bool,
        #[arg(long)]
        dry_run: bool,
    },
    Tweak {
        #[command(subcommand)]
        command: TweakCommand,
    },
    Profile {
        #[command(subcommand)]
        command: ProfileCommand,
    },
    Completions {
        shell: clap_complete::Shell,
    },
    Tui(UiArgs),
    Gui(UiArgs),
    /// Skadi'nin kendi iç işleri; doğrudan kullanılmak için değildir.
    #[command(hide = true)]
    Internal {
        #[command(subcommand)]
        command: InternalCommand,
    },
    Man,
}

#[derive(Subcommand)]
enum SignCommand {
    Keygen {
        name: PathBuf,
    },
    File {
        file: PathBuf,
        #[arg(long, value_name = "FILE")]
        key: PathBuf,
    },
    Verify {
        file: PathBuf,
        #[arg(long, value_name = "FILE")]
        public_key: PathBuf,
    },
}

#[derive(Subcommand)]
enum CatalogCommand {
    Add,
}

#[derive(Subcommand)]
enum DnsCommand {
    Test,
    Status,
    Set {
        provider: String,
        #[arg(long)]
        dry_run: bool,
        #[arg(short, long)]
        yes: bool,
    },
    Reset {
        #[arg(long)]
        dry_run: bool,
        #[arg(short, long)]
        yes: bool,
    },
}

#[derive(Subcommand)]
enum NetworkCommand {
    Ports,
    Connections,
    Reset {
        #[arg(long)]
        dry_run: bool,
        #[arg(short, long)]
        yes: bool,
    },
    Diagnose,
    Proxy {
        #[arg(value_parser = ["set", "reset"], requires_if("set", "address"))]
        action: Option<String>,
        address: Option<String>,
        #[arg(long)]
        dry_run: bool,
        #[arg(short, long)]
        yes: bool,
    },
}

#[derive(Subcommand)]
enum InternalCommand {
    /// Zamanlanmış güncellemenin systemd birimlerini kurar ya da kaldırır (Linux, root).
    ScheduleUnits {
        #[arg(value_parser = ["install", "remove"])]
        action: String,
    },
    /// Hosts dosyasındaki Skadi engel bloğunu yetkili olarak yazar ya da kaldırır.
    HostsSet {
        #[arg(value_parser = ["block", "unblock"])]
        mode: String,
        list: String,
    },
    /// Skadi'ye ait yapılandırma dosyasını yetkili olarak yazar ya da siler (tweak motoru kullanır).
    FileSet {
        path: String,
        reload: String,
        guard: String,
        #[arg(value_parser = ["write", "remove"])]
        mode: String,
        #[arg(allow_hyphen_values = true)]
        content: String,
    },
    /// Yapılandırma dosyasında tek satırı yetkili olarak düzenler (tweak motoru kullanır).
    ConfigSet {
        path: String,
        section: String,
        key: String,
        mode: String,
        #[arg(allow_hyphen_values = true)]
        text: String,
    },
}

#[derive(Subcommand)]
enum TweakCommand {
    List {
        #[arg(long)]
        category: Option<String>,
        #[arg(long, requires = "public_key", value_name = "FILE")]
        pack: Option<PathBuf>,
        #[arg(long, requires = "pack", value_name = "FILE")]
        public_key: Option<PathBuf>,
    },
    Apply(TweakArgs),
    Revert(TweakArgs),
    Backup {
        file: PathBuf,
    },
    DefaultUser {
        #[command(flatten)]
        args: TweakArgs,
        #[arg(long)]
        revert: bool,
    },
}

#[derive(Subcommand)]
enum ProfileCommand {
    List,
    Apply {
        target: String,
        #[arg(long, value_name = "FILE")]
        public_key: Option<PathBuf>,
        #[arg(long)]
        dry_run: bool,
        #[arg(short, long)]
        yes: bool,
    },
    Export {
        path: PathBuf,
    },
    Git {
        dir: PathBuf,
    },
    Convert {
        #[arg(value_parser = ["ansible", "brewfile", "winget", "nix", "intune"])]
        format: String,
        target: String,
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
}

#[derive(Args)]
struct TweakArgs {
    #[arg(value_name = "TWEAK", required_unless_present_any = ["safe", "all", "last"])]
    tweaks: Vec<String>,
    #[arg(long, conflicts_with_all = ["tweaks", "all", "last"])]
    safe: bool,
    #[arg(long, conflicts_with_all = ["tweaks", "last"])]
    all: bool,
    #[arg(long, conflicts_with = "tweaks")]
    last: bool,
    #[arg(long)]
    snapshot: bool,
    #[arg(long)]
    dry_run: bool,
    #[arg(short, long)]
    yes: bool,
    #[arg(long, requires = "public_key", value_name = "FILE")]
    pack: Option<PathBuf>,
    #[arg(long, requires = "pack", value_name = "FILE")]
    public_key: Option<PathBuf>,
}

#[derive(Args)]
struct OpArgs {
    #[arg(required_unless_present = "resume", value_name = "APP")]
    apps: Vec<String>,
    #[arg(long, conflicts_with = "apps")]
    resume: bool,
    #[arg(short, long)]
    manager: Option<String>,
    #[arg(long)]
    dry_run: bool,
    #[arg(short, long)]
    yes: bool,
    #[arg(long)]
    purge: bool,
}

/// `--screenshot` README görüntüleri için gizli bir seçenektir.
#[derive(Args, Default)]
struct UiArgs {
    #[arg(long, hide = true)]
    screenshot: Option<PathBuf>,
    #[arg(long, hide = true, default_value = "apps")]
    tab: String,
}

#[derive(Args)]
struct PinArgs {
    #[arg(required = true, value_name = "APP")]
    apps: Vec<String>,
    #[arg(long)]
    dry_run: bool,
}

/// `--lang` değerini clap'ten önce okur; yardım metinleri o dilde kurulabilsin diye.
fn early_lang() -> Lang {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let value = match arg.strip_prefix("--lang") {
            Some("") => args.next(),
            Some(rest) => rest.strip_prefix('=').map(str::to_owned),
            None => continue,
        };
        if let Some(lang) = value.as_deref().and_then(Lang::from_tag) {
            return lang;
        }
    }
    Lang::detect()
}

/// Yardım metinlerini seçilen dilde kurar.
fn localized_command(lang: Lang) -> clap::Command {
    let t = |en: &'static str, tr: &'static str| lang.pick(en, tr).to_owned();
    let dry_run = || {
        t(
            "Show the commands without running them",
            "Komutları çalıştırmadan göster",
        )
    };
    let yes = || t("Answer yes to confirmations", "Onay sorularına evet de");
    let category = || t("Show only this category", "Yalnızca bu kategoriyi göster");
    let op = |cmd: clap::Command, en: &'static str, tr: &'static str| {
        cmd.about(t(en, tr))
            .mut_arg("apps", |a| {
                a.help(t("App ids from the catalog", "Katalogdaki uygulama kimlikleri"))
            })
            .mut_arg("manager", |a| {
                a.help(t(
                    "Use only this package manager",
                    "Yalnızca bu paket yöneticisini kullan",
                ))
            })
            .mut_arg("resume", |a| {
                a.help(t(
                    "Continue the last install that did not finish (install only)",
                    "Yarıda kalan son kurulumu sürdür (yalnızca kurulum)",
                ))
            })
            .mut_arg("dry_run", |a| a.help(dry_run()))
            .mut_arg("yes", |a| a.help(yes()))
    };
    let pin = |cmd: clap::Command, en: &'static str, tr: &'static str| {
        cmd.about(t(en, tr))
            .mut_arg("apps", |a| {
                a.help(t("App ids from the catalog", "Katalogdaki uygulama kimlikleri"))
            })
            .mut_arg("dry_run", |a| a.help(dry_run()))
    };
    let tweak_args = |cmd: clap::Command, en: &'static str, tr: &'static str| {
        cmd.about(t(en, tr))
            .mut_arg("tweaks", |a| a.help(t("Tweak ids", "Tweak kimlikleri")))
            .mut_arg("safe", |a| {
                a.help(t(
                    "Every tweak marked safe for this system",
                    "Bu sistem için güvenli işaretli tüm tweak'ler",
                ))
            })
            .mut_arg("all", |a| {
                a.help(t(
                    "Every tweak with saved state (revert only)",
                    "Kayıtlı durumu olan tüm tweak'ler (yalnızca geri alma)",
                ))
            })
            .mut_arg("last", |a| {
                a.help(t(
                    "Tweaks applied in the last session (revert only)",
                    "Son oturumda uygulanan tweak'ler (yalnızca geri alma)",
                ))
            })
            .mut_arg("snapshot", |a| {
                a.help(t(
                    "Create a restore point or snapshot first (apply only)",
                    "Önce geri yükleme noktası ya da anlık görüntü oluştur (yalnızca uygulama)",
                ))
            })
            .mut_arg("dry_run", |a| a.help(dry_run()))
            .mut_arg("yes", |a| a.help(yes()))
    };

    Cli::command()
        .about(t(
            "Cross-platform app installer and system debloater",
            "Platformlar arası uygulama kurucu ve sistem sadeleştirici",
        ))
        .mut_arg("lang", |a| {
            a.help(t("Interface language (en, tr)", "Arayüz dili (en, tr)"))
        })
        .mut_arg("json", |a| {
            a.help(t(
                "Machine-readable JSON output",
                "Makine tarafından okunabilir JSON çıktısı",
            ))
        })
        .mut_arg("script", |a| {
            a.help(t(
                "Write the commands to a .ps1 or .sh script instead of running them",
                "Komutları çalıştırmak yerine .ps1 ya da .sh betiğine yaz",
            ))
        })
        .mut_subcommand("info", |c| {
            c.about(t(
                "Show system info and package managers",
                "Sistem bilgisini ve paket yöneticilerini göster",
            ))
        })
        .mut_subcommand("list", |c| {
            c.about(t("List apps in the catalog", "Katalogdaki uygulamaları listele"))
                .mut_arg("category", |a| a.help(category()))
                .mut_arg("ids", |a| {
                    a.help(t(
                        "Print only the ids of apps installable on this system",
                        "Yalnızca bu sistemde kurulabilen uygulamaların kimliklerini yaz",
                    ))
                })
        })
        .mut_subcommand("installed", |c| {
            c.about(t(
                "Show catalog apps that are installed",
                "Kurulu katalog uygulamalarını göster",
            ))
        })
        .mut_subcommand("show", |c| {
            c.about(t(
                "Show details, privacy notes and alternatives for an app",
                "Bir uygulamanın ayrıntılarını, gizlilik notlarını ve alternatiflerini göster",
            ))
            .mut_arg("app", |a| {
                a.help(t("App id from the catalog", "Katalogdaki uygulama kimliği"))
            })
        })
        .mut_subcommand("install", |c| op(c, "Install apps", "Uygulama kur"))
        .mut_subcommand("remove", |c| op(c, "Remove apps", "Uygulama kaldır"))
        .mut_subcommand("update", |c| {
            c.about(t("Update everything", "Her şeyi güncelle"))
                .mut_arg("rollback", |a| {
                    a.help(t(
                        "Put a package back to the version it had before the last update",
                        "Bir paketi son güncellemeden önceki sürümüne döndür",
                    ))
                })
                .mut_arg("notify", |a| {
                    a.help(t(
                        "Show a desktop notification when finished",
                        "Bitince masaüstü bildirimi göster",
                    ))
                })
                .mut_arg("check", |a| {
                    a.help(t(
                        "Only list available updates",
                        "Yalnızca mevcut güncellemeleri listele",
                    ))
                })
                .mut_arg("firmware", |a| {
                    a.help(t(
                        "Also update firmware with fwupd (Linux)",
                        "Firmware'i de fwupd ile güncelle (Linux)",
                    ))
                })
                .mut_arg("manager", |a| {
                    a.help(t(
                        "Use only this package manager",
                        "Yalnızca bu paket yöneticisini kullan",
                    ))
                })
                .mut_arg("dry_run", |a| a.help(dry_run()))
                .mut_arg("yes", |a| a.help(yes()))
        })
        .mut_subcommand("pin", |c| {
            pin(
                c,
                "Keep apps at their current version",
                "Uygulamaları mevcut sürümde sabitle",
            )
        })
        .mut_subcommand("unpin", |c| {
            pin(c, "Allow updates again", "Güncellemelere yeniden izin ver")
        })
        .mut_subcommand("schedule", |c| {
            c.about(t("Weekly automatic updates", "Haftalık otomatik güncelleme"))
                .mut_arg("action", |a| a.help("enable | disable | status"))
        })
        .mut_subcommand("clean-metadata", |c| {
            c.about(t(
                "Remove location, camera and other metadata from JPEG and PNG files",
                "JPEG ve PNG dosyalarından konum, kamera ve diğer meta verileri sil",
            ))
            .mut_arg("files", |a| a.help(t("Image files", "Görüntü dosyaları")))
            .mut_arg("in_place", |a| {
                a.help(t(
                    "Overwrite the files instead of writing .clean copies",
                    ".clean kopyalar yazmak yerine dosyaların üzerine yaz",
                ))
            })
        })
        .mut_subcommand("dns", |c| {
            c.about(t("Private DNS providers", "Gizlilik odaklı DNS sağlayıcıları"))
                .mut_subcommand("test", |c| {
                    c.about(t(
                        "Measure provider response times",
                        "Sağlayıcıların yanıt sürelerini ölç",
                    ))
                })
                .mut_subcommand("status", |c| {
                    c.about(t("Show the DNS servers in use", "Kullanılan DNS sunucularını göster"))
                })
                .mut_subcommand("set", |c| {
                    c.about(t(
                        "Use a provider (quad9, cloudflare, mullvad, adguard), encrypted where supported",
                        "Bir sağlayıcı kullan (quad9, cloudflare, mullvad, adguard); destekleniyorsa şifreli",
                    ))
                })
                .mut_subcommand("reset", |c| {
                    c.about(t("Go back to the network's DNS", "Ağın DNS'ine geri dön"))
                })
        })
        .mut_subcommand("network", |c| {
            c.about(t("Network reports and tools", "Ağ raporları ve araçları"))
                .mut_subcommand("ports", |c| {
                    c.about(t(
                        "Listening ports and their programs",
                        "Dinleyen portlar ve programları",
                    ))
                })
                .mut_subcommand("connections", |c| {
                    c.about(t(
                        "Open connections and their programs",
                        "Açık bağlantılar ve programları",
                    ))
                })
                .mut_subcommand("reset", |c| {
                    c.about(t("Reset the network stack", "Ağ yığınını sıfırla"))
                })
                .mut_subcommand("proxy", |c| {
                    c.about(t(
                        "Show proxy settings (passwords are masked)",
                        "Proxy ayarlarını göster (parolalar gizlenir)",
                    ))
                })
                .mut_subcommand("diagnose", |c| {
                    c.about(t("Basic connectivity checks", "Temel bağlantı denetimleri"))
                })
        })
        .mut_subcommand("clean", |c| {
            c.about(t(
                "Clean package caches, unused dependencies and old temporary files",
                "Paket önbelleklerini, kullanılmayan bağımlılıkları ve eski geçici dosyaları temizle",
            ))
        })
        .mut_subcommand("power", |c| {
            c.about(t(
                "List or switch power plans (Windows) and power profiles (Linux)",
                "Güç planlarını (Windows) ve güç profillerini (Linux) listele ya da değiştir",
            ))
        })
        .mut_subcommand("setup", |c| {
            c.about(t(
                "Fresh install wizard: apps, cleanup level and private DNS",
                "Temiz kurulum sihirbazı: uygulamalar, sadeleştirme seviyesi ve gizli DNS",
            ))
        })
        .mut_subcommand("bug-report", |c| {
            c.about(t(
                "Print a bug report draft without personal data",
                "Kişisel veri içermeyen bir hata bildirimi taslağı yazdır",
            ))
        })
        .mut_subcommand("catalog", |c| {
            c.about(t("Help with the app catalog", "Uygulama kataloğu için yardımcılar")).mut_subcommand("add", |c| {
                c.about(t(
                    "Prepare a new catalog entry for a pull request",
                    "Pull request için yeni bir katalog kaydı hazırla",
                ))
            })
        })
        .mut_subcommand("scan", |c| {
            c.about(t(
                "Scan for malware (Defender on Windows, ClamAV elsewhere; --rootkit runs rkhunter)",
                "Zararlı yazılım taraması (Windows'ta Defender, diğerlerinde ClamAV; --rootkit rkhunter çalıştırır)",
            ))
        })
        .mut_subcommand("share", |c| {
            c.about(t(
                "Turn an app selection into one-line install commands and a QR code",
                "Bir uygulama seçimini tek satırlık kurulum komutlarına ve QR koda çevir",
            ))
        })
        .mut_subcommand("default-browser", |c| {
            c.about(t(
                "Show or set the default web browser (Linux); opens the right Settings page on Windows and macOS",
                "Varsayılan tarayıcıyı göster ya da ayarla (Linux); Windows ve macOS'ta ilgili Ayarlar sayfasını açar",
            ))
        })
        .mut_subcommand("gnome-extensions", |c| {
            c.about(t(
                "List, turn off or turn on GNOME Shell extensions",
                "GNOME Shell eklentilerini listele, kapat ya da aç",
            ))
        })
        .mut_subcommand("spotlight", |c| {
            c.about(t(
                "Show, turn off or turn on Spotlight indexing (macOS)",
                "Spotlight dizinlemesini göster, kapat ya da aç (macOS)",
            ))
        })
        .mut_subcommand("macos-profile", |c| {
            c.about(t(
                "Write a macOS configuration profile with privacy and security settings",
                "Gizlilik ve güvenlik ayarlarıyla bir macOS yapılandırma profili yaz",
            ))
        })
        .mut_subcommand("certs", |c| {
            c.about(t(
                "List root certificates that deserve a second look",
                "Gözden geçirilmesi gereken kök sertifikaları listele",
            ))
        })
        .mut_subcommand("wifi", |c| {
            c.about(t(
                "Export or import Wi-Fi profiles",
                "Wi-Fi profillerini dışa ya da içe aktar",
            ))
        })
        .mut_subcommand("snap-to-flatpak", |c| {
            c.about(t(
                "Replace an app's Snap with its Flatpak (Linux)",
                "Bir uygulamanın Snap'ini Flatpak sürümüyle değiştir (Linux)",
            ))
        })
        .mut_subcommand("hosts", |c| {
            c.about(t(
                "Block ads and malware domains in the hosts file, or undo it",
                "Reklam ve zararlı yazılım alan adlarını hosts dosyasında engelle ya da geri al",
            ))
        })
        .mut_subcommand("firewall", |c| {
            c.about(t(
                "Show the firewall status or turn it on",
                "Güvenlik duvarı durumunu göster ya da aç",
            ))
        })
        .mut_subcommand("pagefile", |c| {
            c.about(t(
                "Show swap or page file usage; --auto lets Windows manage it",
                "Takas ya da sayfa dosyası kullanımını göster; --auto boyutu Windows'a bırakır",
            ))
        })
        .mut_subcommand("start-pins", |c| {
            c.about(t(
                "Keep only Settings and File Explorer pinned in Start (Windows 11 Pro+)",
                "Başlat'ta yalnızca Ayarlar ve Dosya Gezgini sabit kalsın (Windows 11 Pro+)",
            ))
        })
        .mut_subcommand("dotfiles", |c| {
            c.about(t(
                "Back up or restore shell and editor settings (keys and tokens are left out)",
                "Kabuk ve düzenleyici ayarlarını yedekle ya da geri yükle (anahtar ve tokenlar dışarıda kalır)",
            ))
        })
        .mut_subcommand("bookmarks", |c| {
            c.about(t(
                "Back up browser bookmarks (no cookies, history or passwords)",
                "Tarayıcı yer imlerini yedekle (çerez, geçmiş ve parola olmadan)",
            ))
        })
        .mut_subcommand("wipe-free", |c| {
            c.about(t(
                "Clean free disk space (cipher on Windows, TRIM on Linux)",
                "Boş disk alanını temizle (Windows'ta cipher, Linux'ta TRIM)",
            ))
        })
        .mut_subcommand("appimage", |c| {
            c.about(t(
                "Install or remove an AppImage verified by its SHA-256 (Linux)",
                "SHA-256 ile doğrulanan bir AppImage kur ya da kaldır (Linux)",
            ))
        })
        .mut_subcommand("bundle", |c| {
            c.about(t(
                "Download packages for offline install, or install such a bundle (Linux)",
                "Çevrimdışı kurulum için paket indir ya da böyle bir klasörü kur (Linux)",
            ))
        })
        .mut_subcommand("remote", |c| {
            c.about(t(
                "Apply a preset or profile on other machines over SSH",
                "Bir hazır seti ya da profili SSH ile başka makinelere uygula",
            ))
        })
        .mut_subcommand("source", |c| {
            c.about(t(
                "List or add package sources such as an internal mirror",
                "Kurum içi ayna gibi paket kaynaklarını listele ya da ekle",
            ))
        })
        .mut_subcommand("icons", |c| {
            c.about(t(
                "Download app icons from their official websites for the GUI, or clear them",
                "Arayüz için uygulama simgelerini resmi sitelerinden indir ya da sil",
            ))
        })
        .mut_subcommand("sign", |c| {
            c.about(t(
                "Create keys, sign files and verify signatures (profiles, tweak packs, policies, plugins)",
                "Anahtar oluştur, dosya imzala ve imza doğrula (profiller, tweak paketleri, politikalar, eklentiler)",
            ))
        })
        .mut_subcommand("policy", |c| {
            c.about(t(
                "Show, install or remove the signed organisation policy of allowed apps and tweaks",
                "İzin verilen uygulama ve tweak'lerin imzalı kurum politikasını göster, kur ya da kaldır",
            ))
        })
        .mut_subcommand("tray", |c| {
            c.about(t(
                "Show a tray (Windows) or menu bar (macOS) icon that checks for updates",
                "Güncellemeleri denetleyen tepsi (Windows) ya da menü çubuğu (macOS) simgesi göster",
            ))
        })
        .mut_subcommand("report", |c| {
            c.about(t(
                "Hardware and disk health report (no computer or user name)",
                "Donanım ve disk sağlığı raporu (bilgisayar ve kullanıcı adı içermez)",
            ))
        })
        .mut_subcommand("du", |c| {
            c.about(t(
                "Show what takes the most space in a folder",
                "Bir klasörde en çok yer kaplayanları göster",
            ))
        })
        .mut_subcommand("repair", |c| {
            c.about(t(
                "Repair system files (Windows) or interrupted package installs (apt)",
                "Sistem dosyalarını (Windows) ya da yarım kalmış paket kurulumlarını (apt) onar",
            ))
        })
        .mut_subcommand("permissions", |c| {
            c.about(t(
                "Apps with camera, microphone or location access (Windows)",
                "Kamera, mikrofon ya da konum izni olan uygulamalar (Windows)",
            ))
        })
        .mut_subcommand("suggest", |c| {
            c.about(t(
                "Suggest privacy-friendly alternatives for installed apps",
                "Kurulu uygulamalar için gizlilik dostu alternatifler öner",
            ))
        })
        .mut_subcommand("audit", |c| {
            c.about(t(
                "Security checks and privacy score (read-only)",
                "Güvenlik denetimi ve gizlilik puanı (yalnızca okur)",
            ))
            .mut_arg("html", |a| {
                a.help(t(
                    "Write a printable HTML report to this file",
                    "Yazdırılabilir HTML raporunu bu dosyaya yaz",
                ))
            })
        })
        .mut_subcommand("startup", |c| {
            c.about(t(
                "List programs that start automatically",
                "Otomatik başlayan programları listele",
            ))
        })
        .mut_subcommand("history", |c| {
            c.about(t("Commands Skadi has run", "Skadi'nin çalıştırdığı komutlar"))
                .mut_arg("limit", |a| {
                    a.help(t("How many entries to show", "Gösterilecek kayıt sayısı"))
                })
        })
        .mut_subcommand("bootstrap", |c| {
            c.about(t(
                "Install a missing package manager",
                "Eksik bir paket yöneticisini kur",
            ))
            .mut_arg("manager", |a| {
                a.help(t("Package manager id", "Paket yöneticisi kimliği"))
            })
            .mut_arg("yes", |a| a.help(yes()))
            .mut_arg("dry_run", |a| a.help(dry_run()))
        })
        .mut_subcommand("tweak", |c| {
            c.about(t("Debloat and privacy tweaks", "Sadeleştirme ve gizlilik ayarları"))
                .mut_subcommand("list", |c| {
                    c.mut_arg("pack", |a| {
                        a.help(t(
                            "Signed tweak pack (.toml) or sandboxed plugin (.wasm)",
                            "İmzalı tweak paketi (.toml) ya da korumalı eklenti (.wasm)",
                        ))
                    })
                    .mut_arg("public_key", |a| {
                        a.help(t(
                            "Public key that signed the pack",
                            "Paketi imzalayan açık anahtar",
                        ))
                    })
                })
                .mut_subcommand("apply", |c| {
                    c.mut_arg("pack", |a| {
                        a.help(t(
                            "Signed tweak pack (.toml) or sandboxed plugin (.wasm)",
                            "İmzalı tweak paketi (.toml) ya da korumalı eklenti (.wasm)",
                        ))
                    })
                    .mut_arg("public_key", |a| {
                        a.help(t(
                            "Public key that signed the pack",
                            "Paketi imzalayan açık anahtar",
                        ))
                    })
                })
                .mut_subcommand("revert", |c| {
                    c.mut_arg("pack", |a| {
                        a.help(t(
                            "Signed tweak pack (.toml) or sandboxed plugin (.wasm)",
                            "İmzalı tweak paketi (.toml) ya da korumalı eklenti (.wasm)",
                        ))
                    })
                    .mut_arg("public_key", |a| {
                        a.help(t(
                            "Public key that signed the pack",
                            "Paketi imzalayan açık anahtar",
                        ))
                    })
                })
                .mut_subcommand("list", |c| {
                    c.about(t("List tweaks and their status", "Tweak'leri ve durumlarını listele"))
                        .mut_arg("category", |a| a.help(category()))
                })
                .mut_subcommand("apply", |c| tweak_args(c, "Apply tweaks", "Tweak uygula"))
                .mut_subcommand("revert", |c| {
                    tweak_args(
                        c,
                        "Undo tweaks applied by Skadi",
                        "Skadi'nin uyguladığı tweak'leri geri al",
                    )
                })
        })
        .mut_subcommand("profile", |c| {
            c.about(t("Presets and profile files", "Hazır setler ve profil dosyaları"))
                .mut_subcommand("list", |c| c.about(t("List built-in presets", "Hazır setleri listele")))
                .mut_subcommand("apply", |c| {
                    c.about(t(
                        "Apply a preset id or a profile file",
                        "Bir hazır seti ya da profil dosyasını uygula",
                    ))
                    .mut_arg("target", |a| {
                        a.help(t(
                            "Preset id or path to a .toml profile",
                            "Hazır set kimliği ya da .toml profil yolu",
                        ))
                    })
                    .mut_arg("dry_run", |a| a.help(dry_run()))
                    .mut_arg("yes", |a| a.help(yes()))
                })
                .mut_subcommand("convert", |c| {
                    c.about(t(
                        "Convert a preset or profile's apps to Ansible, a Brewfile or a winget import file",
                        "Bir hazır setin ya da profilin uygulamalarını Ansible, Brewfile ya da winget içe aktarma dosyasına dönüştür",
                    ))
                })
                .mut_subcommand("export", |c| {
                    c.about(t(
                        "Save installed catalog apps and applied tweaks to a profile",
                        "Kurulu katalog uygulamalarını ve uygulanmış tweak'leri profile kaydet",
                    ))
                    .mut_arg("path", |a| {
                        a.help(t("Profile file to write", "Yazılacak profil dosyası"))
                    })
                })
        })
        .mut_subcommand("completions", |c| {
            c.about(t("Print shell completions", "Kabuk tamamlama betiğini yaz"))
                .mut_arg("shell", |a| a.help(t("Target shell", "Hedef kabuk")))
        })
        .mut_subcommand("man", |c| c.about(t("Print the man page", "Man sayfasını yaz")))
        .mut_subcommand("tui", |c| {
            c.about(t("Open the terminal interface", "Terminal arayüzünü aç"))
        })
        .mut_subcommand("gui", |c| {
            c.about(t("Open the graphical interface", "Grafik arayüzü aç"))
        })
}

/// Komut ağacı ve yönlendirme tablosu büyüktür; hata ayıklama derlemelerinde Windows'un 1 MiB'lık
/// ana iş parçacığı yığınını aşabilir. Ayrıştırma ve komutlar geniş yığınlı bir iş parçacığında
/// çalışır; GUI ise macOS'un istediği gibi ana iş parçacığında kalır.
const WORKER_STACK: usize = 32 * 1024 * 1024;

fn main() -> ExitCode {
    let parsed = std::thread::Builder::new()
        .name("skadi-cli".into())
        .stack_size(WORKER_STACK)
        .spawn(parse_cli)
        .ok()
        .and_then(|handle| handle.join().ok());
    let Some((lang, out, command)) = parsed else {
        return ExitCode::FAILURE;
    };
    if let Command::Gui(args) = &command {
        return finish(lang, run_gui(lang, args));
    }
    std::thread::Builder::new()
        .name("skadi".into())
        .stack_size(WORKER_STACK)
        .spawn(move || dispatch(lang, &out, command))
        .ok()
        .and_then(|handle| handle.join().ok())
        .unwrap_or(ExitCode::FAILURE)
}

fn parse_cli() -> (Lang, Output, Command) {
    let early = early_lang();
    let cli = match Cli::from_arg_matches(&localized_command(early).get_matches()) {
        Ok(cli) => cli,
        Err(error) => error.exit(),
    };
    let lang = cli.lang.unwrap_or(early);
    // Skadi'nin çalıştırdığı her komut denetim günlüğüne yazılır.
    if let Some(dir) = Os::current().ok().and_then(skadi_core::paths::data_dir) {
        skadi_core::exec::set_audit_log(dir.join("audit.log"));
    }
    let out = Output {
        json: cli.json,
        script: cli.script,
    };
    (lang, out, cli.command.unwrap_or_else(default_interface))
}

fn finish(lang: Lang, result: skadi_core::Result<()>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{}: {}", lang.pick("error", "hata"), error.message(lang));
            ExitCode::FAILURE
        }
    }
}

fn dispatch(lang: Lang, out: &Output, command: Command) -> ExitCode {
    let out = out.clone();
    let result = match command {
        Command::Info => commands::info(lang, &out),
        Command::List { category, tag, ids } => commands::list(lang, &out, category.as_deref(), tag.as_deref(), ids),
        Command::Installed => maintain::installed(lang, &out),
        Command::Show { app } => commands::show(lang, &out, &app),
        Command::Install(op) => commands::apply(lang, &out, &op.into(), Op::Install),
        Command::Remove(op) => {
            let mode = if op.purge { Op::Purge } else { Op::Remove };
            commands::apply(lang, &out, &op.into(), mode)
        }
        Command::Update {
            rollback: Some(package),
            manager,
            dry_run,
            yes,
            ..
        } => maintain::rollback(lang, &package, manager.as_deref(), dry_run, yes),
        Command::Update {
            check,
            firmware,
            notify,
            rollback: None,
            manager,
            dry_run,
            yes,
        } => maintain::update(
            lang,
            &out,
            &maintain::UpdateOptions {
                check,
                firmware,
                manager,
                dry_run,
                yes,
                notify,
            },
        ),
        Command::Pin(args) => maintain::pin(lang, &args.apps, true, args.dry_run),
        Command::Schedule { action } => maintain::schedule(lang, &action),
        Command::History { limit, timeline } => {
            if timeline {
                maintain::timeline(lang, limit)
            } else {
                maintain::history(lang, &out, limit)
            }
        }
        Command::Startup { action, name, dry_run } => match (action, name) {
            (Some(action), Some(name)) => maintain::startup_toggle(lang, &name, action == "enable", dry_run),
            _ => maintain::startup(lang, &out),
        },
        Command::Audit { html } => maintain::audit(lang, &out, html.as_deref()),
        Command::CleanMetadata { files, in_place } => maintain::clean_metadata(lang, &files, in_place),
        Command::Suggest => maintain::suggest(lang, &out),
        Command::Permissions => maintain::permissions(lang, &out),
        Command::Clean { dry_run, yes } => care::clean(lang, &out, dry_run, yes),
        Command::Power { target } => care::power(lang, &out, target.as_deref()),
        Command::Report => care::report(lang, &out),
        Command::Tray => tray::tray(lang),
        Command::Sign { command } => match command {
            SignCommand::Keygen { name } => trust::keygen(lang, &name),
            SignCommand::File { file, key } => trust::sign_file(lang, &file, &key),
            SignCommand::Verify { file, public_key } => trust::verify_file(lang, &file, &public_key),
        },
        Command::Policy {
            action,
            file,
            public_key,
            yes,
        } => trust::policy(lang, &action, file.as_deref(), public_key.as_deref(), yes),
        Command::Icons { action, yes } => extras::icons(lang, &action, yes),
        Command::Dotfiles { action, dir, force } => extras::dotfiles(lang, &action, &dir, force),
        Command::Bookmarks { dir } => extras::bookmarks(lang, &dir),
        Command::WipeFree { path, yes } => extras::wipe_free(lang, path.as_deref(), yes),
        Command::Appimage {
            action,
            name,
            url,
            sha256,
        } => {
            if action == "install" {
                match (url, sha256) {
                    (Some(url), Some(sha)) => extras::appimage_install(lang, &name, &url, &sha),
                    _ => Err(skadi_core::Error::InvalidOption("NAME URL --sha256 HASH")),
                }
            } else {
                extras::appimage_remove(lang, &name)
            }
        }
        Command::Bundle { action, dir, apps, yes } => {
            if action == "create" {
                extras::bundle_create(lang, &dir, &apps)
            } else {
                extras::bundle_install(lang, &dir, yes)
            }
        }
        Command::Remote {
            target,
            hosts,
            dry_run,
            yes,
            ..
        } => extras::remote_apply(lang, &target, &hosts, dry_run, yes),
        Command::Source {
            action,
            manager,
            name,
            url,
            yes,
        } => extras::source(lang, &action, &manager, name.as_deref(), url.as_deref(), yes),
        Command::Certs => controls::certs(lang, &out),
        Command::Wifi {
            action,
            dir,
            with_passwords,
            yes,
        } => controls::wifi(lang, &action, &dir, with_passwords, yes),
        Command::SnapToFlatpak {
            app,
            remove_snapd,
            dry_run,
            yes,
        } => controls::snap_to_flatpak(lang, &app, remove_snapd, dry_run, yes),
        Command::Hosts { action, yes } => controls::hosts(lang, &action, yes),
        Command::Firewall { action, dry_run, yes } => controls::firewall(lang, &action, dry_run, yes),
        Command::Pagefile { auto, dry_run, yes } => controls::pagefile(lang, auto, dry_run, yes),
        Command::StartPins { action, dry_run, yes } => controls::start_pins(lang, &action, dry_run, yes),
        Command::Internal {
            command: InternalCommand::HostsSet { mode, list },
        } => controls::internal_hosts_set(&mode, &list),
        Command::DefaultBrowser { app } => desktop::default_browser(lang, app.as_deref()),
        Command::GnomeExtensions { action, uuid } => desktop::gnome_extensions(action.as_deref(), uuid.as_deref()),
        Command::Spotlight { state, yes } => desktop::spotlight(lang, state.as_deref(), yes),
        Command::MacosProfile { path } => desktop::macos_profile(lang, &path),
        Command::Share { apps } => share::share(lang, &apps),
        Command::Scan { path, rootkit } => care::scan(lang, path.as_deref(), rootkit),
        Command::Catalog {
            command: CatalogCommand::Add,
        } => contribute::add(lang),
        Command::BugReport => care::bug_report(lang),
        Command::Setup { dry_run } => profiles::setup(lang, &out, dry_run),
        Command::Du { path, top } => care::du(lang, &out, &path.unwrap_or_else(|| PathBuf::from(".")), top),
        Command::Repair { dry_run, yes } => care::repair(lang, dry_run, yes),
        Command::Dns { command } => match command {
            DnsCommand::Test => net::dns_test(lang, &out),
            DnsCommand::Status => skadi_core::system::System::detect().and_then(|s| net::dns_status(lang, &s)),
            DnsCommand::Set { provider, dry_run, yes } => skadi_core::system::System::detect()
                .and_then(|s| net::dns_apply(lang, &s, Some(&provider), dry_run, yes)),
            DnsCommand::Reset { dry_run, yes } => {
                skadi_core::system::System::detect().and_then(|s| net::dns_apply(lang, &s, None, dry_run, yes))
            }
        },
        Command::Network { command } => match command {
            NetworkCommand::Ports => net::network_sockets(lang, &out, true),
            NetworkCommand::Connections => net::network_sockets(lang, &out, false),
            NetworkCommand::Reset { dry_run, yes } => net::network_reset(lang, dry_run, yes),
            NetworkCommand::Diagnose => net::network_diagnose(lang),
            NetworkCommand::Proxy { action: None, .. } => net::proxy(lang, &out),
            NetworkCommand::Proxy {
                action: Some(action),
                address,
                dry_run,
                yes,
            } => controls::proxy_set(
                lang,
                if action == "set" { address.as_deref() } else { None },
                dry_run,
                yes,
            ),
        },
        Command::Unpin(args) => maintain::pin(lang, &args.apps, false, args.dry_run),
        Command::Bootstrap { manager, yes, dry_run } => commands::bootstrap(lang, &manager, yes, dry_run),
        Command::Tweak { command } => match command {
            TweakCommand::List {
                category,
                pack,
                public_key,
            } => tweaks::list(lang, &out, category.as_deref(), pack.zip(public_key).as_ref()),
            TweakCommand::Apply(args) => tweaks::apply(lang, &out, &args.into()),
            TweakCommand::Revert(args) => tweaks::revert(lang, &args.into()),
            TweakCommand::Backup { file } => extras::registry_backup(lang, &file),
            TweakCommand::DefaultUser { args, revert } => tweaks::default_user(lang, &args.into(), revert),
        },
        Command::Profile { command } => match command {
            ProfileCommand::List => profiles::list(lang, &out),
            ProfileCommand::Apply {
                target,
                public_key,
                dry_run,
                yes,
            } => profiles::apply(lang, &out, &target, public_key.as_deref(), dry_run, yes),
            ProfileCommand::Export { path } => profiles::export(lang, &path),
            ProfileCommand::Git { dir } => extras::profile_git(lang, &dir),
            ProfileCommand::Convert { format, target, output } => {
                profiles::convert(lang, &format, &target, output.as_deref())
            }
        },
        Command::Completions { shell } => {
            clap_complete::generate(shell, &mut localized_command(lang), "skadi", &mut std::io::stdout());
            Ok(())
        }
        Command::Internal {
            command: InternalCommand::ScheduleUnits { action },
        } => maintain::schedule_units(&action),
        Command::Internal {
            command:
                InternalCommand::ConfigSet {
                    path,
                    section,
                    key,
                    mode,
                    text,
                },
        } => internal_config_set(&path, &section, &key, &mode, text),
        Command::Internal {
            command:
                InternalCommand::FileSet {
                    path,
                    reload,
                    guard,
                    mode,
                    content,
                },
        } => {
            use skadi_core::tweak::{Guard, Reload, config::write_managed};
            match (Reload::parse(&reload), Guard::parse(&guard)) {
                (Some(reload), Some(guard)) => {
                    write_managed(&path, (mode == "write").then_some(content.as_str()), reload, guard)
                }
                _ => Err(skadi_core::Error::InvalidOption("file-set")),
            }
        }
        Command::Tui(args) => run_tui(lang, &args),
        Command::Gui(args) => run_gui(lang, &args),
        Command::Man => clap_mangen::Man::new(localized_command(lang))
            .render(&mut std::io::stdout())
            .map_err(|e| skadi_core::Error::State(e.to_string())),
    };
    finish(lang, result)
}

fn internal_config_set(path: &str, section: &str, key: &str, mode: &str, text: String) -> skadi_core::Result<()> {
    use skadi_core::tweak::config::{ConfigEdit, edit_file};
    let edit = match mode {
        "value" => ConfigEdit::Value(text),
        "line" => ConfigEdit::Line(text),
        "remove" => ConfigEdit::Remove,
        _ => return Err(skadi_core::Error::InvalidOption("config-set mode")),
    };
    let section = (!section.is_empty()).then_some(section);
    edit_file(path, section, key, &edit)
}

/// Komut verilmezse: ekran varsa GUI, yoksa (ör. SSH oturumu) TUI.
fn default_interface() -> Command {
    let has_display = match Os::current() {
        Ok(Os::Linux) => std::env::var_os("DISPLAY").is_some() || std::env::var_os("WAYLAND_DISPLAY").is_some(),
        Ok(_) => std::env::var_os("SSH_CONNECTION").is_none(),
        Err(_) => false,
    };
    if has_display && cfg!(feature = "gui") {
        Command::Gui(UiArgs::default())
    } else {
        Command::Tui(UiArgs::default())
    }
}

#[cfg(feature = "tui")]
fn run_tui(lang: Lang, args: &UiArgs) -> skadi_core::Result<()> {
    match &args.screenshot {
        Some(path) => ui::tui::screenshot(lang, path, &args.tab),
        None => ui::tui::run(lang),
    }
}

#[cfg(not(feature = "tui"))]
fn run_tui(_lang: Lang, _args: &UiArgs) -> skadi_core::Result<()> {
    Err(skadi_core::Error::Unsupported("tui"))
}

#[cfg(feature = "gui")]
fn run_gui(lang: Lang, args: &UiArgs) -> skadi_core::Result<()> {
    ui::gui::run(lang, args.screenshot.as_deref(), &args.tab)
}

#[cfg(not(feature = "gui"))]
fn run_gui(_lang: Lang, _args: &UiArgs) -> skadi_core::Result<()> {
    Err(skadi_core::Error::Unsupported("gui"))
}

impl From<OpArgs> for commands::OpOptions {
    fn from(op: OpArgs) -> Self {
        Self {
            apps: op.apps,
            resume: op.resume,
            manager: op.manager,
            dry_run: op.dry_run,
            yes: op.yes,
        }
    }
}

impl From<TweakArgs> for tweaks::Selection {
    fn from(args: TweakArgs) -> Self {
        Self {
            pack: args.pack.zip(args.public_key),
            ids: args.tweaks,
            safe: args.safe,
            all: args.all,
            last: args.last,
            snapshot: args.snapshot,
            dry_run: args.dry_run,
            yes: args.yes,
        }
    }
}
