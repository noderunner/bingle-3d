"""Checkers: a wooden draughtsman, and a king, which is two of them, one on the other.

A man is turned from a profile: a raised rim round a face with two rings cut in it, the same on
both faces, so one sits on another by their rims. Its edge is milled: ridges up and down it, all
the way round, as a hand finds them.

Made for a 28 mm cell: 23 mm across and 6 mm thick. Each is centred on its middle, with no
material: an appearance file says what each seat's are made of."""

import math

import bpy
from mathutils import Matrix

import common
import meshkit

RADIUS = 0.0115
THICKNESS = 0.0060
RIDGES = 32
# Three vertices to a ridge: its crest and the hollows either side.
SEGMENTS = RIDGES * 3
RIDGE_DEPTH = 0.00032

# A face from its middle out to the edge, in millimetres: (radius, depth below the rim's top).
FACE = [
    (0.0, 0.40),
    (3.5, 0.40),
    (3.9, 0.85),
    (4.5, 0.85),
    (4.9, 0.40),
    (6.9, 0.40),
    (7.3, 0.85),
    (7.9, 0.85),
    (8.3, 0.40),
    (8.9, 0.40),
    (9.4, 0.0),
    (10.75, 0.0),
    (11.25, 0.18),
    (11.5, 0.62),
]


def man(name="Man"):
    mm = 0.001
    top = [(r * mm, THICKNESS - d * mm) for r, d in FACE]
    bottom = [(r, THICKNESS - z) for r, z in top]
    obj = meshkit.turned(name, "piece", bottom + top[::-1], SEGMENTS)
    # The milled edge: its side goes in and out round it.
    side = RADIUS - 1e-6
    for v in obj.data.vertices:
        r = math.hypot(v.co.x, v.co.y)
        if r >= side:
            dent = RIDGE_DEPTH * (0.5 - 0.5 * math.cos(RIDGES * math.atan2(v.co.y, v.co.x)))
            v.co.x *= (r - dent) / r
            v.co.y *= (r - dent) / r
    return obj


def king():
    lower, upper = man("King"), man("Upper")
    # The upper one turned by half a ridge, as two never line up.
    upper.matrix_world = Matrix.Translation((0, 0, THICKNESS)) @ Matrix.Rotation(math.pi / RIDGES, 4, "Z")
    meshkit.apply_transform(upper)
    return meshkit.join([lower, upper], "King")


def make(name):
    """A man or a king, centred on its middle and mapped for a wood. Returns it and its height."""
    obj = meshkit.triangulate({"man": man, "king": king}[name]())
    meshkit.finish(obj, "piece", sharp=35.0)
    tall = meshkit.centre_height(obj)
    return meshkit.box_uv(obj, 1.0, detail=True, offset=(0.23, 0.41, 0.0)), tall


def run():
    for name in ("man", "king"):
        bpy.ops.wm.read_factory_settings(use_empty=True)
        obj, tall = make(name)
        meshkit.export_glb(obj, common.output("models", "checkers", f"{name}.glb"))
        print(f"   {name}: {tall * 1000:.1f} mm tall")
