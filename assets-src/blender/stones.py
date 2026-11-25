"""Stones: a lens-shaped stone, modelled here and exported as glTF, and the colour of two kinds of
stone, drawn here as images.

The shape is turned from a profile: the height at distance r from the middle is
h · (1 - (r/a)²)^k, top and bottom alike, so it is a gentle dome with a rounded rim (k < 1
rounds it; k = 0.5 would be an ellipse, and k = 1 a sharp rim). Its real size: 22.4 mm across and 8.8 mm thick, which
fills 80 % of a 28 mm cell. It is mapped flat from above, the texture's middle on the stone's.

- Slate: near black, a little cool, faintly mottled.
- Polished minerals (`minerals.py`): lapis lazuli, red jasper, tiger's eye and black slate, for
  games of coloured stones.
- Polished marbles (`minerals.py` too), white and black, for pieces mapped in metres (a disc, a
  ring): each the same again every 64 mm, so a piece of any size is cut from it.
- Shell: warm white with fine growth lines, the way a shell grows: thin lines a fraction of a
  millimetre apart, bending a little, some stronger than others, and a few broad soft bands.

The client turns each stone a random way about its axis, so the lines never repeat in a row."""

import math
import os

import bpy
import numpy as np

import common
import minerals

RADIUS = 0.0112
HALF_THICKNESS = 0.0044
ROUNDING = 0.8
SEGMENTS = 64
RINGS = 24
# How much of the texture the stone's diameter covers.
UV_SPAN = 0.92


def profile():
    """(r, height) from the middle to the rim, closer together near the rim, where it bends."""
    out = []
    for i in range(RINGS + 1):
        t = i / RINGS * math.pi / 2.0
        r = RADIUS * math.sin(t)
        h = HALF_THICKNESS * max(1.0 - (r / RADIUS) ** 2, 0.0) ** ROUNDING
        out.append((r, h))
    return out


def stone_mesh():
    """Vertices and faces, in Blender's axes (z up)."""
    rings = profile()
    verts = [(0.0, 0.0, HALF_THICKNESS)]
    # Top rings from the middle out, the rim, then bottom rings from the rim in.
    ring_list = [(r, h) for r, h in rings[1:]] + [(r, -h) for r, h in reversed(rings[1:-1])]
    for r, z in ring_list:
        for s in range(SEGMENTS):
            a = 2.0 * math.pi * s / SEGMENTS
            verts.append((r * math.cos(a), r * math.sin(a), z))
    verts.append((0.0, 0.0, -HALF_THICKNESS))
    bottom = len(verts) - 1
    faces = []
    for s in range(SEGMENTS):
        faces.append((0, 1 + s, 1 + (s + 1) % SEGMENTS))
    for ring in range(len(ring_list) - 1):
        a0 = 1 + ring * SEGMENTS
        b0 = a0 + SEGMENTS
        for s in range(SEGMENTS):
            n = (s + 1) % SEGMENTS
            faces.append((a0 + s, b0 + s, b0 + n, a0 + n))
    last = 1 + (len(ring_list) - 1) * SEGMENTS
    for s in range(SEGMENTS):
        faces.append((bottom, last + (s + 1) % SEGMENTS, last + s))
    return verts, faces


def make_stone_object(name="Stone"):
    verts, faces = stone_mesh()
    mesh = bpy.data.meshes.new(name)
    mesh.from_pydata(verts, [], faces)
    mesh.update()
    for poly in mesh.polygons:
        poly.use_smooth = True
    uv = mesh.uv_layers.new(name="UVMap")
    for loop in mesh.loops:
        x, y, _ = mesh.vertices[loop.vertex_index].co
        uv.data[loop.index].uv = (
            0.5 + x / (2.0 * RADIUS) * UV_SPAN,
            0.5 + y / (2.0 * RADIUS) * UV_SPAN,
        )
    obj = bpy.data.objects.new(name, mesh)
    bpy.context.scene.collection.objects.link(obj)
    return obj


