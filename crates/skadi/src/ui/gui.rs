//! Pencereli arayüz (egui).
//!
//! Arayüz yetkisiz çalışır ve işleri `skadi` CLI'sine yalnızca kimlik vererek yaptırır
//! (bkz. `ui::Model::jobs`). Çıktı canlı günlükte akar; iptal, o anki öğe bittikten sonra
//! sıradakilerin başlatılmamasıdır, hiçbir kurulum yarıda kesilmez.

use std::{
    collections::{BTreeSet, HashMap, HashSet, VecDeque},
    path::Path,
    sync::mpsc::Receiver,
    time::Duration,
};

use eframe::egui::{self, Color32, RichText};
use skadi_core::{
    Error, Lang, Result,
    catalog::{App, Category},
    platform::Os,
    profile::Profile,
    tweak::{Risk, TweakCategory, TweakStatus},
};

use super::{Job, Loaded, Model, RunEvent, Task, load_in_background, spawn};

const ACCENT: Color32 = Color32::from_rgb(91, 164, 207);
const GOOD: Color32 = Color32::from_rgb(94, 186, 125);
const WARN: Color32 = Color32::from_rgb(214, 160, 60);
const BAD: Color32 = Color32::from_rgb(214, 90, 100);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Apps,
    Tweaks,
    Profiles,
    Updates,
}

/// Önizleme ya da denetim penceresi: bir CLI çıktısını gösterir.
struct OutputWindow {
    title: String,
    lines: Vec<String>,
    rx: Option<Receiver<RunEvent>>,
}

struct SkadiApp {
    model: Model,
    loading: Option<Receiver<Loaded>>,
    installed: Option<HashSet<String>>,
    statuses: Option<HashMap<String, Option<TweakStatus>>>,
    drifted: HashSet<String>,
    /// Paylaşım penceresi: Windows ve Linux/macOS komutları.
    share: Option<(String, String)>,
    /// README görüntüsü: hedef dosya ve veriler yüklendikten sonra beklenen kare sayısı.
    screenshot: Option<(std::path::PathBuf, u32)>,

    tab: Tab,
    search: String,
    category: Option<Category>,
    foss_only: bool,
    installed_only: bool,
    apps: BTreeSet<String>,
    tweaks: BTreeSet<String>,

    confirm: Option<Task>,
    queue: VecDeque<Job>,
    total: usize,
    done: usize,
    running: Option<(String, Receiver<RunEvent>)>,
    cancel: bool,
    failures: Vec<String>,
    log: Vec<String>,
    show_log: bool,
    output: Option<OutputWindow>,
    notice: Option<(String, Color32)>,
    profile_path: String,
    snapshot: bool,
}

impl SkadiApp {
    fn new(model: Model) -> Self {
        let loading = Some(load_in_background(&model));
        let profile_path = skadi_core::paths::data_dir(model.system.os)
            .map(|dir| dir.join("profile.toml").display().to_string())
            .unwrap_or_else(|| "profile.toml".to_owned());
        Self {
            model,
            loading,
            installed: None,
            statuses: None,
            drifted: HashSet::new(),
            share: None,
            screenshot: None,
            tab: Tab::Apps,
            search: String::new(),
            category: None,
            foss_only: false,
            installed_only: false,
            apps: BTreeSet::new(),
            tweaks: BTreeSet::new(),
            confirm: None,
            queue: VecDeque::new(),
            total: 0,
            done: 0,
            running: None,
            cancel: false,
            failures: Vec::new(),
            log: Vec::new(),
            show_log: false,
            output: None,
            notice: None,
            profile_path,
            snapshot: true,
        }
    }

