# Roadmap

Planned features, grouped into phases. Each phase builds on the previous one. Status:
**done**, **in progress**, **planned**, **deferred** (needs a design decision first).

Platforms: W = Windows, L = Linux, M = macOS, all = every platform.

## Phase 1: Foundation

| # | Feature | Platform | Status |
|---|---------|----------|--------|
| 1 | Presets: Developer, Gamer, Office, Privacy | all | done |
| 2 | Export and import selections as profile files | all | done |
| 3 | Scan for catalog apps that are already installed | all | done |
| 5 | Version pinning through each manager's native pin | all | done |
| 16 | Free disk space check before installing | all | done |
| 26 | Update everything with one command | all | done |
| 27 | Update preview (current and new version where the manager reports it) | all | done: Chocolatey, Homebrew, apt, dnf, pacman, Flatpak, Snap; winget's table is localized and not parsed |
| 28 | Exclude apps from updates (pins) | all | done |
| 60 | Undo every change Skadi made with one command | all | done |
| 102 | Timestamped audit log of every command Skadi ran | all | done |
| 130 | Export installed apps and reinstall them on a new machine | all | done |
| 131 | One profile file for apps and tweaks | all | done |
| 138 | Undo all changes of the last session | all | done |
| 154 | `--json` output for scripts | all | done |
| 155 | Shell completions: bash, zsh, fish, PowerShell | all | done |
| 156 | Plain output mode | all | done: output is plain text with no colors or decorations |
| 157 | Man page | L, M | done |
| 193 | Export a dry run as a PowerShell or shell script | all | done |

## Phase 2: Interfaces

| # | Feature | Platform | Status |
|---|---------|----------|--------|
| 142 | Keyboard-driven TUI (ratatui) | all | done |
| 143 | Native GUI (egui) | all | done |
| 144 | Search and filters: category, FOSS, installed; apps for other platforms are hidden | all | done |
| 145 | Locally cached app icons | all | done: `skadi icons fetch` downloads icons from each app's official site only when you ask; the GUI shows the local copies |
| 146 | Light and dark theme following the system | all | done |
| 147 | Progress bar and live log | all | done |
| 148 | Change preview before applying | all | done |
| 149 | Risk colour codes | all | done |
| 150 | Keyboard shortcuts | all | done |
| 151 | Share a selection as a link or QR code | all | done: `skadi share` and the GUI Share window print one-line install commands and a QR code |
| 158 | Safe cancellation of multi-step operations | all | done: stops after the current item |
| 159 | Screen reader support (AccessKit) | all | done: AccessKit enabled; not yet tested with a screen reader |
| 192 | Privilege separation: the UI runs as the user, a small helper runs elevated | L, M | done on Linux (pkexec) and macOS (administrator prompt); on Windows UAC cannot pipe output from an elevated child, so the whole app runs as administrator |

## Phase 3: Install and tweak engine

