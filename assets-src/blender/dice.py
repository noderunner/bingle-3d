"""A die of six sides: a cube with its edges and corners rounded, each side's pips hollowed out
of it.

`models/die.glb`: 16 mm across, about the origin. Its mesh is two parts, each named for the
material the client gives it: `body` and `pips`. It is mapped in metres from every side, as the
client maps a wood.

As a die is made, opposite sides add to seven, and 1, 2 and 3 go anticlockwise round the corner
they share. As it lies here: 1 is up (+z), 6 down; 2 faces the player (-y), 5 the far side; 3 is
to the right (+x), 4 to the left. The client turns it to show what was thrown."""

import bmesh
import bpy
from mathutils import Matrix, Vector

import common
import meshkit

SIDE = 0.016
ROUNDING = 0.0019
PIP_RADIUS = 0.00145
PIP_DEPTH = 0.00045
# How far a side's outer pips are from its middle.
PIP_REACH = 0.0042
PARTS = ["body", "pips"]

# Each side's number: the way it faces, and the two ways across it.
SIDES = {
    1: ((0, 0, 1), (1, 0, 0), (0, 1, 0)),
    6: ((0, 0, -1), (1, 0, 0), (0, -1, 0)),
    2: ((0, -1, 0), (1, 0, 0), (0, 0, 1)),
    5: ((0, 1, 0), (-1, 0, 0), (0, 0, 1)),
    3: ((1, 0, 0), (0, 1, 0), (0, 0, 1)),
    4: ((-1, 0, 0), (0, -1, 0), (0, 0, 1)),
}
# Where a side's pips are, across it, in steps of `PIP_REACH`.
PIPS = {
    1: [(0, 0)],
    2: [(-1, -1), (1, 1)],
    3: [(-1, -1), (0, 0), (1, 1)],
    4: [(-1, -1), (1, -1), (-1, 1), (1, 1)],
    5: [(-1, -1), (1, -1), (0, 0), (-1, 1), (1, 1)],
    6: [(-1, -1), (1, -1), (-1, 0), (1, 0), (-1, 1), (1, 1)],
}


def body():
    bm = bmesh.new()
    bmesh.ops.create_cube(bm, size=SIDE)
    bmesh.ops.bevel(bm, geom=list(bm.edges), offset=ROUNDING, segments=4, profile=0.5, affect="EDGES")
    return meshkit.from_bmesh(bm, "Die", "body", sharp=None)


def hollows():
    """Every pip as a ball that dips into its side: what is cut out of the cube."""
    # A ball of this radius, its middle this far outside the side, leaves a hollow of the
    # pip's width and depth.
    ball = (PIP_RADIUS**2 + PIP_DEPTH**2) / (2 * PIP_DEPTH)
    bm = bmesh.new()
    for number, (out, across, up) in SIDES.items():
        out, across, up = Vector(out), Vector(across), Vector(up)
        for a, b in PIPS[number]:
            at = out * (SIDE / 2 + ball - PIP_DEPTH) + (across * a + up * b) * PIP_REACH
            # Its pole to the side, so the hollow's rim is a ring of the ball's own.
            turn = Vector((0, 0, 1)).rotation_difference(out).to_matrix().to_4x4()
            bmesh.ops.create_uvsphere(bm, u_segments=20, v_segments=12, radius=ball, matrix=Matrix.Translation(at) @ turn)
    return meshkit.from_bmesh(bm, "Pips", "pips", sharp=None)


def die():
    obj = meshkit.boolean(body(), hollows(), keep_materials=True)
    names = [m.name for m in obj.data.materials]
    if names != PARTS:
        raise SystemExit(f"the die's parts are {names}, not {PARTS}")
    meshkit.triangulate(obj)
    for poly in obj.data.polygons:
        poly.use_smooth = True
    # Sharp where a pip meets its side; the rounded edges stay smooth.
    obj.data.set_sharp_from_angle(angle=0.5)
    meshkit.flat_faces_stay_flat(obj)
    return meshkit.box_uv(obj, 1.0, detail=True, offset=(0.31, 0.57, 0.0))


def run():
    bpy.ops.wm.read_factory_settings(use_empty=True)
    path = common.output("models", "die.glb")
    meshkit.export_glb(die(), path, materials=True)
    meshkit.check_parts(path, PARTS)
