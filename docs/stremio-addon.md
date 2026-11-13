# MovieBox in Stremio (`moviebox-stremio`)

`moviebox-stremio` is a second program in this repo. It runs MovieBox-TUI's
MovieBox provider as a **Stremio addon** on your own Android phone or tablet,
so you can watch MovieBox titles inside Stremio on your phone, Android TV, PC
or in the browser (web.stremio.com). No paid server is needed.

What it gives Stremio:

| Stremio feature | Source |
| --- | --- |
| **MovieBox Trending** rows (movies + series) on the Board / Discover | MovieBox homepage |
| **Search** results ("MovieBox" row when you search) | MovieBox search |
| Details, seasons and episodes for MovieBox titles | MovieBox details |
| **Streams on normal Stremio titles** (Cinemeta / IMDb `tt…` pages) | Title + year matched against MovieBox |
| All dubs of a title (Hindi, Tamil, …) as separate streams | MovieBox dubs |
| Subtitles attached to every stream | MovieBox captions |

## How it fits together

```
 Stremio (TV / phone / browser)
        │  https://xxxx.trycloudflare.com/manifest.json
        ▼
 Cloudflare quick tunnel (free, gives the addon an https:// link)
        │
        ▼
 Android phone/tablet at home, in Termux
   cloudflared  ──►  moviebox-stremio on http://127.0.0.1:7000  ──►  MovieBox
   (both kept running by the `mbx` script, started on boot by Termux:Boot)
```

- Stremio only accepts **https** addon links from other devices, so a plain
  `http://192.168.x.x:7000` link fails with *"Failed to fetch"*. The
  Cloudflare tunnel provides the https link.
