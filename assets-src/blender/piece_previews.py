"""Pictures of the pieces, rendered in Cycles, to look at before the client's own screenshots:
the chessmen, the checkers and Tak's pieces, and the cards and dice, on a board on the table, in
the room's light. Each is made as its step makes it, and wears the materials its appearance file
gives it (the card and the die their generic look, repeated here from `src/look.rs`), so a
picture shows what the client will.

They go to `target/asset-previews/`; nothing here is shipped. `PIECE_VIEWS=chess,chess-knight`
renders only those."""

import math
import os
import random

import bpy

import common
import matlib
import previews

PITCH = 0.028
BOARD_TOP = 0.016


def stage(board=(0.26, 0.20)):
    """The table, a board on it, and the room."""
    bpy.ops.wm.read_factory_settings(use_empty=True)
    world = bpy.data.worlds.new("Room")
    world.use_nodes = True
    env = world.node_tree.nodes.new("ShaderNodeTexEnvironment")
    env.image = bpy.data.images.load(common.source("lythwood_room_2k.hdr"))
    world.node_tree.links.new(env.outputs["Color"], world.node_tree.nodes["Background"].inputs[0])
    bpy.context.scene.world = world
    previews.box("Table", (1.6, 1.6, 0.02), (0, 0, -0.01), previews.material("table", previews.TABLE, True))
    if board:
        previews.box(
            "Board", (board[0], board[1], BOARD_TOP), (0, 0, BOARD_TOP / 2), previews.material("board", previews.BOARD, True)
        )


def camera(at, target, lens=60):
    cam = bpy.data.objects.new("Camera", bpy.data.cameras.new("Camera"))
    cam.data.lens = lens
    cam.data.clip_start = 0.005
    bpy.context.scene.collection.objects.link(cam)
    cam.location = at
    aim = bpy.data.objects.new("Target", None)
    bpy.context.scene.collection.objects.link(aim)
    aim.location = target
    track = cam.constraints.new("TRACK_TO")
    track.target = aim
    bpy.context.scene.camera = cam
    return cam, aim


def wants(name):
    only = os.environ.get("PIECE_VIEWS")
    return not only or name in only.split(",")


def shot(name, at, target, lens=60, samples=160):
    if not wants(name):
        return
    for obj in list(bpy.data.objects):
        if obj.type in ("CAMERA", "EMPTY"):
            bpy.data.objects.remove(obj, do_unlink=True)
    camera(at, target, lens)
    previews.render(os.path.join(common.PREVIEWS, f"{name}.png"), samples=samples)


def materials(look):
    for name, spec in look["materials"].items():
        matlib.build(name, spec)


def chess():
    import chessmen

    stage()
    look = previews.appearance("chessmen")
    materials(look)
    seats = list(look["seats"].values())
    order = ["rook", "knight", "bishop", "queen", "king", "pawn"]
    for row, seat in enumerate(seats[:2]):
        for i, name in enumerate(order):
            obj, tall = chessmen.make(name)
            obj.data.materials.clear()
            obj.data.materials.append(bpy.data.materials[seat["pieces"]])
            x = (i - 2.5) * PITCH * 1.25
            y = (-0.9 if row == 0 else 1.1) * PITCH
            obj.location = (x, y, BOARD_TOP + tall / 2)
            # The far side's look back at the near side's.
            obj.rotation_euler = (0, 0, math.pi if row == 1 else 0.0)
            if row == 0 and name == "knight":
                obj.rotation_euler = (0, 0, math.radians(-62))
    shot("chess", (0.05, -0.30, 0.15), (0.0, 0.0, 0.03))
    shot("chess-knight", (0.06, -0.115, 0.055), (-0.05, -0.02, 0.032), lens=85)
    shot("chess-heads", (-0.02, -0.16, 0.085), (0.035, -0.02, 0.045), lens=85)


def stand(obj, tall, material, x, y, on=BOARD_TOP, yaw=0.0):
    obj.data.materials.clear()
    obj.data.materials.append(bpy.data.materials[material])
    obj.location = (x, y, on + tall / 2)
    obj.rotation_euler = (0, 0, yaw)
    return on + tall