| # | Feature | Platform | Status |
|---|---------|----------|--------|
| 4 | Parallel downloads, sequential installs | all | done: package managers download in parallel (linux-pacman-parallel, linux-dnf-parallel tweaks) and Skadi installs one at a time |
| 6 | User-defined package manager preference | all | done: `prefer` in settings.toml |
| 7 | AUR helpers: paru, yay | L | done (`aur`); from the GUI it needs a terminal for paru's own sudo prompt |
| 8 | Scoop buckets: extras, games, nerd-fonts | W | done: added on demand, allowlisted |
| 9 | Flatpak user installs (`--user`) | L | done: `flatpak_user` in settings.toml |
| 10 | Homebrew taps | M | done: Homebrew taps automatically for `user/tap/formula` ids |
| 11 | Mac App Store (`mas`) | M | done |
| 12 | AppImage, only for apps that publish signatures or checksums | L | done: `skadi appimage install NAME URL --sha256 HASH` refuses a download whose checksum does not match; `skadi appimage remove` |
| 13 | Nix profile and home-manager export | L | done: `skadi profile convert nix` writes a home-manager `home.packages` list |
| 14 | Repology-based contributor tool for distribution package names | L | done: `.github/scripts/repology-suggest.py`, run weekly by the catalog suggestions workflow |
| 15 | Conflict warnings between apps | all | done: `conflicts` field, warning before install |
| 17 | FOSS, proprietary and telemetry labels | all | done: `foss` for every app; `telemetry` with an official `privacy_source` for 92 apps, unknown where no official statement exists |
| 18 | Per-app privacy notes and privacy-friendly alternatives | all | done: privacy notes and privacy-friendly alternatives for every app with confirmed telemetry, shown by `skadi show` and `skadi suggest` |
| 19 | Resume an interrupted install | all | done: `skadi install --resume` |
| 20 | Offline install bundles | W, M | done: `skadi bundle create/install` downloads packages with their dependencies and installs them without a network (apt, dnf, pacman, zypper) |
| 21 | Fonts: Nerd Fonts, Google Fonts | all | done: JetBrains Mono, Fira Code, Hack (plain and Nerd Font), Cascadia Code, Noto Sans, Roboto and Inter |
| 22 | Language version managers: Node, Python, Rust, Go | all | done: rustup, nvm, pyenv, mise (covers Go), fnm, uv |
| 23 | VS Code / VSCodium extension sets | all | done: `vscode_extensions` in presets and profiles |
| 24 | Browser extensions through policies | all | done: uBlock Origin Lite for Chrome, Chromium and Brave (Windows, Linux, macOS profile) and uBlock Origin for Firefox |
| 25 | Clean up leftover configuration and caches after removal | all | done: `skadi remove --purge` uses apt purge and Homebrew --zap; pacman already removes with -Rns |
| 29 | Scheduled automatic updates | all | done: Task Scheduler, systemd timer, launchd; only for Skadi installed in a protected location |
| 30 | Security-only Windows updates, defer feature updates | W | done: feature updates deferred 365 days (Pro and higher) |
| 31 | Exclude drivers from Windows Update | W | done |
| 32 | Reboot-required notice after kernel updates | L | done |
| 33 | macOS software update settings | M | done: automatic checks and security responses |
| 34 | Firmware updates with fwupd | L | done: `skadi update --firmware` |
| 35 | Update history and rollback | all | done: `skadi history`, and `skadi update --rollback` with versions recorded before each update (winget, Chocolatey, apt, dnf, yum, zypper, Snap) |
| 38 | Minimal, Standard and Advanced tweak profiles | all | done: `tweaks-minimal`, `tweaks-standard`, `tweaks-advanced` presets |
| 39 | Restore point before applying tweaks | W | done: `--snapshot`, on by default in the GUI |
| 40 | Btrfs or Timeshift snapshot | L | done: Timeshift, otherwise Snapper |
| 41 | Time Machine local snapshot | M | done |
| 42 | Configuration file action (e.g. `countme` in `dnf.conf`) | L | done: limited to files under /etc |
| 43 | KDE Plasma settings action | L | done: `kconfig` action, Baloo indexing tweak |
| 44 | Group Policy action | W | done: policies are applied as their registry values; `.pol` files are not edited |
| 45 | Windows optional features: Hyper-V, WSL, .NET 3.5 | W | done, plus Windows Sandbox |
| 46 | Reversible OneDrive removal | W | done: sync disabled by policy, the app stays installed |
| 47 | Default browser and app associations | W | done: `skadi default-browser` sets it on Linux; Windows and macOS do not let programs change it, so Skadi opens the right Settings page |
| 48 | Disable Widgets and News | W | done |
| 49 | Classic context menu | W | done |
| 50 | Clear Start menu pins | W | done: `skadi start-pins` sets the Configure Start Pins policy (Windows 11 Pro, Enterprise, Education) |
| 51 | Remove Snap and move Firefox to deb or Flatpak on Ubuntu | L | done: `skadi snap-to-flatpak` installs the Flatpak first, copies the Firefox profile, then removes the Snap; `--remove-snapd` when no Snap apps remain |
| 52 | GNOME settings and extensions | L | done: GNOME tweaks and `skadi gnome-extensions` |
| 53 | Detect unnecessary startup services | all | done: `skadi startup` marks items most people can turn off |
| 54 | Disable Spotlight and Siri suggestions | M | done: Spotlight internet suggestions off through `skadi macos-profile` |
| 55 | List and disable LaunchAgents and LaunchDaemons | M | done: `skadi startup disable/enable` uses launchctl for LaunchAgents and LaunchDaemons |
| 56 | A "why" explanation and official source link for every tweak | all | done: every tweak has a description with its side effects and a `source` link; a test keeps it that way |
| 57 | Detect tweaks that touch the same setting | all | done: conflicting values fail catalog validation |
| 58 | Filter tweaks by Windows version and edition | W | done: `requires_pro`, `windows_min_build` |
| 59 | Filter tweaks by distribution version | L | done: `distro_versions` |
| 61 | Drift detection when an update reverts a setting | all | done: shown in `skadi tweak list`, the GUI and the TUI |
| 62 | Signed community tweak packs | all | done: `skadi tweak list/apply/revert --pack FILE --public-key KEY` loads Ed25519-signed tweak packs; they pass the same checks as built-in tweaks and cannot reuse built-in ids |

