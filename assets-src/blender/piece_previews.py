"""Pictures of the pieces, rendered in Cycles, to look at before the client's own screenshots:
the chessmen, the checkers and Tak's pieces, Blooms' stones, the cards and dice, and the two-sided
discs and their rings, on a board on the table, in the room's light. Each is made as its step
makes it, and wears the materials its appearance file gives it (the card, the die and the disc
their generic look, repeated here from `src/look.rs`), so a picture shows what the client will.

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
    import chessmen
    import tak

    stage()
    look = previews.appearance("tak")
    materials(look)
    rng = random.Random(8)
    pale, dark = look["seats"]["white"]["pieces"], look["seats"]["black"]["pieces"]

    def pile(gx, gy, kinds, top=None):
        on = BOARD_TOP
        # Set down by hand, as the client sets a block down (`board::stance`): never quite in
        # the middle, never quite square.
        aside = lambda: rng.uniform(-0.0013, 0.0013)
        askew = lambda: rng.uniform(-0.09, 0.09)
        for kind in kinds:
            obj, tall = tak.make("flat")
            turn = rng.choice((0, 1, 2, 3)) * math.pi / 2 + askew()
            on = stand(obj, tall, kind, gx * PITCH + aside(), gy * PITCH + aside(), on, turn)
        if top and top[0] == "cap":
            # The capstone is a pawn.
            obj, tall = chessmen.make("pawn")
            stand(obj, tall, top[1], gx * PITCH, gy * PITCH, on)
        elif top:
            obj, tall = tak.make(top[0])
            turn = rng.choice((0, 1)) * math.pi / 2 + askew()
            stand(obj, tall, top[1], gx * PITCH + aside(), gy * PITCH + aside(), on, turn)

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


def cards_and_dice():
    import dice
    import playing_cards

    stage(board=None)
    for name, spec in (("back", CARD_BACK), ("edge", PAPER), ("body", DIE), ("pips", PIPS)):
        matlib.build(name, spec)
    matlib.build("face", PAPER)

    def lay(x, y, yaw, up=True, on=0.0, face=None):
        obj = playing_cards.card()
        if face:
            # One of the usual deck's faces (`card_faces.py`), as the client prints it.
            file = f"textures/cards/faces/{face}_albedo.png"
            obj.data.materials[0] = matlib.build(f"face {face}", {**CARD_BACK, "albedo": file})
        obj.location = (x, y, on + playing_cards.THICKNESS / 2)
        # Turned over about its long axis, as a card is.
        obj.rotation_euler = (0, 0 if up else math.pi, yaw)
        return obj

    # A seven is not the same in a mirror or upside down, so a side mapped the wrong way shows.
    lay(-0.075, 0.0, 0.10, face="hearts_7")
    lay(0.0, 0.004, -0.06, up=False)
    lay(0.075, -0.002, 0.04, face="spades_k")
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


def blooms():
    """Blooms' stones of polished minerals on the board (its bowls are the client's to look at:
    they are Go's, smaller)."""
    import stones

    stage()
    look = previews.appearance("blooms")
    materials(look)
    rng = random.Random(9)
    kinds = [look["seats"][c]["pieces"] for c in ("red", "yellow", "blue", "black")]
    spots = [(-3, -1), (-1, -1), (1, -1), (3, -1), (-2, 0), (0, 0), (2, 0), (-1, 1), (1, 1)]
    for i, (gx, gy) in enumerate(spots):
        obj = stones.make_stone_object(f"Stone {i}")
        obj.data.materials.append(bpy.data.materials[kinds[i % 4]])
        obj.location = (gx * PITCH, gy * PITCH, BOARD_TOP + stones.HALF_THICKNESS)
        obj.rotation_euler = (0, 0, rng.uniform(0, 6.28))
    shot("blooms", (0.0, -0.36, 0.22), (0.0, -0.01, 0.01))
    shot("blooms-close", (0.02, -0.11, 0.07), (0.02, -0.01, 0.02), lens=85)