def draughts():
    import checkers

    stage()
    look = previews.appearance("draughts")
    materials(look)
    rng = random.Random(4)
    red, black = look["seats"]["red"]["pieces"], look["seats"]["black"]["pieces"]
    spots = [(-3, -1, red), (-1, -1, red), (1, -1, red), (-2, 0, red), (0, 0, black), (2, 0, black), (-1, 1, black), (3, 1, black)]
    for gx, gy, kind in spots:
        obj, tall = checkers.make("man")
        stand(obj, tall, kind, gx * PITCH, gy * PITCH, yaw=rng.uniform(0, 6.28))
    for gx, gy, kind in ((3, -1, red), (1, 1, black)):
        obj, tall = checkers.make("king")
        stand(obj, tall, kind, gx * PITCH, gy * PITCH, yaw=rng.uniform(0, 6.28))
    shot("checkers", (0.04, -0.26, 0.15), (0.0, 0.0, 0.02))
    shot("checkers-close", (0.035, -0.085, 0.05), (0.07, -0.02, 0.022), lens=85)


def tak_pieces():
    import tak

    stage()
    look = previews.appearance("tak")
    materials(look)
    rng = random.Random(8)
    pale, dark = look["seats"]["white"]["pieces"], look["seats"]["black"]["pieces"]

    def pile(gx, gy, kinds, top=None):
        on = BOARD_TOP
        for kind in kinds:
            obj, tall = tak.make("flat")
            on = stand(obj, tall, kind, gx * PITCH, gy * PITCH, on, rng.choice((0, 1, 2, 3)) * math.pi / 2 + rng.uniform(-0.05, 0.05))
        if top:
            obj, tall = tak.make(top[0])
            stand(obj, tall, top[1], gx * PITCH, gy * PITCH, on, rng.choice((0, 1)) * math.pi / 2 + rng.uniform(-0.05, 0.05))

    pile(-3, 0, [pale])
    pile(-2, 0, [dark, pale, dark])
    pile(-1, 0, [], ("wall", pale))
    pile(0, 0, [pale, dark], ("wall", dark))
    pile(1, 0, [], ("cap", pale))
    pile(2, 0, [dark, dark, pale], ("cap", dark))
    pile(3, 0, [dark])
    pile(-1, 1, [dark, pale, pale, dark, pale])
    pile(1, -1, [pale])
    pile(0, -1, [dark])
    shot("tak", (0.04, -0.26, 0.15), (0.0, 0.0, 0.025))
    shot("tak-close", (-0.01, -0.10, 0.055), (0.012, 0.0, 0.028), lens=85)


# The generic looks of a card and of a die, as in `src/look.rs`.
PAPER = {"tint": [0.955, 0.945, 0.915], "roughness": 0.5, "clearcoat": 0.15, "clearcoat_roughness": 0.3}
CARD_BACK = {**PAPER, "tint": [1.0, 1.0, 1.0], "albedo": "textures/cards/back_albedo.jpg"}
DIE = {"tint": [0.93, 0.90, 0.82], "roughness": 0.3, "clearcoat": 0.5, "clearcoat_roughness": 0.1}
PIPS = {"tint": [0.05, 0.05, 0.06], "roughness": 0.5}


