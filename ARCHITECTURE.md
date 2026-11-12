# Architecture

Skadi is a cross-platform app installer and system debloater for Windows, Linux and macOS,
in the spirit of Ninite and WinUtil. It is written in Rust and ships as a single binary with no
runtime dependencies.

This document explains how the code is organized and how to extend it. Read it before your first
contribution.

## Goals

1. **Privacy and security first.** Nothing is collected and nothing is sent anywhere. Every
   system change is explicit and visible to the user.
2. **Small and fast.** A single static binary that starts instantly.
3. **Easy to extend.** Most contributions (new apps, new languages) are data, not code.
4. **One core, many front ends.** The CLI, TUI and GUI are thin layers over the same core.

## Layout

```
Skadi/
├── catalog/apps.toml          App catalog (data, embedded at compile time)
├── catalog/presets.toml       Built-in presets: app sets and tweak sets
├── tweaks/                    Debloat and privacy tweaks (data, embedded at compile time)
│   ├── windows.toml
│   ├── linux.toml
│   └── macos.toml
├── crates/
│   ├── skadi-core/            Library: all logic, no UI
│   │   └── src/
│   │       ├── platform.rs    OS and Linux distribution detection
│   │       ├── exec.rs        Safe command execution and privilege handling
│   │       ├── manager.rs     Package manager definitions (the SPECS table)
│   │       ├── catalog.rs     Catalog schema, loading and validation
│   │       ├── plan.rs        Pure planner: apps -> steps per package manager
│   │       ├── maintain.rs    Installed scan, update all, update preview, native pins
│   │       ├── profile.rs     Profile files and presets
│   │       ├── paths.rs       Skadi's data directory (state, audit log)
│   │       ├── system.rs      The only layer that touches the system
│   │       ├── tweak/         Debloat engine
│   │       │   ├── mod.rs     Tweak schema, typed actions, catalog validation
│   │       │   ├── action.rs  Actions -> commands, output parsers (pure)
│   │       │   ├── engine.rs  Observe, apply (with rollback), revert
│   │       │   └── state.rs   Saved previous values (the undo log)
│   │       ├── i18n.rs        Languages (English, Turkish)
│   │       └── error.rs       Error type with localized messages
│   └── skadi/                 Binary: the `skadi` command
│       └── src/
│           ├── main.rs        Argument parsing and localized help
│           ├── commands.rs    info / list / install / remove / bootstrap
│           ├── maintain.rs    installed / update / pin / unpin
│           ├── profiles.rs    profile list / apply / export
│           ├── tweaks.rs      tweak list / apply / revert
│           ├── output.rs      --json and --script output
│           └── ui/            Shared UI model and job runner
│               ├── tui.rs     Terminal interface (ratatui)
│               └── gui.rs     Graphical interface (egui)
└── assets/                    Logos and icons
```

## Layers

```
 ┌──────────────┬──────────────┬──────────────┐
 │     CLI      │  TUI (next)  │  GUI (next)  │   front ends: parse input, show plans, ask for consent
 └──────┬───────┴──────┬───────┴──────┬───────┘
        └──────────────┼──────────────┘
                ┌──────▼──────┐
                │    plan     │   pure: catalog + available managers -> steps
                └──────┬──────┘
                ┌──────▼──────┐
                │   system    │   detect managers, prune, build invocations, bootstrap
                └──────┬──────┘
                ┌──────▼──────┐
                │    exec     │   absolute paths, argument vectors, sudo/doas wrapping
                └─────────────┘
```

Rules:

- Front ends never run commands themselves. They call `skadi-core`.
- `plan` has no side effects and is fully unit-tested.
- Only `system` and `exec` touch the operating system.
- `skadi-core` contains no UI code, so every front end gets the same behavior.

## Data flow: `skadi install firefox git`

1. `System::detect()` detects the OS, reads `/etc/os-release` on Linux, checks privileges and
   finds the installed package managers. It moves the distribution's own manager to the front.
