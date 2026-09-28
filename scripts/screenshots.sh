#!/usr/bin/env bash
# Regenerate docs/*.png from the mock screens in src/ui.rs. Needs rsvg-convert.
set -euo pipefail
cd "$(dirname "$0")/.."
command -v rsvg-convert >/dev/null || { echo "rsvg-convert not found (librsvg)" >&2; exit 1; }
cargo test generate -- --ignored
for f in docs/*.svg; do
    rsvg-convert -z 1 "$f" -o "${f%.svg}.png"
done
rm -f docs/*.svg
echo "Updated: $(ls docs/*.png | wc -l) screenshots in docs/"
