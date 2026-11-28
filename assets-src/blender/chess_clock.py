"""A chess clock that stands on the table beside a board: a wooden case with an analog face at
each end, an amber lamp under each face, and a button on top above each.

`models/chess_clock.glb`: the clock stands on z = 0 about the origin, its long side along x, its
front (the faces, tilted back) to the player (-y). Each end's parts are named `left` or `right`
as seen from the front.

- The case is the node `case`, its mesh of six parts, each named for the material the client gives
  it: `frame` (the wooden case), `face` (the two dials), `marks` (their minute and hour marks),
  `bezel` (a metal ring round each dial and each lamp), `lamp_left` and `lamp_right`.
- Each part that moves is a node of its own, its mesh about its own pivot, so the client turns
  it by turning the node: `hour_left`, `minute_left`, `second_left`, `flag_left` and
  `button_left`, and the same with `right`. A hand and a flag point to the twelve as made, and
  turn about the node's own z (glTF's axes: out of the face); a button stands up as made and
  goes down along the node's own y.

Nothing is from a source: it is made here from primitives."""

import math

import bmesh
import bpy
from mathutils import Matrix, Vector

import common
import meshkit

# The case: how long, how deep at its foot, how tall, and how far its front leans back.
LENGTH = 0.160
DEPTH = 0.058
HEIGHT = 0.086
TILT = math.radians(15.0)
EASE = 0.0035
# Each dial: its middle along x, how high up the front (along it), and its radius.
DIAL_X = 0.041
DIAL_UP = 0.053
RADIUS = 0.028
# The dial stands this proud of the front, its marks a little more; the bezel's ring.
DIAL_PROUD = 0.0015
MARKS_PROUD = 0.0003
BEZEL = (0.0024, 0.003)
# Each lamp, under its dial: how far below the dial's middle, along the front, and its radius.
LAMP_BELOW = RADIUS + 0.011
LAMP_RADIUS = 0.0045
# The hands: length, width at the pivot and at the tip, and how far over the dial each lies.
HANDS = {
    "hour": (0.56 * RADIUS, 0.003, 0.0014, 0.0008),
    "minute": (0.86 * RADIUS, 0.0022, 0.0009, 0.0014),
    "second": (0.92 * RADIUS, 0.0008, 0.0005, 0.0020),
}
HAND_THICK = 0.0005
# How far behind the pivot the second hand reaches, and the hub over all three.
SECOND_TAIL = 0.22 * RADIUS
HUB = 0.0018
# The flag: its pivot on the dial (right of the twelve, up), its length, and how far over it is.
FLAG_AT = (0.07 * RADIUS, 0.64 * RADIUS)
FLAG = (0.25 * RADIUS, 0.003, 0.0011)
# The buttons: across, how tall they stand over the top, and where on the top (from the front).
BUTTON_X = 0.050
BUTTON = (0.0078, 0.011)
BUTTON_SINK = 0.005

CASE_PARTS = ["frame", "face", "marks", "bezel", "lamp_left", "lamp_right"]
MOVING = ["hour", "minute", "second", "flag", "button"]
SIDES = {"left": -1.0, "right": 1.0}


def front(height):
    """How far forward (-y) the front is at a height."""
    return -DEPTH / 2.0 + height * math.tan(TILT)


def dial_frame(x):
    """A dial's own axes as a matrix: x across it, y up it, z out of it, its middle at the
    origin: where on the front its dial is."""
    up = Vector((0.0, math.sin(TILT), math.cos(TILT)))
    out = Vector((0.0, -math.cos(TILT), math.sin(TILT)))
    height = DIAL_UP * math.cos(TILT)
    middle = Vector((x, front(height), height))
    m = Matrix.Identity(4)
    for row in range(3):
        m[row][0] = (1.0, 0.0, 0.0)[row]
        m[row][1] = up[row]
        m[row][2] = out[row]
        m[row][3] = middle[row]
    return m


# A moving part's own axes in its node, as Blender has them: out of the dial is its -y and up
# the dial its z, so that the exporter, turning Blender's z up into glTF's y up, gives a node
# whose z is out of the dial and whose y is up it.
OWN = Matrix(((1, 0, 0, 0), (0, 0, -1, 0), (0, 1, 0, 0), (0, 0, 0, 1)))