2. `Catalog::builtin()` loads the embedded catalog and validates every entry.
3. `plan()` assigns each app to the first available manager that has a package for it, and
   groups the apps per manager. Apps that no installed manager can provide are reported as
   *unresolved*, together with the managers that could provide them.
4. For unresolved apps, the front end offers to **bootstrap** the missing manager (for example
   winget or Chocolatey on Windows, Homebrew on macOS, Flatpak or Snap on Linux). It shows the
   official installer and runs it only after the user confirms.
5. `System::prune()` skips apps that are already installed, when the manager can report that.
6. `System::invocations()` builds the final commands, and the front end runs them.

## Debloat engine

A tweak is a named group of **typed actions**. There is deliberately no "run this script"
action: every action can be read, validated, previewed and undone.

| Action | OS | Applies | Reverts to |
|--------|----|---------|------------|
| `registry` | Windows | `reg add` (HKLM, HKCU, HKU); an empty name is the key's default value | the saved value, or deletes it; keys Skadi created are removed if nothing else is left in them |
| `windows-feature` | Windows | `Enable-WindowsOptionalFeature` | `Disable-WindowsOptionalFeature` if it was off |
| `service` | Windows | service `Start` value | the saved start type |
| `scheduled-task` | Windows | `Disable-ScheduledTask` | `Enable-ScheduledTask` if it was enabled |
| `appx` | Windows | removes the app for all users | **cannot be undone** (flagged in the UI) |
| `systemd` | Linux | `systemctl disable --now` or `mask --now` | `unmask` / `enable --now` as needed |
| `package` | Linux | removes a package with the system manager | reinstalls it |
| `gsettings` | Linux (GNOME) | `gsettings set` | the saved value |
| `kconfig` | Linux (KDE) | `kwriteconfig6` (or 5) | the saved value, or `--delete` |
| `config-line` | Linux | sets `key=value` in a section of a file under `/etc`, through `skadi internal config-set` run with root | puts the original line back, or removes it |
| `defaults` | macOS | `defaults write` (user, or system plist with root) | the saved value, or deletes it |

### Lifecycle

1. **Observe.** For each action the engine reads the current state and returns
   `Unavailable` (the service, task, unit or setting does not exist here), `Done` (already in
   the target state) or `Pending(previous)`. Status is derived from this: applied, not applied,
   partial or n/a.
2. **Apply.** Pending actions run one by one. If one fails, every action already applied in
   this tweak is rolled back in reverse order. The previous values of the successful actions
   are written to the state store.
3. **Revert.** Uses only the state store. Each record is matched to its action by a stable key
   (`registry:HKCU\...\Name`), so reordering the catalog does not break undo. Applying a tweak
   twice keeps the **first** saved value, so undo always returns to the original state.

### Safety rules

- Registry values are read in process (`windows-registry`). A value of an unexpected type is
  never touched; otherwise undo could delete data it misread as missing.
- Values loaded from the state file are re-validated before use, since the file is user-writable.
- The state file is written atomically (temp file + rename), with mode `0600` on Unix.
  Location: `%LOCALAPPDATA%\Skadi`, `~/Library/Application Support/Skadi`,
  `$XDG_STATE_HOME/skadi` (or `~/.local/state/skadi`).
- Tweak commands run quietly; on failure the last line of their output is attached to the error.
- A created registry key is only deleted when it is a prefix of the action's own key, at least
  three levels below the hive (four under `HKU`), not a shared key such as `Policies` or
  `Classes`, and empty of any other value.
- Two tweaks that write different values to the same setting fail catalog validation.
- `--snapshot` creates a restore point (Windows), a Timeshift or Snapper snapshot (Linux) or a
  Time Machine local snapshot (macOS) first; if that fails the user decides whether to go on.

### Performance

Status checks must stay fast. Registry reads are in process, installed AppX packages are read
with one PowerShell call, and scheduled tasks are prefetched in one call through the Task
Scheduler COM API instead of listing every task.

## Interfaces

