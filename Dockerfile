# Runs the moviebox-stremio addon on a server (Render, Koyeb, Hugging Face, VPS).
# Uses the prebuilt binary from this repo's `stremio-addon` release.
FROM ubuntu:24.04

ARG REPO=8307788179sS/MovieBox-Tui
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates curl \
 && curl -fL -o /usr/local/bin/moviebox-stremio \
      "https://github.com/${REPO}/releases/download/stremio-addon/moviebox-stremio-linux-x86_64" \
 && chmod +x /usr/local/bin/moviebox-stremio \
 && apt-get purge -y curl && apt-get autoremove -y && rm -rf /var/lib/apt/lists/*

ENV HOME=/tmp
EXPOSE 7000
# Hosts give the port in $PORT; default 7000.
CMD ["sh", "-c", "exec moviebox-stremio --host 0.0.0.0 --port ${PORT:-7000}"]
