//! Klavyeyle kullanılan terminal arayüzü (ratatui).
//!
//! Bir iş başlatıldığında TUI terminali bırakır ve `skadi` CLI'sini çalıştırır: CLI planı
//! gösterir, onay ister ve gerekiyorsa `sudo` parolasını sorar. İş bitince TUI geri gelir.

use std::{
    collections::{HashMap, HashSet},
    io::Write,
    path::Path,
    process::Command,
    sync::mpsc,
    time::Duration,
};

use ratatui::{
    Frame, Terminal,
    backend::TestBackend,
    buffer::Buffer,
    crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Cell, Paragraph, Row, Table, TableState, Tabs, Wrap},
};
use skadi_core::{
    Error, Lang, Result,
    catalog::{App, Category},
    tweak::{Risk, Tweak, TweakStatus},
};

use super::{Loaded, Model, load_in_background};

const ACCENT: Color = Color::Rgb(91, 164, 207);
const MUTED: Color = Color::Rgb(128, 136, 150);
const GOOD: Color = Color::Rgb(94, 186, 125);
const WARN: Color = Color::Rgb(230, 180, 80);
const BAD: Color = Color::Rgb(224, 108, 117);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Apps,
    Tweaks,
    Profiles,
    Updates,
}

impl Tab {
    const ALL: [Self; 4] = [Self::Apps, Self::Tweaks, Self::Profiles, Self::Updates];

    fn title(self, lang: Lang) -> &'static str {
        match self {
            Self::Apps => lang.pick("Apps", "Uygulamalar"),
            Self::Tweaks => lang.pick("Tweaks", "Ayarlar"),
            Self::Profiles => lang.pick("Profiles", "Profiller"),
            Self::Updates => lang.pick("Updates", "Güncellemeler"),
        }
    }
}

struct Tui<'m> {
    model: &'m Model,
    tab: Tab,
    table: TableState,
    search: String,
    searching: bool,
    category: Option<Category>,
    foss_only: bool,
    apps: HashSet<String>,
    tweaks: HashSet<String>,
    installed: Option<HashSet<String>>,
    statuses: Option<HashMap<String, Option<TweakStatus>>>,
    drifted: HashSet<String>,
    message: Option<String>,
}

/// Bir tuş sonrası yapılacak iş.
enum Step {
    Continue,
    Quit,
    /// TUI'yi bırakıp bu argümanlarla CLI'yi çalıştır.
    Run(Vec<String>),
}

impl<'m> Tui<'m> {
    fn new(model: &'m Model) -> Self {
        let mut table = TableState::default();
        table.select(Some(0));
        Self {
            model,
            tab: Tab::Apps,
            table,
            search: String::new(),
            searching: false,
            category: None,
            foss_only: false,
            apps: HashSet::new(),
            tweaks: HashSet::new(),
            installed: None,
            statuses: None,
            drifted: HashSet::new(),
            message: None,
        }
    }

