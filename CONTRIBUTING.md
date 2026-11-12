# Contributing to Skadi

Thank you for helping. Skadi's first goals are privacy and security, so every change is reviewed
with those in mind. Please read [ARCHITECTURE.md](ARCHITECTURE.md) before larger changes and report
security problems privately as described in [SECURITY.md](SECURITY.md), not in public issues.

## Before you open a pull request

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --locked
cargo test --locked
```

All three must pass without warnings. Add a test for new logic (a parser, a branch, anything that
touches the system). Do not add dependencies for a few lines of code.

## Adding an app (`catalog/apps.toml`)

- Use the official package id for every manager you add. Check each one with a read-only query
  (`winget show --id`, `choco info`, `apt-cache policy`, `brew info`, `flatpak remote-info`, …)
  and say in the pull request how you checked it.
- `homepage` is the project's official site, over HTTPS.
- Set `foss` honestly. If the app sends usage data by default, set `telemetry = true` and suggest
  privacy-friendly `alternatives` from the catalog.
- Write `description` in English and Turkish (`{ en = "…", tr = "…" }`), one plain sentence each.
- Apps that cannot be installed side by side go in `conflicts` on both entries.

## Adding a tweak (`tweaks/*.toml`)

Review rules. A pull request that breaks one of them is not merged:

1. **Official source.** Every new tweak has a `source` link (HTTPS) to the vendor's documentation
   for the setting: Microsoft Learn, Apple support or developer docs, the distribution's or the
   project's own docs. When the vendor documents only the feature, link that page. When the vendor
   does not document it at all, link the reviewed WinUtil documentation for the same values (the
   project several Windows tweaks came from, credited in NOTICE). Blog posts and forum threads are
   not sources, and a tweak with no source of either kind is not added. A test enforces this.
2. **Typed actions only.** Use the existing action types (`registry`, `service`, `scheduled-task`,
   `appx`, `systemd`, `package`, `gsettings`, `defaults`, `config-line`, `managed-file`,
   `windows-feature`, `kconfig`). There is no action that runs arbitrary commands, and there will
   not be one.
3. **Reversible.** Skadi must be able to restore the previous state. Irreversible actions (such as
   removing a built-in app) are marked `risk = "advanced"` and say so in the description.
4. **Honest risk.** `safe` changes nothing a typical user notices. `moderate` has a side effect,
   which the description names ("Side effect: …"). `advanced` can break features some people need.
5. **Both languages.** `name` and `description` in English and Turkish, written for non-experts.
6. **Narrow scope.** Use `windows_min_build`, `requires_pro` or `distro_versions` when the setting
   only exists on some versions. A tweak must not change the same value as another tweak; the
   catalog check rejects conflicts.
7. **Privacy first.** Tweaks reduce data collection, ads and bloat, or improve security. Tweaks that
   weaken security (turning off updates, the firewall, Defender or UAC) are not accepted.

## Translations

User-facing text lives next to the code with `lang.pick("English", "Türkçe")` or in the catalogs as
`{ en, tr }`. Keep both languages in the same pull request.

## Code of conduct

This project follows the [Contributor Covenant](CODE_OF_CONDUCT.md).
