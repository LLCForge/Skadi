"""Prints the ids of catalog apps that are new or changed between two versions of apps.toml.

Usage: python3 changed-apps.py <base apps.toml> <head apps.toml>
"""

import sys
import tomllib


def load(path):
    with open(path, "rb") as f:
        return {app["id"]: app for app in tomllib.load(f).get("app", [])}


def main():
    base, head = load(sys.argv[1]), load(sys.argv[2])
    changed = [app_id for app_id, app in head.items() if base.get(app_id) != app]
    print(" ".join(changed))


if __name__ == "__main__":
    main()