def sample_face(path):
    """A picture to try on a card's face: the seven of hearts, plainly drawn. A seven is not the
    same in a mirror or upside down, so a side mapped the wrong way shows."""
    import numpy as np

    w, h = 500, 700
    x, y = np.meshgrid((np.arange(w) + 0.5) / w * 100.0, (np.arange(h) + 0.5) / h * 140.0)
    red = np.zeros((h, w))

    def heart(cx, cy, size, flip=False):
        u = (x - cx) / (size * 0.42)
        v = (cy - y) / (size * 0.42) * (-1.0 if flip else 1.0) + 0.25
        return np.clip(-((u * u + v * v - 1.0) ** 3 - u * u * v**3) * 60.0, 0.0, 1.0)

    def stroke(a, b, width):
        ax, ay, bx, by = *a, *b
        t = np.clip(((x - ax) * (bx - ax) + (y - ay) * (by - ay)) / ((bx - ax) ** 2 + (by - ay) ** 2), 0.0, 1.0)
        d = np.hypot(x - ax - t * (bx - ax), y - ay - t * (by - ay))
        return np.clip((width / 2 - d) * 5.0 + 0.5, 0.0, 1.0)

    for cx, cy in ((30, 34), (70, 34), (50, 52), (30, 70), (70, 70)):
        red = np.maximum(red, heart(cx, cy, 17))
    for cx, cy in ((30, 106), (70, 106)):
        red = np.maximum(red, heart(cx, cy, 17, flip=True))
    # The index in two corners: a 7 and a small heart, the lower one upside down.
    for sx, sy, sign in ((7.0, 8.0, 1.0), (93.0, 132.0, -1.0)):
        p = lambda dx, dy: (sx + sign * dx, sy + sign * dy)
        red = np.maximum(red, stroke(p(0, 0), p(6.5, 0), 1.7))
        red = np.maximum(red, stroke(p(6.5, 0), p(2.2, 11), 1.7))
        red = np.maximum(red, heart(sx + sign * 3.2, sy + sign * 18.5, 7.5, flip=sign < 0))
    paper, ink = np.array([0.955, 0.945, 0.915]), np.array([0.72, 0.08, 0.10])
    common.write_png(path, paper * (1.0 - red[..., None]) + ink * red[..., None])


def cards_and_dice():
    import dice
    import playing_cards

    stage(board=None)
    for name, spec in (("back", CARD_BACK), ("edge", PAPER), ("body", DIE), ("pips", PIPS)):
        matlib.build(name, spec)
    face_file = os.path.join(common.PREVIEWS, "card-face-sample.png")
    sample_face(face_file)
    face = matlib.build("face", PAPER)
    picture = face.node_tree.nodes.new("ShaderNodeTexImage")
    picture.image = bpy.data.images.load(face_file)
    bsdf = next(n for n in face.node_tree.nodes if n.type == "BSDF_PRINCIPLED")
    face.node_tree.links.new(picture.outputs["Color"], bsdf.inputs["Base Color"])

    def lay(x, y, yaw, up=True, on=0.0):
        obj = playing_cards.card()
        obj.location = (x, y, on + playing_cards.THICKNESS / 2)
        # Turned over about its long axis, as a card is.
        obj.rotation_euler = (0, 0 if up else math.pi, yaw)
        return obj

    lay(-0.075, 0.0, 0.10)
    lay(0.0, 0.004, -0.06, up=False)
    lay(0.075, -0.002, 0.04)
    rng = random.Random(2)
    for i in range(12):
        lay(0.17 + rng.uniform(-0.0006, 0.0006), 0.01 + rng.uniform(-0.0006, 0.0006), rng.uniform(-0.03, 0.03), up=False, on=i * playing_cards.THICKNESS)
    turns = [(0, 0, 0.3), (math.pi / 2, 0, 1.1), (0, math.pi / 2, -0.4), (math.pi, 0, 2.0)]
    for i, turn in enumerate(turns):
        obj = dice.die()
        obj.location = (-0.03 + 0.027 * i + rng.uniform(-0.004, 0.004), -0.085 + rng.uniform(-0.008, 0.008), dice.SIDE / 2)
        obj.rotation_euler = turn
    shot("cards-dice", (0.05, -0.30, 0.20), (0.03, -0.02, 0.0))
    shot("dice-close", (0.03, -0.17, 0.05), (0.01, -0.085, 0.006), lens=85)
    shot("card-close", (0.0, -0.10, 0.06), (0.0, 0.0, 0.0), lens=50)


SCENES = {
    chess: ("chess", "chess-knight", "chess-heads"),
    draughts: ("checkers", "checkers-close"),
    tak_pieces: ("tak", "tak-close"),
    cards_and_dice: ("cards-dice", "dice-close", "card-close"),
}


def run():
    os.makedirs(common.PREVIEWS, exist_ok=True)
    for scene, views in SCENES.items():
        if any(wants(view) for view in views):
            scene()