    fn t(&self, en: &'static str, tr: &'static str) -> &'static str {
        self.model.lang.pick(en, tr)
    }

    fn busy(&self) -> bool {
        self.running.is_some() || !self.queue.is_empty()
    }

    fn matches(&self, texts: &[&str]) -> bool {
        let needle = self.search.trim().to_lowercase();
        needle.is_empty() || texts.iter().any(|t| t.to_lowercase().contains(&needle))
    }

    fn visible_apps(&self) -> Vec<&App> {
        let lang = self.model.lang;
        self.model
            .catalog
            .apps()
            .iter()
            .filter(|app| self.model.is_for_this_os(app))
            .filter(|app| self.category.is_none_or(|c| c == app.category))
            .filter(|app| !self.foss_only || app.foss == Some(true))
            .filter(|app| !self.installed_only || self.installed.as_ref().is_some_and(|s| s.contains(&app.id)))
            .filter(|app| self.matches(&[&app.id, &app.name, app.description.get(lang)]))
            .collect()
    }

    /// Arka plan yüklemesini ve çalışan işin çıktısını işler.
    fn poll(&mut self) {
        if let Some(rx) = &self.loading {
            while let Ok(loaded) = rx.try_recv() {
                match loaded {
                    Loaded::Installed(set) => self.installed = Some(set),
                    Loaded::Statuses(map) => self.statuses = Some(map),
                    Loaded::Drifted(set) => self.drifted = set,
                }
            }
        }
        if let Some(window) = &mut self.output
            && let Some(rx) = &window.rx
        {
            let mut finished = false;
            while let Ok(event) = rx.try_recv() {
                match event {
                    RunEvent::Line(line) => window.lines.push(line),
                    RunEvent::Finished(_) => finished = true,
                }
            }
            if finished {
                window.rx = None;
            }
        }
        let mut finished = None;
        if let Some((label, rx)) = &self.running {
            while let Ok(event) = rx.try_recv() {
                match event {
                    RunEvent::Line(line) => self.log.push(line),
                    RunEvent::Finished(result) => finished = Some((label.clone(), result)),
                }
            }
        }
        if let Some((label, result)) = finished {
            self.running = None;
            self.done += 1;
            if let Err(error) = result {
                self.log.push(format!("[{label}] {error}"));
                self.failures.push(label);
            }
            self.start_next();
        }
    }

    fn start_next(&mut self) {
        if self.cancel {
            self.queue.clear();
        }
        match self.queue.pop_front() {
            Some(job) => {
                self.log.push(format!("> skadi {}", job.args.join(" ")));
                match spawn(&job) {
                    Ok(rx) => self.running = Some((job.label, rx)),
                    Err(error) => {
                        self.log.push(error);
                        self.failures.push(job.label);
                        self.done += 1;
                        self.start_next();
                    }
                }
            }
            None if self.total > 0 => self.finish(),
            None => {}
        }
    }

    fn finish(&mut self) {
        let message = if self.cancel {
            (
                self.t("Cancelled after the current item.", "O anki öğeden sonra iptal edildi.")
                    .to_owned(),
                WARN,
            )
        } else if self.failures.is_empty() {
            (self.t("Done.", "Tamamlandı.").to_owned(), GOOD)
        } else {
            (
                format!("{}: {}", self.t("Failed", "Başarısız"), self.failures.join(", ")),
                BAD,
            )
        };
        self.notice = Some(message);
        self.total = 0;
        self.cancel = false;
        // Değişikliklerden sonra kurulu listesi ve tweak durumları yeniden okunur.
        self.installed = None;
        self.statuses = None;
        self.loading = Some(load_in_background(&self.model));
    }

    fn start(&mut self, task: &Task) {
        let jobs = self.model.jobs(task);
        self.total = jobs.len();
        self.done = 0;
        self.failures.clear();
        self.cancel = false;
        self.notice = None;
        self.queue = jobs.into();
        self.show_log = true;
        self.start_next();
    }

    fn open_output(&mut self, title: String, args: Vec<String>) {
        let mut full = vec!["--lang".to_owned(), self.model.lang.as_str().to_owned()];
        full.extend(args);
        let job = Job {
            args: full,
            elevated: false,
            label: title.clone(),
        };
        let (lines, rx) = match spawn(&job) {
            Ok(rx) => (Vec::new(), Some(rx)),
            Err(error) => (vec![error], None),
        };
        self.output = Some(OutputWindow { title, lines, rx });
    }

    fn top_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let logo = if ui.visuals().dark_mode {
                    egui::include_image!("../../../../assets/mark-white-256.png")
                } else {
                    egui::include_image!("../../../../assets/mark-black-256.png")
                };
                ui.add(egui::Image::new(logo).fit_to_exact_size(egui::vec2(28.0, 28.0)));
                ui.label(RichText::new("Skadi").size(20.0).strong());
                ui.add_space(16.0);
                for (tab, en, tr) in [
                    (Tab::Apps, "Apps", "Uygulamalar"),
                    (Tab::Tweaks, "Tweaks", "Ayarlar"),
                    (Tab::Profiles, "Profiles", "Profiller"),
                    (Tab::Updates, "Updates", "Güncellemeler"),
                ] {
                    if ui
                        .selectable_label(self.tab == tab, RichText::new(self.t(en, tr)).size(15.0))
                        .clicked()
                    {
                        self.tab = tab;
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let system = &self.model.system;
                    let admin = if system.privileges.is_elevated {
                        self.t("administrator", "yönetici")
                    } else {
                        self.t("user", "kullanıcı")
                    };
                    ui.label(
                        RichText::new(format!("{} {} · {admin}", system.os, system.arch))
                            .small()
                            .weak(),
                    );
                    let hint = self.t("Search (Ctrl+F)", "Ara (Ctrl+F)");
                    let search_id = egui::Id::new("search");
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut self.search)
                            .id(search_id)
                            .hint_text(hint)
                            .desired_width(220.0),
                    );
                    if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::F)) {
                        response.request_focus();
                    }
                    if response.has_focus() && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                        self.search.clear();
                    }
                });
            });
            ui.add_space(6.0);
            if self.model.system.os == Os::Windows && !self.model.system.privileges.is_elevated {
                ui.horizontal(|ui| {
                    ui.colored_label(
                        WARN,
                        self.t(
                            "System-wide changes need administrator rights.",
                            "Sistem genelindeki değişiklikler yönetici yetkisi gerektirir.",
                        ),
                    );
                    if ui
                        .button(self.t("Restart as administrator", "Yönetici olarak yeniden başlat"))
                        .clicked()
                    {
                        #[cfg(windows)]
                        match super::relaunch_elevated(&["gui"]) {
                            Ok(()) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                            Err(error) => self.notice = Some((error, BAD)),
                        }
                    }
                });
                ui.add_space(4.0);
            }
        });
    }

    fn bottom_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("bottom").show(ctx, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let busy = self.busy();
                match self.tab {
                    Tab::Apps | Tab::Profiles => {
                        let ids: Vec<String> = self.apps.iter().cloned().collect();
                        ui.label(format!("{} {}", ids.len(), self.t("apps selected", "uygulama seçili")));
                        let enabled = !ids.is_empty() && !busy;
                        if ui
                            .add_enabled(enabled, egui::Button::new(self.t("Preview", "Önizle")))
                            .clicked()
                        {
                            let mut args = vec!["install".to_owned()];
                            args.extend(ids.clone());
                            args.push("--dry-run".to_owned());
                            self.open_output(self.t("Preview", "Önizleme").to_owned(), args);
                        }
                        if ui
                            .add_enabled(
                                enabled,
                                egui::Button::new(RichText::new(self.t("Install", "Kur")).strong()).fill(ACCENT),
                            )
                            .on_hover_text("Ctrl+Enter")
                            .clicked()
                            || (enabled && ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Enter)))
                        {
                            self.confirm = Some(Task::Install(ids.clone()));
                        }
                        if ui
                            .add_enabled(enabled, egui::Button::new(self.t("Remove", "Kaldır")))
                            .clicked()
                        {
                            self.confirm = Some(Task::Remove(ids.clone()));
                        }
                        if ui
                            .add_enabled(
                                !ids.is_empty(),
                                egui::Button::new(self.t("Copy command", "Komutu kopyala")),
                            )
                            .on_hover_text(self.t(
                                "Copy the selection as a skadi command",
                                "Seçimi bir skadi komutu olarak kopyala",
                            ))
                            .clicked()
                        {
                            ctx.copy_text(format!("skadi install {}", ids.join(" ")));
                            self.notice = Some((self.t("Copied.", "Kopyalandı.").to_owned(), GOOD));
                        }
                        if ui
                            .add_enabled(!ids.is_empty(), egui::Button::new(self.t("Share", "Paylaş")))
                            .on_hover_text(self.t(
                                "One-line install commands and a QR code for another computer",
                                "Başka bir bilgisayar için tek satırlık kurulum komutları ve QR kod",
                            ))
                            .clicked()
                        {
                            self.share = Some(crate::share::launch_lines(&ids));
                        }
                        if ui
                            .add_enabled(!ids.is_empty() && !busy, egui::Button::new(self.t("Clear", "Temizle")))
                            .clicked()
                        {
                            self.apps.clear();
                        }
                    }
                    Tab::Tweaks => {
                        let ids: Vec<String> = self.tweaks.iter().cloned().collect();
                        ui.label(format!("{} {}", ids.len(), self.t("tweaks selected", "ayar seçili")));
                        let enabled = !ids.is_empty() && !busy;
                        if ui
                            .add_enabled(enabled, egui::Button::new(self.t("Preview", "Önizle")))
                            .clicked()
                        {
                            let mut args = vec!["tweak".to_owned(), "apply".to_owned()];
                            args.extend(ids.clone());
                            args.push("--dry-run".to_owned());
                            self.open_output(self.t("Preview", "Önizleme").to_owned(), args);
                        }
                        if ui
                            .add_enabled(
                                enabled,
                                egui::Button::new(RichText::new(self.t("Apply", "Uygula")).strong()).fill(ACCENT),
                            )
                            .on_hover_text("Ctrl+Enter")
                            .clicked()
                            || (enabled && ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Enter)))
                        {
                            self.confirm = Some(Task::ApplyTweaks(ids.clone(), self.snapshot));
                        }
                        let snapshot_label = self.t("Restore point first", "Önce geri yükleme noktası");
                        ui.checkbox(&mut self.snapshot, snapshot_label);
                        if ui
                            .add_enabled(enabled, egui::Button::new(self.t("Undo", "Geri al")))
                            .clicked()
                        {
                            self.confirm = Some(Task::RevertTweaks(ids.clone()));
                        }
                        if ui
                            .add_enabled(!ids.is_empty() && !busy, egui::Button::new(self.t("Clear", "Temizle")))
                            .clicked()
                        {
                            self.tweaks.clear();
                        }
                    }
                    Tab::Updates => {
                        if ui
                            .add_enabled(
                                !busy,
                                egui::Button::new(self.t("Check for updates", "Güncellemeleri denetle")),
                            )
                            .clicked()
                        {
                            self.open_output(
                                self.t("Available updates", "Mevcut güncellemeler").to_owned(),
                                vec!["update".to_owned(), "--check".to_owned()],
                            );
                        }
                        if ui
                            .add_enabled(
                                !busy,
                                egui::Button::new(
                                    RichText::new(self.t("Update everything", "Her şeyi güncelle")).strong(),
                                )
                                .fill(ACCENT),
                            )
                            .clicked()
                        {
                            self.confirm = Some(Task::Update);
                        }
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.selectable_label(self.show_log, self.t("Log", "Günlük")).clicked() {
                        self.show_log = !self.show_log;
                    }
                    if self.busy() {
                        if ui
                            .add_enabled(!self.cancel, egui::Button::new(self.t("Cancel", "İptal")))
                            .on_hover_text(self.t(
                                "Stops after the current item; nothing is interrupted halfway.",
                                "O anki öğe bitince durur; hiçbir şey yarıda kesilmez.",
                            ))
                            .clicked()
                        {
                            self.cancel = true;
                        }
                        let progress = self.done as f32 / self.total.max(1) as f32;
                        let current = self
                            .running
                            .as_ref()
                            .map(|(label, _)| label.clone())
                            .unwrap_or_default();
                        ui.add(
                            egui::ProgressBar::new(progress)
                                .desired_width(220.0)
                                .text(format!("{}/{} {current}", self.done, self.total)),
                        );
                    } else if let Some((text, color)) = &self.notice {
                        ui.colored_label(*color, text);
                    }
                });
            });
            ui.add_space(6.0);
            if self.show_log {
                egui::ScrollArea::vertical()
                    .max_height(160.0)
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        for line in &self.log {
                            ui.label(RichText::new(line).monospace().small());
                        }
                    });
                ui.add_space(6.0);
            }
        });
    }

    fn side_panel(&mut self, ctx: &egui::Context) {
        if self.tab != Tab::Apps {
            return;
        }
        egui::SidePanel::left("categories")
            .resizable(false)
            .exact_width(190.0)
            .show(ctx, |ui| {
                ui.add_space(8.0);
                let lang = self.model.lang;
                let count = |category: Option<Category>| {
                    self.model
                        .catalog
                        .apps()
                        .iter()
                        .filter(|a| self.model.is_for_this_os(a) && category.is_none_or(|c| c == a.category))
                        .count()
                };
                let all = format!("{} ({})", self.t("All apps", "Tüm uygulamalar"), count(None));
                if ui.selectable_label(self.category.is_none(), all).clicked() {
                    self.category = None;
                }
                for category in Category::ALL {
                    let n = count(Some(category));
                    if n > 0
                        && ui
                            .selectable_label(
                                self.category == Some(category),
                                format!("{} ({n})", category.label(lang)),
                            )
                            .clicked()
                    {
                        self.category = Some(category);
                    }
                }
                ui.separator();
                let foss_label = self.t("Open source only", "Yalnızca açık kaynak");
                let installed_label = self.t("Installed only", "Yalnızca kurulu");
                ui.checkbox(&mut self.foss_only, foss_label);
                ui.add_enabled(
                    self.installed.is_some(),
                    egui::Checkbox::new(&mut self.installed_only, installed_label),
                );
                if self.installed.is_none() {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(
                            RichText::new(self.t("Scanning installed apps", "Kurulu uygulamalar taranıyor"))
                                .small()
                                .weak(),
                        );
                    });
                }
            });
    }

    fn apps_view(&mut self, ui: &mut egui::Ui) {
        let lang = self.model.lang;
        let apps: Vec<(String, String, String, bool, bool, Category)> = self
            .visible_apps()
            .into_iter()
            .map(|a| {
                let installed = self.installed.as_ref().is_some_and(|s| s.contains(&a.id));
                (
                    a.id.clone(),
                    a.name.clone(),
                    a.description.get(lang).to_owned(),
                    a.foss == Some(true),
                    installed,
                    a.category,
                )
            })
            .collect();
        if apps.is_empty() {
            ui.label(self.t("No apps match.", "Eşleşen uygulama yok."));
            return;
        }
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            for category in Category::ALL {
                let group: Vec<_> = apps.iter().filter(|a| a.5 == category).collect();
                if group.is_empty() {
                    continue;
                }
                ui.add_space(8.0);
                ui.label(RichText::new(category.label(lang)).size(16.0).strong().color(ACCENT));
                ui.add_space(4.0);
                let width = 250.0;
                ui.horizontal_wrapped(|ui| {
                    for (id, name, description, foss, installed, _) in group {
                        let mut checked = self.apps.contains(id);
                        let mut label = RichText::new(name.as_str());
                        if *installed {
                            label = label.color(GOOD);
                        }
                        ui.allocate_ui(egui::vec2(width, 24.0), |ui| {
                            ui.horizontal(|ui| {
                                // Simgeler yalnızca `skadi icons fetch` ile indirildiyse yerel önbellekten gelir.
                                if let Some(path) = self.model.icons.get(id) {
                                    ui.add(
                                        egui::Image::new(format!("file://{}", path.display()))
                                            .fit_to_exact_size(egui::vec2(16.0, 16.0)),
                                    );
                                }
                                let response = ui.checkbox(&mut checked, label);
                                if *foss {
                                    ui.label(RichText::new("FOSS").small().color(ACCENT));
                                }
                                if *installed {
                                    ui.label(RichText::new(self.t("installed", "kurulu")).small().color(GOOD));
                                }
                                response.on_hover_text(description.as_str());
                            });
                        });
                        if checked {
                            self.apps.insert(id.clone());
                        } else {
                            self.apps.remove(id);
                        }
                    }
                });
            }
        });
    }

    fn tweaks_view(&mut self, ui: &mut egui::Ui) {
        let lang = self.model.lang;
        let tweaks: Vec<_> = self
            .model
            .system_tweaks()
            .into_iter()
            .filter(|t| self.matches(&[&t.id, t.name.get(lang), t.description.get(lang)]))
            .map(|t| {
                (
                    t.id.clone(),
                    t.name.get(lang).to_owned(),
                    t.description.get(lang).to_owned(),
                    t.risk,
                    t.category,
                    t.is_reversible(),
                )
            })
            .collect();
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            for category in TweakCategory::ALL {
                let group: Vec<_> = tweaks.iter().filter(|t| t.4 == category).collect();
                if group.is_empty() {
                    continue;
                }
                ui.add_space(8.0);
                ui.label(RichText::new(category.label(lang)).size(16.0).strong().color(ACCENT));
                ui.add_space(2.0);
                for (id, name, description, risk, _, reversible) in group {
                    let mut checked = self.tweaks.contains(id);
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut checked, RichText::new(name.as_str()).strong())
                            .on_hover_text(description.as_str());
                        let risk_color = match risk {
                            Risk::Safe => GOOD,
                            Risk::Moderate => WARN,
                            Risk::Advanced => BAD,
                        };
                        ui.label(RichText::new(risk.label(lang)).small().color(risk_color));
                        let status = self.statuses.as_ref().map(|m| m.get(id).copied().flatten());
                        let (text, color) = match status {
                            None => (self.t("checking", "denetleniyor"), Color32::GRAY),
                            Some(Some(TweakStatus::Applied)) => (self.t("applied", "uygulandı"), GOOD),
                            Some(Some(TweakStatus::Partial)) => (self.t("partial", "kısmen"), WARN),
                            Some(Some(TweakStatus::NotApplied)) => {
                                (self.t("not applied", "uygulanmadı"), Color32::GRAY)
                            }
                            Some(Some(TweakStatus::Unavailable)) => (self.t("n/a", "yok"), Color32::GRAY),
                            Some(None) => ("?", Color32::GRAY),
                        };
                        ui.label(RichText::new(text).small().color(color));
                        if self.drifted.contains(id) {
                            ui.label(RichText::new(self.t("drifted", "sürüklendi")).small().color(WARN))
                                .on_hover_text(self.t(
                                    "Skadi applied this, but something (often an update) changed it back. Apply it again.",
                                    "Skadi bunu uyguladı ama bir şey (çoğu zaman bir güncelleme) geri aldı. Yeniden uygulayın.",
                                ));
                        }
                        if !reversible {
                            ui.label(
                                RichText::new(self.t("cannot be undone", "geri alınamaz"))
                                    .small()
                                    .color(BAD),
                            );
                        }
                    });
                    ui.label(RichText::new(description.as_str()).small().weak());
                    ui.add_space(4.0);
                    if checked {
                        self.tweaks.insert(id.clone());
                    } else {
                        self.tweaks.remove(id);
                    }
                }
            }
        });
    }

    fn profiles_view(&mut self, ui: &mut egui::Ui) {
        let lang = self.model.lang;
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            ui.add_space(8.0);
            ui.label(
                RichText::new(self.t("Presets", "Hazır setler"))
                    .size(16.0)
                    .strong()
                    .color(ACCENT),
            );
            let presets: Vec<_> = self
                .model
                .presets
                .iter()
                .map(|p| {
                    (
                        p.name.get(lang).to_owned(),
                        p.description.get(lang).to_owned(),
                        p.profile(),
                    )
                })
                .collect();
            for (name, description, profile) in presets {
                egui::Frame::group(ui.style()).inner_margin(10.0).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&name).strong());
                        ui.label(
                            RichText::new(format!(
                                "{} {} · {} {}",
                                profile.apps.len(),
                                self.t("apps", "uygulama"),
                                profile.tweaks.len(),
                                self.t("tweaks", "ayar")
                            ))
                            .small()
                            .weak(),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button(self.t("Select", "Seç")).clicked() {
                                self.select_profile(&profile);
                            }
                        });
                    });
                    ui.label(RichText::new(description).small());
                });
            }
            ui.add_space(12.0);
            ui.label(
                RichText::new(self.t("Profile file", "Profil dosyası"))
                    .size(16.0)
                    .strong()
                    .color(ACCENT),
            );
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.profile_path).desired_width(420.0));
                if ui.button(self.t("Import", "İçe aktar")).clicked() {
                    match Profile::load(Path::new(&self.profile_path), &self.model.catalog, &self.model.tweaks) {
                        Ok(profile) => self.select_profile(&profile),
                        Err(error) => self.notice = Some((error.message(lang), BAD)),
                    }
                }
                if ui
                    .button(self.t("Export installed", "Kuruluları dışa aktar"))
                    .on_hover_text(self.t(
                        "Installed catalog apps and applied tweaks",
                        "Kurulu katalog uygulamaları ve uygulanmış ayarlar",
                    ))
                    .clicked()
                {
                    let path = self.profile_path.clone();
                    self.open_output(
                        self.t("Export", "Dışa aktar").to_owned(),
                        vec!["profile".into(), "export".into(), path],
                    );
                }
            });
        });
    }

    /// Profildeki bu sisteme uygun uygulama ve ayarları seçer; kullanıcı sonra Kur/Uygula der.
    fn select_profile(&mut self, profile: &Profile) {
        let tweaks: HashSet<&str> = self.model.system_tweaks().iter().map(|t| t.id.as_str()).collect();
        for id in &profile.apps {
            if self.model.catalog.get(id).is_some_and(|a| self.model.is_for_this_os(a)) {
                self.apps.insert(id.clone());
            }
        }
        for id in &profile.tweaks {
            if tweaks.contains(id.as_str()) {
                self.tweaks.insert(id.clone());
            }
        }
        self.notice = Some((
            format!(
                "{} {} · {} {}",
                self.apps.len(),
                self.t("apps selected", "uygulama seçili"),
                self.tweaks.len(),
                self.t("tweaks selected", "ayar seçili")
            ),
            GOOD,
        ));
    }

    fn updates_view(&mut self, ui: &mut egui::Ui) {
        ui.add_space(8.0);
        ui.label(self.t(
            "Updates run each package manager's own update command. Pinned apps are skipped by the manager.",
            "Güncellemeler her paket yöneticisinin kendi komutuyla yapılır. Sabitlenen uygulamaları yönetici atlar.",
        ));
        ui.add_space(8.0);
        egui::Grid::new("managers")
            .striped(true)
            .spacing([24.0, 6.0])
            .show(ui, |ui| {
                for id in &self.model.available {
                    ui.label(RichText::new(id.as_str()).strong());
                    ui.label(if id.upgrade().is_some() {
                        self.t("update all", "hepsini güncelle")
                    } else {
                        "-"
                    });
                    ui.label(if id.outdated().is_some() {
                        self.t("preview", "önizleme")
                    } else {
                        ""
                    });
                    ui.end_row();
                }
            });
    }

    fn confirm_dialog(&mut self, ctx: &egui::Context) {
        let Some(task) = self.confirm.clone() else { return };
        let lang = self.model.lang;
        let (title, ids) = match &task {
            Task::Install(ids) => (
                self.t("Install these apps?", "Bu uygulamalar kurulsun mu?"),
                ids.clone(),
            ),
            Task::Remove(ids) => (
                self.t("Remove these apps?", "Bu uygulamalar kaldırılsın mı?"),
                ids.clone(),
            ),
            Task::ApplyTweaks(ids, _) => (self.t("Apply these tweaks?", "Bu ayarlar uygulansın mı?"), ids.clone()),
            Task::RevertTweaks(ids) => (self.t("Undo these tweaks?", "Bu ayarlar geri alınsın mı?"), ids.clone()),
            Task::Update => (self.t("Update everything?", "Her şey güncellensin mi?"), Vec::new()),
        };
        let irreversible: Vec<String> = match &task {
            Task::ApplyTweaks(ids, _) => ids
                .iter()
                .filter_map(|id| self.model.tweaks.get(id))
                .filter(|t| !t.is_reversible())
                .map(|t| t.name.get(lang).to_owned())
                .collect(),
            _ => Vec::new(),
        };
        let response = egui::Modal::new(egui::Id::new("confirm")).show(ctx, |ui| {
            ui.set_width(440.0);
            ui.heading(title);
            ui.add_space(6.0);
            egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                for id in &ids {
                    let name = self
                        .model
                        .catalog
                        .get(id)
                        .map(|a| a.name.clone())
                        .or_else(|| self.model.tweaks.get(id).map(|t| t.name.get(lang).to_owned()))
                        .unwrap_or_else(|| id.clone());
                    ui.label(format!("• {name}"));
                }
            });
            if matches!(task, Task::Install(_)) {
                ui.add_space(6.0);
                ui.label(
                    RichText::new(self.t(
                        "A missing package manager may be installed with its official installer.",
                        "Eksik bir paket yöneticisi, resmî kurulum betiğiyle kurulabilir.",
                    ))
                    .small()
                    .weak(),
                );
            }
            if !irreversible.is_empty() {
                ui.add_space(6.0);
                ui.colored_label(BAD, self.t("Cannot be undone:", "Geri alınamaz:"));
                for name in &irreversible {
                    ui.colored_label(BAD, format!("• {name}"));
                }
            }
            if self.model.system.os != Os::Windows && !self.model.system.privileges.is_elevated {
                ui.add_space(6.0);
                ui.label(
                    RichText::new(self.t(
                        "Steps that need administrator rights will ask for your password.",
                        "Yönetici yetkisi gereken adımlar parolanızı soracak.",
                    ))
                    .small()
                    .weak(),
                );
            }
            ui.add_space(10.0);
            let mut decision = None;
            ui.horizontal(|ui| {
                if ui.button(RichText::new(self.t("Continue", "Devam")).strong()).clicked() {
                    decision = Some(true);
                }
                if ui.button(self.t("Cancel", "Vazgeç")).clicked() {
                    decision = Some(false);
                }
            });
            decision
        });
        match response.inner {
            Some(true) => {
                self.confirm = None;
                self.start(&task);
            }
            Some(false) => self.confirm = None,
            None if response.should_close() => self.confirm = None,
            None => {}
        }
    }

    fn output_window(&mut self, ctx: &egui::Context) {
        let Some(window) = &self.output else { return };
        let running = window.rx.is_some();
        let mut open = true;
        egui::Window::new(window.title.clone())
            .open(&mut open)
            .default_size([720.0, 380.0])
            .collapsible(false)
            .show(ctx, |ui| {
                if running {
                    ui.spinner();
                }
                egui::ScrollArea::both()
                    .auto_shrink(false)
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        for line in &window.lines {
                            ui.label(RichText::new(line).monospace().small());
                        }
                    });
            });
        if !open {
            self.output = None;
        }
    }
}

