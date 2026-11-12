#!/bin/sh
# Skadi launcher for Linux and macOS.
#
#   curl -fsSL https://raw.githubusercontent.com/LLCForge/Skadi/main/install.sh | sh
#
# Downloads the Skadi binary for this machine from GitHub Releases into a private temporary
# folder, checks its SHA-256 checksum, also checks its signed build provenance when the GitHub
# CLI (gh) is installed, runs it, and deletes it afterwards. Nothing is installed.
#
# SKADI_VERSION=v1.2.3   use a specific release instead of the latest one
# SKADI_KEEP=1           keep the binary in ~/.local/bin/skadi instead of deleting it
set -eu

repo="LLCForge/Skadi"

fail() {
    printf 'skadi: %s\n' "$1" >&2
    exit 1
}

case "$(uname -s)" in
    Linux) os="unknown-linux-gnu" ;;
    Darwin) os="apple-darwin" ;;
    *) fail "unsupported system: $(uname -s)" ;;
esac
case "$(uname -m)" in
    x86_64 | amd64) arch="x86_64" ;;
    aarch64 | arm64) arch="aarch64" ;;
    *) fail "unsupported processor: $(uname -m)" ;;
esac
asset="skadi-$arch-$os"

version="${SKADI_VERSION:-}"
if [ -n "$version" ]; then
    case "$version" in
        v[0-9]*) ;;
        *) fail "SKADI_VERSION must look like v1.2.3" ;;
    esac
    base="https://github.com/$repo/releases/download/$version"
else
    base="https://github.com/$repo/releases/latest/download"
fi

command -v curl >/dev/null 2>&1 || fail "curl is required"

umask 077
dir="$(mktemp -d "${TMPDIR:-/tmp}/skadi.XXXXXX")" || fail "cannot create a temporary folder"
trap 'rm -rf "$dir"' EXIT INT TERM

fetch() {
    curl -fsSL --proto '=https' --tlsv1.2 --retry 3 -o "$2" "$1" || fail "download failed: $1"
}
fetch "$base/$asset" "$dir/skadi"
fetch "$base/SHA256SUMS" "$dir/SHA256SUMS"

expected="$(awk -v name="$asset" '$2 == name || $2 == "*" name { print $1 }' "$dir/SHA256SUMS")"
[ -n "$expected" ] || fail "no checksum for $asset"
if command -v sha256sum >/dev/null 2>&1; then
    actual="$(sha256sum "$dir/skadi" | awk '{ print $1 }')"
else
    actual="$(shasum -a 256 "$dir/skadi" | awk '{ print $1 }')"
fi
[ "$actual" = "$expected" ] || fail "checksum mismatch; the download was not run"

if command -v gh >/dev/null 2>&1; then
    gh attestation verify "$dir/skadi" --repo "$repo" >/dev/null 2>&1 \
        || fail "build provenance check failed; the download was not run"
    echo "skadi: checksum and signed build provenance verified"
else
    echo "skadi: checksum verified (install the GitHub CLI to also verify the signed build provenance)"
fi

chmod 700 "$dir/skadi"
if [ "${SKADI_KEEP:-}" = "1" ]; then
    mkdir -p "$HOME/.local/bin"
    cp "$dir/skadi" "$HOME/.local/bin/skadi"
    chmod 755 "$HOME/.local/bin/skadi"
    echo "skadi: installed to $HOME/.local/bin/skadi"
fi

# The script itself arrives on standard input; the app needs the terminal instead.
status=0
# Without a controlling terminal (cloud-init, CI) /dev/tty exists but cannot be opened.
if (exec </dev/tty) 2>/dev/null; then
    "$dir/skadi" "$@" </dev/tty || status=$?
else
    "$dir/skadi" "$@" || status=$?
fi
exit "$status"
