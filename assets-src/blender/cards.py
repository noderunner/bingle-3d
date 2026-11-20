"""The pictures on the games' cards in the client's list of games to start (`pictures/cards/`),
rendered in Cycles with the same materials as the client's table.

`points.jpg`: a teak board of lines, nine by nine with its star points, and slate and shell stones
on its crossings, from `assets/appearance/points.toml`. The games whose appearance file names it
show it; a game with none has a blank space on its card."""

import math
import os
import random

import bpy

import common
import previews
import stones

# One stone's width between lines, as the stone was made for (`stones.py`).
PITCH = 0.028
LINES = 9
HEIGHT = 0.016
# The lines and the edge, cut into the wood and darkened, as the client draws them.
LINE_WIDTH = 0.0009
EDGE_WIDTH = 0.0018
STAR_POINTS = [(-2, -2), (2, -2), (0, 0), (-2, 2), (2, 2)]
# A position from early in a game, by line from the middle: black's stones, then white's.
# None on a star point, so they show.
BLACK = [(-2, -1), (1, 2), (-1, 1), (2, -1), (0, -2), (-3, 2)]
WHITE = [(1, -2), (-1, 2), (2, 1), (-2, -3), (3, -1), (0, 1)]
SIZE = 512


def dark(name, colour):
    mat = bpy.data.materials.new(name)
    mat.use_nodes = True
    bsdf = mat.node_tree.nodes["Principled BSDF"]
    bsdf.inputs["Base Color"].default_value = [previews.srgb_to_linear(c) for c in colour] + [1.0]
    bsdf.inputs["Roughness"].default_value = 0.7
    return mat


def points_board():
    bpy.ops.wm.read_factory_settings(use_empty=True)
    look = previews.appearance("points")
    mats = {k: previews.material(k, v, "wood" in v) for k, v in look["materials"].items()}

    world = bpy.data.worlds.new("Room")
    world.use_nodes = True
    env = world.node_tree.nodes.new("ShaderNodeTexEnvironment")
    env.image = bpy.data.images.load(common.source("lythwood_room_2k.hdr"))
    world.node_tree.links.new(env.outputs["Color"], world.node_tree.nodes["Background"].inputs[0])
    bpy.context.scene.world = world

    span = (LINES - 1) * PITCH
    side = span + 2 * PITCH
    previews.box("Table", (1.2, 1.2, 0.02), (0, 0, -0.01), previews.material("table", previews.TABLE, True))
    previews.box("Board", (side, side, HEIGHT), (0, 0, HEIGHT / 2), previews.material("board", previews.BOARD, True))

    cut = dark("cut", (0.16, 0.10, 0.06))
    top = HEIGHT + 0.0001
    half = (LINES - 1) // 2
    for i in range(LINES):
        at = (i - half) * PITCH
        width = EDGE_WIDTH if i in (0, LINES - 1) else LINE_WIDTH
        length = span + width
        previews.box(f"Row {i}", (length, width, 0.0002), (0, at, top), cut)
        previews.box(f"Column {i}", (width, length, 0.0002), (at, 0, top), cut)
    for gx, gy in STAR_POINTS:
        bpy.ops.mesh.primitive_cylinder_add(radius=0.0016, depth=0.0002, location=(gx * PITCH, gy * PITCH, top))
        bpy.context.object.data.materials.append(cut)

    rng = random.Random(5)
    seats = look["seats"]
    for kind, spots in ((seats["black"]["pieces"], BLACK), (seats["white"]["pieces"], WHITE)):
        for gx, gy in spots:
            obj = stones.make_stone_object(f"Stone {kind} {gx} {gy}")
            obj.data.materials.append(mats[kind])
            # Placed by hand, never quite on the crossing.
            obj.location = (
                gx * PITCH + rng.uniform(-0.0008, 0.0008),
                gy * PITCH + rng.uniform(-0.0008, 0.0008),
                top + stones.HALF_THICKNESS,
            )
            obj.rotation_euler = (rng.uniform(-0.02, 0.02), rng.uniform(-0.02, 0.02), rng.uniform(0, 2 * math.pi))

    cam = bpy.data.objects.new("Camera", bpy.data.cameras.new("Camera"))
    cam.data.lens = 50
    cam.data.clip_start = 0.005
    bpy.context.scene.collection.objects.link(cam)
    # From the player's side and above, the whole board in the square picture.
    cam.location = (0.0, -0.25, 0.355)
    target = bpy.data.objects.new("Target", None)
    bpy.context.scene.collection.objects.link(target)
    target.location = (0.0, 0.004, HEIGHT)
    track = cam.constraints.new("TRACK_TO")
    track.target = target
    bpy.context.scene.camera = cam


def run():
    out = os.path.join(common.ASSETS, "pictures", "cards")
    os.makedirs(out, exist_ok=True)
    points_board()
    previews.render(os.path.join(out, "points.jpg"), samples=256, size=(SIZE, SIZE), jpeg=True)
