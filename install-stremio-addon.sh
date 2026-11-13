#!/usr/bin/env bash
# Installs the moviebox-stremio addon server (Termux or Linux).
# On Termux it also installs cloudflared, the `mbx` service script and a
# Termux:Boot start-up script. Step-by-step guide: docs/stremio-addon.md
#
#   curl -fsSL https://raw.githubusercontent.com/<you>/MovieBox-Tui/main/install-stremio-addon.sh \
#     | MBX_REPO=<you>/MovieBox-Tui bash
set -euo pipefail

REPO="${MBX_REPO:-}"
if [ -z "$REPO" ]; then
    echo "Set MBX_REPO to the GitHub repo that has the 'stremio-addon' release, e.g." >&2
    echo "  MBX_REPO=yourname/MovieBox-Tui bash install-stremio-addon.sh" >&2
    exit 1
fi

ARCH="$(uname -m)"
if [ -n "${PREFIX:-}" ] && [ -d "${PREFIX}/bin" ] && echo "$PREFIX" | grep -q "com.termux"; then
    case "$ARCH" in
        aarch64|arm64) ASSET="moviebox-stremio-android-arm64" ;;
        armv7l|armv8l|arm) ASSET="moviebox-stremio-android-armv7" ;;
        *) echo "Unsupported Termux architecture: $ARCH" >&2; exit 1 ;;
    esac
    DEST="$PREFIX/bin/moviebox-stremio"
else
    case "$ARCH" in
        x86_64|amd64) ASSET="moviebox-stremio-linux-x86_64" ;;
        *) echo "No prebuilt binary for $ARCH; build with: cargo build --release --bin moviebox-stremio" >&2; exit 1 ;;
    esac
    mkdir -p "$HOME/.local/bin"
    DEST="$HOME/.local/bin/moviebox-stremio"
fi

URL="https://github.com/$REPO/releases/download/stremio-addon/$ASSET"
echo "Downloading $ASSET from $REPO ..."
TMP="$(mktemp)"
curl -fL --progress-bar "$URL" -o "$TMP"
chmod +x "$TMP"
mv "$TMP" "$DEST"
echo "Installed: $DEST"

if ! { [ -n "${PREFIX:-}" ] && echo "$PREFIX" | grep -q "com.termux"; }; then
    echo
    echo "Start it with:  moviebox-stremio"
    echo "Then open Stremio and install: http://127.0.0.1:7000/manifest.json"
    exit 0
fi

# ----- Termux extras: HTTPS tunnel, the `mbx` service script, auto-start -----

# Stremio needs an https:// addon link; cloudflared provides one for free.
if ! command -v cloudflared >/dev/null 2>&1; then
    echo "Installing cloudflared ..."
    pkg install -y cloudflared
fi

# `mbx` keeps the addon and the tunnel running and saves the current link.
mkdir -p "$HOME/.mbx"
echo "$REPO" >"$HOME/.mbx/repo"
TMP="$(mktemp)"
curl -fsSL "https://raw.githubusercontent.com/$REPO/main/scripts/termux/mbx" -o "$TMP"
chmod 755 "$TMP"
mv "$TMP" "$PREFIX/bin/mbx"
echo "Installed: $PREFIX/bin/mbx"

# Termux:Boot runs this at phone start-up (if the Termux:Boot app is installed).
BOOT="$HOME/.termux/boot/start"
BOOT_SCRIPT='#!/data/data/com.termux/files/usr/bin/sh
# Termux:Boot - start the MovieBox Stremio addon and its HTTPS tunnel.
termux-wake-lock
mbx start'
mkdir -p "$HOME/.termux/boot"
if [ -f "$BOOT" ] && [ "$(cat "$BOOT")" != "$BOOT_SCRIPT" ]; then
    cp "$BOOT" "$BOOT.bak"
    echo "Saved your old boot script as $BOOT.bak"
fi
printf '%s\n' "$BOOT_SCRIPT" >"$BOOT"
chmod 755 "$BOOT"
echo "Installed: $BOOT"

echo
echo "Next:"
echo "  mbx start     # start the addon + HTTPS tunnel"
echo "  mbx link      # after ~20 s: the link to install in Stremio"
