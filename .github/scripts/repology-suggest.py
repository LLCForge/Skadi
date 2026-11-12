"""Suggests missing Linux and macOS package names for catalog apps using the Repology API.

Prints a Markdown report. Suggestions are only hints: a maintainer must check each id with a
read-only query before adding it (see CONTRIBUTING.md).

Usage: python3 .github/scripts/repology-suggest.py catalog/apps.toml [limit]
"""

import json
import sys
import time
import tomllib
import urllib.parse
import urllib.request

API = "https://repology.org/api/v1/project/"
USER_AGENT = "skadi-catalog-bot/1 (+https://github.com/LLCForge/Skadi)"
# Repology repository name prefix -> Skadi manager id
REPOS = {
    "debian_": "apt",
    "ubuntu_": "apt",
    "fedora_": "dnf",
    "arch": "pacman",
    "homebrew": "brew",
    "homebrew_casks": "brew-cask",
    "flathub": "flatpak",
}


def manager_for(repo):
    if repo == "homebrew_casks":
        return "brew-cask"
    for prefix, manager in REPOS.items():
        if repo == prefix or (prefix.endswith("_") and repo.startswith(prefix)):
            return manager
    return None


def lookup(project):
    request = urllib.request.Request(API + urllib.parse.quote(project), headers={"User-Agent": USER_AGENT})
    with urllib.request.urlopen(request, timeout=30) as response:
        return json.load(response)


def main():
    apps = tomllib.load(open(sys.argv[1], "rb"))["app"]
    limit = int(sys.argv[2]) if len(sys.argv) > 2 else 60
    rows = []
    checked = 0
    for app in apps:
        missing = [m for m in ("apt", "dnf", "pacman", "flatpak", "brew", "brew-cask") if m not in app["packages"]]
        if not missing or checked >= limit:
            continue
        checked += 1
        try:
            packages = lookup(app["id"])
        except Exception as error:  # network or rate limit: report and continue
            print(f"<!-- {app['id']}: {error} -->", file=sys.stderr)
            packages = []
        time.sleep(1.1)  # Repology asks for at most one request per second
        found = {}
        for package in packages:
            manager = manager_for(package.get("repo", ""))
            name = package.get("binname") or package.get("srcname")
            if manager in missing and name and manager not in found:
                found[manager] = name
        if found:
            pairs = ", ".join(f"`{m}` = `{n}`" for m, n in sorted(found.items()))
            rows.append(f"| `{app['id']}` | {app['name']} | {pairs} |")
    print("## Package names Repology knows for catalog apps\n")
    print(f"Checked {checked} apps that miss at least one Linux or macOS package. "
          "Verify every id with a read-only query before adding it.\n")
    if rows:
        print("| App | Name | Suggestions |\n|---|---|---|")
        print("\n".join(rows))
    else:
        print("No suggestions this time.")


if __name__ == "__main__":
    main()
