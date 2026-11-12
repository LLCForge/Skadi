# Changelog

All notable changes to Skadi, including the app and tweak catalogs, are listed here.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and versions follow
[Semantic Versioning](https://semver.org/). The catalogs are built into each release, so a catalog
change ships with the next version.

## [Unreleased]

### Catalog

- 256 apps across browsers, communication, development, multimedia, office, security, utilities,
  gaming, system and self-hosted, with package ids for 21 package managers.
- Telemetry labels with an official source for 92 apps, privacy notes and alternatives, community
  notes and derived tags.
- 99 tweaks: 60 for Windows, 31 for Linux, 8 for macOS, each with a documentation link.
- 7 presets: developer, gamer, office, privacy, and minimal, standard and advanced tweak levels.

### Added

- App installation and removal with winget, Microsoft Store, Chocolatey, Scoop, apt, dnf, yum,
  zypper, pacman, AUR helpers, apk, xbps, emerge, eopkg, urpmi, Flatpak, Snap, Nix, Homebrew and
  mas; missing managers are installed on request.
- Tweak engine with typed actions, preview, rollback on failure, undo and drift detection.
- Graphical and terminal interfaces in English and Turkish.
- Updates, pinning, scheduled updates, command history and an audit log.
- Security audit, privacy suggestions, app permission report, private DNS and network tools.
- Maintenance: cleanup, power plans, hardware and disk health report, disk usage, system repair,
  startup item control and a fresh install wizard.
- Release builds for Windows, Linux and macOS on x64 and ARM64 with checksums, an SBOM and signed
  build provenance, plus one-line launchers that verify downloads before running them.
- Portable mode and a bug report draft without personal data.
- Signed profiles, tweak packs, sandboxed WASM plugins and an organisation policy; encrypted state.
- Managed configuration files for automatic updates, SSH hardening, MAC address privacy and browser
  policies; startup, firewall, hosts, proxy, Wi-Fi, certificate and page file controls.
- Rollback of updates, offline bundles, AppImage with checksums, dotfiles and bookmark backups,
  remote apply over SSH, a tray icon and exports for Ansible, Intune, Nix, Homebrew and winget.
