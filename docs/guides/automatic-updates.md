# Automatic security updates

[Türkçe](automatic-updates.tr.md)

Security fixes only help once they are installed. `skadi audit` shows whether automatic security
updates are on. Skadi does not switch them on by itself, because each distribution configures them
differently; these are the official ways.

## Debian and Ubuntu

```bash
sudo apt install unattended-upgrades
sudo dpkg-reconfigure --priority=low unattended-upgrades
```

Answer **Yes** to download and install stable updates automatically. Ubuntu usually has this on
already.

## Fedora

Fedora 41 and later (dnf5):

```bash
sudo dnf install dnf5-plugin-automatic
```

Set `apply_updates = yes` in `/etc/dnf/automatic.conf`, then:

```bash
sudo systemctl enable --now dnf5-automatic.timer
```

Older releases (dnf4) use the `dnf-automatic` package and
`sudo systemctl enable --now dnf-automatic-install.timer`.

## Arch Linux and other rolling releases

Arch recommends updating by hand and reading the news first, because some updates need manual
steps. Run `skadi update` regularly, or `skadi schedule enable` for a weekly update.

## Windows and macOS

- **Windows:** Windows Update installs security updates automatically. Do not turn it off; Skadi's
  update tweaks never stop security updates.
- **macOS:** System Settings > General > Software Update > Automatic updates: turn on
  **Install Security Responses and system files**.

For apps installed with Skadi, `skadi schedule enable` sets up a weekly `skadi update`.