def case():
    """The wooden case: its side's outline (a foot, a front leaning back, a level top), along
    x, its edges eased."""
    bm = bmesh.new()
    outline = [(-DEPTH / 2.0, 0.0), (DEPTH / 2.0, 0.0), (DEPTH / 2.0, HEIGHT), (front(HEIGHT), HEIGHT)]
    near = [bm.verts.new((-LENGTH / 2.0, y, z)) for y, z in outline]
    far = [bm.verts.new((LENGTH / 2.0, y, z)) for y, z in outline]
    bm.faces.new(near[::-1])
    bm.faces.new(far)
    for i in range(4):
        j = (i + 1) % 4
        bm.faces.new((near[i], near[j], far[j], far[i]))
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    bmesh.ops.bevel(bm, geom=list(bm.edges), offset=EASE, segments=3, profile=0.5, affect="EDGES")
    obj = meshkit.from_bmesh(bm, "case", "frame", sharp=50.0)
    return obj


def disc(bm, matrix, radius, low, high, segments=48):
    """A flat disc on a dial's axes, from `low` to `high` out of it."""
    made = bmesh.ops.create_cone(
        bm, cap_ends=True, segments=segments, radius1=radius, radius2=radius, depth=high - low,
        matrix=matrix @ Matrix.Translation((0, 0, (low + high) / 2.0)),
    )
    return made


def ring(matrix, inner, outer, height, segments=48):
    """A ring standing on a dial's axes, its top rounded a little: a bezel."""
    lip = (outer - inner) / 2.0
    profile = [
        (inner, -0.0005),
        (inner, height - lip * 0.6),
        (inner + lip * 0.5, height),
        (outer - lip * 0.5, height),
        (outer, height - lip * 0.6),
        (outer, -0.0005),
    ]
    verts, faces, _ = meshkit.lathe(profile, segments)
    return [tuple(matrix @ Vector(v)) for v in verts], faces


def part(name, material, build):
    bm = bmesh.new()
    build(bm)
    return meshkit.from_bmesh(bm, name, material, sharp=40.0)


def dials():
    def build(bm):
        for x in SIDES.values():
            disc(bm, dial_frame(x * DIAL_X), RADIUS, 0.0, DIAL_PROUD)
    return part("Faces", "face", build)


def marks():
    """Sixty marks round each dial, every fifth longer and wider: each its top only, lying on the
    dial (nothing sees their sides)."""

    def build(bm):
        for x in SIDES.values():
            frame = dial_frame(x * DIAL_X)
            for k in range(60):
                long = k % 5 == 0
                inner = (0.76 if long else 0.86) * RADIUS
                outer = 0.95 * RADIUS
                half = (0.0019 if long else 0.0006) / 2.0
                turn = frame @ Matrix.Rotation(-2.0 * math.pi * k / 60.0, 4, "Z")
                z = DIAL_PROUD + MARKS_PROUD
                corners = [(-half, inner), (half, inner), (half, outer), (-half, outer)]
                bm.faces.new([bm.verts.new(tuple(turn @ Vector((a, b, z)))) for a, b in corners])
    return part("Marks", "marks", build)


def bezels():
    shape = meshkit.Shape()
    for x in SIDES.values():
        frame = dial_frame(x * DIAL_X)
        shape.add(*ring(frame, RADIUS - 0.0004, RADIUS + BEZEL[0], BEZEL[1]))
        lamp = frame @ Matrix.Translation((0.0, -LAMP_BELOW, 0.0))
        shape.add(*ring(lamp, LAMP_RADIUS - 0.0003, LAMP_RADIUS + 0.0018, 0.0018, segments=24))
    return shape.build("Bezels", "bezel", sharp=50.0)


def lamp(side):
    """A dome of amber glass under a dial."""
    bm = bmesh.new()
    frame = dial_frame(SIDES[side] * DIAL_X) @ Matrix.Translation((0.0, -LAMP_BELOW, 0.0))
    bmesh.ops.create_uvsphere(bm, u_segments=24, v_segments=10, radius=LAMP_RADIUS, matrix=frame @ Matrix.Diagonal((1, 1, 0.6, 1)))
    # Only the half out of the front.
    bmesh.ops.delete(bm, geom=[v for v in bm.verts if (frame.inverted() @ v.co).z < -1e-6], context="VERTS")
    return meshkit.from_bmesh(bm, f"Lamp {side}", f"lamp_{side}", sharp=None)


