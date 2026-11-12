#!/usr/bin/env bash
# Installs and removes catalog apps with Skadi and reports the result of each one.
#
# Environment:
#   SKADI       path to the skadi binary (required)
#   SHARD       zero-based shard index (default 0)
#   SHARDS      number of shards (default 1)
#   APPS        optional space-separated app ids; overrides the catalog and sharding
#   SKADI_RUN   optional command prefix, e.g. "sudo -E" on Linux
#
# Written for bash 3.2 (macOS) as well as newer versions.

set -o pipefail

: "${SKADI:?SKADI must point to the skadi binary}"
SHARD="${SHARD:-0}"
SHARDS="${SHARDS:-1}"
SUMMARY="${GITHUB_STEP_SUMMARY:-/dev/stdout}"

case "$SHARD$SHARDS" in
  *[!0-9]*) echo "SHARD and SHARDS must be numbers" >&2; exit 2 ;;
esac

skadi() {
  # SKADI_RUN is an operator-controlled prefix (workflow file), deliberately word-split.
  # shellcheck disable=SC2086
  $SKADI_RUN "$SKADI" "$@"
}

apps=""
if [ -n "${APPS:-}" ]; then
  # SECURITY FIX: CWE-78 APPS comes from a manual workflow input; only catalog-style ids are accepted.
  case "$APPS" in
    *[!a-z0-9\ -]*) echo "APPS may only contain lowercase letters, digits, '-' and spaces" >&2; exit 2 ;;
  esac
  apps="$APPS"
else
  apps=$(skadi list --ids | tr -d '\r' | awk -v s="$SHARD" -v n="$SHARDS" '(NR - 1) % n == s')
fi

if [ -z "$apps" ]; then
  echo "No apps to test in this shard."
  exit 0
fi

{
  echo "### Install test: $(uname -s) shard $SHARD/$SHARDS"
  echo
  echo "| App | Install | Remove |"
  echo "|-----|---------|--------|"
} >> "$SUMMARY"

failed=0
total=0
for app in $apps; do
  total=$((total + 1))
  echo "::group::$app"
  if skadi install "$app" --yes; then
    install="ok"
    if skadi remove "$app" --yes; then remove="ok"; else remove="FAILED"; failed=$((failed + 1)); fi
  else
    install="FAILED"
    remove="-"
    failed=$((failed + 1))
  fi
  # Flatpak runtimes are large; drop the unused ones so later apps still fit on the runner.
  if command -v flatpak >/dev/null 2>&1; then
    $SKADI_RUN flatpak uninstall --unused -y --noninteractive >/dev/null 2>&1 || true
  fi
  echo "::endgroup::"
  echo "| \`$app\` | $install | $remove |" >> "$SUMMARY"
done

echo >> "$SUMMARY"
echo "$((total - failed)) of $total apps passed." >> "$SUMMARY"
echo "$((total - failed)) of $total apps passed."
[ "$failed" -eq 0 ]
