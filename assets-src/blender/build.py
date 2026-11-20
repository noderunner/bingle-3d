"""Builds `assets/` from `assets-src/`: run by `scripts/assets.sh`, inside Blender.

    blender -b --factory-startup --python-exit-code 1 -P assets-src/blender/build.py -- [step ...]

With no steps, every step runs. `previews` is not a step of the build: it renders the stones and
woods in Cycles to `target/asset-previews/`, for a person (or an agent) to look at."""

import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import bpy  # noqa: E402

import common  # noqa: E402

# The Blender the assets are made with. Another version may make different bytes; say so, rather
# than quietly committing them.
BLENDER = (5, 3)

STEPS = ["environment", "woods", "stones", "engraving"]


def main():
    args = sys.argv[sys.argv.index("--") + 1 :] if "--" in sys.argv else []
    if bpy.app.version[:2] != BLENDER:
        print(
            f"warning: this is Blender {bpy.app.version_string}; the assets are made with "
            f"{BLENDER[0]}.{BLENDER[1]}"
        )
    steps = args or STEPS
    unknown = [s for s in steps if s not in STEPS + ["previews", "fetch"]]
    if unknown:
        raise SystemExit(f"unknown steps: {unknown}; the steps are {STEPS + ['previews']}")
    common.fetch()
    for step in steps:
        if step == "fetch":
            continue
        print(f"== {step}")
        module = __import__(step)
        module.run()


main()