## Phase 4: Privacy, security and network

| # | Feature | Platform | Status |
|---|---------|----------|--------|
| 63 | Privacy score of the current system | all | done: `skadi audit` privacy score |
| 64 | Reversible telemetry domain blocking via hosts or DNS | all | done: `skadi hosts` adds a reversible block list that never blocks system domains; DNS-level blocking with `skadi dns set adguard` |
| 65 | Audit apps with camera, microphone and location access | all | done: `skadi permissions` on Windows, Flatpak portal permissions on Linux and the TCC database on macOS |
| 66 | List TCC permissions | M | done: `skadi permissions` reads TCC after the terminal is given Full Disk Access |
| 67 | Browser hardening: Firefox `user.js`, Chromium policies | all | done: Firefox policies on Windows, Linux and macOS; Chromium-family policies on all three |
| 68 | Local account guide instead of a Microsoft account | W | done: `docs/guides/local-account.md` |
| 69 | Recall status and data audit | W | done: Recall snapshots off by policy (`win-ai`) |
| 70 | Disable the advertising ID everywhere | all | done: Windows (telemetry tweak) and macOS |
| 71 | Clear activity history and recent files | all | done: Windows and GNOME tweaks; macOS steps in `docs/guides/macos-privacy.md` |
| 72 | Clipboard history and access settings | all | done: clipboard history and cross-device sync off (Windows) |
| 73 | Turn off location services completely | all | done: Windows and GNOME tweaks; macOS has no supported command, steps in `docs/guides/macos-privacy.md` |
| 74 | Disable crash reporting in one profile: WER, Apport, CrashReporter | all | done: WER (Windows), Apport and ABRT (Linux), crash dialog (macOS) |
| 75 | Snapshot of which process connects where (from OS tools) | all | done: `skadi network connections` |
| 76 | List catalog apps known to send telemetry | all | done: `skadi list --tag telemetry` and `skadi suggest` |
| 77 | Remove EXIF and document metadata | all | done: `skadi clean-metadata` for JPEG and PNG |
| 78 | Secure free-space wiping | all | done: `skadi wipe-free` uses cipher /w on Windows and TRIM on Linux; macOS trims by itself |
| 79 | Wi-Fi MAC address randomization | all | done: linux-mac-randomization (NetworkManager); macOS uses private Wi-Fi addresses by default; Windows has no documented setting beyond the Settings switch |
| 80 | Disable LLMNR, NetBIOS and mDNS broadcasts | W, L | done: Windows (LLMNR, mDNS) and Linux (systemd-resolved, Avahi); NetBIOS planned |
| 81 | Stop sending the hostname over DHCP | L | done: linux-dhcp-hostname (NetworkManager) |
| 82 | Privacy-friendly alternative suggestions | all | done: `skadi suggest` |
| 83 | CIS-style security audit report | all | done: `skadi audit` |
| 84 | Firewall management: Defender Firewall, ufw/firewalld, pf | all | done: `skadi firewall on/status` for Windows Defender Firewall, ufw (SSH kept open), firewalld and the macOS firewall |
| 85 | Disk encryption status and guide: BitLocker, LUKS, FileVault | all | done: status in `skadi audit` and `docs/guides/disk-encryption.md` |
| 86 | Secure Boot and TPM checks | W, L | done: TPM in `skadi audit` (tpmtool on Windows, sysfs on Linux) with Secure Boot |
| 87 | Defender Attack Surface Reduction rules | W | done: four rules via policy |
| 88 | Disable SMBv1 and old TLS versions | W | done: SMBv1 server, TLS 1.0 and 1.1 |
| 89 | PowerShell execution policy and script logging | W | done: script block logging |
| 90 | SSH server hardening | L, M | done: linux-ssh-hardening and linux-ssh-keys-only, checked with `sshd -t` and refused when they could lock you out |
| 91 | Automatic security updates: unattended-upgrades, dnf-automatic | L | done: linux-auto-updates-apt, -dnf5 and -dnf4 tweaks; status in `skadi audit` |
| 92 | Gatekeeper and SIP status | M | done: `skadi audit` |
| 93 | Audit administrator and passwordless accounts | all | done: `skadi audit` checks the built-in Administrator, empty passwords, extra UID 0 accounts and passwordless sudo/doas |
| 94 | Screen lock timeout and password requirement | all | done: Windows and GNOME tweaks; macOS through `skadi macos-profile` |
| 95 | Disable USB autorun | W | done |
| 96 | Office macro security | W, M | done: Word, Excel and PowerPoint |
| 97 | Install and run ClamAV and rkhunter | L | done: `skadi scan` (Microsoft Defender, ClamAV, `--rootkit` for rkhunter) |
| 98 | Open ports and listening services report | all | done: `skadi network ports` |
| 99 | sudo/doas configuration audit | L, M | done: `skadi audit` flags passwordless sudo/doas rules |
| 100 | AppArmor and SELinux status | L | done: `skadi audit` |
| 101 | Suspicious root certificate audit | all | done: `skadi certs` compares Windows roots with Mozilla's list and lists administrator-added roots on Linux and macOS |
| 103 | DNS provider choice: Quad9, Cloudflare, Mullvad, AdGuard | all | done: `skadi dns set` with Quad9, Cloudflare, Mullvad, AdGuard |
| 104 | DNS over HTTPS and DNS over TLS | all | done: DoH on Windows 11, DoT with systemd-resolved; plain DNS on macOS |
| 105 | DNS speed test | all | done: `skadi dns test` |
| 106 | IPv6 preferences | all | done: linux-ipv6-privacy; Windows uses temporary addresses by default and win-teredo turns off tunnels |
| 107 | Proxy settings | all | done: `skadi network proxy` shows, sets and resets the system proxy (no passwords stored) |
| 108 | Reversible hosts file manager with block lists | all | done: `skadi hosts block/unblock/status` |
| 109 | Network reset | W | done: Windows (Winsock, TCP/IP, DNS cache) and Linux (NetworkManager) |
| 110 | Export and import Wi-Fi profiles | all | done: `skadi wifi export/import` (passwords only with `--with-passwords`) and `docs/guides/wifi-profiles.md` |
| 111 | VPN setup and kill switch guide | all | done: `docs/guides/vpn.md` |
| 112 | Disable Teredo and legacy tunnels | W | done: Teredo, 6to4 and ISATAP |
| 113 | NTP server choice | all | done: Cloudflare time server on Windows and Linux |
| 114 | Connection diagnostics: ping, traceroute, DNS | all | done: `skadi network diagnose` |