def hand(name, length, wide, narrow, tail=0.0):
    """A hand pointing up its dial from the pivot, flat, on a moving part's own axes."""
    half = HAND_THICK / 2.0
    outline = [(-wide / 2.0, -tail), (wide / 2.0, -tail), (narrow / 2.0, length), (0.0, length + narrow), (-narrow / 2.0, length)]
    bm = bmesh.new()
    low = [bm.verts.new((x, y, -half)) for x, y in outline]
    high = [bm.verts.new((x, y, half)) for x, y in outline]
    bm.faces.new(low[::-1])
    bm.faces.new(high)
    n = len(outline)
    for i in range(n):
        j = (i + 1) % n
        bm.faces.new((low[i], low[j], high[j], high[i]))
    if name.startswith("second") or name.startswith("hour"):
        # A hub over the pivot.
        r = HUB if name.startswith("second") else HUB * 1.4
        bmesh.ops.create_cone(bm, cap_ends=True, segments=24, radius1=r, radius2=r, depth=HAND_THICK * 1.6)
    for v in bm.verts:
        v.co = (OWN @ v.co.to_4d()).to_3d()
    return bm


def moving(kind, side):
    """A moving part as a node of its own: its mesh about its pivot, the node where it turns."""
    name = f"{kind}_{side}"
    frame = dial_frame(SIDES[side] * DIAL_X)
    if kind in HANDS:
        length, wide, narrow, over = HANDS[kind]
        bm = hand(kind, length, wide, narrow, SECOND_TAIL if kind == "second" else 0.0)
        at = frame @ Matrix.Translation((0.0, 0.0, DIAL_PROUD + over))
        material = "second" if kind == "second" else "hand"
    elif kind == "flag":
        length, wide, over = FLAG
        bm = hand(kind, length, wide, wide * 0.5)
        at = frame @ Matrix.Translation((FLAG_AT[0], FLAG_AT[1], DIAL_PROUD + over))
        material = "flag"
    else:
        radius, tall = BUTTON
        bm = bmesh.new()
        profile = [(0.0, tall), (radius * 0.7, tall), (radius * 0.92, tall - 0.0012), (radius, tall - 0.003), (radius, -BUTTON_SINK)]
        verts, faces, _ = meshkit.lathe(profile, 40)
        for v in verts:
            bm.verts.new(v)
        bm.verts.ensure_lookup_table()
        for f in faces:
            bm.faces.new([bm.verts[i] for i in f])
        bmesh.ops.remove_doubles(bm, verts=bm.verts, dist=1e-7)
        bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
        # Its own axes are Blender's: y up in glTF is z here.
        top_y = (front(HEIGHT) + DEPTH / 2.0) / 2.0
        at = Matrix.Translation((SIDES[side] * BUTTON_X, top_y, HEIGHT))
        material = "button"
    if kind == "button":
        obj = meshkit.from_bmesh(bm, name, material, sharp=50.0)
    else:
        # Flat: a hand is a thin cut sheet.
        obj = meshkit.from_bmesh(bm, name, material, smooth=False)
    obj.matrix_world = at if kind == "button" else at @ OWN.inverted()
    return obj


def clock():
    body = case()
    parts = [dials(), marks(), bezels(), lamp("left"), lamp("right")]
    whole = meshkit.join([body] + parts, "case")
    names = [m.name for m in whole.data.materials]
    if names != CASE_PARTS:
        raise SystemExit(f"the case's parts are {names}, not {CASE_PARTS}")
    meshkit.triangulate(whole)
    meshkit.box_uv(whole, 1.0, detail=True, offset=(0.13, 0.41, 0.0))
    nodes = [whole]
    for side in SIDES:
        for kind in MOVING:
            obj = moving(kind, side)
            meshkit.triangulate(obj)
            meshkit.box_uv(obj, 1.0)
            nodes.append(obj)
    return nodes


def run():
    bpy.ops.wm.read_factory_settings(use_empty=True)
    path = common.output("models", "chess_clock.glb")
    meshkit.export_glb(clock(), path, materials=True)
    check(path)


def check(path):
    """Checks that the file has the nodes the client turns, and the case its parts, by name."""
    import json
    import struct

    with open(path, "rb") as f:
        data = f.read()
    length = struct.unpack_from("<I", data, 12)[0]
    doc = json.loads(data[20 : 20 + length])
    nodes = {n["name"]: n["mesh"] for n in doc["nodes"]}
    wanted = ["case"] + [f"{kind}_{side}" for side in SIDES for kind in MOVING]
    if sorted(nodes) != sorted(wanted):
        raise SystemExit(f"{path}: its nodes are {sorted(nodes)}, not {sorted(wanted)}")
    names = [doc["materials"][p["material"]]["name"] for p in doc["meshes"][nodes["case"]]["primitives"]]
    if names != CASE_PARTS:
        raise SystemExit(f"{path}: the case's parts are {names}, not {CASE_PARTS}")
