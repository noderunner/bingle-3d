#!/usr/bin/env bash
# Builds assets/ from assets-src/ with Blender, run without a window (assets-src/README.md).
#
#   bash scripts/assets.sh [step ...]     # environment, woods, stones; previews renders pictures
#
# Blender is $BLENDER if set, else `blender` on PATH, else Flathub's org.blender.Blender.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
build="$root/assets-src/blender/build.py"

if [[ -n "${BLENDER:-}" ]]; then
    blender=("$BLENDER")
elif command -v blender >/dev/null; then
    blender=(blender)
elif command -v flatpak >/dev/null && flatpak info org.blender.Blender >/dev/null 2>&1; then
    blender=(flatpak run org.blender.Blender)
elif command -v flatpak-spawn >/dev/null; then
    # Inside a toolbox: Blender is the host's.
    blender=(flatpak-spawn --host flatpak run org.blender.Blender)
else
    echo "No Blender: set BLENDER, or install org.blender.Blender from Flathub." >&2
    exit 1
fi

# No window: Blender runs in the background, and must not find a display to open one on.
exec env -u DISPLAY -u WAYLAND_DISPLAY "${blender[@]}" \
    --background --factory-startup --python-exit-code 1 --python "$build" -- "$@"
