"""A playing card: a blank one, with a place on each side for a picture, and a back to give it
until a game brings its own.

- `models/card.glb`: a card of the usual size (poker size: 63.5 × 88.9 mm, which is 5 to 7, and
  0.3 mm thick), its corners rounded. It lies on its back about the origin, its top to the far
  side of the table (+y here). Its mesh is three parts, each named for the material the client
  gives it: `face`, `back` and `edge`.
- `textures/cards/back_albedo.jpg`: a back, drawn here: a lattice in blue ink inside a white
  border. It reads the same either way up, as a back must.

A side's picture fills it: any image of 5 to 7 (a JPEG or a PNG, as the client reads them; an SVG
is first made one). On the face its top is the card's top. On the back it is mapped the other way
across, so that it reads rightly when the card is turned over about its long axis, as cards are.
The edge is paper."""

import math

import bpy
import numpy as np

import common
import meshkit

WIDTH = 0.0635
HEIGHT = 0.0889
THICKNESS = 0.0003
CORNER = 0.0032
CORNER_STEPS = 6
PARTS = ["face", "back", "edge"]

# The back's picture, in pixels (5 to 7), and its colours (sRGB).
BACK_SIZE = (640, 896)
PAPER = (0.955, 0.945, 0.915)
INK = (0.06, 0.15, 0.44)


def outline():
    """The card's edge from above, anticlockwise: (x, y)."""
    out = []
    w, h = WIDTH / 2 - CORNER, HEIGHT / 2 - CORNER
    for corner, (cx, cy) in enumerate(((w, h), (-w, h), (-w, -h), (w, -h))):
        for step in range(CORNER_STEPS + 1):
            a = math.pi / 2 * (corner + step / CORNER_STEPS)
            out.append((cx + CORNER * math.cos(a), cy + CORNER * math.sin(a)))
    return out


def card():
    rim = outline()
    n = len(rim)
    top, bottom = THICKNESS / 2, -THICKNESS / 2
    verts = [(x, y, top) for x, y in rim] + [(x, y, bottom) for x, y in rim] + [(0, 0, top), (0, 0, bottom)]
    faces, parts = [], []
    for i in range(n):
        j = (i + 1) % n
        faces += [(2 * n, i, j), (2 * n + 1, n + j, n + i), (i, n + i, n + j, j)]
        parts += [0, 1, 2]
    mesh = bpy.data.meshes.new("Card")
    mesh.from_pydata(verts, [], faces)
    mesh.update()
    for name in PARTS:
        mesh.materials.append(meshkit.slot(name))
    uv = mesh.uv_layers.new(name="UVMap")
    around = 0.0
    lengths = [math.dist(rim[i], rim[(i + 1) % n]) for i in range(n)]
    starts = [sum(lengths[:i]) for i in range(n)]
    for poly, part in zip(mesh.polygons, parts):
        poly.material_index = part
        poly.use_smooth = False
        for loop in poly.loop_indices:
            index = mesh.loops[loop].vertex_index
            x, y, z = mesh.vertices[index].co
            if part == 0:
                st = (x / WIDTH + 0.5, y / HEIGHT + 0.5)
            elif part == 1:
                st = (0.5 - x / WIDTH, y / HEIGHT + 0.5)
            else:
                # Round the edge, in metres: paper has no picture, but tangents need a mapping.
                i = index % n
                first = poly.vertices[0] % n
                along = starts[i] if not (i == 0 and first == n - 1) else sum(lengths)
                st = (along, z)
            uv.data[loop].uv = st
    obj = bpy.data.objects.new("Card", mesh)
    bpy.context.scene.collection.objects.link(obj)
    return obj


def back(size=BACK_SIZE, oversample=2):
    """The back's picture, as sRGB values (height, width, 3)."""
    w, h = size[0] * oversample, size[1] * oversample
    mm = WIDTH * 1000.0 / w
    x = (np.arange(w) + 0.5) * mm - WIDTH * 500.0
    y = (np.arange(h) + 0.5) * mm - HEIGHT * 500.0
    x, y = np.meshgrid(x, y)
    half = np.array([WIDTH * 500.0, HEIGHT * 500.0])

    def box(inset, radius):
        """How far inside a rounded box `inset` from the card's edge a point is (negative out)."""
        q = np.stack([np.abs(x) - (half[0] - inset - radius), np.abs(y) - (half[1] - inset - radius)])
        outside = np.linalg.norm(np.maximum(q, 0.0), axis=0)
        return radius - outside - np.minimum(np.maximum(q[0], q[1]), 0.0)

    def line(distance, width):
        """A line `width` wide about distance 0, its edges soft over a pixel."""
        return np.clip((width / 2 - np.abs(distance)) / mm + 0.5, 0.0, 1.0)

    def fill(distance):
        return np.clip(distance / mm + 0.5, 0.0, 1.0)

    panel = box(3.6, 1.6)
    ink = fill(panel)
    # A white line a little inside the panel's edge.
    white = line(panel - 1.25, 0.34)
    # The lattice: lines both ways across, and a dot in every diamond.
    field = fill(panel - 2.4)
    period = 3.4
    a, b = (x + y) / period, (x - y) / period
    da = (np.abs(a - np.round(a))) * period / math.sqrt(2)
    db = (np.abs(b - np.round(b))) * period / math.sqrt(2)
    lattice = np.maximum(line(da, 0.26), line(db, 0.26))
    ca, cb = a - np.floor(a) - 0.5, b - np.floor(b) - 0.5
    dots = fill(0.36 - np.hypot(ca, cb) * period / math.sqrt(2))
    # The middle: an oval, a ring in it, and a flower of eight petals.
    oval = 1.0 - np.hypot(x / 11.5, y / 15.5)
    inside = fill(oval * 11.5)
    ring = line(oval * 11.5 - 1.1, 0.34)
    r, t = np.hypot(x, y * 11.5 / 15.5), np.arctan2(y * 11.5 / 15.5, x)
    flower = line(7.6 * (0.42 + 0.58 * np.abs(np.cos(4 * t)) ** 0.8) - r, 0.34)
    eye = line(r - 2.4, 0.3)
    pattern = np.maximum(lattice, dots) * (1.0 - inside) + inside * np.maximum(ring, np.maximum(flower, eye))
    white = np.maximum(white, field * pattern)
    amount = (ink * (1.0 - white))[..., None]
    paper, blue = common.srgb_to_linear(np.array(PAPER)), common.srgb_to_linear(np.array(INK))
    linear = paper * (1.0 - amount) + blue * amount
    return common.linear_to_srgb(common.resize_to(linear.astype(np.float32), size[0], size[1]))


def run():
    bpy.ops.wm.read_factory_settings(use_empty=True)
    path = common.output("models", "card.glb")
    meshkit.export_glb(card(), path, materials=True)
    meshkit.check_parts(path, PARTS)
    common.write_jpeg(common.output("textures", "cards", "back_albedo.jpg"), back(), 90)