- Video is relayed through the phone/tablet (the addon adds the headers
  MovieBox's CDN requires), so that device must stay on and online while you
  watch.

---

## Step-by-step setup

Do steps 1–2 once per GitHub account and steps 3–9 once per phone/tablet.

### Step 1 — Fork the repo

On GitHub, open this repository and click **Fork**. Your fork is
`https://github.com/YOURNAME/MovieBox-Tui`. In every command below, replace
`YOURNAME` with your GitHub username.

### Step 2 — Build the addon on GitHub

1. In your fork, open the **Actions** tab. If GitHub asks, click
   **I understand my workflows, go ahead and enable them**.
2. Choose **Stremio addon** on the left → **Run workflow** → **Run workflow**.
3. Wait until the run shows a green tick (about 5–10 minutes).

This publishes a release called `stremio-addon` with ready-made binaries
(`android-arm64` for most phones, `android-armv7` for older 32-bit devices,
`linux-x86_64` for PCs). Every later push to `main` that changes the code
rebuilds it automatically.

### Step 3 — Install the Android apps

On the phone/tablet that will run the addon, install from
[F-Droid](https://f-droid.org) (or the GitHub releases of each app):

- **Termux**: required. Don't use the Google Play version; it is outdated.
- **Termux:Boot**: optional, starts the addon automatically after a reboot.
  **Open it once** after installing, or Android never runs it.

### Step 4 — Allow Termux to save files

Open Termux and run:

```bash
termux-setup-storage
```

Tap **Allow**. This lets the addon write its current link to your
**Download** folder, so you can find it without typing any commands.

### Step 5 — Install the addon

Still in Termux:

```bash
pkg update -y && pkg install -y curl
curl -fsSL https://raw.githubusercontent.com/YOURNAME/MovieBox-Tui/main/install-stremio-addon.sh \
  | MBX_REPO=YOURNAME/MovieBox-Tui bash
```

The installer sets up:

| What | Where | Why |
| --- | --- | --- |
| `moviebox-stremio` | `$PREFIX/bin/` | the addon server |
| `cloudflared` | Termux package | the free https tunnel |
| `mbx` | `$PREFIX/bin/` | starts both, restarts them if they crash, saves the link |
| boot script | `~/.termux/boot/start` | runs `mbx start` when the device starts |

If you already had a boot script, it is saved as `~/.termux/boot/start.bak`.

### Step 6 — Start it and get your link

```bash
mbx start
```

Wait about 20 seconds, then:

```bash
mbx link
```

It prints a link like:

```
https://word-word-word-word.trycloudflare.com/manifest.json
```

The same link is saved in **Download/moviebox-stremio-link.txt**.

### Step 7 — Install the addon in Stremio

Use the https link from step 6. Pick one way:

- **Stremio app (phone or TV):** **Addons** → paste the link in the search
  box at the top → **Install**.
- **Browser:** open <https://web.stremio.com>, **sign in**, go to
  **Addons** → **Add addon** → paste the link → **Install**.

Installed addons sync through your Stremio account, so installing once
(while signed in) adds it to every device on that account. Typing a long link
on a TV is painful, so the browser route is usually easiest.

### Step 8 — Watch something

Open any movie or series in Stremio. Under the streams you'll see entries
like **MovieBox 1080p**, **MovieBox 480p**, with the language (Original
Audio, Hindi dub, …), size and codec. Pick one and it plays, with subtitles.
The **MovieBox Trending** rows on the Board and **MovieBox** search results
work too.

In the **browser**, prefer streams marked **h264**: many browsers can't play
**hevc** streams. The Stremio apps play both.

### Step 9 — Keep it running in the background

Android kills background apps to save battery. To stop that:

- **Settings → Apps → Termux → Battery → Unrestricted** (wording varies by
  phone; on Xiaomi it's *Battery saver → No restrictions*, and also enable
  **Autostart** for Termux and Termux:Boot).
- Keep the device on charge and on Wi-Fi if it's your always-on box.

`mbx start` already takes a Termux wake lock (the Termux notification shows
"wake lock held").

---

## After a reboot: the link changes

A quick tunnel gets a **new** `trycloudflare.com` address every time it
starts: after a reboot, after `mbx restart`, or when `mbx` replaces a tunnel
that stopped working. When MovieBox streams stop appearing in Stremio:

1. Open **Download/moviebox-stremio-link.txt** (or run `mbx link`).
2. Install that link in Stremio again, as in step 7.

The new install **replaces** the old one automatically (same addon ID), so
you don't need to uninstall anything.

To get a link that never changes you need a domain on a free Cloudflare
account and a *named* tunnel (`cloudflared tunnel login`, then
`cloudflared tunnel create` …), or host the addon on a server (see
`render.yaml`). Both are optional.

---

## Everyday commands

| Command | What it does |
| --- | --- |
| `mbx status` | Is the addon running? Is the tunnel running? Current link |
| `mbx link` | Print the current Stremio link |
| `mbx start` | Start (or restart) addon + tunnel in the background |
| `mbx stop` | Stop both |
| `mbx restart` | Stop and start again (**the link changes**) |
| `mbx update` | Install the newest build from your fork; **the link stays the same** |

Logs are in `~/.mbx/`: `addon.log` (the addon), `tunnel.log` (cloudflared)
and `mbx.log` (starts, restarts and health checks).

---

## Troubleshooting

| Problem | Fix |
| --- | --- |
| *"Failed to fetch"* when installing the addon | You used an `http://` link. Use the `https://….trycloudflare.com/manifest.json` link from `mbx link`. |
| Every title says *"No streams were found"* | 1. `mbx status`: both must be *running*. 2. The link in Stremio must match `mbx link`; if not, install the new one (step 7). 3. Builds before commit `de34c3d` sent a stream field (`title`) that makes Stremio drop all MovieBox streams. Run `mbx update`. |
| One title has no MovieBox streams | MovieBox may name it differently. Search it in Stremio and open the result from the **MovieBox** row instead. Opening a title for the very first time can also take a few seconds; reopen it. |
| Stream won't play in the browser | It's probably **hevc**. Pick an **h264** stream, or use the Stremio app. |
| Stream starts, then stops | The CDN link expired. Close the player and pick the stream again. |
| Nothing works after the screen turns off | Step 9: battery unrestricted for Termux, and Autostart on Xiaomi phones. |
| Nothing starts after a reboot | Open the Termux:Boot app once, then reboot. Or start it by hand: `mbx start`. |
| No link file in Download | Run `termux-setup-storage` (step 4), then `mbx restart`. |
| Need more detail | `tail -n 50 ~/.mbx/addon.log`. For per-request logs: `mbx stop`, then `MBX_DEBUG=1 moviebox-stremio`. |

---

## Reference

### Running the addon by hand

`mbx` is the easy way. To run the server directly (for example to debug):

```bash
termux-wake-lock
moviebox-stremio
```

It prints:

```
  Same device:   http://127.0.0.1:7000/manifest.json
  Other devices: http://192.168.1.23:7000/manifest.json
```

The `http://127.0.0.1:7000/manifest.json` link works in Stremio **on the
same device** without a tunnel.

Options (flags or environment variables):

| Flag | Env | Default | Meaning |
| --- | --- | --- | --- |
| `--port` | `MBX_PORT` | `7000` | Port |
| `--host` | `MBX_HOST` | `0.0.0.0` | Bind address (`127.0.0.1` = this device only) |
| `--public-url` | `MBX_PUBLIC_URL` | – | Base URL to put in links (for tunnels/reverse proxies) |
| `--no-proxy` | `MBX_PROXY=0` | proxy on | Give Stremio raw CDN links + `proxyHeaders` instead |
| – | `MBX_DEBUG=1` | off | Log every request |

`mbx` expects the default port 7000.

### Building on the phone instead of GitHub

Slow (15–40 min the first time), but needs no fork:

```bash
pkg update && pkg install -y rust git clang make binutils
git clone https://github.com/YOURNAME/MovieBox-Tui.git
cd MovieBox-Tui
cargo build --release --bin moviebox-stremio
cp target/release/moviebox-stremio $PREFIX/bin/
```

Then install `cloudflared` and `mbx` yourself:

```bash
pkg install -y cloudflared
cp scripts/termux/mbx $PREFIX/bin/mbx && chmod 755 $PREFIX/bin/mbx
```

### Running it on the Android TV itself

Install Termux on the TV (sideload from F-Droid) and follow steps 3–9 there.
On the same device Stremio can also use `http://127.0.0.1:7000/manifest.json`,
with no tunnel needed.
