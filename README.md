# Haven

A media player for your own **Jellyfin, Emby and Plex** servers and for
**IPTV**, and a **film and TV show tracker**. One app for Windows, macOS,
Linux, Android, iPhone, iPad and Apple TV. The computer and Android apps
are written in Rust with [Slint](https://slint.dev); the iPhone, iPad and
Apple TV apps in Swift.

Haven plays what you already have: it does not host, sell or supply any
films, shows or channels.

### Add Haven to SideStore / AltStore / SideInstaller (iPhone and iPad)

Copy this source URL:

```
https://tinustk.github.io/Haven/source.json
```

1. Open [SideStore](https://github.com/SideStore/SideStore) or
   [AltStore](https://github.com/altstoreio/AltStore) and go to **Sources**.
   No SideStore yet? On iOS 27,
   [SideInstaller](https://github.com/FrizzleM/SideInstaller) puts
   SideStore on your iPhone without a computer.
2. Tap **+**, paste the URL above and confirm.
3. Install Haven from the source. New versions then show up as updates by
   themselves.

Other sideloading apps (Feather, ESign and the like) take the same URL
under **Sources** or **Repos**. This is Haven's official source.

## What it does

- **Home, Explore and Search** across your media servers and a built-in
  catalogue of films and series, with posters and details from TMDB.
- **A page for every title**: details over the artwork, seasons and
  episodes, and where to watch it (streaming services, rental and download
  shops for your country, and your own servers).
- **Live TV and Radio** from an M3U playlist or an Xtream Codes account,
  with favourites and groups.
- **Watchlist and Calendar**: keep track of what you want to see, and see
  when new films and episodes come out, by month, in your local time.
- **Tracking** with Trakt or Simkl, kept in step both ways: your
  Watchlist and every film and episode you watch. Something added, taken
  off or marked unwatched on one device shows on your others too. Haven
  checks when it opens, after you watch something and every so often
  while it is open.
- **Its own video player** (mpv) on every device: audio and subtitle
  tracks, subtitle size and timing, speed, picture fit, next episode, a
  live network speed indicator, and a volume that can go up to 200% for
  quiet films (Settings > General). Or hand videos to VLC or another player
  of your choice.
- **Casting** to Chromecast from a computer, and AirPlay from an iPhone or
  iPad.
- **Updates itself** on Windows, Linux and macOS: Settings > About shows
  what is new and installs a newer version when you say so.
- Dark and light look. On a phone the pages are tabs along the bottom, on
  an iPad they sit beside a sidebar, and on Apple TV everything works with
  the remote.

Haven can also use **add-ons** (the open Stremio add-on format) for more
catalogues and sources, installed by address or brought in by signing in
to your Stremio account (Settings > Accounts). Every version on this page
has them, the Android APK and the iPhone and Apple TV IPAs included. **The
App Store (TestFlight) and Google Play versions do not have add-ons.**

## Download

Get the newest version from the
[Releases page](https://github.com/TinusTK/Haven/releases/latest).

| System | File | How to install |
| --- | --- | --- |
| Windows 10/11 (64-bit) | `Haven-windows-x64-setup.exe` | Run the installer. |
| macOS (Intel and Apple silicon) | `Haven-macos.dmg` | Open it and drag Haven to Applications. Signed and notarised by Apple. |
| Linux (Debian, Ubuntu and alike, x64) | `haven_amd64.deb` | `sudo apt install ./haven_amd64.deb` |
| Android 8 or newer | `Haven-android.apk` | Open the file on the phone and allow your browser or file app to install apps when asked. |
| iPhone and iPad (iOS 17 or newer) | `Haven-ios-unsigned.ipa` | Not signed: add [Haven's source](#add-haven-to-sidestore--altstore--sideinstaller-iphone-and-ipad) to SideStore or AltStore, or install the IPA with your own Apple ID using Sideloadly or a similar tool. |
| Apple TV (tvOS 17 or newer) | `Haven-tvos-unsigned.ipa` | Not signed: install it with your own Apple ID using Sideloadly or Xcode (SideStore and AltStore do not install Apple TV apps). |

On phones, tablets and Apple TV, videos play in Haven's own player. The
pages are Home, Explore, Library (Watchlist, History, Downloads), Live TV
(with Radio), Calendar and Settings.

## Getting started

Setting up your media servers, Live TV, tracking, using servers away from
home and backing up to a new device: see **[Getting started](GettingStarted.md)**.

## Found a problem?

Use the red **Report an issue** button in Settings > About, or
[open an issue](https://github.com/TinusTK/Haven/issues/new) here on
GitHub. Say which system and Haven version you use and what happened.
Settings > About also shows where Haven's log file is.

## Licence

Copyright © 2026 TinusTK.

Haven is free software: you can redistribute it and/or modify it under the
terms of the GNU General Public License version 3, as published by the Free
Software Foundation. It is distributed in the hope that it will be useful,
but WITHOUT ANY WARRANTY; without even the implied warranty of
MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See `LICENSE` for the
full text, and `THIRD-PARTY.md` for the components Haven is built with.

This product uses the TMDB API but is not endorsed or certified by TMDB.