    fn t(&self, en: &'static str, tr: &'static str) -> &'static str {
        self.model.lang.pick(en, tr)
    }

    fn matches(&self, texts: &[&str]) -> bool {
        let needle = self.search.to_lowercase();
        needle.is_empty() || texts.iter().any(|t| t.to_lowercase().contains(&needle))
    }

    fn visible_apps(&self) -> Vec<&'m App> {
        let lang = self.model.lang;
        self.model
            .catalog
            .apps()
            .iter()
            .filter(|app| self.model.is_for_this_os(app))
            .filter(|app| self.category.is_none_or(|c| c == app.category))
            .filter(|app| !self.foss_only || app.foss == Some(true))
            .filter(|app| self.matches(&[&app.id, &app.name, app.description.get(lang)]))
            .collect()
    }

    fn visible_tweaks(&self) -> Vec<&'m Tweak> {
        let lang = self.model.lang;
        self.model
            .system_tweaks()
            .into_iter()
            .filter(|t| self.matches(&[&t.id, t.name.get(lang), t.description.get(lang)]))
            .collect()
    }

    fn row_count(&self) -> usize {
        match self.tab {
            Tab::Apps => self.visible_apps().len(),
            Tab::Tweaks => self.visible_tweaks().len(),
            Tab::Profiles => self.model.presets.len(),
            Tab::Updates => 0,
        }
    }

    fn receive(&mut self, rx: &mpsc::Receiver<Loaded>) {
        while let Ok(loaded) = rx.try_recv() {
            match loaded {
                Loaded::Installed(set) => self.installed = Some(set),
                Loaded::Statuses(map) => self.statuses = Some(map),
                Loaded::Drifted(set) => self.drifted = set,
            }
        }
    }

    fn switch(&mut self, tab: Tab) {
        self.tab = tab;
        self.table.select(Some(0));
        self.message = None;
    }

    fn cli(&self, args: &[&str], ids: Vec<String>) -> Vec<String> {
        let mut out = vec!["--lang".to_owned(), self.model.lang.as_str().to_owned()];
        out.extend(args.iter().map(|a| (*a).to_owned()));
        out.extend(ids);
        out
    }

    fn sorted(set: &HashSet<String>) -> Vec<String> {
        let mut ids: Vec<String> = set.iter().cloned().collect();
        ids.sort();
        ids
    }

    fn key(&mut self, code: KeyCode, modifiers: KeyModifiers) -> Step {
        if self.searching {
            match code {
                KeyCode::Esc => {
                    self.search.clear();
                    self.searching = false;
                }
                KeyCode::Enter => self.searching = false,
                KeyCode::Backspace => {
                    self.search.pop();
                }
                KeyCode::Char(c) => self.search.push(c),
                _ => {}
            }
            self.table.select(Some(0));
            return Step::Continue;
        }
        let rows = self.row_count();
        let current = self.table.selected().unwrap_or(0);
        match code {
            KeyCode::Char('q') | KeyCode::Esc => return Step::Quit,
            KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => return Step::Quit,
            KeyCode::Tab | KeyCode::Right => {
                let next = Tab::ALL[(Tab::ALL.iter().position(|t| *t == self.tab).unwrap_or(0) + 1) % 4];
                self.switch(next);
            }
            KeyCode::BackTab | KeyCode::Left => {
                let next = Tab::ALL[(Tab::ALL.iter().position(|t| *t == self.tab).unwrap_or(0) + 3) % 4];
                self.switch(next);
            }
            KeyCode::Char(n @ '1'..='4') => self.switch(Tab::ALL[n as usize - '1' as usize]),
            KeyCode::Down | KeyCode::Char('j') if rows > 0 => self.table.select(Some((current + 1).min(rows - 1))),
            KeyCode::Up | KeyCode::Char('k') => self.table.select(Some(current.saturating_sub(1))),
            KeyCode::PageDown if rows > 0 => self.table.select(Some((current + 15).min(rows - 1))),
            KeyCode::PageUp => self.table.select(Some(current.saturating_sub(15))),
            KeyCode::Char('/') => self.searching = true,
            KeyCode::Char('f') if self.tab == Tab::Apps => {
                self.foss_only = !self.foss_only;
                self.table.select(Some(0));
            }
            KeyCode::Char('c') if self.tab == Tab::Apps => {
                let index = self.category.and_then(|c| Category::ALL.iter().position(|x| *x == c));
                self.category = match index {
                    None => Some(Category::ALL[0]),
                    Some(i) if i + 1 < Category::ALL.len() => Some(Category::ALL[i + 1]),
                    Some(_) => None,
                };
                self.table.select(Some(0));
            }
            KeyCode::Char(' ') => match self.tab {
                Tab::Apps => {
                    if let Some(app) = self.visible_apps().get(current) {
                        toggle(&mut self.apps, &app.id);
                        self.table.select(Some((current + 1).min(rows.saturating_sub(1))));
                    }
                }
                Tab::Tweaks => {
                    if let Some(tweak) = self.visible_tweaks().get(current) {
                        toggle(&mut self.tweaks, &tweak.id);
                        self.table.select(Some((current + 1).min(rows.saturating_sub(1))));
                    }
                }
                _ => {}
            },
            KeyCode::Char('i') if self.tab == Tab::Apps && !self.apps.is_empty() => {
                return Step::Run(self.cli(&["install"], Self::sorted(&self.apps)));
            }
            KeyCode::Char('r') if self.tab == Tab::Apps && !self.apps.is_empty() => {
                return Step::Run(self.cli(&["remove"], Self::sorted(&self.apps)));
            }
            KeyCode::Char('a') if self.tab == Tab::Tweaks && !self.tweaks.is_empty() => {
                return Step::Run(self.cli(&["tweak", "apply"], Self::sorted(&self.tweaks)));
            }
            KeyCode::Char('u') if self.tab == Tab::Tweaks && !self.tweaks.is_empty() => {
                return Step::Run(self.cli(&["tweak", "revert"], Self::sorted(&self.tweaks)));
            }
            KeyCode::Enter if self.tab == Tab::Profiles => {
                if let Some(preset) = self.model.presets.get(current) {
                    return Step::Run(self.cli(&["profile", "apply"], vec![preset.id.clone()]));
                }
            }
            KeyCode::Char('c') if self.tab == Tab::Updates => {
                return Step::Run(self.cli(&["update", "--check"], vec![]));
            }
            KeyCode::Char('u') if self.tab == Tab::Updates => return Step::Run(self.cli(&["update"], vec![])),
            _ => {}
        }
        Step::Continue
    }
}

