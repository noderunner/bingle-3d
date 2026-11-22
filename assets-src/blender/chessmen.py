"""Chessmen: a king, a queen, a bishop, a knight, a rook and a pawn, of the pattern most sets
follow (Staunton's), for the games played with them or with a few of them.

Five are turned, as on a lathe, from a profile written here: a foot, a dome, a stem, collars and
the head that says which it is. What a lathe cannot do is added after: the rook's battlements, the
queen's coronet, the cut in the bishop's mitre, the king's cross. The knight stands on the same
foot; its head is a side view drawn here as an outline, given the thickness a carved head has
(round at its edges, broad at the neck, narrow at the muzzle), with a mane, an eye and a mouth
cut in.

They are made for a 28 mm cell, the king 46 mm tall, each centred on its middle (the client
stands a piece by its foot, which it finds in the file). The knight looks to the far side of the
table (+y here), which is forwards for the first seat. None has a material: an
appearance file says what each seat's are made of. Each is mapped in metres from every side, as
the client maps a wood, each kind from its own place in the wood so no two are alike."""

import math

import bmesh
import bpy
import numpy as np
from mathutils import Matrix, Vector

import common
import meshkit
from meshkit import arc

SEGMENTS = 32

# Each piece's foot, across, in metres: the profiles below are in units of its radius.
RADIUS = {"king": 0.0100, "queen": 0.0096, "bishop": 0.0088, "knight": 0.0088, "rook": 0.0088, "pawn": 0.0076}


def foot(top):
    """The foot every piece stands on, and the dome above it, up to `top` (radius, height)."""
    return [
        [(0.0, 0.0), (0.95, 0.0)],
        [2, (0.99, 0.015), (1.0, 0.06)],
        [(1.0, 0.20)],
        [2, (0.985, 0.25), (0.94, 0.275)],
        [(0.87, 0.285)],
        [2, (0.885, 0.35), (0.84, 0.45), (0.73, 0.56), (0.60, 0.65), top],
    ]


def stem(start, neck, to):
    """A bead on the dome at `start` (radius, height), then the stem, narrowing to `neck` and
    opening a little under the collar at height `to`."""
    r, z = start
    bead = z + 0.11
    span = to - bead
    return [
        [2, (r + 0.045, z + 0.02), (r + 0.045, z + 0.08), (r - 0.02, bead)],
        [
            (r - 0.11, bead + 0.10 * span),
            (neck + 0.03, bead + 0.38 * span),
            (neck, bead + 0.68 * span),
            (neck + 0.015, bead + 0.90 * span),
            (neck + 0.06, to),
        ],
    ]


def collar(inner, at, wide, second=None):
    """A collar of radius `wide` at height `at` on a stem of radius `inner`; a second, smaller
    one above it if asked for. Ends at (inner, the returned height)."""
    runs = [
        [(wide, at + 0.02)],
        [2, (wide + 0.03, at + 0.055), (wide, at + 0.09)],
        [(inner + 0.04, at + 0.115)],
    ]
    top = at + 0.115
    if second:
        runs += [
            [(second, top + 0.02)],
            [2, (second + 0.025, top + 0.05), (second, top + 0.08)],
            [(inner + 0.02, top + 0.105)],
        ]
        top += 0.105
    return runs, top


def pawn():
    runs = foot((0.47, 0.74)) + stem((0.47, 0.74), 0.27, 1.70)
    neck, top = collar(0.33, 1.70, 0.58)
    ball = 0.56
    runs += neck + [arc((0.0, top + 0.50), ball, -62, 90, 10)]
    return meshkit.turned("Pawn", "piece", meshkit.profile(runs, RADIUS["pawn"]), SEGMENTS)


def bishop():
    radius = RADIUS["bishop"]
    runs = foot((0.47, 0.74)) + stem((0.47, 0.74), 0.25, 1.95)
    neck, top = collar(0.31, 1.95, 0.58, 0.46)
    runs += neck + [
        # The mitre: widest low down, drawn up to a point.
        [(0.45, top + 0.20), (0.52, top + 0.50), (0.47, top + 0.86), (0.33, top + 1.20), (0.16, top + 1.46)],
        [(0.10, top + 1.52)],
        arc((0.0, top + 1.64), 0.135, -48, 90, 5),
    ]
    obj = meshkit.turned("Bishop", "piece", meshkit.profile(runs, radius), SEGMENTS)
    # The cut across the mitre: a thin slab, leaning, from its side in towards its middle.
    cut = meshkit.rounded_box("Cut", (0.085 * radius, 3.0 * radius, 1.1 * radius), (0, 0, 0), "piece", radius=0.0)
    cut.matrix_world = (
        Matrix.Translation((0.36 * radius, 0.0, (top + 1.02) * radius)) @ Matrix.Rotation(math.radians(38), 4, "Y")
    )
    meshkit.apply_transform(cut)
    return meshkit.boolean(obj, cut)


