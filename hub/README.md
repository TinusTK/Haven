# Haven Hub

Haven Hub is Haven without a screen. It runs all the time on a Raspberry Pi
or a small server and keeps one setup for all your devices: your add-ons,
media server sign-ins, TV sources, keys, watchlist and what you have
watched. Haven on your phone, tablet or computer adds the Hub like a media
server and gets its shelves, details and ways to watch from it.

## What you need

- A Raspberry Pi 4 or 5 with 64-bit Raspberry Pi OS, any other 64-bit
  Linux machine, or a small cloud server (Oracle Cloud's free Ampere server
  works well).
- Docker. On a Pi: `curl -fsSL https://get.docker.com | sh`, then
  `sudo usermod -aG docker $USER` and log in again.

## Install

```bash
docker run -d --name haven-hub --restart unless-stopped \
  -p 8590:8590 -v haven-hub:/data ghcr.io/tinustk/haven-hub:latest
```

Or with Docker Compose: copy `docker-compose.yml` from this folder and
run `docker compose up -d` next to it.

Update to a newer Hub:

```bash
docker pull ghcr.io/tinustk/haven-hub:latest && docker rm -f haven-hub
```

then run the install command again. Everything the Hub keeps is in the
`haven-hub` volume and stays.

## Pair your devices

1. Open `http://<the Hub's address>:8590` in a web browser. The page shows
   a six-digit pairing code. (Find a Pi's address with `hostname -I`.)
2. In Haven, open **Settings › Media servers**, choose **Haven Hub**, type
   the Hub's address and the code, and press **Pair**.
3. On the device you already set up, press **Send everything to the Hub**.
   Other devices take the Hub's setup when they pair.

The code changes after each device pairs. Paired devices are listed on the
Hub's page, where you can forget one.

## Add-ons

Add them on the Hub's page (paste the add-on's address), or install them in
a Haven build that has add-ons and press **Send everything to the Hub**.
Their catalogues show in every app as the Hub's shelves, and their ways to
watch show on a title's page under the Hub's name.

## What it does in the background

- **Trakt and Simkl:** link an account in Haven on any device and press
  **Send everything to the Hub**. From then on the Hub sends what you
  watched and your Watchlist changes, and brings in what the accounts
  hold, every 15 minutes, even while every device is off.
- **New episodes:** every few hours it fetches the dates of the series on
  your Watchlist or that you are watching. Each device's calendar takes
  them from the Hub, and the Hub's page lists what airs this week.
- **Add-ons:** their lists of catalogues are fetched again every few
  hours.

## Reaching the Hub

- **At home:** the Hub's address on your network, such as
  `192.168.1.20`. Nothing else to set up.
- **Away from home, or a cloud server:** use
  [Tailscale](https://tailscale.com) (free). Install it on the Hub's
  machine and on your devices, and use the Hub's Tailscale address
  (`100.x.y.z`). No ports are opened to the internet.

Do not open port 8590 to the internet: the Hub's page shows the pairing
code to anyone who can reach it.

## Without Docker

Build the program from the repository with Rust installed:

```bash
cd app && cargo build --release -p haven-hub
```

and run `target/release/haven-hub`. It keeps its database in `/data`, or
in `HAVEN_HUB_DATA` if set. `HAVEN_HUB_PORT` changes the port (8590) and
`HAVEN_HUB_NAME` the name the apps show.
