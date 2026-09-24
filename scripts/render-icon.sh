#!/usr/bin/env bash
# Renders assets/icon.svg to assets/icon.png (1024px), the committed PNG that the app
# embeds as its window icon and bundle-macos.sh turns into Redash.icns.
# Needs rsvg-convert (brew install librsvg); rerun after editing the SVG.
set -euo pipefail
cd "$(dirname "$0")/.."
rsvg-convert -w 1024 -h 1024 assets/icon.svg -o assets/icon.png
