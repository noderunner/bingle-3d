"""The garden (`environments/garden.glb`): a place to play in, modelled here and exported whole.

The table top is at height 0 with the table's middle at the origin, as the client's boards expect,
and everything is in metres. In Blender's axes x is to the player's right, y away from the player
and z up; the exporter turns them to the client's.

What stands where (the player looks along +y, north):

- under the table, a platform of planks with four and a half tatami mats, a cushion for each seat
  (one in the file, `Cushion`, which the client lays out), on posts over the gravel;
- north, a sea of raked gravel with rocks on islands of moss, the furrows going round them;
- north-east, a maple in autumn, between the table and the low sun, so its shade falls across the
  platform; a stone lantern beside it;
- north-west, a pond among rocks, and by the platform a stone basin fed by a bamboo spout;
- bamboo in the corners, clipped shrubs on a bank of moss along a bamboo fence, and beyond the
  fence trees and hills, which are pictures on cards.

Each part's material slot is named for a material in `assets/environments/garden.toml`, which the
client and the previews here both read. The file is exported with no materials of its own."""

import math
import os

import bpy
from mathutils import Matrix, Vector, noise

import common
import matlib
import meshkit
from meshkit import Shape

# Heights: the tatami's top (the floor one sits on), the planks' top, and the gravel.
FLOOR = -0.33
PLANKS = FLOOR - 0.014
GROUND = -0.80
# The platform and its mats.
DECK = 4.5
MAT = 0.91
# The fence's distance from the table, and its height.
FENCE = 10.5
FENCE_HEIGHT = 1.75
# One copy of the gravel's furrows covers this (`surfaces.py`), eight furrows.
GRAVEL = 0.64

POND = (-5.9, 4.4)
MAPLE = (3.0, 4.3)
# Islands of moss in the gravel, with their rocks: (x, y, radius, height, seed).
ISLANDS = [
    (0.3, 6.0, 1.25, 0.16, 11),
    (4.2, 7.9, 0.85, 0.12, 12),
    (-1.7, 3.5, 0.5, 0.09, 13),
]
RINGS = 6


def ground_at(z):
    return GROUND + z


def table():
    """Low and square: a thick top on four short legs, with a rail between them."""
    size, top, leg = 0.94, 0.042, 0.065
    parts = [meshkit.rounded_box("Table", (size, size, top), (0, 0, -top / 2), "table", 0.008, 3, detail=True)]
    inset = size / 2 - 0.10
    height = -FLOOR - top
    for sx in (-1, 1):
        for sy in (-1, 1):
            parts.append(
                meshkit.rounded_box(
                    "Leg", (leg, leg, height), (sx * inset, sy * inset, -top - height / 2), "table", 0.006, 2, detail=True
                )
            )
    rail = 0.05
    for along_x in (True, False):
        for s in (-1, 1):
            length = 2 * inset - leg
            dims = (length, 0.022, rail) if along_x else (0.022, length, rail)
            at = (0, s * inset, -top - rail / 2 - 0.004) if along_x else (s * inset, 0, -top - rail / 2 - 0.004)
            parts.append(meshkit.rounded_box("Rail", dims, at, "table", 0.003, 1, detail=True))
    return meshkit.join(parts, "Table")


def cushion():
    """A floor cushion: square, plump, stitched down in the middle. It stands on z = 0 about the
    origin; the client lays one out for each seat."""
    size, height, n = 0.56, 0.085, 18
    shape = Shape()

    def surface(sign):
        verts = []
        for j in range(n + 1):
            for i in range(n + 1):
                u, v = i / n * 2 - 1, j / n * 2 - 1
                # A pillow: full in the middle of each side, drawn in at the corners.
                edge = (1 - abs(u) ** 4) ** 0.5 * (1 - abs(v) ** 4) ** 0.5
                pinch = 1.0 - 0.45 * math.exp(-(u * u + v * v) / 0.03)
                z = height / 2 * (1 + sign * edge**0.6 * pinch)
                corner = 1.0 - 0.06 * (abs(u) * abs(v)) ** 2
                verts.append((u * size / 2 * corner, v * size / 2 * corner, z))
        return verts

    for sign in (1, -1):
        verts = surface(sign)
        faces, uvs = [], []
        for j in range(n):
            for i in range(n):
                a, b = j * (n + 1) + i, j * (n + 1) + i + 1
                c, d = (j + 1) * (n + 1) + i + 1, (j + 1) * (n + 1) + i
                f = (a, b, c, d) if sign > 0 else (a, d, c, b)
                faces.append(f)
                uvs.append([(verts[k][0], verts[k][1]) for k in f])
        shape.add(verts, faces, uvs)
    obj = shape.build("Cushion", "cushion", sharp=None)
    meshkit.apply_modifier(obj, "WELD", merge_threshold=0.0005)
    return obj


