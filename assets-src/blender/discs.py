"""A disc that is light on one side and dark on the other, and a ring a disc fits inside.

`models/disc.glb`: a disc 15 mm across and 3.6 mm thick, its edges rounded, a shallow dish in each
face inside a rim. It is two parts, each named for the material the client gives it: `light`, the
upper face and the upper half of the edge, and `dark`, the lower. As it lies here the light side is
up; turned over about any axis across it, the dark side is.

`models/ring.glb`: a ring 25 mm across and 7 mm tall, with a hole 17 mm across, so a disc lies
inside it with a millimetre to spare, and neighbours on a 28 mm lattice do not touch. One part, with
no material: an appearance file says what each seat's rings are made of.

Both are turned from profiles, centred on their middle, and mapped in metres from every side."""

import math

import bpy

import common
import meshkit

MM = 0.001

DISC_RADIUS = 7.5 * MM
DISC_THICKNESS = 3.6 * MM
DISC_EASE = 0.9 * MM
# The dish in each face: how deep, and how far out it goes before the rim.
DISH_DEPTH = 0.22 * MM
DISH_RADIUS = 5.0 * MM
RIM_INNER = 5.8 * MM
DISC_SEGMENTS = 56
PARTS = ["light", "dark"]

RING_INNER = 8.5 * MM
RING_OUTER = 12.5 * MM
RING_HEIGHT = 7.0 * MM
# The base's corners eased, and the walls leaning in a little towards the rounded top.
RING_EASE = 0.6 * MM
RING_LEAN = 0.25 * MM
RING_SEGMENTS = 80


def corner(cr, cz, radius, start, end, count):
    """Points round an arc about (cr, cz), from `start` to `end` degrees, both included."""
    out = []
    for i in range(count + 1):
        a = math.radians(start + (end - start) * i / count)
        out.append((cr + radius * math.cos(a), cz + radius * math.sin(a)))
    return out


def disc_half(z_face, up):
    """One face of the disc, from its middle out to the middle of its edge: (radius, z). `up` is
    the upper face, whose z grows towards the face."""
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


def disc():
    bottom = disc_half(0.0, up=False)
    top = disc_half(DISC_THICKNESS, up=True)
    # From the axis at the bottom, out and up the side, and back in to the axis at the top; the
    # side's middle once.
    outline = bottom + top[::-1][1:]
    obj = meshkit.turned("Disc", PARTS[0], outline, DISC_SEGMENTS)
    obj.data.materials.append(meshkit.slot(PARTS[1]))
    middle = DISC_THICKNESS / 2
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
    e, lean = RING_EASE, RING_LEAN
    top_r = (RING_OUTER - RING_INNER - 2 * lean) / 2
    top_c = (RING_INNER + RING_OUTER) / 2
    top_z = RING_HEIGHT - top_r
    # A closed section, round from the inside of its base: out along the base, up the outer
    # wall, over the top, down the inner wall, and back to where it began.
    outline = [(RING_INNER + e, 0.0)]
    outline += corner(RING_OUTER - e, e, e, 270, 360, 4)
    outline += corner(top_c, top_z, top_r, 0, 180, 18)
    outline += corner(RING_INNER + e, e, e, 180, 270, 4)
    obj = meshkit.turned("Ring", "piece", outline, RING_SEGMENTS)
    meshkit.triangulate(obj)
    meshkit.finish(obj, "piece", sharp=35.0)
    tall = meshkit.centre_height(obj)
    return meshkit.box_uv(obj, 1.0, detail=True, offset=(0.41, 0.29, 0.0)), tall


def run():
    bpy.ops.wm.read_factory_settings(use_empty=True)
    obj, tall = disc()
    path = common.output("models", "disc.glb")
    meshkit.export_glb(obj, path, materials=True)
    meshkit.check_parts(path, PARTS)
    print(f"   disc: {tall * 1000:.1f} mm thick")
    bpy.ops.wm.read_factory_settings(use_empty=True)
    obj, tall = ring()
    meshkit.export_glb(obj, common.output("models", "ring.glb"))
    print(f"   ring: {tall * 1000:.1f} mm tall")
