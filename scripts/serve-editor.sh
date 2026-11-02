#!/usr/bin/env bash
# Serve wkGolf map editor on PVE (static files).
ROOT="$(cd "$(dirname "$0")/.." && pwd)/editor"
PORT="${1:-8790}"
echo "wkGolf editor → http://0.0.0.0:${PORT}/"
echo "  tileset: editor/assets/tileset.png"
cd "$ROOT" && exec python3 -m http.server "$PORT" --bind 0.0.0.0
