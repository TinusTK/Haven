# Getting started with Haven

## First steps

1. Open **Settings** and sign in to your Jellyfin, Emby or Plex server, or
   add your IPTV playlist or Xtream Codes account under **Live TV**.
2. Add your own **TMDB key** in **Settings › Keys**. It is free: make an
   account at [themoviedb.org](https://www.themoviedb.org), then go to
   Settings › API and copy the API key (or the API read access token).
   The key turns on TMDB's shelves on Home, the TMDB source in Explore,
   posters and pictures, and **Where to watch**. Home, Explore and Search
   also fill with what your servers hold and with the built-in Cinemeta
   catalogue, which needs no key.
3. For Trakt or Simkl, add your own keys under Settings > Keys, then
   connect your account under **Tracking**. Link the same account on each
   device and your Watchlist and what you have watched stay the same on
   all of them, both ways (Settings › Sync options shows how it stands).

Haven keeps your data (servers, Watchlist, settings) on your own device.

## Media servers and the Local Area Network

All your existing media servers (Jellyfin, Plex and Emby) work only over
your **Local Area Network**: the phone, tablet or computer must be on the
same network as the server.

To use them away from home, install [Tailscale](https://tailscale.com)
(free) on **both** systems, the server and your device, and turn it on
on both. Then add the server in Haven with its Tailscale address
(`100.x.y.z`). No ports are opened to the internet.

## Backups and a new device

Haven backs up your setup (servers, Live TV sources, add-ons, keys,
accounts, Watchlist and settings) by itself while it is open: shortly after
it starts and then every 10 minutes.

- **iPhone and iPad (App Store):** to iCloud Drive › Haven, so a new or
  reset device signed in to the same Apple ID can get it back. With iCloud
  off, the backup goes to Files › On My iPhone › Haven.
- **Android, Windows, macOS and Linux:** to Documents › Haven. Copy that
  file to the new device's Documents › Haven folder.

To put a backup back, open **Settings › Sync options** and press
**Restore**.

Found a problem? See [Found a problem?](README.md#found-a-problem) on the front page.
