"""Engraved letters: the glyphs the board's coordinates are cut in, as an atlas the client lays
over the wood (`src/coordinates.rs`).

Blender sets each glyph (Fira Sans SemiBold, the client's own font) as a text object and renders
them all, white on black, from straight above: the glyph's shape, antialiased. From that shape:

- `glyphs_albedo.png`: the cut's color, a mid brown on its walls (the wood, cut across, which
  the light shows up or leaves in shade) darkening to the bottom of the groove, and how much of
  it covers the wood (alpha), so the grain still shows a little through it;
- `glyphs_normal.png`: the groove's walls, sloping down from the surface into the cut, so the
  edge facing the light catches it and the other falls into shade (OpenGL convention, green up).

Each glyph is centred in its cell, its baseline a little below the middle; the client knows the
order and the grid (`GLYPHS`, `COLUMNS`, `ROWS` there and here)."""

import os

import bpy
import numpy as np

import common

GLYPHS = "0123456789abcdefghijklmnopqrstuvwxyz"
COLUMNS = 8
ROWS = 5
CELL = 128
FONT = os.path.join(common.ASSETS, "fonts", "FiraSans-SemiBold.ttf")
# The glyph's size against its cell, and its baseline's height (in cells, from the middle).
SIZE = 0.8
BASELINE = -0.22
# The cut's color (sRGB) on its walls and at its bottom, how much it covers, and how wide its
# walls are (in pixels).
WALLS = (0.46, 0.33, 0.22)
BOTTOM = (0.07, 0.045, 0.03)
COVER = 0.92
WALL = 5.0
# How steep the walls look.
DEPTH = 5.0


def render_mask():
    bpy.ops.wm.read_factory_settings(use_empty=True)
    scene = bpy.context.scene
    font = bpy.data.fonts.load(FONT)
    white = bpy.data.materials.new("White")
    white.use_nodes = True
    nodes = white.node_tree.nodes
    nodes.clear()
    emit = nodes.new("ShaderNodeEmission")
    out = nodes.new("ShaderNodeOutputMaterial")
    white.node_tree.links.new(emit.outputs["Emission"], out.inputs["Surface"])
    for i, ch in enumerate(GLYPHS):
        col, row = i % COLUMNS, i // COLUMNS
        curve = bpy.data.curves.new(f"glyph {ch}", type="FONT")
        curve.body = ch
        curve.font = font
        curve.size = SIZE
        curve.align_x = "CENTER"
        curve.align_y = "BOTTOM_BASELINE"
        curve.materials.append(white)
        obj = bpy.data.objects.new(f"glyph {ch}", curve)
        obj.location = (col - COLUMNS / 2 + 0.5, ROWS / 2 - row - 0.5 + BASELINE, 0.0)
        scene.collection.objects.link(obj)
    cam = bpy.data.objects.new("Camera", bpy.data.cameras.new("Camera"))
    cam.data.type = "ORTHO"
    cam.data.ortho_scale = COLUMNS
    cam.location = (0.0, 0.0, 5.0)
    scene.collection.objects.link(cam)
    scene.camera = cam
    world = bpy.data.worlds.new("Black")
    world.use_nodes = True
    world.node_tree.nodes["Background"].inputs["Color"].default_value = (0, 0, 0, 1)
    scene.world = world
    scene.render.engine = "CYCLES"
    scene.cycles.device = "CPU"
    # Enough samples, and every pixel all of them, for clean edges.
    scene.cycles.samples = 256
    scene.cycles.use_adaptive_sampling = False
    scene.cycles.use_denoising = False
    scene.cycles.seed = 0
    scene.view_settings.view_transform = "Standard"
    scene.render.resolution_x = COLUMNS * CELL
    scene.render.resolution_y = ROWS * CELL
    scene.render.image_settings.file_format = "PNG"
    scene.render.image_settings.color_mode = "BW"
    scene.render.image_settings.color_depth = "16"
    os.makedirs(common.PREVIEWS, exist_ok=True)
    path = os.path.join(common.PREVIEWS, "glyph-mask.png")
    scene.render.filepath = path
    bpy.ops.render.render(write_still=True)
    mask = common.read_image(path)[..., 0]
    return np.clip(mask, 0.0, 1.0)


def blur(img, radius):
    """A soft blur: three box blurs, each `radius` either way, with the edges held."""
    r = max(int(round(radius)), 1)
    out = img
    for _ in range(3):
        for axis in (0, 1):
            pad = [(0, 0), (0, 0)]
            pad[axis] = (r, r)
            p = np.pad(out, pad, mode="edge")
            c = np.cumsum(p, axis=axis)
            c = np.concatenate([np.zeros_like(np.take(c, [0], axis=axis)), c], axis=axis)
            n = out.shape[axis]
            hi = np.take(c, np.arange(2 * r + 1, 2 * r + 1 + n), axis=axis)
            lo = np.take(c, np.arange(0, n), axis=axis)
            out = (hi - lo) / (2 * r + 1)
    return out


def run():
    mask = render_mask()
    # The cut's floor is the glyph's shape; its walls slope over `WALL` pixels round the edge.
    height = -blur(mask, WALL / 2.0)
    dy, dx = np.gradient(height)
    # Image rows run down; the normal map's green is up the texture.
    normal = np.stack([-dx * DEPTH * CELL / 16.0, dy * DEPTH * CELL / 16.0, np.ones_like(dx)], -1)
    normal /= np.linalg.norm(normal, axis=-1, keepdims=True)
    normal_rgb = normal * 0.5 + 0.5
    # How deep each point is, 0 at the surface to 1 at the bottom: its walls are what lies
    # between, and darken going down.
    depth = np.clip(-height, 0.0, 1.0)
    bottom = np.clip((depth - 0.35) / 0.5, 0.0, 1.0)[..., None]
    color = np.array(WALLS) * (1.0 - bottom) + np.array(BOTTOM) * bottom
    # It covers the whole cut, walls and all, fading out at the very lip.
    cover = np.clip(blur(mask, WALL / 2.0) * 2.5, 0.0, 1.0) * COVER
    albedo = np.empty(mask.shape + (4,), np.float32)
    albedo[..., :3] = color
    albedo[..., 3] = cover
    common.write_png(common.output("textures", "engraving", "glyphs_albedo.png"), albedo)
    common.write_png(common.output("textures", "engraving", "glyphs_normal.png"), normal_rgb)
