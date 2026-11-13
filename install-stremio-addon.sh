#!/usr/bin/env bash
# Installs the moviebox-stremio addon server (Termux or Linux).
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

echo
echo "Installed: $DEST"
echo
echo "Start it with:"
if [ -n "${PREFIX:-}" ] && echo "$PREFIX" | grep -q "com.termux"; then
    echo "  termux-wake-lock; moviebox-stremio"
else
    echo "  moviebox-stremio"
fi
echo
echo "Then open Stremio and install: http://127.0.0.1:7000/manifest.json"
