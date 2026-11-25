"""A disc that is light on one side and dark on the other, and a ring a disc fits inside.

`models/disc.glb`: a disc 15 mm across and 3.6 mm thick, cut from a slab: its faces flat and its
edges sharp, only just broken. It is two parts, each named for the material the client gives it:
`light`, the upper face and the upper half of the edge, and `dark`, the lower, the one the
other's mirror image. As it lies here the light side is up; turned over about any axis across
it, the dark side is.
`models/disc_large.glb` is a disc 24 mm across and 4.4 mm thick, to fill a square of a 28 mm
board, turned as a counter is: its edges rounded, a shallow dish in each face inside a rim.

`models/ring.glb`: a ring 25 mm across and 4 mm tall, with a hole 17 mm across, so a disc lies
inside it with a millimetre to spare, and neighbours on a 28 mm lattice do not touch. Cut as the
disc is: a flat top, upright walls, sharp edges. It is two parts: `piece`, which is what its
seat's pieces are made of, and `trim`, a band let in round each edge of its top (the outer and
the hole's), over the corner and a little way down the wall, for a metal; a look that gives the
trim nothing has it of the piece's own material, and the ring is plain.

All are turned from profiles, centred on their middle, and mapped in metres from every side."""

import math

import bpy

import common
import meshkit

MM = 0.001

# Each disc made: its file's name, its radius and thickness, how far its edges are rounded (a
# dished disc's) or broken (a flat one's), how many sides its round has, and whether its faces
# are dished. A dish is the same part of its face whatever the disc's size.
DISCS = [
    ("disc", 7.5 * MM, 3.6 * MM, 0.12 * MM, 64, False),
    ("disc_large", 12.0 * MM, 4.4 * MM, 1.1 * MM, 72, True),
]
# The dish in each face: how deep, and how far out it goes before the rim, of the radius.
DISH_DEPTH = 0.22 * MM
DISH_OUT = 2 / 3
RIM_OUT = 0.773
PARTS = ["light", "dark"]

RING_INNER = 8.5 * MM
RING_OUTER = 12.5 * MM
RING_HEIGHT = 4.0 * MM
# Its edges broken, as the flat disc's are: sharp to look at, and still catching the light.
RING_BREAK = 0.12 * MM
# The trim: how far in from each edge of the top it goes, and how far down the wall.
TRIM_IN = 1.0 * MM
TRIM_DOWN = 1.0 * MM
RING_PARTS = ["piece", "trim"]
RING_SEGMENTS = 96


def corner(cr, cz, radius, start, end, count):
    """Points round an arc about (cr, cz), from `start` to `end` degrees, both included."""
    out = []
    for i in range(count + 1):
        a = math.radians(start + (end - start) * i / count)
        out.append((cr + radius * math.cos(a), cz + radius * math.sin(a)))
    return out


def flat_half(z_face, up, radius, thickness, ease):
    """One face of a flat disc, from its middle out to the middle of its edge: (radius, z). The
    face is flat to its edge, which is broken by `ease` at 45 degrees."""
    s = 1.0 if up else -1.0
    return [(0.0, z_face), (radius - ease, z_face), (radius, z_face - s * ease), (radius, thickness / 2)]


def disc_half(z_face, up, radius, thickness, ease):
    """One face of a dished disc, from its middle out to the middle of its edge: (radius, z).
    `up` is the upper face, whose z grows towards the face."""
    DISC_RADIUS, DISC_THICKNESS, DISC_EASE = radius, thickness, ease
    DISH_RADIUS, RIM_INNER = radius * DISH_OUT, radius * RIM_OUT
    s = 1.0 if up else -1.0
    dish = z_face - s * DISH_DEPTH
    out = [(0.0, dish), (DISH_RADIUS * 0.5, dish + s * DISH_DEPTH * 0.08), (DISH_RADIUS, dish + s * DISH_DEPTH * 0.35)]
    # The dish rises to the rim.
    for i in range(1, 5):
        t = i / 4
        r = DISH_RADIUS + (RIM_INNER - DISH_RADIUS) * t
        out.append((r, dish + s * DISH_DEPTH * (0.35 + 0.65 * (0.5 - 0.5 * math.cos(math.pi * t)))))
    out.append((DISC_RADIUS - DISC_EASE, z_face))
    # The edge rounded down (or up) to the side, then the side to its middle.
    middle = DISC_THICKNESS / 2
    if up:
        out += corner(DISC_RADIUS - DISC_EASE, z_face - DISC_EASE, DISC_EASE, 90, 0, 6)[1:]
    else:
        out += corner(DISC_RADIUS - DISC_EASE, z_face + DISC_EASE, DISC_EASE, -90, 0, 6)[1:]
    out.append((DISC_RADIUS, middle))
    return out