`skadi` with no subcommand opens the GUI when a display is available and the TUI otherwise
(for example over SSH). `skadi gui` and `skadi tui` choose explicitly. Both are optional
features of the binary (`tui`, `gui`, on by default).

Neither interface runs system commands itself. Every operation starts the `skadi` CLI as a
child process and passes it **only catalog ids**; the CLI rebuilds the commands from the
embedded catalog. A compromised or buggy UI therefore cannot ask the elevated side to run
arbitrary commands.

| Platform | How elevated work runs |
|----------|------------------------|
| Linux | The GUI runs as the user. Work that needs root runs in one child started with `pkexec`, so the system password dialog appears once per batch. User-level work (GNOME settings, Flatpak) stays unprivileged. |
| macOS | The GUI runs as the user (Homebrew must not run as root). System plist tweaks run in one child through the administrator password prompt. |
| Windows | UAC cannot pipe output from an elevated child back to an unelevated window, so the whole app runs as administrator, as WinUtil does. The GUI offers "Restart as administrator". |

The TUI hands the terminal to the CLI for each operation, so the CLI shows its plan, asks for
confirmation and lets `sudo` prompt for a password, then the TUI resumes.

Unprivileged work runs as one child per item, which gives a real progress bar and makes
cancellation safe: cancelling stops before the next item and never interrupts an install.
Elevated work is batched into one child so the password is asked once.

## Privacy and security tools

| Command | What it does |
|---------|--------------|
| `skadi audit` | Read-only security checks (disk encryption, Secure Boot, firewall, UAC/SIP/Gatekeeper, AppArmor/SELinux, SSH, automatic updates) and a privacy score from the privacy tweaks. A check that cannot be read without elevation is reported as unknown, never as failed. |
| `skadi clean-metadata` | Removes EXIF/GPS, XMP, IPTC and comments from JPEG and text, EXIF and time chunks from PNG, in pure Rust. Pixel data and colour profiles are kept; by default a `.clean` copy is written next to the original. |
| `skadi suggest` | Lists installed apps with known telemetry and their privacy-friendly alternatives from the catalog. |
| `skadi startup` | Lists programs that start automatically and turns them off or on without deleting them: the `StartupApproved` value on Windows (the same one Task Manager uses) and a user copy with `Hidden=true` for Linux autostart entries. Only names from the current list are accepted. |
| `skadi clean` | Each package manager's own cache and autoremove command (no pipes, nothing that removes rollback such as `nix-collect-garbage -d`), plus temporary files older than 7 days. Symlinks and junctions are not followed; under sudo shared temp folders are skipped. |
| `skadi power` | Windows power plans by alias or GUID (`powercfg`), Linux profiles through power-profiles-daemon. Values come from an allowlist. |
| `skadi report` / `skadi du` | Read-only hardware report with disk health, and folder sizes. The report leaves out computer and user names. |
| `skadi repair` | DISM and sfc on Windows, `dpkg --configure -a` and `apt-get install -f` on apt systems, `diskutil verifyVolume` on macOS. |
| `skadi setup` | Fresh install wizard: an app preset, a tweak level and a DNS provider. Each step shows its own plan and asks again. |
| `skadi permissions` | Windows apps with camera, microphone or location access and when they last used it. |
| `skadi dns test/status/set/reset` | Measures Quad9, Cloudflare, Mullvad and AdGuard with a raw DNS query, and switches DNS: DoH on Windows 11 (no silent fallback to plain DNS), DoT through systemd-resolved on Linux, plain DNS on macOS. Commands are built only from the built-in provider table. |
| `skadi network proxy` | Proxy settings from the environment, Windows (WinINET), GNOME and macOS, with user names and passwords masked. Read-only. |
| `skadi network ports/connections/reset/diagnose` | Listening sockets and open connections with their programs, a network stack reset, and basic connectivity checks. |

## Settings

`settings.toml` in the data directory holds user preferences:

```toml
[install]
prefer = ["flatpak", "apt"]   # try these package managers first
flatpak_user = true           # install Flatpaks for the user, without root
```