fn toggle(set: &mut HashSet<String>, id: &str) {
    if !set.remove(id) {
        set.insert(id.to_owned());
    }
}

fn risk_style(risk: Risk) -> Style {
    Style::new().fg(match risk {
        Risk::Safe => GOOD,
        Risk::Moderate => WARN,
        Risk::Advanced => BAD,
    })
}

fn draw(frame: &mut Frame, app: &mut Tui) {
    let lang = app.model.lang;
    let [header, tabs, body, footer] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(1),
        Constraint::Min(5),
        Constraint::Length(2),
    ])
    .areas(frame.area());

    // Üst bilgi: ad, sürüm ve sistem özeti.
    let system = &app.model.system;
    let managers = app
        .model
        .available
        .iter()
        .map(|m| m.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let admin = if system.privileges.is_elevated {
        app.t("admin", "yönetici")
    } else {
        app.t("user", "kullanıcı")
    };
    let distro = system
        .distro
        .as_ref()
        .map(|d| format!(" · {}", d.display_name()))
        .unwrap_or_default();
    let title = Line::from(vec![
        Span::styled(
            " SKADI ",
            Style::new().fg(Color::Black).bg(ACCENT).add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(format!("v{}", env!("CARGO_PKG_VERSION")), Style::new().fg(MUTED)),
        Span::raw(format!(
            "   {} {}{distro} · {admin} · {managers}",
            system.os, system.arch
        )),
    ]);
    frame.render_widget(
        Paragraph::new(title).block(
            Block::new()
                .borders(Borders::BOTTOM)
                .border_style(Style::new().fg(MUTED))
                .padding(ratatui::widgets::Padding::top(1)),
        ),
        header,
    );

    let titles: Vec<String> = Tab::ALL
        .iter()
        .enumerate()
        .map(|(i, tab)| format!(" {} {} ", i + 1, tab.title(lang)))
        .collect();
    let selected = Tab::ALL.iter().position(|t| *t == app.tab).unwrap_or(0);
    frame.render_widget(
        Tabs::new(titles)
            .select(selected)
            .highlight_style(Style::new().fg(Color::Black).bg(ACCENT).add_modifier(Modifier::BOLD))
            .divider(""),
        tabs,
    );

    match app.tab {
        Tab::Apps => draw_apps(frame, app, body),
        Tab::Tweaks => draw_tweaks(frame, app, body),
        Tab::Profiles => draw_profiles(frame, app, body),
        Tab::Updates => draw_updates(frame, app, body),
    }

    let hints = match app.tab {
        Tab::Apps => app.t(
            "space select · i install · r remove · / search · c category · f FOSS · tab next · q quit",
            "boşluk seç · i kur · r kaldır · / ara · c kategori · f FOSS · tab sonraki · q çık",
        ),
        Tab::Tweaks => app.t(
            "space select · a apply · u undo · / search · tab next · q quit",
            "boşluk seç · a uygula · u geri al · / ara · tab sonraki · q çık",
        ),
        Tab::Profiles => app.t(
            "enter apply preset · tab next · q quit",
            "enter seti uygula · tab sonraki · q çık",
        ),
        Tab::Updates => app.t(
            "c check · u update all · tab next · q quit",
            "c denetle · u hepsini güncelle · tab sonraki · q çık",
        ),
    };
    let status = if app.searching {
        Line::from(vec![
            Span::styled(format!("/{}", app.search), Style::new().fg(ACCENT)),
            Span::raw("▏"),
        ])
    } else if let Some(message) = &app.message {
        Line::from(Span::styled(message.clone(), Style::new().fg(WARN)))
    } else {
        Line::from(Span::styled(hints, Style::new().fg(MUTED)))
    };
    let mut lines = vec![status];
    let counts = format!(
        "{} {} · {} {}",
        app.apps.len(),
        app.t("apps selected", "uygulama seçili"),
        app.tweaks.len(),
        app.t("tweaks selected", "ayar seçili")
    );
    lines.insert(0, Line::from(Span::styled(counts, Style::new().fg(MUTED))));
    frame.render_widget(Paragraph::new(lines), footer);
}

fn block(title: String) -> Block<'static> {
    Block::new()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(MUTED))
        .title(Span::styled(
            format!(" {title} "),
            Style::new().fg(ACCENT).add_modifier(Modifier::BOLD),
        ))
}