def disc(name="disc"):
    """A disc of `DISCS`, by its name. Returns it and its thickness."""
    _, radius, thickness, ease, segments, dished = next(d for d in DISCS if d[0] == name)
    half = disc_half if dished else flat_half
    bottom = half(0.0, False, radius, thickness, ease)
    top = half(thickness, True, radius, thickness, ease)
    # From the axis at the bottom, out and up the side, and back in to the axis at the top; the
    # side's middle once.
    outline = bottom + top[::-1][1:]
    obj = meshkit.turned("Disc", PARTS[0], outline, segments)
    obj.data.materials.append(meshkit.slot(PARTS[1]))
    middle = thickness / 2
    for poly in obj.data.polygons:
        poly.material_index = 0 if poly.center.z > middle else 1
    meshkit.triangulate(obj)
    # Smooth but at a sharp turn; `finish` would leave it one part.
    for poly in obj.data.polygons:
        poly.use_smooth = True
    obj.data.set_sharp_from_angle(angle=math.radians(35.0))
    tall = meshkit.centre_height(obj)
    return meshkit.box_uv(obj, 1.0, detail=True, offset=(0.17, 0.63, 0.0)), tall


def ring():
    e, h = RING_BREAK, RING_HEIGHT
    # A closed section, round from the inside of its base: out along the base, up the outer
    # wall, in over the top, down the hole's wall, and back to where it began. A point wherever
    # the trim begins or ends, so a face is all trim or none of it.
    outline = [
        (RING_INNER + e, 0.0),
        (RING_OUTER - e, 0.0),
        (RING_OUTER, e),
        (RING_OUTER, h - TRIM_DOWN),
        (RING_OUTER, h - e),
        (RING_OUTER - e, h),
        (RING_OUTER - TRIM_IN, h),
        (RING_INNER + TRIM_IN, h),
        (RING_INNER + e, h),
        (RING_INNER, h - e),
        (RING_INNER, h - TRIM_DOWN),
        (RING_INNER, e),
        (RING_INNER + e, 0.0),
    ]
    obj = meshkit.turned("Ring", RING_PARTS[0], outline, RING_SEGMENTS)
    obj.data.materials.append(meshkit.slot(RING_PARTS[1]))
    # The trim: what is above where it ends on a wall, and within its reach of an edge of the top.
    grain = 0.01 * MM
    for poly in obj.data.polygons:
        r = math.hypot(poly.center.x, poly.center.y)
        high = poly.center.z > h - TRIM_DOWN + grain
        edge = r > RING_OUTER - TRIM_IN + grain or r < RING_INNER + TRIM_IN - grain
        poly.material_index = 1 if high and edge else 0
    meshkit.triangulate(obj)
    # Smooth but at a sharp turn; `finish` would leave it one part.
    for poly in obj.data.polygons:
        poly.use_smooth = True
    obj.data.set_sharp_from_angle(angle=math.radians(35.0))
    tall = meshkit.centre_height(obj)
    return meshkit.box_uv(obj, 1.0, detail=True, offset=(0.41, 0.29, 0.0)), tall


def run():
    for name, *_ in DISCS:
        bpy.ops.wm.read_factory_settings(use_empty=True)
        obj, tall = disc(name)
        path = common.output("models", f"{name}.glb")
        meshkit.export_glb(obj, path, materials=True)
        meshkit.check_parts(path, PARTS)
        print(f"   {name}: {tall * 1000:.1f} mm thick")
    bpy.ops.wm.read_factory_settings(use_empty=True)
    obj, tall = ring()
    path = common.output("models", "ring.glb")
    meshkit.export_glb(obj, path, materials=True)
    meshkit.check_parts(path, RING_PARTS)
    print(f"   ring: {tall * 1000:.1f} mm tall")
