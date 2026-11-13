# MovieBox in Stremio (`moviebox-stremio`)

`moviebox-stremio` is a second program in this repo. It runs MovieBox-TUI's
MovieBox provider as a **Stremio addon** on your own phone, PC or TV box,
so you can watch MovieBox titles inside the Stremio app (Android phone,
Android TV, desktop).

What it gives Stremio:

| Stremio feature | Source |
| --- | --- |
| **MovieBox Trending** rows (movies + series) on the Board / Discover | MovieBox homepage |
| **Search** results ("MovieBox" row when you search) | MovieBox search |
| Details, seasons and episodes for MovieBox titles | MovieBox details |
| **Streams on normal Stremio titles** (Cinemeta / IMDb `tt…` pages) | Title + year matched against MovieBox |
| All dubs of a title (Hindi, Tamil, …) as separate streams | MovieBox dubs |
| Subtitles attached to every stream | MovieBox captions |

Streams go through a small built-in proxy that adds the headers MovieBox's
CDN requires (User-Agent, Referer, signed cookies) and fetches DASH segments
in small ranges, the same way the TUI's player sidecar does.

---

## 1. Get the binary

### Option A — prebuilt (recommended)

1. Fork this repo on GitHub (your fork must contain this file and
   `.github/workflows/stremio-addon.yml`).
2. In your fork: **Actions → Stremio addon → Run workflow**. Wait ~10 min.
   It publishes a release called `stremio-addon` with these files:
   - `moviebox-stremio-android-arm64` (Termux, most phones / TVs)
   - `moviebox-stremio-android-armv7` (Termux, 32-bit TVs / Fire TV)
   - `moviebox-stremio-linux-x86_64` (PC / VPS)
3. In Termux:

```bash
pkg update && pkg install -y curl
curl -fsSL https://raw.githubusercontent.com/YOURNAME/MovieBox-Tui/main/install-stremio-addon.sh \
  | MBX_REPO=YOURNAME/MovieBox-Tui bash
```

### Option B — build on the phone in Termux

Slow (15–40 min the first time), but needs no GitHub fork:

```bash
pkg update && pkg install -y rust git clang make binutils
git clone https://github.com/YOURNAME/MovieBox-Tui.git
cd MovieBox-Tui
cargo build --release --bin moviebox-stremio
cp target/release/moviebox-stremio $PREFIX/bin/
```

---

## 2. Run it

```bash
termux-wake-lock        # stops Android from killing Termux in the background
moviebox-stremio
```

It prints something like:

```
  Same device:   http://127.0.0.1:7000/manifest.json
  Other devices: http://192.168.1.23:7000/manifest.json
```

Keep Termux open (it can be in the background) while you watch.

Options (flags or environment variables):

| Flag | Env | Default | Meaning |
| --- | --- | --- | --- |
| `--port` | `MBX_PORT` | `7000` | Port |
| `--host` | `MBX_HOST` | `0.0.0.0` | Bind address (`127.0.0.1` = this device only) |
| `--public-url` | `MBX_PUBLIC_URL` | – | Base URL to put in links (for tunnels/reverse proxies) |
| `--no-proxy` | `MBX_PROXY=0` | proxy on | Give Stremio raw CDN links + `proxyHeaders` instead |
| – | `MBX_DEBUG=1` | off | Log every request |

---

## 3. Install in Stremio

**Phone (addon running on the same phone):**
open Stremio → **Addons** → paste `http://127.0.0.1:7000/manifest.json`
in the search box → **Install**. Or open `http://127.0.0.1:7000/` in the
phone's browser and tap **Install in Stremio**.

**Android TV:** two ways:

- *Run the addon on the TV itself*: install Termux on the TV
  (F-Droid / sideload), follow steps 1–2 there, install
  `http://127.0.0.1:7000/manifest.json` in Stremio on the TV.
- *Run it on your phone/PC and use it from the TV*: in Stremio on the TV,
  install `http://<phone-ip>:7000/manifest.json` (the "Other devices" URL).
  The phone must stay on the same Wi-Fi, and the video is relayed through
  the phone. If Stremio refuses a plain-`http` LAN address, make an HTTPS
  tunnel on the phone:

  ```bash
  pkg install -y cloudflared
  cloudflared tunnel --url http://127.0.0.1:7000
  ```

  and install the printed `https://….trycloudflare.com/manifest.json`.

Stremio syncs installed addons through your account, so an addon URL that
works on both devices only needs to be installed once.

---

## Troubleshooting

- **No streams / "no MovieBox match"** on a Cinemeta title: MovieBox may name
  it differently. Search it in Stremio and open the result from the
  **MovieBox** row instead.
- **Stream starts then stops**: the CDN link expired. Close and reopen the
  title to get fresh links.
- **Nothing loads after the screen turns off**: run `termux-wake-lock`, and
  disable battery optimisation for Termux.
- Errors are printed in the Termux window; run with `MBX_DEBUG=1` for more.