fn draw_apps(frame: &mut Frame, app: &mut Tui, area: Rect) {
    let lang = app.model.lang;
    let apps = app.visible_apps();
    let rows = apps.iter().map(|a| {
        let check = if app.apps.contains(&a.id) { "[x]" } else { "[ ]" };
        let installed = app
            .installed
            .as_ref()
            .map_or("…", |set| if set.contains(&a.id) { "●" } else { "" });
        let manager = app.model.manager_for(a).map_or("-", |m| m.as_str());
        Row::new(vec![
            Cell::from(check).style(Style::new().fg(ACCENT)),
            Cell::from(a.name.clone()).bold(),
            Cell::from(installed).style(Style::new().fg(GOOD)),
            Cell::from(if a.foss == Some(true) { "FOSS" } else { "" }).style(Style::new().fg(ACCENT)),
            Cell::from(manager).style(Style::new().fg(MUTED)),
            Cell::from(a.description.get(lang)),
        ])
    });
    let category = app.category.map_or(app.t("all", "tümü"), |c| c.label(lang));
    let foss = if app.foss_only { " · FOSS" } else { "" };
    let title = format!("{} ({}) · {category}{foss}", app.t("Apps", "Uygulamalar"), apps.len());
    let table = Table::new(
        rows,
        [
            Constraint::Length(3),
            Constraint::Length(24),
            Constraint::Length(1),
            Constraint::Length(4),
            Constraint::Length(9),
            Constraint::Min(10),
        ],
    )
    .block(block(title))
    .row_highlight_style(Style::new().bg(Color::Rgb(38, 50, 66)))
    .column_spacing(1);
    frame.render_stateful_widget(table, area, &mut app.table);
}

fn draw_tweaks(frame: &mut Frame, app: &mut Tui, area: Rect) {
    let lang = app.model.lang;
    let tweaks = app.visible_tweaks();
    let [list, detail] = Layout::vertical([Constraint::Min(5), Constraint::Length(4)]).areas(area);
    let rows = tweaks.iter().map(|t| {
        let check = if app.tweaks.contains(&t.id) { "[x]" } else { "[ ]" };
        let status = app.statuses.as_ref().map(|map| map.get(&t.id).copied().flatten());
        let (label, color) = match status {
            None => ("…", MUTED),
            Some(Some(TweakStatus::Applied)) => (app.t("applied", "uygulandı"), GOOD),
            Some(Some(TweakStatus::Partial)) => (app.t("partial", "kısmen"), WARN),
            Some(Some(TweakStatus::NotApplied)) => (app.t("not applied", "uygulanmadı"), MUTED),
            Some(Some(TweakStatus::Unavailable)) => (app.t("n/a", "yok"), MUTED),
            Some(None) => ("?", MUTED),
        };
        let (label, color) = if app.drifted.contains(&t.id) {
            (app.t("drifted", "sürüklendi"), WARN)
        } else {
            (label, color)
        };
        let irreversible = if t.is_reversible() { "" } else { "!" };
        Row::new(vec![
            Cell::from(check).style(Style::new().fg(ACCENT)),
            Cell::from(t.name.get(lang)).bold(),
            Cell::from(label).style(Style::new().fg(color)),
            Cell::from(t.risk.label(lang)).style(risk_style(t.risk)),
            Cell::from(irreversible).style(Style::new().fg(BAD).add_modifier(Modifier::BOLD)),
            Cell::from(t.category.label(lang)).style(Style::new().fg(MUTED)),
        ])
    });
    let title = format!("{} ({})", app.t("Tweaks", "Ayarlar"), tweaks.len());
    let table = Table::new(
        rows,
        [
            Constraint::Length(3),
            Constraint::Min(30),
            Constraint::Length(12),
            Constraint::Length(9),
            Constraint::Length(1),
            Constraint::Length(22),
        ],
    )
    .block(block(title))
    .row_highlight_style(Style::new().bg(Color::Rgb(38, 50, 66)))
    .column_spacing(1);
    let current = app.table.selected().and_then(|i| tweaks.get(i).copied());
    frame.render_stateful_widget(table, list, &mut app.table);
    if let Some(tweak) = current {
        let mut text = vec![Line::from(tweak.description.get(lang))];
        if !tweak.is_reversible() {
            text.push(Line::from(Span::styled(
                app.t(
                    "! Removes apps; cannot be undone.",
                    "! Uygulama kaldırır; geri alınamaz.",
                ),
                Style::new().fg(BAD),
            )));
        }
        frame.render_widget(
            Paragraph::new(text)
                .wrap(Wrap { trim: true })
                .block(Block::new().padding(ratatui::widgets::Padding::horizontal(1))),
            detail,
        );
    }
}