## Phase 5: Maintenance and backup

| # | Feature | Platform | Status |
|---|---------|----------|--------|
| 115 | Temporary file and cache cleanup | all | done: `skadi clean` removes temporary files older than 7 days (symlinks not followed, files in use skipped) |
| 116 | Package manager cache cleanup | all | done: `skadi clean` uses each manager's own cache command |
| 117 | Remove unused dependencies | L, M | done: apt, dnf, yum, xbps, eopkg, flatpak, brew autoremove in `skadi clean` |
| 118 | Startup apps manager | all | done: `skadi startup disable/enable` on Windows, Linux autostart and systemd (core services protected), and macOS launchd |
| 119 | Power plans, including Ultimate Performance | W | done: `skadi power` lists and switches plans, creates Ultimate Performance on demand |
| 120 | CPU governor and power profiles: TLP, power-profiles-daemon | L | done: `skadi power` with power-profiles-daemon or TLP |
| 121 | SMART disk health | all | done: in `skadi report` (Windows Get-PhysicalDisk, Linux smartctl, macOS diskutil) |
| 122 | Disk usage analysis | all | done: `skadi du` |
| 123 | Swap and page file settings | all | done: `skadi pagefile` (usage, `--auto` on Windows) and linux-swappiness |
| 124 | Game mode and GPU scheduling | W | done: win-game-mode and win-gpu-scheduling tweaks; GameMode app for Linux |
| 125 | Reduce visual effects | W, M | done: win-visual-effects and mac-reduce-animations tweaks |
| 126 | Search indexing control: Windows Search, Tracker, Spotlight | all | done: win-search-indexing, linux-gnome-tracker and `skadi spotlight` |
| 127 | System repair with `sfc` and `DISM` | W | done: `skadi repair` runs DISM and sfc |
| 128 | Broken package repair | L | done: `skadi repair` fixes apt and zypper; dnf and pacman have no official repair, so Skadi runs their checks |
| 129 | Hardware and system report | all | done: `skadi report` (no computer or user name, safe to share) |
| 132 | Cross-platform app mapping | all | done: one catalog id maps to every OS; profiles, exports and `skadi share` carry across systems |
| 133 | Dotfiles backup and restore | L, M | done: `skadi dotfiles backup/restore` leaves out SSH and GPG keys, cloud credentials and files containing tokens or passwords |
| 134 | Browser profile backup | all | done: `skadi bookmarks` backs up bookmarks only, never cookies, history or saved passwords |
| 135 | Keep profiles in a Git repository | all | done: `skadi profile git DIR` exports the profile and commits it with your own Git identity |
| 136 | Verify profiles from others with signatures | all | done: `skadi profile apply FILE --public-key KEY` refuses a profile whose signature does not match; `skadi sign keygen/file/verify` |
| 137 | Change history timeline | all | done: `skadi history --timeline` |
| 139 | Registry backup | W | done: `skadi tweak backup FILE.reg` exports every registry key the tweaks on this PC touch |
| 140 | Fresh install wizard | all | done: `skadi setup` |
| 141 | Encrypted state file | all | done: `[state] encrypt = true` encrypts the state file with ChaCha20-Poly1305; the key lives in DPAPI, the macOS Keychain or the Secret Service |

