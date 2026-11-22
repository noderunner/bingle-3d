"""Tak's pieces: a square block of wood, which is a flat stone lying down and a standing stone
(a wall) on its edge. The capstone is a chess pawn (`chessmen.py`), as Tak is often played.

- `models/tak/flat.glb`: the block lying flat, its edges eased.
- `models/tak/wall.glb`: the same block stood on an edge, corner to corner of its square, as a
  wall is set down to tell it from a flat stone at a glance.

Made for a 28 mm cell: the block is 18.5 mm square and 6.5 mm thick, so a stack of five stands
as tall as a cell is wide. Each is centred on its middle, with no material: an appearance file
says what each seat's are made of."""

import math

import bpy
from mathutils import Matrix

import common
import meshkit

SIDE = 0.0185
THICKNESS = 0.0065
EASE = 0.0007

def block(name):
    return meshkit.rounded_box(name, (SIDE, SIDE, THICKNESS), (0, 0, THICKNESS / 2), "piece", radius=EASE, segments=2)


def flat():
    return block("Flat")


def wall():
    obj = block("Wall")
    # On its edge, and turned an eighth: from corner to corner of its square.
    obj.matrix_world = (
        Matrix.Rotation(math.radians(45), 4, "Z")
        @ Matrix.Translation((0, 0, SIDE / 2))
        @ Matrix.Rotation(math.radians(90), 4, "X")
        @ Matrix.Translation((0, 0, -THICKNESS / 2))
    )
    return meshkit.apply_transform(obj)


PIECES = {"flat": flat, "wall": wall}


def make(name):
    """One piece, centred on its middle and mapped for a wood. Returns it and its height."""
    obj = meshkit.triangulate(PIECES[name]())
    meshkit.finish(obj, "piece", sharp=40.0)
    meshkit.flat_faces_stay_flat(obj)
    tall = meshkit.centre_height(obj)
    place = 0.17 * (1 + list(PIECES).index(name))
    return meshkit.box_uv(obj, 1.0, detail=True, offset=(place, 0.29 * place, 0.0)), tall


def run():
    for name in PIECES:
        bpy.ops.wm.read_factory_settings(use_empty=True)
        obj, tall = make(name)
        meshkit.export_glb(obj, common.output("models", "tak", f"{name}.glb"))
        print(f"   {name}: {tall * 1000:.1f} mm tall")