def export_glb(path):
    bpy.ops.wm.read_factory_settings(use_empty=True)
    obj = make_stone_object()
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
    print(f"wrote {os.path.relpath(path, common.REPO)}")


def smooth_noise(rng, size, cells):
    """Smooth random values in -1..1 over a size × size image, varying over about size/cells."""
    grid = rng.uniform(-1.0, 1.0, (cells + 3, cells + 3, 1)).astype(np.float32)
    big = common.resize(grid, size + 3 * size // cells)
    off = size // cells
    return big[off : off + size, off : off + size, 0]


def texel_mm(size):
    """Each texel's position in millimetres on the stone, from its middle: (x, y)."""
    span_mm = 2.0 * RADIUS * 1000.0 / UV_SPAN
    t = ((np.arange(size) + 0.5) / size - 0.5) * span_mm
    return np.meshgrid(t, t)


def shell(size, seed=7):
    rng = np.random.default_rng(seed)
    x, y = texel_mm(size)
    # Growth lines are arcs of a wide circle, wandering a little.
    s = y + x * x / (2.0 * 70.0) + 0.35 * smooth_noise(rng, size, 6)
    # The lines along s: random spacing about 0.42 mm, random strengths, each a soft dark line.
    lo, hi = s.min() - 1.0, s.max() + 1.0
    positions = [lo]
    while positions[-1] < hi:
        positions.append(positions[-1] + rng.uniform(0.22, 0.62))
    positions = np.array(positions)
    strength = rng.gamma(1.6, 0.5, len(positions))
    axis = np.linspace(lo, hi, 16384)
    lines = np.zeros_like(axis)
    for p, k in zip(positions, strength):
        lines += k * np.exp(-0.5 * ((axis - p) / 0.055) ** 2)
    # Broad soft bands, every few millimetres.
    bands = np.zeros_like(axis)
    p = lo
    while p < hi:
        bands += rng.uniform(0.2, 1.0) * np.exp(-0.5 * ((axis - p) / 0.5) ** 2)
        p += rng.uniform(2.0, 4.5)
    line = np.interp(s, axis, lines)
    band = np.interp(s, axis, bands)
    mottle = smooth_noise(rng, size, 24)

    base = np.array([0.88, 0.865, 0.82])  # linear: a warm white
    # Lines darken blue most, so they lean yellow; bands are a plain grey.
    line_dark = 0.09 * np.clip(line, 0.0, 2.5)[..., None] * np.array([0.6, 0.8, 1.4])
    band_dark = 0.05 * band[..., None]
    colour = base * (1.0 - line_dark) * (1.0 - band_dark) * (1.0 + 0.015 * mottle[..., None])
    return np.clip(colour, 0.0, 1.0)


def slate(size, seed=11):
    rng = np.random.default_rng(seed)
    base = np.array([0.020, 0.021, 0.024])  # linear: near black, a little cool
    mottle = 0.6 * smooth_noise(rng, size, 10) + 0.4 * smooth_noise(rng, size, 40)
    grain = rng.normal(0.0, 1.0, (size, size))
    colour = base[None, None, :] * (1.0 + 0.18 * mottle + 0.05 * grain)[..., None]
    return np.clip(colour, 0.0, 1.0)


def run():
    export_glb(common.output("models", "stone.glb"))
    for name, size, make in (("shell", 1024, shell), ("slate", 512, slate)):
        albedo = common.linear_to_srgb(make(size))
        common.write_jpeg(common.output("textures", "stone", f"{name}_albedo.jpg"), albedo, 90)
    span_mm = 2.0 * RADIUS * 1000.0 / UV_SPAN
    for name, (size, make) in minerals.MINERALS.items():
        albedo = common.linear_to_srgb(make(size, span_mm))
        common.write_jpeg(common.output("textures", "stone", f"{name}_albedo.jpg"), albedo, 90)
    for name, (size, make) in minerals.MARBLES.items():
        albedo = common.linear_to_srgb(make(size, minerals.MARBLE_SPAN_MM))
        common.write_jpeg(common.output("textures", "stone", f"{name}_albedo.jpg"), albedo, 90)