Unknown fields and unknown manager ids are rejected with a clear error instead of being ignored.

## Scheduled updates

`skadi schedule enable` creates a weekly Task Scheduler task (Windows), a systemd timer (Linux,
through `skadi internal schedule-units` run as root) or a user LaunchAgent (macOS). Because the
Windows and Linux jobs run Skadi with administrator rights, they are only created when the Skadi
binary is in a protected location (Program Files, or a root-owned directory such as
`/usr/local/bin`); otherwise any program running as the user could replace the binary and gain
administrator rights at the next run.

## Maintenance, profiles and audit

- **Installed scan.** Each manager lists what it has installed with one command (`winget export`,
  `brew list`, `flatpak list`, `dpkg-query`, `rpm -qa`, ...). Managers are queried in parallel.
- **Updates.** `skadi update` runs each manager's own "update everything" command;
  `skadi update --check` shows pending updates where the manager reports them in a parseable form.
- **Pins.** `skadi pin` uses the manager's native mechanism (`winget pin`, `choco pin`,
  `apt-mark hold`, `brew pin`, `flatpak mask`, `snap refresh --hold`, `zypper addlock`), so
  updates run outside Skadi respect them too.
- **Profiles.** A profile is a TOML list of app and tweak ids, nothing else, so a profile from
  someone else can only select actions the catalog already defines. Presets in
  `catalog/presets.toml` are built-in profiles; ids for other systems are skipped.
- **Audit log.** Every command Skadi runs is appended to `audit.log` in the data directory as one
  JSON line (time, program, arguments, exit code). It is best-effort: a write failure never blocks
  the user's operation.
- **Scripts.** `--script file.ps1|file.sh` writes the planned commands, fully quoted, instead of
  running them, so they can be reviewed or run elsewhere.

## Package managers

| OS      | Managers |
|---------|----------|
| Windows | winget, Microsoft Store (through winget), Chocolatey, Scoop |
| Linux   | apt, dnf, yum, zypper, pacman, AUR (paru or yay), apk, xbps, emerge, eopkg, urpmi, Flatpak, Snap, Nix |
| macOS   | Homebrew Cask, Homebrew, Mac App Store (mas) |

Each manager is a single `ManagerSpec` entry in `manager.rs` that describes:

- its program name, plus fallback paths for when it is not on `PATH` yet (for example right
  after installing it),
- the install and remove command templates,
- an optional "is it installed?" query and how to read its result,
- the privilege it needs (`Any`, `Root` or `NonRoot`),
- whether it accepts several packages in one command,
- environment variables (for example `HOMEBREW_NO_ANALYTICS=1`),
- how to bootstrap it if it is missing.

The order of the `SPECS` table is also the preference order.

### Privileges

| Privilege | Not elevated | Elevated |
|-----------|--------------|----------|
| `Any`     | run directly | run directly |
| `Root`    | Unix: wrap with `sudo`/`doas`. Windows: error asking the user to run as administrator | run directly |
| `NonRoot` | run directly | Unix: drop to the invoking user with `sudo -u`, or refuse |

Because `sudo` resets the environment, the required variables are passed explicitly through
`env`.

## Extending Skadi

### Add an app

Add an `[[app]]` entry to `catalog/apps.toml`. No Rust is needed. Only list package ids you
have verified on that manager. `cargo test` validates the whole catalog.

All 240 apps install on Windows; 106 also have macOS (Homebrew) ids and 74 have Linux ids,
mostly Flatpak. Every winget, Microsoft Store and Chocolatey id was checked against its
repository, and every Flatpak and Homebrew id against the Flathub and Homebrew indexes, without
installing anything. Linux and macOS ids were only added when both the app name and the
homepage matched a single candidate. Adding verified ids for the remaining apps, especially
distribution packages, is the most valuable catalog contribution.