def mats():
    """Four and a half mats, laid as a tea room's are: four round a half mat, no four corners
    meeting. A mat's rush runs across it and its cloth borders along its long sides."""
    h = MAT / 2
    # (x0, y0, x1, y1): the long side first decides which way the borders run.
    layout = [
        (-3 * h, h, h, 3 * h),
        (h, -h, 3 * h, 3 * h),
        (-h, -3 * h, 3 * h, -h),
        (-3 * h, -3 * h, -h, h),
        (-h, -h, h, h),
    ]
    thick = FLOOR - PLANKS
    rush, cloth = Shape(), Shape()
    gap, border = 0.002, 0.03
    for x0, y0, x1, y1 in layout:
        x0, y0, x1, y1 = x0 + gap, y0 + gap, x1 - gap, y1 - gap
        long_x = (x1 - x0) >= (y1 - y0)
        verts = [(x0, y0, FLOOR), (x1, y0, FLOOR), (x1, y1, FLOOR), (x0, y1, FLOOR)]

        # The scan holds two mats one above the other with their borders: one mat's rush is the
        # band between them, its long side along u.
        def st(x, y):
            along, across = (x - x0, (y - y0) / (y1 - y0)) if long_x else (y - y0, (x - x0) / (x1 - x0))
            return (along / 1.9, 0.04 + 0.42 * across)

        rush.add(verts, [(0, 1, 2, 3)], [[st(x, y) for x, y, _ in verts]])
        # Its sides, down to the planks.
        low = [(x, y, PLANKS) for x, y, _ in verts]
        for i in range(4):
            j = (i + 1) % 4
            rush.add([verts[i], verts[j], low[j], low[i]], [(3, 2, 1, 0)], [[(0.1, 0.1)] * 4])
        # The borders: along the long sides, a hair above the rush.
        z = FLOOR + 0.0008
        for side in (0, 1):
            if long_x:
                ya = y0 if side == 0 else y1 - border
                strip = [(x0, ya, z), (x1, ya, z), (x1, ya + border, z), (x0, ya + border, z)]
            else:
                xa = x0 if side == 0 else x1 - border
                strip = [(xa, y0, z), (xa + border, y0, z), (xa + border, y1, z), (xa, y1, z)]
            cloth.add(strip, [(0, 1, 2, 3)], [[(x, y) for x, y, _ in strip]])
    return [rush.build("Mats", "tatami", smooth=False), cloth.build("Mat borders", "tatami_border", smooth=False)]


def deck():
    """The platform: planks, a beam round its edge, short posts on stones, and a stone to step
    down onto on the two sides a path leaves from."""
    half, thick = DECK / 2, 0.045
    parts = [
        meshkit.rounded_box(
            "Planks", (DECK, DECK, thick), (0, 0, PLANKS - thick / 2), "deck", 0.004, 1, metres=1.0
        )
    ]
    beams = []
    beam = 0.13
    for along_x in (True, False):
        for s in (-1, 1):
            dims = (DECK + 0.04, 0.07, beam) if along_x else (0.07, DECK + 0.04, beam)
            at = (0, s * (half - 0.02), PLANKS - thick - beam / 2 + 0.01)
            at = at if along_x else (at[1], 0, at[2])
            beams.append(meshkit.rounded_box("Beam", dims, at, "timber", 0.006, 1))
    post_top = PLANKS - thick - beam + 0.02
    stones = []
    for ix in (-1, 0, 1):
        for iy in (-1, 0, 1):
            x, y = ix * (half - 0.12), iy * (half - 0.12)
            height = post_top - (GROUND + 0.09)
            beams.append(
                meshkit.rounded_box("Post", (0.11, 0.11, height), (x, y, post_top - height / 2), "timber", 0.006, 1)
            )
            stones.append(
                meshkit.rounded_box("Footing", (0.26, 0.26, 0.12), (x, y, GROUND + 0.04), "stone", 0.03, 2, metres=1.0)
            )
    # Stepping down: a long flat stone before the south and the east sides.
    for at, dims in (((0, -half - 0.42, GROUND + 0.11), (1.25, 0.62, 0.26)), ((half + 0.42, -0.3, GROUND + 0.11), (0.62, 1.1, 0.26))):
        step = meshkit.rounded_box("Step", dims, at, "stone", 0.06, 3, metres=1.0)
        roughen(step, 0.012, 2.5)
        stones.append(step)
    return [parts[0], meshkit.join(beams, "Posts and beams"), meshkit.join(stones, "Footings")]


def roughen(obj, amount, scale, seed=0.0):
    """Moves each vertex in and out a little by noise, so cut stone is not machined."""
    for v in obj.data.vertices:
        n = noise.noise(Vector((v.co.x, v.co.y, v.co.z + seed)) * scale)
        v.co += v.normal * n * amount
    obj.data.update()
    return obj


def pond_outline():
    rng = meshkit.rng(7)
    return [(x * 1.25 + POND[0], y * 0.95 + POND[1]) for x, y in meshkit.blob_outline(rng, 2.0, 0.22, 56)]


def scaled(outline, centre, factor):
    return [(centre[0] + (x - centre[0]) * factor, centre[1] + (y - centre[1]) * factor) for x, y in outline]


def gravel(pond):
    """The gravel: flat, its furrows running east and west, with a hole where the pond is."""
    cell, half = 0.5, FENCE + 0.5
    n = int(round(2 * half / cell))
    hole = scaled(pond, POND, 1.16)
    shape = Shape()
    for j in range(n):
        for i in range(n):
            x0, y0 = -half + i * cell, -half + j * cell
            if meshkit.inside(hole, x0 + cell / 2, y0 + cell / 2):
                continue
            verts = [(x0, y0, GROUND), (x0 + cell, y0, GROUND), (x0 + cell, y0 + cell, GROUND), (x0, y0 + cell, GROUND)]
            shape.add(verts, [(0, 1, 2, 3)], [[(x, y) for x, y, _ in verts]])
    obj = shape.build("Gravel", "gravel", smooth=False)
    meshkit.apply_modifier(obj, "WELD", merge_threshold=0.001)
    return obj


def island_outline(x, y, radius, seed):
    rng = meshkit.rng(seed)
    return meshkit.blob_outline(rng, radius, 0.16, 48)


