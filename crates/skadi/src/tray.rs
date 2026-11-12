//! `skadi tray`: güncelleme bildirimli sistem tepsisi (Windows) ve menü çubuğu (macOS) simgesi.
//! Simge işletim sisteminin kendi araçlarıyla çizilir (Windows'ta .NET NotifyIcon, macOS'ta
//! JavaScript for Automation); Skadi'ye yeni bir arayüz kütüphanesi eklenmez. Betikler sabittir;
//! değişken olan tek şey ortam değişkeniyle verilen Skadi yolu ve çevrilmiş etiketlerdir.

use std::process::{Command, Stdio};

use skadi_core::{Error, Lang, Result, exec::find_program, platform::Os};

const WINDOWS_SCRIPT: &str = r#"
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
$icon = New-Object System.Windows.Forms.NotifyIcon
$icon.Icon = [System.Drawing.Icon]::ExtractAssociatedIcon($env:SKADI_EXE)
$icon.Text = 'Skadi'
$icon.Visible = $true
function Update-Count {
    try {
        $data = (& $env:SKADI_EXE --json update --check 2>$null | Out-String) | ConvertFrom-Json
        $count = 0
        foreach ($manager in $data.updates) { if ($manager.packages) { $count += @($manager.packages).Count } }
        $icon.Text = "Skadi: $count"
        if ($count -gt 0) { $icon.ShowBalloonTip(5000, 'Skadi', ($env:SKADI_UPDATES -f $count), 'Info') }
    } catch { }
}
$menu = New-Object System.Windows.Forms.ContextMenuStrip
$open = $menu.Items.Add($env:SKADI_OPEN)
$open.add_Click({ Start-Process -FilePath $env:SKADI_EXE -ArgumentList 'gui' })
$check = $menu.Items.Add($env:SKADI_CHECK)
$check.add_Click({ Update-Count })
$quit = $menu.Items.Add($env:SKADI_QUIT)
$quit.add_Click({ $icon.Visible = $false; $icon.Dispose(); [System.Windows.Forms.Application]::Exit() })
$icon.ContextMenuStrip = $menu
$icon.add_DoubleClick({ Start-Process -FilePath $env:SKADI_EXE -ArgumentList 'gui' })
$timer = New-Object System.Windows.Forms.Timer
$timer.Interval = 6 * 60 * 60 * 1000
$timer.add_Tick({ Update-Count })
$timer.Start()
Update-Count
[System.Windows.Forms.Application]::Run()
"#;

const MACOS_SCRIPT: &str = r#"
ObjC.import('Cocoa');
ObjC.import('stdlib');
const env = (name) => $.getenv(name);
const exe = env('SKADI_EXE');
const shell = Application.currentApplication();
shell.includeStandardAdditions = true;
const quote = (text) => "'" + text.replace(/'/g, "'\\''") + "'";
function countUpdates() {
  try {
    const data = JSON.parse(shell.doShellScript(quote(exe) + ' --json update --check 2>/dev/null'));
    let count = 0;
    for (const manager of data.updates) { if (manager.packages) { count += manager.packages.length; } }
    return count;
  } catch (e) { return 0; }
}
ObjC.registerSubclass({
  name: 'SkadiTrayHandler',
  methods: {
    'open:': { types: ['void', ['id']], implementation: function () { shell.doShellScript(quote(exe) + ' gui >/dev/null 2>&1 &'); } },
    'check:': { types: ['void', ['id']], implementation: function () { refresh(); } },
    'quit:': { types: ['void', ['id']], implementation: function () { $.NSApplication.sharedApplication.terminate(null); } },
  },
});
const handler = $.SkadiTrayHandler.alloc.init;
const app = $.NSApplication.sharedApplication;
app.setActivationPolicy($.NSApplicationActivationPolicyAccessory);
const item = $.NSStatusBar.systemStatusBar.statusItemWithLength($.NSVariableStatusItemLength);
item.button.title = 'Skadi';
const menu = $.NSMenu.alloc.init;
for (const [label, action] of [[env('SKADI_OPEN'), 'open:'], [env('SKADI_CHECK'), 'check:'], [env('SKADI_QUIT'), 'quit:']]) {
  const entry = $.NSMenuItem.alloc.initWithTitleActionKeyEquivalent(label, action, '');
  entry.target = handler;
  menu.addItem(entry);
}
item.menu = menu;
function refresh() {
  const count = countUpdates();
  item.button.title = count > 0 ? 'Skadi ' + count : 'Skadi';
  if (count > 0) { shell.displayNotification(env('SKADI_UPDATES').replace('{0}', String(count)), { withTitle: 'Skadi' }); }
}
refresh();
$.NSTimer.scheduledTimerWithTimeIntervalTargetSelectorUserInfoRepeats(6 * 60 * 60, handler, 'check:', null, true);
app.run;
"#;

/// Simgeyi arka planda başlatır ve hemen döner.
pub fn tray(lang: Lang) -> Result<()> {
    let t = |en, tr| lang.pick(en, tr);
    let exe = std::env::current_exe().map_err(|e| Error::State(e.to_string()))?;
    let (program, args): (&str, Vec<&str>) = match Os::current()? {
        Os::Windows => (
            "powershell",
            vec![
                "-NoProfile",
                "-NonInteractive",
                "-WindowStyle",
                "Hidden",
                "-Command",
                WINDOWS_SCRIPT,
            ],
        ),
        Os::Macos => ("osascript", vec!["-l", "JavaScript", "-e", MACOS_SCRIPT]),
        Os::Linux => {
            return Err(Error::State(
                t(
                    "GNOME has no tray area; Skadi shows a notification when a scheduled update finishes instead (skadi schedule enable).",
                    "GNOME'da tepsi alanı yok; bunun yerine zamanlanmış güncelleme bitince bildirim gösterilir (skadi schedule enable).",
                )
                .to_owned(),
            ));
        }
    };
    let path = find_program(program).ok_or(Error::ProgramNotFound("tray host"))?;
    Command::new(path)
        .args(&args)
        .env("SKADI_EXE", &exe)
        .env("SKADI_OPEN", t("Open Skadi", "Skadi'yi aç"))
        .env("SKADI_CHECK", t("Check for updates", "Güncellemeleri denetle"))
        .env("SKADI_QUIT", t("Quit", "Çık"))
        .env("SKADI_UPDATES", t("{0} updates are available.", "{0} güncelleme var."))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| Error::State(e.to_string()))?;
    println!(
        "{}",
        t(
            "Skadi is now in the tray and checks for updates every 6 hours.",
            "Skadi artık tepside; güncellemeleri 6 saatte bir denetler."
        )
    );
    Ok(())
}
