# Haven

A media player for your own **Jellyfin, Emby and Plex** servers and for
**IPTV**, and a **film and TV show tracker**. One app for Windows, macOS,
Linux, Android and iOS, written in Rust with [Slint](https://slint.dev).

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
- **Tracking** with Trakt or Simkl (bring your own keys).
- **Its own video player** on computers and phones (mpv): audio and subtitle tracks,
  subtitle size and timing, speed, picture fit, next episode, a live
  network speed indicator. Or hand videos to VLC or another player of your
  choice.
- **Casting** to Chromecast.
- **Updates itself** on Windows, Linux and macOS: Settings > About shows
  what is new and installs a newer version when you say so.
- Dark and light look. On a phone the pages are laid out for a narrow
  screen with tabs along the bottom.

Haven can also use **add-ons** (the open Stremio add-on format) for more
catalogues and sources. Every version on this page has them, the Android
APK and the iOS IPA included. **The App Store (TestFlight) and Google Play
versions, when they come, will not have add-ons.**

## Download

Get the newest version from the
[Releases page](https://github.com/TinusTK/Haven/releases/latest).

| System | File | How to install |
| --- | --- | --- |
| Windows 10/11 (64-bit) | `Haven-windows-x64-setup.exe` | Run the installer. |
| macOS (Intel and Apple silicon) | `Haven-macos.dmg` | Open it and drag Haven to Applications. Signed and notarised by Apple. |
| Linux (Debian, Ubuntu and alike, x64) | `haven_amd64.deb` | `sudo apt install ./haven_amd64.deb` |
| Android 8 or newer | `Haven-android.apk` | Open the file on the phone and allow your browser or file app to install apps when asked. |
| iPhone and iPad (iOS 15 or newer) | `Haven-ios-unsigned.ipa` | Not signed: add [Haven's source](#add-haven-to-sidestore--altstore--sideinstaller-iphone-and-ipad) to SideStore or AltStore, or install the IPA with your own Apple ID using Sideloadly or a similar tool. |

On Android and iOS, videos play in Haven's own player. The pages Home, Explore,
Search and Live TV are tabs along the bottom; Radio, Watchlist, Calendar,
Add-ons and Settings are under **More**.

## Getting started

Setting up your media servers, Live TV, tracking, using servers away from
home and setting up **Haven Hub**: see **[Getting started](GettingStarted.md)**.

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