impl eframe::App for SkadiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll();
        self.take_screenshot(ctx);
        if self.busy()
            || self.installed.is_none()
            || self.statuses.is_none()
            || self.output.as_ref().is_some_and(|o| o.rx.is_some())
        {
            ctx.request_repaint_after(Duration::from_millis(150));
        }
        self.top_bar(ctx);
        self.bottom_bar(ctx);
        self.side_panel(ctx);
        egui::CentralPanel::default().show(ctx, |ui| match self.tab {
            Tab::Apps => self.apps_view(ui),
            Tab::Tweaks => self.tweaks_view(ui),
            Tab::Profiles => self.profiles_view(ui),
            Tab::Updates => self.updates_view(ui),
        });
        self.confirm_dialog(ctx);
        self.output_window(ctx);
        self.share_window(ctx);
    }
}

impl SkadiApp {
    /// Veriler yüklendikten sonra birkaç kare bekler, pencerenin görüntüsünü PNG olarak kaydeder ve
    /// uygulamayı kapatır (README görüntüleri için gizli `--screenshot`).
    fn take_screenshot(&mut self, ctx: &egui::Context) {
        let Some((path, frames)) = self.screenshot.as_mut() else {
            return;
        };
        let image = ctx.input(|i| {
            i.raw.events.iter().find_map(|event| match event {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(image) = image {
            let pixels: Vec<u8> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
            if let Some(buffer) = image::RgbaImage::from_raw(image.size[0] as u32, image.size[1] as u32, pixels) {
                let _ = buffer.save(&*path);
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if self.installed.is_none() || self.statuses.is_none() {
            ctx.request_repaint_after(Duration::from_millis(100));
            return;
        }
        // README görüntüsü bu bilgisayarda kurulu uygulamaları göstermemeli.
        self.installed = Some(HashSet::new());
        *frames += 1;
        if *frames == 8 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
        }
        ctx.request_repaint();
    }

    fn share_window(&mut self, ctx: &egui::Context) {
        let Some((windows, unix)) = self.share.clone() else {
            return;
        };
        let mut open = true;
        egui::Window::new(self.t("Share selection", "Seçimi paylaş"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                for (label, line) in [("Windows (PowerShell)", &windows), ("Linux, macOS", &unix)] {
                    ui.label(RichText::new(label).strong());
                    ui.horizontal(|ui| {
                        ui.add(egui::Label::new(RichText::new(line.as_str()).monospace().small()).wrap());
                        if ui.button(self.t("Copy", "Kopyala")).clicked() {
                            ctx.copy_text(line.clone());
                        }
                    });
                    ui.add_space(6.0);
                }
                let current = if self.model.system.os == Os::Windows {
                    &windows
                } else {
                    &unix
                };
                if let Some((width, modules)) = crate::share::qr_modules(current) {
                    // Okuyucular için koyu modüller açık zeminde, etrafında 4 modül boşluk.
                    let quiet = 4;
                    let cell = 4.0;
                    let size = (width + quiet * 2) as f32 * cell;
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
                    let painter = ui.painter_at(rect);
                    painter.rect_filled(rect, 0.0, Color32::WHITE);
                    for (index, dark) in modules.iter().enumerate() {
                        if *dark {
                            let x = (index % width + quiet) as f32 * cell;
                            let y = (index / width + quiet) as f32 * cell;
                            let min = rect.min + egui::vec2(x, y);
                            painter.rect_filled(
                                egui::Rect::from_min_size(min, egui::vec2(cell, cell)),
                                0.0,
                                Color32::BLACK,
                            );
                        }
                    }
                }
            });
        if !open {
            self.share = None;
        }
    }
}

pub fn run(lang: Lang, screenshot: Option<&Path>, tab: &str) -> Result<()> {
    let model = Model::load(lang)?;
    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../../../../assets/icon-tile-black-256.png")).ok();
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Skadi")
        .with_inner_size([1120.0, 740.0])
        .with_min_inner_size([820.0, 520.0]);
    if let Some(icon) = icon {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "Skadi",
        options,
        Box::new(|cc| {
            egui_extras::install_image_loaders(&cc.egui_ctx);
            let mut app = SkadiApp::new(model);
            if let Some(path) = screenshot {
                app.screenshot = Some((path.to_path_buf(), 0));
                if tab == "tweaks" {
                    app.tab = Tab::Tweaks;
                    let ids: Vec<String> = app.model.system_tweaks().iter().take(3).map(|t| t.id.clone()).collect();
                    app.tweaks.extend(ids);
                } else {
                    app.apps
                        .extend(["firefox", "git", "vlc", "keepassxc"].map(str::to_owned));
                }
            }
            Ok(Box::new(app))
        }),
    )
    .map_err(|e| Error::State(e.to_string()))
}
