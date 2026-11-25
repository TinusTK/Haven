#!/usr/bin/env python3
"""Builds Haven's official AltStore / SideStore source from its GitHub releases.

Every release with an iOS IPA becomes a version in the source, newest first.
Writes the source to the folder given (default `site/`), as `source.json`,
next to the app's icon. Run by `.github/workflows/sidestore-source.yml`
whenever a release is published; the folder is then put on GitHub Pages.
"""
import json
import os
import shutil
import sys
import urllib.request

REPO = "TinusTK/Haven"
SITE = "https://tinustk.github.io/Haven"
BUNDLE_ID = "net.haven.app"
# The lowest iOS the IPA runs on: 15 for the Rust app (ios/bundle.sh),
# 17 for the Swift app (iphone/project.yml), which ships from 0.2.5 on.
MIN_IOS = "15.0"
MIN_IOS_SWIFT = "17.0"
SWIFT_FROM = (0, 2, 5)


def min_ios(version):
    try:
        parts = tuple(int(p) for p in version.split(".")[:3])
    except ValueError:
        return MIN_IOS
    return MIN_IOS_SWIFT if parts >= SWIFT_FROM else MIN_IOS
ICON = "app/assets/icon/haven-tile.png"

HEADERS = {"Accept": "application/vnd.github+json", "User-Agent": "haven-source"}
if os.environ.get("GITHUB_TOKEN"):
    HEADERS["Authorization"] = f"Bearer {os.environ['GITHUB_TOKEN']}"


def releases():
    page = 1
    while True:
        url = f"https://api.github.com/repos/{REPO}/releases?per_page=50&page={page}"
        with urllib.request.urlopen(urllib.request.Request(url, headers=HEADERS)) as answer:
            batch = json.load(answer)
        if not batch:
            return
        yield from batch
        page += 1


def main():
    out = sys.argv[1] if len(sys.argv) > 1 else "site"
    os.makedirs(out, exist_ok=True)
    versions = []
    for release in releases():
        if release["draft"] or release["prerelease"]:
            continue
        ipa = next((a for a in release["assets"] if a["name"].endswith("-ios-unsigned.ipa")), None)
        if not ipa:
            continue
        # A later build of the same version is tagged "v0.2.6-build2": it
        # is still 0.2.6 (the app's own version), as the IPA says.
        version = release["tag_name"].lstrip("vV").split("-")[0]
        versions.append({
            "version": version,
            "buildVersion": version,
            "date": release["published_at"],
            "localizedDescription": release["body"] or release["name"] or release["tag_name"],
            "downloadURL": ipa["browser_download_url"],
            "size": ipa["size"],
            "minOSVersion": min_ios(version),
        })
    versions.sort(key=lambda v: v["date"], reverse=True)

    icon = f"{SITE}/icon.png"
    source = {
        "name": "Haven",
        "identifier": BUNDLE_ID,
        "subtitle": "The official source for Haven",
        "description": "Haven's own source: new versions of Haven show up here as soon as they are released on GitHub.",
        "iconURL": icon,
        "website": f"https://github.com/{REPO}",
        "tintColor": "#F2C66D",
        "apps": [{
            "name": "Haven",
            "bundleIdentifier": BUNDLE_ID,
            "developerName": "TinusTK",
            "subtitle": "Media player and film and TV tracker",
            "localizedDescription": (
                "A media player for your own Jellyfin, Emby and Plex servers and for IPTV, "
                "and a film and TV show tracker: Home, Explore and Search, Live TV and Radio, "
                "Watchlist and Calendar, Trakt and Simkl, and add-ons. "
                "Videos open in VLC, Infuse or Outplayer."
            ),
            "iconURL": icon,
            "tintColor": "#F2C66D",
            "category": "entertainment",
            "appPermissions": {"entitlements": [], "privacy": {}},
            "versions": versions,
        }],
        "news": [],
    }
    with open(os.path.join(out, "source.json"), "w") as f:
        json.dump(source, f, indent=2)
        f.write("\n")
    shutil.copyfile(ICON, os.path.join(out, "icon.png"))
    latest = versions[0]["version"] if versions else "none"
    print(f"source.json: {len(versions)} version(s), latest {latest}")


if __name__ == "__main__":
    main()
