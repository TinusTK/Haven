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

1. Open **Settings** and sign in to your Jellyfin, Emby or Plex server, or
   add your IPTV playlist or Xtream Codes account under **Live TV**.
2. Home, Explore and Search fill with what your servers hold and with the
   built-in catalogue. Haven comes with its own TMDB key; you can put your
   own in Settings > Keys to use that instead.
3. For Trakt or Simkl, add your own keys under Settings > Keys, then
   connect your account under **Tracking**.

Haven keeps your data (servers, Watchlist, settings) on your own device.

## Media servers and the Local Area Network

All your existing media servers (Jellyfin, Plex and Emby) work only over
your **Local Area Network**: the phone, tablet or computer must be on the
same network as the server.

To use them away from home, install [Tailscale](https://tailscale.com)
(free) on **both** systems, the server and your device, and turn it on
on both. Then add the server in Haven with its Tailscale address
(`100.x.y.z`). No ports are opened to the internet.

## Haven Hub

Haven Hub is Haven without a screen, running all the time on a Raspberry
Pi, a small Linux machine or a cloud server. It keeps one setup for all
your devices: your media server sign-ins, TV sources, keys, Watchlist,
what you have watched and your Trakt or Simkl accounts. It keeps Trakt and
Simkl in sync and checks for new episodes even while every device is off.

**Haven Hub works exactly the same as your other media servers:** only
over your Local Area Network, or from anywhere once Tailscale is
installed and turned on on both the Hub's machine and your device.

### Setting up Haven Hub

It is best to **start with Haven on a Windows PC or a Mac**: set
everything up there first, then send it all over to the Hub. Your phones
and tablets then take that setup from the Hub.

1. **Set up Haven on your computer.** Install Haven on Windows or macOS
   (see [Download](#download)), sign in to your media servers, add your
   Live TV sources and keys, and link Trakt or Simkl under **Tracking**.
2. **Install the Hub.** On the Raspberry Pi or server, install Docker
   (on a Pi: `curl -fsSL https://get.docker.com | sh`, then
   `sudo usermod -aG docker $USER` and log in again), and run:

   ```bash
   docker run -d --name haven-hub --restart unless-stopped \
     -p 8590:8590 -v haven-hub:/data ghcr.io/tinustk/haven-hub:latest
   ```

3. **Optional, for use away from home:** install Tailscale on the Hub's
   machine and on each device, and turn it on on all of them.
4. **Open the Hub's page.** In a web browser go to
   `http://<the Hub's address>:8590` (find a Pi's address with
   `hostname -I`, or use its Tailscale address). The page shows a
   six-digit pairing code.
5. **Pair your computer.** In Haven on the computer, open
   **Settings › Media servers**, choose **Haven Hub**, type the Hub's
   address and the code, and press **Pair**.
6. **Send your setup to the Hub.** Still on the computer, press
   **Send everything to the Hub**. Your servers, sources, keys, lists and
   Trakt or Simkl sign-ins are now on the Hub.
7. **Pair your phones and tablets.** On each one, open
   **Settings › Media servers › Haven Hub**, enter the Hub's address and
   the new code from the Hub's page (it changes after each device pairs),
   and press **Pair**. The device takes the Hub's setup, so there is
   nothing else to set up on it.

Paired devices are listed on the Hub's page, where you can forget one.
Never open port 8590 to the internet: the Hub's page shows the pairing
code to anyone who can reach it. More about the Hub, updating it and
running it without Docker: [hub/README.md](hub/README.md).

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