fn draw_profiles(frame: &mut Frame, app: &mut Tui, area: Rect) {
    let lang = app.model.lang;
    let rows = app.model.presets.iter().map(|p| {
        let counts = format!(
            "{} {} · {} {}",
            p.apps.len(),
            app.t("apps", "uygulama"),
            p.tweaks.len(),
            app.t("tweaks", "ayar")
        );
        Row::new(vec![
            Cell::from(p.name.get(lang)).bold(),
            Cell::from(counts).style(Style::new().fg(MUTED)),
            Cell::from(p.description.get(lang)),
        ])
    });
    let table = Table::new(
        rows,
        [Constraint::Length(20), Constraint::Length(22), Constraint::Min(10)],
    )
    .block(block(app.t("Presets", "Hazır setler").to_owned()))
    .row_highlight_style(Style::new().bg(Color::Rgb(38, 50, 66)))
    .column_spacing(1);
    frame.render_stateful_widget(table, area, &mut app.table);
}

fn draw_updates(frame: &mut Frame, app: &mut Tui, area: Rect) {
    let mut lines = vec![
        Line::from(app.t(
            "Updates run each package manager's own update command.",
            "Güncellemeler her paket yöneticisinin kendi güncelleme komutuyla yapılır.",
        )),
        Line::from(""),
    ];
    for id in &app.model.available {
        let support = if id.upgrade().is_some() {
            app.t("update all", "hepsini güncelle")
        } else {
            "-"
        };
        let preview = if id.outdated().is_some() {
            app.t("preview", "önizleme")
        } else {
            ""
        };
        lines.push(Line::from(vec![
            Span::styled(format!("  {:<12}", id.as_str()), Style::new().fg(ACCENT).bold()),
            Span::raw(format!("{support}  ")),
            Span::styled(preview, Style::new().fg(MUTED)),
        ]));
    }
    frame.render_widget(
        Paragraph::new(lines).block(block(app.t("Updates", "Güncellemeler").to_owned())),
        area,
    );
}

/// TUI'yi çalıştırır.
pub fn run(lang: Lang) -> Result<()> {
    let model = Model::load(lang)?;
    let mut rx = load_in_background(&model);
    let mut app = Tui::new(&model);
    let io = |e: std::io::Error| Error::State(e.to_string());
    let mut terminal = ratatui::init();
    let result = loop {
        app.receive(&rx);
        if let Err(error) = terminal.draw(|frame| draw(frame, &mut app)) {
            break Err(io(error));
        }
        match event::poll(Duration::from_millis(200)) {
            Ok(false) => continue,
            Ok(true) => {}
            Err(error) => break Err(io(error)),
        }
        let Ok(Event::Key(key)) = event::read() else { continue };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        match app.key(key.code, key.modifiers) {
            Step::Continue => {}
            Step::Quit => break Ok(()),
            Step::Run(args) => {
                ratatui::restore();
                let outcome = run_cli(lang, &args);
                terminal = ratatui::init();
                app.message = outcome.err();
                // Değişiklikten sonra kurulu listesi ve durumlar yeniden okunur.
                app.installed = None;
                app.statuses = None;
                rx = load_in_background(&model);
            }
        }
    };
    ratatui::restore();
    result
}

/// CLI'yi aynı terminalde çalıştırır; kullanıcı Enter'a basınca TUI'ye döner.
fn run_cli(lang: Lang, args: &[String]) -> std::result::Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    println!();
    let status = Command::new(exe).args(args).status().map_err(|e| e.to_string())?;
    print!(
        "\n{} ",
        lang.pick("Press Enter to return to Skadi.", "Skadi'ye dönmek için Enter'a basın.")
    );
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    let _ = std::io::stdin().read_line(&mut line);
    if status.success() {
        Ok(())
    } else {
        Err(lang
            .pick("The last operation failed.", "Son işlem başarısız oldu.")
            .to_owned())
    }
}