def islands():
    """Each island of moss, and round it rings of furrows, as a rake drawn round the island leaves
    them: laid a hair over the straight furrows, which stop at the outermost ring."""
    moss, rings = Shape(), Shape()
    for x, y, radius, height, seed in ISLANDS:
        outline = island_outline(x, y, radius, seed)
        rng = meshkit.rng(seed)
        verts, faces, uvs = meshkit.mound(outline, height, 5, rng=rng, roughness=0.12, skirt=0.02)
        moss.add(verts, faces, uvs, Matrix.Translation((x, y, GROUND)))
        # The rings: each vertex of the outline pushed out along its own direction.
        pitch = GRAVEL / 8.0
        width = RINGS * pitch
        count = len(outline)
        around = max(1, round(2 * math.pi * (radius + width / 2) / GRAVEL))
        steps = 3
        ring_verts = []
        for s in range(steps + 1):
            d = -0.05 + (width + 0.05) * s / steps
            for px, py in outline:
                r = math.hypot(px, py)
                k = (r + d) / r
                ring_verts.append((x + px * k, y + py * k, GROUND + 0.004))
        faces, uvs = [], []
        for s in range(steps):
            for i in range(count):
                j = (i + 1) % count
                f = (s * count + i, (s + 1) * count + i, (s + 1) * count + j, s * count + j)
                # u goes round a whole number of times; v counts furrows outwards (in metres).
                d0 = -0.05 + (width + 0.05) * s / steps
                d1 = -0.05 + (width + 0.05) * (s + 1) / steps
                u0, u1 = i / count * around * GRAVEL, (i + 1) / count * around * GRAVEL
                faces.append(f)
                uvs.append([(u0, d0), (u0, d1), (u1, d1), (u1, d0)])
        rings.add(ring_verts, faces, uvs)
    return [moss.build("Islands", "moss"), rings.build("Raked rings", "gravel", smooth=False)]


def pond(outline):
    """The pond: its bed sloping down from the bank, the water a little below the gravel, and a
    bank of moss round it that hides where the gravel stops."""
    cx, cy = POND
    local = [(x - cx, y - cy) for x, y in outline]
    bed = Shape()
    verts, faces, uvs = meshkit.mound(scaled(local, (0, 0), 1.12), -0.55, 5)
    # A bowl: the mound turned over, its rim at the gravel.
    verts = [(x, y, -0.55 - z) for x, y, z in verts]
    faces = [tuple(reversed(f)) for f in faces]
    uvs = [list(reversed(u)) for u in uvs]
    bed.add(verts, faces, uvs, Matrix.Translation((cx, cy, GROUND + 0.02)))

    water = Shape()
    level = GROUND - 0.07
    rim = [(x, y, level) for x, y in outline]
    water.add([(cx, cy, level)] + rim, [(0, 1 + i, 1 + (i + 1) % len(rim)) for i in range(len(rim))])
    water.uvs = [[(water.verts[k][0], water.verts[k][1]) for k in f] for f in water.faces]

    bank = Shape()
    count = len(local)
    profile = [(0.90, -0.10), (0.97, 0.03), (1.06, 0.075), (1.17, 0.05), (1.28, -0.02)]
    verts = []
    for k, z in profile:
        for i, (x, y) in enumerate(local):
            bump = 0.02 * noise.noise(Vector((x * 1.7, y * 1.7, k * 3.0)))
            verts.append((cx + x * k, cy + y * k, GROUND + z + (bump if 0 < profile.index((k, z)) < 4 else 0.0)))
    faces = []
    for s in range(len(profile) - 1):
        for i in range(count):
            j = (i + 1) % count
            faces.append((s * count + i, (s + 1) * count + i, (s + 1) * count + j, s * count + j))
    bank.add(verts, faces, [[(verts[k][0], verts[k][1]) for k in f] for f in faces])
    # On the water, lily pads: round leaves with a notch, in drifts. In it, carp, seen from above.
    rng = meshkit.rng(53)
    pads, fish = Shape(), Shape()
    spots = []
    while len(spots) < 16:
        a, r = rng.uniform(0, 2 * math.pi), rng.uniform(0.2, 0.8)
        k = rng.randrange(len(local))
        x, y = local[k][0] * r, local[k][1] * r
        if all(math.hypot(x - px, y - py) > 0.22 for px, py in spots):
            spots.append((x, y))
    for x, y in spots:
        size, turn = rng.uniform(0.07, 0.13), rng.uniform(0, 2 * math.pi)
        rim = [(0.0, 0.0)] + [
            (size * math.cos(turn + 0.25 + (2 * math.pi - 0.5) * i / 12), size * math.sin(turn + 0.25 + (2 * math.pi - 0.5) * i / 12))
            for i in range(13)
        ]
        verts = [(cx + x + px, cy + y + py, level + 0.004) for px, py in rim]
        faces = [(0, i, i + 1) for i in range(1, 13)]
        pads.add(verts, faces, [[(verts[v][0] * 3, verts[v][1] * 3) for v in f] for f in faces])
    for x, y, turn, length in ((0.5, 0.2, 0.6, 0.34), (-0.7, -0.3, 2.4, 0.28), (0.1, -0.6, 4.4, 0.38), (-0.2, 0.7, 3.3, 0.25)):
        # A body that thickens behind the head and tapers to the tail, which fans out.
        spine = [(-0.5, 0.0), (-0.42, 0.045), (-0.2, 0.085), (0.1, 0.07), (0.33, 0.03), (0.42, 0.02), (0.5, 0.09)]
        bend = rng.uniform(-0.35, 0.35)
        top, left, right = [], [], []
        for t, w in spine:
            sway = bend * t * t * length
            c, sn = math.cos(turn), math.sin(turn)
            for side, row in ((0.0, top), (1.0, left), (-1.0, right)):
                lx, ly = t * length, sway + side * w * length
                row.append((cx + x + lx * c - ly * sn, cy + y + lx * sn + ly * c, level - 0.035 - (0.0 if side == 0.0 else 0.02)))
        base = len(fish.verts)
        n = len(spine)
        verts = top + left + right
        faces = []
        for i in range(n - 1):
            faces.append((i, n + i, n + i + 1, i + 1))
            faces.append((i, i + 1, 2 * n + i + 1, 2 * n + i))
        fish.add(verts, faces, [[(0.5, 0.5)] * 4 for _ in faces])
    return [
        bed.build("Pond bed", "pond_bed"),
        water.build("Pond", "water", smooth=False),
        bank.build("Pond bank", "moss"),
        pads.build("Lily pads", "lily", smooth=False),
        fish.build("Carp", "carp"),
    ]