def fendo():
    """Fendo's pawns and fences on a few squares (lines drawn on the stand-in board), and the bowl
    of fences beside them."""
    import chessmen
    import fences
    import tableware

    stage(board=(0.30, 0.22))
    look = previews.appearance("fendo")
    materials(look)
    line = previews.material("line", {"wood": "walnut", "tint": [0.12, 0.10, 0.09]}, True)
    for k in range(-3, 4):
        previews.box(f"Line x{k}", (0.0006, 4 * PITCH, 0.0002), ((k + 0.5) * PITCH, 0.5 * PITCH, BOARD_TOP), line)
        previews.box(f"Line y{k}", (6 * PITCH, 0.0006, 0.0002), (0.5 * PITCH, (k + 0.5) * PITCH, BOARD_TOP), line)
    white, orange = look["seats"]["white"]["pieces"], look["seats"]["orange"]["pieces"]
    for gx, gy, kind in ((-1, 0, white), (1, 1, orange), (2, -1, white)):
        obj, tall = chessmen.make("pawn")
        stand(obj, tall, kind, gx * PITCH, gy * PITCH)
    # Fences on squares' sides: (square, the step to the neighbour it faces).
    for (gx, gy), (dx, dy) in (((-1, 0), (1, 0)), ((-1, 0), (0, 1)), ((1, 1), (-1, 0)), ((1, 1), (0, -1)), ((0, 1), (0, -1))):
        obj = fences.fence()
        stand(obj, fences.HEIGHT, "fence", (gx + dx / 2) * PITCH, (gy + dy / 2) * PITCH, yaw=math.pi / 2 if dx else 0.0)
    bowl = tableware.bowl()
    heap = fences.heap()
    for obj, material in ((bowl, "bowl"), (heap, "fence")):
        obj.data.materials.clear()
        obj.data.materials.append(bpy.data.materials[material])
        obj.scale = (0.62, 0.62, 0.62)
        obj.location = (0.21, -0.02, 0.0)
    shot("fendo", (0.06, -0.36, 0.22), (0.05, 0.0, 0.02))
    shot("fendo-close", (-0.01, -0.12, 0.08), (-0.01, 0.01, 0.03), lens=85)


# The generic looks of a two-sided disc's sides, as in `src/look.rs`; the rings here wear them too.
DISC_LIGHT = {"tint": [0.90, 0.88, 0.82], "roughness": 0.3, "clearcoat": 0.4, "clearcoat_roughness": 0.12}
DISC_DARK = {"tint": [0.045, 0.045, 0.05], "roughness": 0.3, "clearcoat": 0.4, "clearcoat_roughness": 0.12}


def discs_and_rings():
    """Rings on a few points of a lattice of 28 mm, a disc lying in some of them, and discs
    showing either side: whether a disc fits a ring, and whether its sides read apart."""
    import discs

    stage(board=(0.24, 0.18))
    for name, spec in (("light", DISC_LIGHT), ("dark", DISC_DARK)):
        matlib.build(name, spec)
    rng = random.Random(5)

    def disc(x, y, light_up):
        obj, tall = discs.disc()
        obj.location = (x, y, BOARD_TOP + tall / 2)
        # Turned over about the axis across the table, as a disc is flipped.
        obj.rotation_euler = (0 if light_up else math.pi, 0, rng.uniform(0, 6.28))

    def ring(x, y, material):
        obj, tall = discs.ring()
        stand(obj, tall, material, x, y, yaw=rng.uniform(0, 6.28))

    # Points of a lattice of equilateral triangles: a row's points half a step aside of the next.
    def at(col, row):
        return ((col + 0.5 * (row % 2)) * PITCH, row * PITCH * math.sqrt(3) / 2)

    for col, row, material, inside in ((-2, 0, "light", True), (0, 0, "dark", False), (1, 1, "dark", True), (-1, -1, "light", False), (1, -1, "dark", True)):
        x, y = at(col, row)
        ring(x, y, material)
        if inside:
            disc(x, y, material == "light")
    for col, row, light_up in ((-1, 0, True), (1, 0, False), (2, 0, True), (0, 1, False), (-1, 1, True), (0, -1, True)):
        disc(*at(col, row), light_up)
    shot("discs", (0.04, -0.28, 0.17), (0.0, 0.0, 0.01))
    shot("discs-close", (0.03, -0.10, 0.055), (0.0, -0.005, 0.02), lens=85)


SCENES = {
    chess: ("chess", "chess-knight", "chess-heads"),
    draughts: ("checkers", "checkers-close"),
    tak_pieces: ("tak", "tak-close"),
    cards_and_dice: ("cards-dice", "dice-close", "card-close"),
    blooms: ("blooms", "blooms-close"),
    fendo: ("fendo", "fendo-close"),
    discs_and_rings: ("discs", "discs-close"),
}


def run():
    os.makedirs(common.PREVIEWS, exist_ok=True)
    for scene, views in SCENES.items():
        if any(wants(view) for view in views):
            scene()