def queen():
    runs = foot((0.47, 0.74)) + stem((0.47, 0.74), 0.26, 2.25)
    neck, top = collar(0.32, 2.25, 0.60, 0.47)
    rim = top + 1.02
    runs += neck + [
        # The cup opening to the coronet, its rim, and the crown of the head within.
        [(0.36, top + 0.28), (0.45, top + 0.62), (0.60, top + 0.94)],
        [(0.63, rim)],
        [(0.52, rim)],
        [(0.48, rim - 0.12)],
        [(0.40, rim - 0.06), (0.26, rim + 0.10), (0.13, rim + 0.20)],
        [(0.08, rim + 0.25)],
        arc((0.0, rim + 0.37), 0.13, -52, 90, 5),
    ]
    radius = RADIUS["queen"]
    obj = meshkit.turned("Queen", "piece", meshkit.profile(runs, radius), 40)
    # The coronet's points: the rim rises and falls eight times round.
    for v in obj.data.vertices:
        r = math.hypot(v.co.x, v.co.y)
        if abs(v.co.z - rim * radius) < 1e-6 and r > 0.5 * radius:
            v.co.z += 0.17 * radius * (0.5 + 0.5 * math.cos(8 * math.atan2(v.co.y, v.co.x))) ** 0.7
    return obj


def king():
    radius = RADIUS["king"]
    runs = foot((0.47, 0.74)) + stem((0.47, 0.74), 0.27, 2.40)
    neck, top = collar(0.33, 2.40, 0.62, 0.48)
    band = top + 0.96
    runs += neck + [
        [(0.37, top + 0.28), (0.46, top + 0.62), (0.60, top + 0.92)],
        [(0.64, band)],
        [(0.64, band + 0.11)],
        [2, (0.62, band + 0.15), (0.56, band + 0.17)],
        [(0.44, band + 0.23), (0.25, band + 0.31), (0.11, band + 0.33)],
        [(0.11, band + 0.38)],
        [(0.0, band + 0.38)],
    ]
    body = meshkit.turned("King", "piece", meshkit.profile(runs, radius), SEGMENTS)
    # The cross: an upright and a bar, their ends a little wider than their middles.
    z = (band + 0.34) * radius
    tall, wide, thick = 0.74 * radius, 0.60 * radius, 0.15 * radius
    upright = meshkit.rounded_box("Upright", (0.19 * radius, thick, tall), (0, 0, z + tall / 2), "piece", radius=0.0003)
    bar = meshkit.rounded_box("Bar", (wide, thick, 0.19 * radius), (0, 0, z + 0.62 * tall), "piece", radius=0.0003)
    middle = Vector((0.0, 0.0, z + 0.62 * tall))
    for part, axis in ((upright, 2), (bar, 0)):
        for v in part.data.vertices:
            along = abs(v.co[axis] - middle[axis]) / (0.5 * wide)
            across = 0 if axis == 2 else 2
            v.co[across] = middle[across] + (v.co[across] - middle[across]) * (1.0 + 0.45 * min(along, 1.0) ** 1.5)
    return meshkit.join([body, upright, bar], "King")


def rook():
    radius = RADIUS["rook"]
    top, floor = 2.95, 2.70
    runs = foot((0.62, 0.70)) + [
        [2, (0.665, 0.72), (0.665, 0.78), (0.60, 0.81)],
        [(0.555, 1.00), (0.515, 1.50), (0.50, 1.96)],
    ]
    neck, at = collar(0.50, 1.96, 0.64)
    runs += neck + [
        [(0.55, at + 0.12), (0.62, at + 0.26), (0.71, at + 0.34)],
        [(0.71, floor)],
        [(0.71, top)],
        [(0.53, top)],
        [(0.53, floor)],
        [(0.0, floor)],
    ]
    # Six merlons round the top: where one ends, two vertices close together, so its side is
    # upright.
    angles, gaps = [], []
    merlons, share, edge = 6, 0.60, math.radians(0.4)
    for m in range(merlons):
        start = 2 * math.pi * m / merlons
        solid = 2 * math.pi / merlons * share
        gap = 2 * math.pi / merlons - solid
        angles += [start + solid * i / 4 for i in range(5)]
        inside = [start + solid + edge + (gap - 2 * edge) * i / 2 for i in range(3)]
        angles += inside
        gaps += inside
    obj = meshkit.turned("Rook", "piece", meshkit.profile(runs, radius), 0, angles)
    for v in obj.data.vertices:
        if v.co.z > (floor + 0.01) * radius:
            a = math.atan2(v.co.y, v.co.x) % (2 * math.pi)
            if any(abs(a - g) < 1e-4 for g in gaps):
                v.co.z = (floor + 0.012) * radius
    return obj