/// README için TUI'nin bir karesini SVG olarak kaydeder (veriler eşzamanlı yüklenir).
pub fn screenshot(lang: Lang, path: &Path, tab: &str) -> Result<()> {
    let model = Model::load(lang)?;
    let rx = load_in_background(&model);
    let mut app = Tui::new(&model);
    for _ in 0..2 {
        if let Ok(loaded) = rx.recv_timeout(Duration::from_secs(60)) {
            match loaded {
                // README görüntüsü bu bilgisayarda kurulu uygulamaları göstermemeli.
                Loaded::Installed(_) => app.installed = Some(HashSet::new()),
                Loaded::Statuses(map) => app.statuses = Some(map),
                Loaded::Drifted(set) => app.drifted = set,
            }
        }
    }
    match tab {
        "tweaks" => {
            app.switch(Tab::Tweaks);
            for id in app
                .visible_tweaks()
                .iter()
                .take(3)
                .map(|t| t.id.clone())
                .collect::<Vec<_>>()
            {
                app.tweaks.insert(id);
            }
        }
        _ => {
            for id in ["firefox", "git", "vlc", "keepassxc"] {
                app.apps.insert(id.to_owned());
            }
            app.table.select(Some(2));
        }
    }
    let mut terminal = Terminal::new(TestBackend::new(120, 34)).map_err(|e| Error::State(e.to_string()))?;
    terminal
        .draw(|frame| draw(frame, &mut app))
        .map_err(|e| Error::State(e.to_string()))?;
    let svg = to_svg(terminal.backend().buffer());
    std::fs::write(path, svg).map_err(|e| Error::State(format!("{}: {e}", path.display())))
}

fn hex(color: Color, default: &str) -> String {
    match color {
        Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
        Color::Black => "#0d1117".into(),
        Color::White => "#e6edf3".into(),
        Color::Reset => default.into(),
        Color::Gray => "#8b949e".into(),
        _ => default.into(),
    }
}

/// Terminal tamponunu, her satırın aynı stildeki hücreleri birleştirilmiş SVG metnine dönüştürür.
fn to_svg(buffer: &Buffer) -> String {
    const W: f32 = 9.0;
    const H: f32 = 19.0;
    const BG: &str = "#0d1117";
    const FG: &str = "#e6edf3";
    let area = buffer.area;
    let escape = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    let mut out = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\">\
<rect width=\"100%\" height=\"100%\" rx=\"10\" fill=\"{BG}\"/>\
<g font-family=\"ui-monospace, SFMono-Regular, Menlo, Consolas, monospace\" font-size=\"14\" xml:space=\"preserve\">",
        w = (f32::from(area.width) * W + 24.0),
        h = (f32::from(area.height) * H + 24.0)
    );
    for y in 0..area.height {
        let mut x = 0;
        while x < area.width {
            let cell = &buffer[(x, y)];
            let style = (cell.fg, cell.bg, cell.modifier);
            let start = x;
            let mut text = String::new();
            while x < area.width && {
                let c = &buffer[(x, y)];
                (c.fg, c.bg, c.modifier) == style
            } {
                text.push_str(buffer[(x, y)].symbol());
                x += 1;
            }
            let px = 12.0 + f32::from(start) * W;
            let py = 12.0 + f32::from(y) * H;
            if style.1 != Color::Reset {
                out.push_str(&format!(
                    "<rect x=\"{px}\" y=\"{py}\" width=\"{}\" height=\"{H}\" fill=\"{}\"/>",
                    f32::from(x - start) * W,
                    hex(style.1, BG)
                ));
            }
            if !text.trim().is_empty() {
                let weight = if style.2.contains(Modifier::BOLD) {
                    " font-weight=\"bold\""
                } else {
                    ""
                };
                out.push_str(&format!(
                    "<text x=\"{px}\" y=\"{}\" fill=\"{}\"{weight} textLength=\"{}\" lengthAdjust=\"spacingAndGlyphs\">{}</text>",
                    py + 14.5,
                    hex(style.0, FG),
                    f32::from(x - start) * W,
                    escape(&text)
                ));
            }
        }
    }
    out.push_str("</g></svg>\n");
    out
}