def rock_meshes():
    """The scanned rocks, each made lighter (a few thousand triangles) and stood on its base
    about its own middle. Returns their meshes, largest first as the set has them."""
    before = set(bpy.data.objects)
    bpy.ops.import_scene.gltf(filepath=common.source("rock_moss_set_01/rock_moss_set_01_2k.gltf"))
    rocks = sorted((o for o in bpy.data.objects if o not in before and o.type == "MESH"), key=lambda o: o.name)
    meshes = []
    for obj in rocks:
        obj.parent = None
        meshkit.apply_transform(obj)
        tris = sum(len(p.vertices) - 2 for p in obj.data.polygons)
        meshkit.apply_modifier(obj, "DECIMATE", ratio=min(1.0, 2600.0 / tris))
        xs = [v.co.x for v in obj.data.vertices]
        ys = [v.co.y for v in obj.data.vertices]
        zs = [v.co.z for v in obj.data.vertices]
        middle = Vector(((min(xs) + max(xs)) / 2, (min(ys) + max(ys)) / 2, min(zs)))
        for v in obj.data.vertices:
            v.co -= middle
        meshkit.finish(obj, "rock", sharp=None)
        obj.data.name = obj.name.replace("rock_moss_set_01_", "Rock ")
        meshes.append(obj.data)
    for obj in rocks:
        bpy.data.objects.remove(obj)
    for o in list(bpy.data.objects):
        if o not in before:
            bpy.data.objects.remove(o)
    return meshes


def put(mesh, name, at, yaw=0.0, scale=1.0, tilt=(0.0, 0.0)):
    obj = bpy.data.objects.new(name, mesh)
    bpy.context.scene.collection.objects.link(obj)
    obj.location = Vector(at)
    obj.rotation_euler = (tilt[0], tilt[1], yaw)
    obj.scale = (scale, scale, scale) if not isinstance(scale, (tuple, list)) else scale
    return obj