The **Install test** workflow (`.github/workflows/install-test.yml`) installs and removes every
app with Skadi on fresh GitHub-hosted Windows, macOS and Linux runners, split into 8 shards per
OS, and writes a pass/fail table to the run summary. It runs weekly and can be started by hand
from the Actions tab with a space-separated list of app ids, which is the way to test a catalog
change before merging it. `skadi list --ids` prints the apps installable on the current system.

### Add a package manager

1. Add a variant to `ManagerId`, plus its entry in `ManagerId::ALL` and `as_str`.
2. Add a `ManagerSpec` to `SPECS` at the **same position**. A test enforces the order.
3. If it is a distribution's native manager, add the distribution family to `DISTRO_MANAGERS`
   in `system.rs`.

### Add a tweak

Add a `[[tweak]]` with one or more `[[tweak.action]]` entries to the right file in `tweaks/`.
Use `distros = ["ubuntu"]` to limit a Linux tweak to a distribution family. `distro_versions`, `requires_pro` (hidden on Windows Home) and `windows_min_build` (for example 22000 for Windows 11) narrow it further. Give it an honest
`risk` (`safe`, `moderate`, `advanced`) and describe side effects in both languages.
`cargo test` validates every tweak, and
`cargo test -p skadi-core --test windows_tweak_roundtrip -- --ignored` runs a real
apply/revert cycle on a scratch registry key on Windows.

### Add an action type

1. Add a variant to `Action` in `tweak/mod.rs`, with validation in `Action::validate`.
2. Add how to observe it (`engine.rs`), how to apply and restore it (`action.rs`), and a
   `Previous` variant for its saved state.
3. Add tests for the parser and for the restore commands.

### Add a language

1. Add a variant to `Lang` and a field to `Text` in `i18n.rs`.
2. The compiler then points out every `match` and catalog entry that needs a translation.

Catalog descriptions require every language, so a missing translation fails the catalog test.

## Guides

`docs/guides/` holds English and Turkish guides for steps Skadi deliberately leaves to the user
because a mistake can lose data or expose secrets: local accounts, disk encryption, automatic
security updates, VPN kill switches and moving Wi-Fi profiles.

## Distribution

- **Release workflow** (`.github/workflows/release.yml`): a version tag builds Windows, Linux and
  macOS on x64 and ARM64 with the pinned toolchain in `rust-toolchain.toml`, `--locked`, a fixed
  `SOURCE_DATE_EPOCH` and remapped paths, then publishes the raw binaries with `SHA256SUMS`, a
  CycloneDX SBOM and Sigstore-signed SLSA provenance. Publishing uses the preinstalled `gh` CLI
  rather than a third-party action. Dependabot keeps actions and crates up to date.
- **Launchers** (`install.ps1`, `install.sh`): download one binary into a private temporary folder,
  refuse to run it if the SHA-256 does not match, verify the provenance when `gh` is available,
  run it and delete it. `SKADI_VERSION` pins a release. CI checks both scripts.
- **Portable mode**: a `skadi.portable` file next to the program moves the data directory to
  `skadi-data` beside it (`paths::portable_dir`).
- **Exports** (`export.rs`): `skadi profile convert` writes a profile's apps as an Ansible
  playbook, a Brewfile or a `winget import` file. Only catalog package ids are written, each as a
  JSON-quoted string. Tweaks are not converted, because only Skadi can apply them reversibly.
- **Reports**: `skadi audit --html` writes a single-file report with a strict CSP and no scripts;
  `skadi bug-report` prints an issue draft with the home folder and user name removed.
- **Proxies**: when elevating with sudo, proxy variables are kept with `--preserve-env=` by name,
  so their values (which may hold passwords) never reach the command line or the audit log.
- **Notifications**: `skadi update --notify` (used by the scheduled update on Windows and macOS)
  shows a desktop notification when it finishes; failures to notify are ignored.
- **Unattended setup**: a profile file plus `--yes`; see `examples/cloud-init.yaml`.
- **Catalog help**: `skadi catalog add` prepares a validated entry for a pull request, and the
  install test runs only the new or changed apps of a catalog pull request.
