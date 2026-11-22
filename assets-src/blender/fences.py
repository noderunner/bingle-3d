"""Fences: a long, slender rail of wood that stands along one side of a square (Fendo's).

- `models/fence.glb`: one fence, 90 % of a 28 mm cell's side long, so a square's corners
  still show between the fences round it; slender and a little taller than it is wide, its edges
  eased. Centred on its middle, lying along x.
- `models/bowl_fences.glb`: fences heaped in the bowl (`tableware.py`'s), as one mesh about
  the bowl's origin, lying every way as they were dropped in, the pile highest in the middle.

Neither has a material: the appearance file says what fences are made of (a dark wood); the
pieces are chess pawns (`chessmen.py`)."""

import math
import random

import bpy
from mathutils import Matrix, Vector

import common
import meshkit
from meshkit import Shape
import tableware

LENGTH = 0.9 * 0.028
WIDTH = 0.0032
HEIGHT = 0.0055
EASE = 0.0006


def fence():
    obj = meshkit.rounded_box("Fence", (LENGTH, WIDTH, HEIGHT), (0, 0, HEIGHT / 2), "piece", radius=EASE, segments=2)
    obj = meshkit.triangulate(obj)
    meshkit.finish(obj, "piece", sharp=40.0)
    meshkit.flat_faces_stay_flat(obj)
    meshkit.centre_height(obj)
    return meshkit.box_uv(obj, 1.0, detail=True, offset=(0.23, 0.07, 0.0))


def plain_fence():
    """A fence as a plain box, for the heap: dozens of them, so few triangles."""
    x, y, z = LENGTH / 2, WIDTH / 2, HEIGHT / 2
    verts = [(sx * x, sy * y, sz * z) for sx in (-1, 1) for sy in (-1, 1) for sz in (-1, 1)]
    faces = [(0, 1, 3, 2), (4, 6, 7, 5), (0, 4, 5, 1), (2, 3, 7, 6), (0, 2, 6, 4), (1, 5, 7, 3)]
    return verts, faces


def inner_radius(height):
    """How far the bowl's inside reaches from its middle at a height (its profile's inside)."""
    inside = tableware.PROFILE[tableware.PROFILE.index((0.050, 0.0765)) :]
    for (r0, h0), (r1, h1) in zip(inside, inside[1:]):
        if h1 <= height <= h0:
            return r0 + (r1 - r0) * (h0 - height) / (h0 - h1)
    return 0.0


def heap(seed=3):
    rng = random.Random(seed)
    box = plain_fence()
    shape = Shape()
    # Layers from the bottom of the pile up, each fewer and nearer the middle.
    for layer, (count, height) in enumerate([(14, 0.047), (11, 0.052), (8, 0.057), (5, 0.062), (2, 0.066)]):
        reach = max(inner_radius(height) - LENGTH / 2 - 0.004, 0.004) * (1.0 - 0.12 * layer)
        for _ in range(count):
            a = rng.uniform(0, 2 * math.pi)
            d = reach * math.sqrt(rng.uniform(0, 1))
            yaw = Matrix.Rotation(rng.uniform(0, math.pi), 4, "Z")
            tilt = Matrix.Rotation(math.radians(rng.uniform(-14, 14)), 4, "X") @ Matrix.Rotation(
                math.radians(rng.uniform(-10, 10)), 4, "Y"
            )
            at = Matrix.Translation((d * math.cos(a), d * math.sin(a), height + rng.uniform(-0.0012, 0.0012)))
            shape.add(box[0], box[1], None, at @ yaw @ tilt)
    obj = shape.build("Fences", "piece", sharp=30.0)
    return meshkit.box_uv(obj, 1.0, detail=True)


def run():
    bpy.ops.wm.read_factory_settings(use_empty=True)
    meshkit.export_glb(fence(), common.output("models", "fence.glb"))
    bpy.ops.wm.read_factory_settings(use_empty=True)
    tableware.export(heap(), common.output("models", "bowl_fences.glb"))