def rocks(meshes, pond_edge):
    """The rocks. On each island a tall one, a lower one leaning to it and a flat one, as such
    groups are set; small ones round the pond; one by the basin."""
    out = []
    # (mesh, x, y, sink, yaw, scale): the set's rocks are two or three metres across.
    groups = [
        (3, 0.15, 6.15, 0.10, 0.4, 0.62),
        (0, 0.95, 5.55, 0.08, 2.1, 0.30),
        (5, -0.55, 5.75, 0.05, 4.0, 0.24),
        (4, 4.25, 8.0, 0.08, 1.2, 0.42),
        (2, 3.75, 7.55, 0.05, 3.3, 0.22),
        (1, -1.7, 3.5, 0.06, 5.0, 0.20),
        (2, 4.75, 3.35, 0.05, 0.7, 0.20),
        (5, -3.25, 0.25, 0.04, 2.6, 0.17),
        (0, -8.6, 6.6, 0.10, 1.0, 0.5),
        (4, -7.9, 7.3, 0.06, 3.9, 0.26),
    ]
    for i, (m, x, y, sink, yaw, scale) in enumerate(groups):
        out.append(put(meshes[m], f"Rock {i}", (x, y, GROUND - sink), yaw, scale))
    rng = meshkit.rng(23)
    count = len(pond_edge)
    for i in range(15):
        x, y = pond_edge[(i * count // 15 + rng.randrange(3)) % count]
        k = rng.uniform(1.0, 1.12)
        x, y = POND[0] + (x - POND[0]) * k, POND[1] + (y - POND[1]) * k
        scale = rng.uniform(0.10, 0.26)
        out.append(
            put(
                meshes[rng.randrange(len(meshes))],
                f"Pond rock {i}",
                (x, y, GROUND - 0.06 - rng.uniform(0, 0.05)),
                rng.uniform(0, 2 * math.pi),
                (scale, scale, scale * rng.uniform(0.7, 1.1)),
            )
        )
    return out


def stepping_stones():
    """Flat stones to walk on, three shapes of them, along the paths that leave the platform."""
    rng = meshkit.rng(17)
    variants = []
    for k in range(3):
        outline = meshkit.blob_outline(rng, 0.24 + 0.04 * k, 0.14, 20, lobes=(2, 3, 4))
        verts, faces, uvs = meshkit.mound(outline, 0.022, 3)
        shape = Shape().add(verts, faces, uvs, Matrix.Translation((0, 0, 0.07)))
        # Its side, down into the ground.
        n = len(outline)
        side = [(x, y, 0.07) for x, y in outline] + [(x * 0.94, y * 0.94, -0.03) for x, y in outline]
        faces = [(i, n + i, n + (i + 1) % n, (i + 1) % n) for i in range(n)]
        shape.add(side, faces, [[(side[v][0] + side[v][2], side[v][1]) for v in f] for f in faces])
        obj = shape.build(f"Stepping stone {k}", "stone", sharp=50)
        meshkit.apply_modifier(obj, "WELD", merge_threshold=0.0005)
        variants.append(obj.data)
        bpy.data.objects.remove(obj)
    paths = [
        # South, from the step to the gate, wandering.
        [(0.1, -3.55), (-0.25, -4.2), (0.2, -4.85), (-0.15, -5.5), (0.3, -6.15), (0.0, -6.8), (0.35, -7.45), (0.1, -8.1), (0.3, -8.75), (0.1, -9.4)],
        # East, to the lantern and the maple.
        [(3.5, -0.35), (4.15, -0.05), (4.8, 0.35), (5.35, 0.85)],
    ]
    out = []
    for path in paths:
        for x, y in path:
            mesh = variants[rng.randrange(3)]
            out.append(put(mesh, "Stepping stone", (x, y, GROUND), rng.uniform(0, 6.28), rng.uniform(0.9, 1.15)))
    return out


def lantern(at):
    """A stone lantern: a base, a shaft, a platform, the box the light is in between its posts,
    a roof with upturned eaves and a jewel on top. Its paper glows."""
    stone, glow = Shape(), Shape()
    six = lambda profile, **kw: meshkit.lathe(profile, 6, **kw)
    z = 0.0
    stone.add(*six([(0.0, z), (0.30, z), (0.30, z + 0.07), (0.23, z + 0.12), (0.0, z + 0.12)]))
    z += 0.12
    stone.add(*meshkit.lathe([(0.15, z), (0.10, z + 0.05), (0.085, z + 0.10), (0.085, z + 0.70), (0.11, z + 0.76)], 12))
    z += 0.76
    stone.add(*six([(0.0, z), (0.16, z), (0.27, z + 0.06), (0.27, z + 0.10), (0.0, z + 0.10)]))
    z += 0.10
    box = 0.24
    # The light's box: six posts, and the paper within.
    for i in range(6):
        a = math.pi / 3 * i
        x, y = 0.17 * math.cos(a), 0.17 * math.sin(a)
        post = meshkit.lathe([(0.0, 0.0), (0.024, 0.0), (0.024, box), (0.0, box)], 4)
        stone.add(*post, Matrix.Translation((x, y, z)) @ Matrix.Rotation(a + math.pi / 4, 4, "Z"))
    glow.add(*six([(0.0, z + 0.01), (0.155, z + 0.01), (0.155, z + box - 0.01), (0.0, z + box - 0.01)]))
    z += box
    stone.add(*six([(0.0, z), (0.20, z), (0.40, z - 0.015), (0.41, z + 0.02), (0.24, z + 0.11), (0.10, z + 0.19), (0.06, z + 0.22), (0.0, z + 0.22)]))
    z += 0.22
    stone.add(*meshkit.lathe([(0.0, z), (0.05, z), (0.085, z + 0.05), (0.06, z + 0.11), (0.015, z + 0.16), (0.0, z + 0.17)], 10))
    obj = stone.build("Lantern", "stone", sharp=35)
    meshkit.box_uv(obj, 1.0)
    roughen(obj, 0.004, 9.0)
    paper = glow.build("Lantern light", "lantern_glow", smooth=False)
    for o in (obj, paper):
        o.location = Vector((at[0], at[1], GROUND - 0.02))
        o.rotation_euler = (0, 0, 0.3)
    return [obj, paper]


def basin(at):
    """A stone basin of water, a bamboo spout running into it, and a ladle laid across."""
    x, y = at
    stone = Shape()
    stone.add(
        *meshkit.lathe(
            [(0.0, -0.05), (0.27, -0.05), (0.32, 0.12), (0.30, 0.30), (0.26, 0.36), (0.21, 0.36), (0.19, 0.26), (0.0, 0.24)],
            18,
        )
    )
    bowl = stone.build("Basin", "stone", sharp=60)
    meshkit.box_uv(bowl, 1.0)
    roughen(bowl, 0.018, 4.0, seed=3.0)
    bowl.location = Vector((x, y, GROUND))

    water = Shape()
    water.add(*meshkit.lathe([(0.0, 0.325), (0.215, 0.325)], 18))
    water.uvs = [[(water.verts[k][0], water.verts[k][1]) for k in f] for f in water.faces]
    surface = water.build("Basin water", "water", smooth=False)
    surface.location = Vector((x, y, GROUND))

    cane = Shape()
    post = (-0.42, 0.18)
    top = 0.78
    cane.add(*meshkit.tube([(post[0], post[1], -0.1), (post[0], post[1], top)], [0.032, 0.03], 10, v_scale=0.96))
    # The spout: from the post out over the water, tipping down.
    end = (-0.06, 0.03, top - 0.20)
    start = (post[0] - 0.05, post[1] + 0.02, top - 0.10)
    cane.add(*meshkit.tube([start, end], [0.019, 0.017], 8, v_scale=0.96))
    # The ladle: a cup on a long handle, across the rim.
    cane.add(*meshkit.tube([(0.30, -0.16, 0.375), (-0.10, 0.10, 0.385)], [0.006, 0.006], 6, v_scale=0.96))
    cane.add(*meshkit.lathe([(0.0, 0.0), (0.034, 0.0), (0.034, 0.055), (0.030, 0.055), (0.030, 0.006), (0.0, 0.006)], 10), Matrix.Translation((-0.13, 0.12, 0.36)))
    pipes = cane.build("Spout", "bamboo", sharp=50)
    pipes.location = Vector((x, y, GROUND))

    trickle = Shape()
    trickle.add(*meshkit.tube([end, (end[0] + 0.012, end[1] - 0.004, 0.33)], [0.004, 0.0035], 6, v_scale=0.2, cap_end=False))
    thread = trickle.build("Trickle", "trickle")
    thread.location = Vector((x, y, GROUND))
    return [bowl, surface, pipes, thread]


def maple(at):
    """A maple: a leaning trunk dividing into limbs that spread wide and flat, and at the twigs'
    ends cards with a spray of leaves on each, most lying flat in tiers as a maple's do."""
    rng = meshkit.rng(5)
    wood, leaves = Shape(), Shape()
    tips = []
    deepest = 5

    def random_unit():
        while True:
            v = Vector((rng.uniform(-1, 1), rng.uniform(-1, 1), rng.uniform(-1, 1)))
            if 0.1 < v.length < 1.0:
                return v.normalized()

    def branch(start, direction, length, radius, depth):
        steps = 5 if depth < 3 else 4
        points, radii = [Vector(start)], [radius]
        d = Vector(direction).normalized()
        end_radius = radius * (0.62 if depth < deepest else 0.3)
        for i in range(steps):
            d = d + random_unit() * (0.16 if depth > 0 else 0.07)
            # Limbs level out as they go; twigs lift a little towards the light.
            if depth >= 1:
                d.z = d.z * 0.82 + 0.05
            d.normalize()
            points.append(points[-1] + d * (length / steps))
            radii.append(radius + (end_radius - radius) * (i + 1) / steps)
        sides = 9 if depth == 0 else 7 if depth == 1 else 5 if depth <= 3 else 3
        wood.add(*meshkit.tube(points, radii, sides, v_scale=0.7, u_repeats=1 if depth < 2 else 0.5))
        if depth >= 2:
            for p in points[(2 if depth == 2 else 1):]:
                tips.append((p, d.copy(), depth))
        if depth == deepest:
            return
        kids = 4 if depth == 0 else 3 if depth == 1 else rng.choice((2, 2, 3))
        for k in range(kids):
            last = k == kids - 1
            # The last carries on from the end; the others leave along the upper half.
            i = steps if last else rng.randrange(steps // 2, steps + 1)
            spread = math.radians(rng.uniform(12, 24) if last else rng.uniform(32, 58))
            axis = d.cross(random_unit()).normalized()
            child = Matrix.Rotation(spread, 3, axis) @ d
            if depth == 0:
                # The limbs go out all round.
                turn = 2 * math.pi * k / kids + rng.uniform(-0.4, 0.4)
                child = Vector((math.cos(turn), math.sin(turn), 0.62)).normalized()
            branch(points[i], child, length * rng.uniform(0.62, 0.78), radii[i] * (0.9 if last else 0.72), depth + 1)

    lean = Vector((-0.16, -0.10, 1.0))
    branch((0, 0, -0.1), lean, 1.7, 0.13, 0)

    for p, d, depth in tips:
        for _ in range(1 if depth < deepest else 2):
            size = rng.uniform(0.45, 0.68)
            # Not all at the twig: a crown is full between its twigs too.
            p = Vector(p) + Vector((rng.uniform(-0.22, 0.22), rng.uniform(-0.22, 0.22), rng.uniform(-0.08, 0.14)))
            out = Vector((d.x, d.y, 0.0))
            out = out.normalized() if out.length > 0.05 else Vector((1, 0, 0))
            turn = Matrix.Rotation(rng.uniform(-1.2, 1.2), 3, "Z")
            up = turn @ out
            if rng.random() < 0.7:
                # Flat, tipping a little: the spray's stem runs outwards.
                up = (up + Vector((0, 0, rng.uniform(-0.35, 0.25)))).normalized()
                right = up.cross(Vector((0, 0, 1))).normalized()
            else:
                # Hanging: seen from the side.
                right = up.copy()
                up = (Vector((0, 0, -1)) + up * rng.uniform(0.2, 0.7)).normalized()
                right = (right - up * right.dot(up)).normalized()
            centre = Vector(p) + up * size * 0.48
            leaves.add(*meshkit.quad(centre, right, up, (size, size)))
    trunk = wood.build("Maple", "bark", sharp=None)
    crown = leaves.build("Maple leaves", "maple_leaves", smooth=False)
    for o in (trunk, crown):
        o.location = Vector((at[0], at[1], GROUND))
    return [trunk, crown]


def bamboo():
    """Bamboo in clumps: three canes modelled, each with its leaves, and stood many times over."""
    rng = meshkit.rng(29)
    variants = []
    for k in range(3):
        height = 5.5 + 1.2 * k
        cane, leaves = Shape(), Shape()
        bend = Vector((rng.uniform(-1, 1), rng.uniform(-1, 1), 0)).normalized() * rng.uniform(0.5, 1.1)
        points, radii = [], []
        steps = 12
        for i in range(steps + 1):
            t = i / steps
            # Upright below, bowing over at the top under its leaves.
            points.append(Vector((0, 0, height * t)) + bend * (t**2.6))
            radii.append(0.036 * (1.0 - 0.75 * t))
        cane.add(*meshkit.tube(points, radii, 6, v_scale=0.96))
        for i in range(4, steps + 1):
            for _ in range(2 if i < steps else 3):
                a = rng.uniform(0, 2 * math.pi)
                out = Vector((math.cos(a), math.sin(a), 0))
                up = (out + Vector((0, 0, rng.uniform(-0.5, 0.35)))).normalized()
                right = up.cross(Vector((0, 0, 1))).normalized()
                if rng.random() < 0.5:
                    right = up.cross(right).normalized()
                size = rng.uniform(0.7, 1.05)
                leaves.add(*meshkit.quad(points[i] + up * size * 0.48, right, up, (size, size)))
        variants.append((cane.build(f"Cane {k}", "bamboo_cane", sharp=None).data, leaves.build(f"Cane leaves {k}", "bamboo_leaves", smooth=False).data))
    for o in [o for o in bpy.data.objects if o.name.startswith("Cane")]:
        bpy.data.objects.remove(o)
    # Clumps: (x, y, canes, spread).
    clumps = [(-8.6, 8.4, 9, 1.3), (-9.2, 5.0, 6, 0.9), (-8.9, 1.2, 7, 1.0), (8.4, 8.9, 8, 1.2), (9.1, 5.6, 5, 0.8), (-9.0, -6.0, 6, 1.0), (8.8, -7.5, 7, 1.1), (6.2, 9.4, 5, 0.9)]
    out = []
    for x, y, count, spread in clumps:
        for _ in range(count):
            px, py = x + rng.gauss(0, spread * 0.5), y + rng.gauss(0, spread * 0.5)
            px, py = max(-FENCE + 0.3, min(FENCE - 0.3, px)), max(-FENCE + 0.3, min(FENCE - 0.3, py))
            yaw, scale = rng.uniform(0, 2 * math.pi), rng.uniform(0.8, 1.1)
            cane, leaves = variants[rng.randrange(3)]
            out.append(put(cane, "Bamboo", (px, py, GROUND), yaw, scale))
            out.append(put(leaves, "Bamboo leaves", (px, py, GROUND), yaw, scale))
    return out


def shrubs():
    """Clipped shrubs: rounded, close-leaved, three shapes of them along the bank and by the
    rocks."""
    rng = meshkit.rng(37)
    variants = []
    for k in range(3):
        bpy.ops.mesh.primitive_ico_sphere_add(subdivisions=3, radius=1.0)
        obj = bpy.context.object
        for v in obj.data.vertices:
            p = v.co.copy()
            lump = 1.0 + 0.16 * noise.noise(p * 1.6 + Vector((k * 7.0, 0, 0))) + 0.05 * noise.noise(p * 5.0)
            v.co = Vector((p.x * lump, p.y * lump, max(p.z * 0.62 * lump, -0.12)))
        meshkit.finish(obj, "hedge", sharp=None)
        meshkit.box_uv(obj, 1.0)
        variants.append(obj.data)
        bpy.data.objects.remove(obj)
    out = []
    spots = []
    # Along the fence, on the bank; fewer to the south, where the gate is.
    for side in range(4):
        for i in range(9):
            t = -FENCE + 1.2 + i * (2 * FENCE - 2.4) / 8 + rng.uniform(-0.5, 0.5)
            d = FENCE - rng.uniform(0.7, 1.3)
            x, y = ((t, d), (d, t), (t, -d), (-d, t))[side]
            if side == 2 and abs(x) < 1.6:
                continue
            spots.append((x, y, rng.uniform(0.45, 0.95)))
    spots += [(1.5, 6.35, 0.42), (-0.9, 6.6, 0.3), (4.9, 7.6, 0.33), (5.3, 3.9, 0.45), (2.6, 4.9, 0.36), (-2.6, 7.2, 0.5), (-8.3, 2.3, 0.55), (-3.0, 5.6, 0.36), (-4.4, 7.0, 0.42)]
    for x, y, size in spots:
        out.append(
            put(
                variants[rng.randrange(3)],
                "Shrub",
                (x, y, GROUND + 0.02),
                rng.uniform(0, 2 * math.pi),
                (size * rng.uniform(0.9, 1.2), size * rng.uniform(0.9, 1.2), size * rng.uniform(0.85, 1.1)),
            )
        )
    return out


def bank():
    """A bank of moss along the foot of the fence, its edge wandering into the gravel."""
    shape = Shape()
    rng = meshkit.rng(43)
    phase = [rng.uniform(0, 6.28) for _ in range(4)]
    steps = 60
    for side in range(4):
        rows = []
        for i in range(steps + 1):
            t = -FENCE + 2 * FENCE * i / steps
            wide = 1.5 + 0.5 * math.sin(t * 0.9 + phase[side]) + 0.3 * math.sin(t * 2.3 + phase[side] * 2)
            profile = [(0.0, 0.22), (wide * 0.5, 0.17), (wide * 0.85, 0.06), (wide, -0.02)]
            row = []
            for d, z in profile:
                u, v = t, FENCE - d
                x, y = ((u, v), (v, -u), (-u, -v), (-v, u))[side]
                row.append((x, y, GROUND + z))
            rows.append(row)
        base = len(shape.verts)
        verts = [p for row in rows for p in row]
        faces = []
        for i in range(steps):
            for k in range(3):
                a, b = i * 4 + k, i * 4 + k + 1
                faces.append((a, b, b + 4, a + 4))
        shape.add(verts, faces, [[(verts[v][0], verts[v][1]) for v in f] for f in faces])
    return shape.build("Bank", "moss")


def fence():
    """A bamboo fence round the garden: panels of upright canes between posts, under a rail, with
    a gate's opening to the south between two taller posts."""
    panels, frame = Shape(), Shape()
    span = 1.75
    count = int(round(2 * FENCE / span))
    span = 2 * FENCE / count
    for side in range(4):
        for i in range(count):
            a, b = -FENCE + i * span, -FENCE + (i + 1) * span
            if side == 2 and abs((a + b) / 2) < 0.8:
                continue

            def at(t, z, side=side):
                return ((t, FENCE, z), (FENCE, -t, z), (-t, -FENCE, z), (-FENCE, t, z))[side]

            verts = [at(a, GROUND), at(b, GROUND), at(b, GROUND + FENCE_HEIGHT), at(a, GROUND + FENCE_HEIGHT)]
            # Seen from inside: the scan's canes stand upright.
            panels.add(verts, [(0, 1, 2, 3)], [[(0, 0), (1, 0), (1, 1), (0, 1)]])
            x, y, _ = at(a, 0)
            frame.add(*meshkit.tube([(x, y, GROUND - 0.1), (x, y, GROUND + FENCE_HEIGHT + 0.12)], [0.045, 0.045], 8, v_scale=0.96))
        # The rail along the top, and one low down.
        for z, r in ((FENCE_HEIGHT - 0.06, 0.03), (0.35, 0.022)):
            p0, p1 = at(-FENCE, GROUND + z), at(FENCE, GROUND + z)
            inwards = Vector((-p0[0] - p1[0], -p0[1] - p1[1], 0)).normalized() * 0.035
            frame.add(*meshkit.tube([Vector(p0) + inwards, Vector(p1) + inwards], [r, r], 6, v_scale=0.96))
    return [panels.build("Fence", "bamboo_fence", smooth=False), frame.build("Fence frame", "bamboo", sharp=50)]


def backdrop():
    """Beyond the fence: rough ground as far as the trees, then the trees and far hills, each a
    picture on a ring of cards."""
    far, near, z = 130.0, FENCE + 0.4, GROUND - 0.03
    land = Shape()
    # A frame round the garden: four strips, from the gravel's edge outwards.
    for x0, y0, x1, y1 in ((-far, near, far, far), (-far, -far, far, -near), (-far, -near, -near, near), (near, -near, far, near)):
        verts = [(x0, y0, z), (x1, y0, z), (x1, y1, z), (x0, y1, z)]
        land.add(verts, [(0, 1, 2, 3)], [[(x, y) for x, y, _ in verts]])
    out = [land.build("Land", "moss", smooth=False)]
    for name, material, radius, bottom, top, repeats in (
        ("Treeline", "treeline", 17.0, GROUND + 0.6, GROUND + 12.6, 4),
        ("Hills", "hills", 120.0, GROUND - 2.0, GROUND + 26.0, 3),
    ):
        shape = Shape()
        segments = 48
        for i in range(segments):
            a0, a1 = 2 * math.pi * i / segments, 2 * math.pi * (i + 1) / segments
            verts = [
                (radius * math.cos(a0), radius * math.sin(a0), bottom),
                (radius * math.cos(a1), radius * math.sin(a1), bottom),
                (radius * math.cos(a1), radius * math.sin(a1), top),
                (radius * math.cos(a0), radius * math.sin(a0), top),
            ]
            u0, u1 = i / segments * repeats, (i + 1) / segments * repeats
            # Facing inwards.
            shape.add(verts, [(1, 0, 3, 2)], [[(u1, 0), (u0, 0), (u0, 1), (u1, 1)]])
        out.append(shape.build(name, material, smooth=False))
    return out


def build():
    """The whole garden, in an empty scene. Returns the cushion, which is a pattern for the
    client to lay out rather than a thing in its place."""
    bpy.ops.wm.read_factory_settings(use_empty=True)
    table()
    seat = cushion()
    mats()
    deck()
    edge = pond_outline()
    gravel(edge)
    islands()
    pond(edge)
    rocks(rock_meshes(), edge)
    stepping_stones()
    lantern((5.6, 1.5))
    basin((-3.0, 0.75))
    maple(MAPLE)
    bamboo()
    shrubs()
    bank()
    fence()
    backdrop()
    return seat


def export(path):
    bpy.ops.object.select_all(action="DESELECT")
    bpy.ops.export_scene.gltf(
        filepath=path,
        export_format="GLB",
        export_yup=True,
        export_apply=True,
        export_tangents=True,
        export_materials="EXPORT",
        export_image_format="NONE",
        export_extras=False,
        export_cameras=False,
        export_lights=False,
        export_animations=False,
    )
    print(
        f"wrote {os.path.relpath(path, common.REPO)} ({os.path.getsize(path) // 1024} KiB, "
        f"{meshkit.triangles()} triangles in {len(bpy.data.meshes)} meshes, {len(bpy.data.objects)} objects)"
    )


def seats(cushion_object, count=2, distance=0.86):
    """For a picture: the cushion laid out as the client lays it for a game of two."""
    cushion_object.location = Vector((0, -distance, FLOOR))
    for i in range(1, count):
        a = 2 * math.pi * i / count
        meshkit.instance(cushion_object, "Cushion", (distance * math.sin(a), -distance * math.cos(a), FLOOR), a)


def run():
    build()
    export(common.output("environments", "garden.glb"))
