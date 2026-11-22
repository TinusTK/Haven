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
   (see [Download](README.md#download)), sign in to your media servers, add your
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

Found a problem? See [Found a problem?](README.md#found-a-problem) on the front page.