# The knight's head from its side, in units of the foot's radius, looking to the right (+x),
# from the front of its chest up and round to the back of its neck. It stands on the foot's top,
# which is at z = 0 here, and goes a little below it.
HEAD = [
    (0.60, -0.10),
    (0.64, 0.10),
    (0.67, 0.30),
    (0.62, 0.56),
    (0.48, 0.82),
    (0.36, 1.04),
    (0.31, 1.22),
    (0.37, 1.30),
    (0.52, 1.22),
    (0.68, 1.11),
    (0.80, 1.07),
    (0.91, 1.11),
    (0.975, 1.24),
    (0.965, 1.38),
    (0.90, 1.52),
    (0.78, 1.74),
    (0.63, 1.98),
    (0.48, 2.20),
    (0.36, 2.34),
    (0.27, 2.43),
    (0.215, 2.60),
    (0.165, 2.76),
    (0.10, 2.66),
    (0.035, 2.50),
    (-0.07, 2.43),
    (-0.26, 2.22),
    (-0.44, 1.88),
    (-0.57, 1.44),
    (-0.645, 0.96),
    (-0.665, 0.50),
    (-0.64, 0.12),
    (-0.62, -0.10),
]
# Where the back of the neck begins in `HEAD` (behind the ear): the mane runs from there down.
MANE_FROM = 24
EYE = (0.42, 1.99)
NOSTRIL = (0.865, 1.33)
MOUTH = ((0.97, 1.185), (0.70, 1.215))
CHEEK = (0.34, 1.60)


def smoothstep(lo, hi, x):
    t = np.clip((x - lo) / (hi - lo), 0.0, 1.0)
    return t * t * (3.0 - 2.0 * t)


def nearest(points, line):
    """Each point's distance to a line of points, and how far along the line that nearest place
    is."""
    a, b = line[:-1], line[1:]
    ab = b - a
    lengths = np.linalg.norm(ab, axis=1)
    starts = np.concatenate([[0.0], np.cumsum(lengths)[:-1]])
    ap = points[:, None, :] - a[None]
    t = np.clip((ap * ab[None]).sum(-1) / np.maximum((ab * ab).sum(-1), 1e-12)[None], 0.0, 1.0)
    d = np.linalg.norm(ap - t[..., None] * ab[None], axis=-1)
    i = d.argmin(1)
    rows = np.arange(len(points))
    return d[rows, i], starts[i] + t[rows, i] * lengths[i]


