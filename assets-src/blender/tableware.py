"""What stands on the table beside a board: a bowl for each player's stones.

- `models/bowl.glb`: a turned wooden bowl, as Go stones are kept in: round-bellied, narrower at
  its mouth, on a low foot. It stands on z = 0 about the origin, and is mapped in metres from
  every side, as the client maps a wood (with the finer set for the wood's relief).
- `models/bowl_stones.glb`: the stones in it, heaped, as one mesh about the same origin: the same
  stone as on the board (`stones.py`) with far fewer triangles, since there are dozens. Each is
  mapped flat from above like the board's, so one material serves both.

Neither has a material: a game's appearance file says what the bowl is made of, and the stones
are their seat's."""

import math
import os
import random

import bpy
from mathutils import Matrix, Vector

import common
import meshkit
import stones
from meshkit import Shape

# The bowl's outside from the middle of its foot up to its rim, then its inside down to the
# middle: (radius, height).
PROFILE = [
    (0.0, 0.0),
    (0.036, 0.0),
    (0.041, 0.002),
    (0.043, 0.006),
    (0.052, 0.014),
    (0.062, 0.028),
    (0.066, 0.042),
    (0.064, 0.056),
    (0.058, 0.068),
    (0.054, 0.074),
    (0.052, 0.0765),
    (0.050, 0.0765),
    (0.048, 0.074),
    (0.052, 0.066),
    (0.058, 0.054),
    (0.060, 0.042),
    (0.056, 0.028),
    (0.046, 0.017),
    (0.030, 0.011),
    (0.0, 0.009),
]
SEGMENTS = 48

# The heap: rings of stones, each (how many, distance from the middle, height of their middles).
HEAP = [(1, 0.0, 0.046), (6, 0.023, 0.046), (12, 0.044, 0.048), (4, 0.012, 0.0535), (9, 0.033, 0.0545), (3, 0.013, 0.061), (5, 0.026, 0.0615), (1, 0.002, 0.067)]


def low_stone(segments=18, rings=5):
    """The board's stone with few triangles: its profile, turned."""
    profile = []
    for i in range(rings + 1):
        t = i / rings * math.pi / 2.0
        r = stones.RADIUS * math.sin(t)
        h = stones.HALF_THICKNESS * max(1.0 - (r / stones.RADIUS) ** 2, 0.0) ** stones.ROUNDING
        profile.append((r, h))
    whole = [(r, h) for r, h in profile] + [(r, -h) for r, h in reversed(profile[:-1])]
    verts, faces, _ = meshkit.lathe(whole, segments)
    span = 2.0 * stones.RADIUS / stones.UV_SPAN
    uvs = [[(0.5 + verts[i][0] / span, 0.5 + verts[i][1] / span) for i in f] for f in faces]
    return verts, faces, uvs


def bowl():
    shape = Shape().add(*meshkit.lathe(PROFILE, SEGMENTS))
    obj = shape.build("Bowl", "bowl", sharp=None)
    meshkit.apply_modifier(obj, "WELD", merge_threshold=0.00001)
    meshkit.finish(obj, "bowl", sharp=None)
    return meshkit.box_uv(obj, 1.0, detail=True)


def heap(seed=1):
    rng = random.Random(seed)
    stone = low_stone()
    shape = Shape()
    for count, distance, height in HEAP:
        start = rng.uniform(0, 2 * math.pi)
        for i in range(count):
            a = start + 2 * math.pi * i / count + rng.uniform(-0.12, 0.12)
            d = distance + rng.uniform(-0.002, 0.002)
            # Lying as they fell: tipped, and more so towards the bowl's wall.
            tip = math.radians(rng.uniform(4, 16) + 240 * d)
            lean = Matrix.Rotation(tip, 4, Vector((-math.sin(a), math.cos(a), 0)))
            turn = Matrix.Rotation(rng.uniform(0, 2 * math.pi), 4, "Z")
            at = Matrix.Translation((d * math.cos(a), d * math.sin(a), height + rng.uniform(-0.001, 0.001)))
            shape.add(*stone, at @ lean @ turn)
    return shape.build("Stones", "stones", sharp=None)


def export(obj, path):
    bpy.ops.object.select_all(action="DESELECT")
    obj.select_set(True)
    bpy.context.view_layer.objects.active = obj
    bpy.ops.export_scene.gltf(
        filepath=path,
        export_format="GLB",
        use_selection=True,
        export_tangents=True,
        export_materials="NONE",
        export_extras=False,
        export_yup=True,
    )
    print(f"wrote {os.path.relpath(path, common.REPO)} ({os.path.getsize(path) // 1024} KiB, {meshkit.triangles([obj])} triangles)")


def run():
    bpy.ops.wm.read_factory_settings(use_empty=True)
    export(bowl(), common.output("models", "bowl.glb"))
    export(heap(), common.output("models", "bowl_stones.glb"))