- **Contributions**: CONTRIBUTING.md holds the review rules; a test requires a `source` link for
  every new tweak, and the count of older tweaks without one can only go down.

## Trust: signatures, policies and plugins

- **Signatures** (`signing.rs`): Ed25519 with strict verification. `skadi sign keygen` writes a
  secret key (owner-only permissions) and a public key; `skadi sign file` writes `FILE.sig`. Signed
  content is read once, verified, and the same bytes are used, so it cannot change in between.
- **Signed profiles and tweak packs**: `--public-key` on `profile apply`, and `--pack FILE
  --public-key KEY` on `tweak list/apply/revert`. A pack is parsed by the same `TweakCatalog`
  validation as the built-in catalog and may not reuse a built-in id.
- **Plugins** (`plugin.rs`): a signed `.wasm` file runs in wasmi with no imports at all (a module
  that imports anything is rejected), a fuel limit, a 16 MiB memory limit and a 512 KiB output limit.
  Its only output is the text of a tweak pack, which then goes through the pack checks.
- **Organisation policy** (`policy.rs`): `skadi policy install` copies a signed `policy.toml` with
  its public key into a machine-wide folder (administrator only). Installs and tweak runs check it;
  if the signature cannot be verified, Skadi refuses everything rather than ignoring the policy.
- **Encrypted state** (`vault.rs`): with `[state] encrypt = true` the tweak state file is sealed with
  ChaCha20-Poly1305. The key is kept by the operating system (DPAPI, Keychain, Secret Service) and
  passed to those tools on standard input where they allow it. Older plain files still load.

## Managed files and system controls

- **`managed-file` action**: writes a whole file that Skadi owns, only inside allowlisted `/etc`
  folders (`apt.conf.d`, `dnf/automatic.conf.d`, `sysctl.d`, `NetworkManager/conf.d`,
  `sshd_config.d`, browser policy folders) and only with names containing `skadi` (plus Firefox's
  fixed `policies.json`, which is never overwritten if it holds someone else's policies). Content
  comes from the embedded catalog. After writing, `sysctl --system`, `nmcli general reload conf` or
  `sshd -t` plus a reload run; if `sshd -t` or `sysctl` fails the previous file is restored. Guards
  refuse SSH changes that could lock the user out.
- **`systemd` action** can now also enable a unit (automatic update timers).
- **Startup control** covers Windows StartupApproved, Linux autostart, systemd (core units are
  protected) and macOS launchd; **firewall**, **hosts block list**, **proxy**, **Wi-Fi profiles**,
  **certificates**, **page file**, **Start pins**, **default browser**, **Spotlight** and **GNOME
  extensions** have their own commands in `controls.rs` and `desktop.rs`.
- **Large-stack worker**: the command tree and dispatcher run on a 32 MiB-stack thread; the GUI
  stays on the main thread, as macOS requires.

## Security model

See [SECURITY.md](SECURITY.md) for the full policy. In short:

- No shell is ever used. Programs are resolved to absolute paths, and relative `PATH` entries
  are ignored.
- Package ids are validated against an allowlist and may not start with `-`.
- The catalog is embedded at compile time and cannot be tampered with at runtime.
- Commands are elevated only when needed, and status queries never are.
- Remote bootstrap installers are shown to the user and require confirmation.

## Roadmap

| Area | Plan |
|------|------|
| Tweaks | More actions (configuration files such as `dnf.conf`, KDE settings), presets, and a Windows restore point before applying. |
| TUI | A `ratatui` front end for terminals and headless servers. |
| GUI | A native windowed front end using the assets in `assets/`. |
| One-line launch | `install.ps1` and `install.sh` that download the right release binary for the OS and architecture and verify its SHA-256 checksum and signature before running it. |
| Releases | CI-built, signed binaries for x86_64 and aarch64 on all three operating systems. |
| More managers | AUR helpers (paru, yay), Scoop buckets, Flatpak user installs. |
