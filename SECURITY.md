# Security Policy

Skadi installs software and changes system settings, often with administrator rights. We treat
every security report as a priority.

## Supported versions

Only the latest release receives security fixes. Please update before reporting.

## Reporting a vulnerability

**Do not open a public issue for security problems.**

Report privately through GitHub:
[Report a vulnerability](https://github.com/LLCForge/Skadi/security/advisories/new)

Please include:

- the affected version and operating system (and distribution on Linux),
- the steps to reproduce,
- the impact you observed or expect.

What to expect:

| Step | Target time |
|------|-------------|
| Acknowledgement | within 3 days |
| Initial assessment | within 7 days |
| Fix or mitigation for confirmed issues | as fast as severity requires, usually within 30 days |

We credit reporters in the release notes unless you prefer to stay anonymous.

## In scope

- Command or argument injection through the catalog, package ids or environment
- Privilege escalation, or running a tool with more privileges than it needs
- Tampering with, or bypassing verification of, Skadi release binaries
- Any data collection or network traffic that Skadi itself initiates without the user's consent

## Out of scope

- Vulnerabilities in the third-party package managers or the apps they install; report those
  upstream
- Issues that require an attacker who already has administrator or root access

## Security design

- **No telemetry.** Skadi collects nothing and does not contact any server of its own. It also
  turns off Homebrew analytics when it runs Homebrew.
- **No shell.** Every command runs as an absolute program path with an explicit argument list.
  No command is ever built as a shell string.
- **Validated input.** Package ids are checked against a strict allowlist, and an id can never
  start with `-`, so it cannot be read as an option.
- **Embedded catalog.** The app catalog is compiled into the binary and cannot be changed at
  runtime.
- **Least privilege.** Commands are elevated only when the package manager requires it. Status
  queries never run elevated, and Homebrew is never run as root.
- **Official sources only.** Apps come from each platform's package manager. Missing package
  managers are installed only with their official installer, and only after the user confirms.
- **Safe PATH lookup.** Relative entries in `PATH` are ignored when looking up programs.
- **No scripts in tweaks.** Debloat tweaks are built only from typed actions (registry,
  service, scheduled task, AppX, systemd, package, gsettings, defaults) whose fields are
  validated against strict allowlists. A tweak cannot run arbitrary commands.
- **Undo by design.** The previous value of every change is saved before it is made, a failed
  tweak is rolled back, and registry values of an unexpected type are never touched. Removing
  built-in Windows apps is the only irreversible action, and Skadi warns before doing it.
- **Untrusted state file.** Saved values are re-validated before they are used to undo a change.

### Known residual risk

The official bootstrap installers for Chocolatey, Scoop and Homebrew are fetched over HTTPS at
run time and cannot be pinned to a checksum, because their maintainers update them in place.
Skadi shows the script before running it and requires confirmation.

## Verifying downloads

Every release publishes `SHA256SUMS`, a CycloneDX SBOM and SLSA build provenance signed with
Sigstore through GitHub artifact attestations. The release builds use a pinned Rust toolchain,
`Cargo.lock` and a fixed timestamp so they can be reproduced.

The one-line launchers (`install.ps1`, `install.sh`) refuse to run a download whose SHA-256 does
not match `SHA256SUMS`. When the GitHub CLI is installed they also verify the signed provenance.
To check a binary yourself:

```bash
gh attestation verify skadi-x86_64-unknown-linux-gnu --repo LLCForge/Skadi
```

The checksum file comes from the same release as the binary, so it detects corrupted or swapped
downloads but not a compromised release; the signed provenance shows that the binary was built
by this repository's release workflow. For the strongest guarantee, read the launcher before
running it, and pin a release with `SKADI_VERSION=v1.2.3` (Linux, macOS) or
`$env:SKADI_VERSION = 'v1.2.3'` (Windows).

## Trust in content from others

- Profiles, tweak packs, plugins and organisation policies from other people can be signed with
  Ed25519 (`skadi sign`). With `--public-key`, Skadi refuses content whose signature does not match.
- Tweak packs and plugins can only describe typed, validated actions; they cannot run commands.
  Plugins run in a WebAssembly sandbox with no access to files, the network or the system.
- An installed organisation policy whose signature cannot be verified blocks every install and
  tweak instead of being ignored.