## Phase 6: Distribution, enterprise and community

| # | Feature | Platform | Status |
|---|---------|----------|--------|
| 152 | Tray and menu bar icon with update notices | all | done: `skadi tray` (Windows tray, macOS menu bar) checks for updates every 6 hours; GNOME has no tray, so Linux uses notifications |
| 153 | Notifications | all | done: notification when a scheduled update finishes on Windows, macOS and Linux (sent to every open desktop session) |
| 160 | One-line launchers: `install.ps1`, `install.sh` | all | partial: `install.ps1` and `install.sh` verify SHA-256 and, with gh, the signed provenance; first real run needs a tagged release |
| 161 | Reproducible builds | all | partial: pinned toolchain, `--locked`, fixed timestamp and path remapping in the release workflow; not yet compared across two builds |
| 162 | SBOM and SLSA provenance | all | partial: CycloneDX SBOM and Sigstore provenance in the release workflow; first run needs a tagged release |
| 163 | ARM64: Windows on ARM, Apple Silicon, aarch64 Linux | all | partial: release workflow builds x64 and ARM64 for Windows, Linux and macOS; first run needs a tagged release |
| 164 | Portable mode | all | done: a `skadi.portable` file next to the program keeps all data in `skadi-data` beside it |
| 172 | Unattended installs from an answer file | all | done: a profile file with `skadi profile apply <file> --yes` |
| 173 | Apply to several machines over SSH | L, M | done: `skadi remote apply TARGET --host user@host` uses your own SSH setup; Skadi stores no keys or passwords |
| 174 | Export for Ansible, Intune and MDM | all | done: `skadi profile convert ansible|brewfile|winget|nix|intune` and `skadi macos-profile` for MDM |
| 175 | Central policy of allowed apps and tweaks | all | done: `skadi policy install/show/remove`; a signed policy limits which apps and tweaks can be used, and an invalid signature blocks everything |
| 176 | HTML/PDF compliance report | all | done: `skadi audit --html` (print to PDF from the browser) |
| 177 | Corporate proxy and certificates | all | done: proxy and extra CA certificate variables are kept under sudo without logging their values |
| 178 | Install from a local mirror | all | done: `skadi source list/add` for winget, Chocolatey, Scoop and Flatpak (HTTPS only, with a warning) |
| 179 | Docker and Dev Container images | L | done: dev container for contributors and a Dockerfile for the command line (catalog tools, conversions, dry runs) |
| 180 | cloud-init and first-boot scripts | L | done: `examples/cloud-init.yaml` |
| 181 | Apply tweaks to all users | all | done: `skadi tweak default-user` writes per-user tweaks into the Windows default profile for every new account; machine-wide tweaks already apply to everyone |
| 182 | Catalog contribution wizard (`skadi catalog add`) | all | done: `skadi catalog add` |
| 183 | CI that installs only the apps changed in a pull request | all | done: catalog pull requests test only new or changed apps |
| 184 | Bot that suggests missing Linux and macOS ids | L, M | done: weekly catalog suggestions workflow keeps one issue updated; nothing is added automatically |
| 185 | Community notes per app | all | done: `catalog/notes.toml` with sourced notes, shown by `skadi show`; added through pull requests, no server |
| 186 | One-command bug report draft without personal data | all | done: `skadi bug-report` (home folder and user name removed) |
| 187 | Catalog versioning and changelog | all | done: `catalog/VERSION` shown by `skadi info`, and a Catalog section in CHANGELOG.md |
| 188 | App screenshots | all | done: `skadi show` links each app's screenshots (Flathub page, or the official site) |
| 189 | Rich tags | all | done: tags derived from the catalog (open-source, telemetry, platforms, flatpak) and `skadi list --tag` |
| 190 | Review rules and mandatory official sources for tweaks | all | done: CONTRIBUTING.md review rules and a test that requires a source for every tweak |
| 191 | Sandboxed WASM plugins | all | done: signed `.wasm` plugins run in wasmi with no host functions, a fuel and memory limit, and may only return a tweak pack |

## Final step

Written only after every planned feature above is done.

| Feature | Status |
|---------|--------|
| Screenshots of the GUI and TUI | done: `docs/images`, produced with the hidden `--screenshot` option (no installed apps shown) |
| README in English and Turkish, logo at the top | done: README.md and README.tr.md |