def horse_head(radius):
    """The head as a mesh in metres, standing on z = 0 and looking along +x."""
    outline = np.array(meshkit.spline(HEAD, 6))
    # A slab with the outline's shape, one unit thick either way, then cut into small even
    # faces all over, so it can be shaped.
    bm = bmesh.new()
    face = bm.faces.new([bm.verts.new((x, -1.0, z)) for x, z in outline])
    made = bmesh.ops.extrude_face_region(bm, geom=[face])
    bmesh.ops.translate(bm, vec=(0.0, 2.0, 0.0), verts=[v for v in made["geom"] if isinstance(v, bmesh.types.BMVert)])
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    obj = meshkit.from_bmesh(bm, "Head", "piece")
    mod = obj.modifiers.new("Remesh", "REMESH")
    mod.mode = "VOXEL"
    mod.voxel_size = 0.02
    mod.adaptivity = 0.0
    bpy.context.view_layer.objects.active = obj
    bpy.ops.object.modifier_apply(modifier=mod.name)

    co = np.array([v.co for v in obj.data.vertices])
    xz = co[:, [0, 2]]
    x, z = xz[:, 0], xz[:, 1]
    edge, _ = nearest(xz, outline)
    back, along = nearest(xz, outline[MANE_FROM * 6 :])

    # How thick it is, half way through: a broad neck, a narrower head, a narrow muzzle, thin
    # ears, and a round cheek.
    thick = 0.52 - 0.17 * smoothstep(0.0, 1.3, z) - 0.05 * smoothstep(1.3, 2.3, z)
    thick -= 0.13 * smoothstep(2.38, 2.76, z)
    thick *= 1.0 - 0.30 * smoothstep(0.50, 0.97, x) * smoothstep(0.9, 1.2, z)
    spot = lambda at, size: np.exp(-(((x - at[0]) ** 2 + (z - at[1]) ** 2) / size**2))
    thick += 0.07 * spot(CHEEK, 0.24)
    # Round at its edge, as a carved thing is: a quarter circle from the outline in.
    reach = np.clip(edge / (0.95 * thick), 0.0, 1.0)
    rounded = np.sqrt(0.10 + 0.90 * (1.0 - (1.0 - reach) ** 2))
    # The mane: a band down the back of the neck, set off by a groove and cut into locks.
    band = 0.25
    in_mane = (back < band) & (z > 0.05)
    groove = 0.16 * np.exp(-(((back - band) / 0.035) ** 2)) * (z > 0.05) * (z < 2.45)
    locks = 0.14 * (0.5 + 0.5 * np.cos(2 * np.pi * (along + 1.1 * back) / 0.30)) ** 2 * in_mane * smoothstep(0.0, 0.08, back)
    # The eye in its socket, the nostril, the mouth.
    eye = 0.30 * spot(EYE, 0.10) - 0.26 * spot(EYE, 0.045)
    nostril = 0.38 * spot(NOSTRIL, 0.055)
    m0, m1 = np.array(MOUTH[0]), np.array(MOUTH[1])
    mouth_d, _ = nearest(xz, np.array([m0, m1]))
    mouth = 0.30 * np.exp(-((mouth_d / 0.035) ** 2))
    half = thick * rounded * (1.0 - groove - locks - eye - nostril - mouth)
    # At the foot the head keeps its breadth: it stands on wood, it does not end in an edge.
    for v, y, h in zip(obj.data.vertices, co[:, 1], half):
        v.co.y = float(y * h)
    obj.data.update()

    bm = bmesh.new()
    bm.from_mesh(obj.data)
    for _ in range(2):
        bmesh.ops.smooth_vert(bm, verts=bm.verts, factor=0.5, use_axis_x=True, use_axis_y=True, use_axis_z=True)
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    bm.to_mesh(obj.data)
    bm.free()
    # Fewer triangles: it is 25 mm tall.
    meshkit.apply_modifier(obj, "DECIMATE", ratio=min(1.0, 3800 / max(meshkit.triangles([obj]), 1)))
    for v in obj.data.vertices:
        v.co *= radius
    return meshkit.finish(obj, "piece", sharp=None)


def knight():
    radius = RADIUS["knight"]
    stand = 0.74
    runs = foot((0.68, 0.61)) + [
        [2, (0.725, 0.63), (0.725, 0.69), (0.67, 0.72)],
        [(0.62, stand)],
        [(0.0, stand)],
    ]
    base = meshkit.turned("Knight", "piece", meshkit.profile(runs, radius), SEGMENTS)
    head = horse_head(radius)
    # Onto the foot, and turned to look to the far side of the table.
    head.matrix_world = Matrix.Translation((0.0, 0.0, stand * radius)) @ Matrix.Rotation(math.radians(90), 4, "Z")
    meshkit.apply_transform(head)
    return meshkit.join([base, head], "Knight")


PIECES = {"king": king, "queen": queen, "bishop": bishop, "knight": knight, "rook": rook, "pawn": pawn}


def make(name):
    """One piece, centred on its middle and mapped for a wood. Returns it and its height."""
    obj = meshkit.triangulate(PIECES[name]())
    meshkit.finish(obj, "piece", sharp=42.0)
    tall = meshkit.centre_height(obj)
    # Each kind from its own place in the wood.
    place = 0.11 * (1 + list(PIECES).index(name))
    return meshkit.box_uv(obj, 1.0, detail=True, offset=(place, 0.37 * place, 0.0)), tall


def run():
    for name in PIECES:
        bpy.ops.wm.read_factory_settings(use_empty=True)
        obj, tall = make(name)
        meshkit.export_glb(obj, common.output("models", "chess", f"{name}.glb"))
        print(f"   {name}: {tall * 1000:.1f} mm tall")
