"""What the steps that draw playing cards share (`card_faces.py`, `card_sets.py`): outlines,
filling them, lettering, and how a face is laid out.

A face is laid out in millimetres about the card's middle, x to the right and y up: the rank and
its suit in two opposite corners, and between them the suit as many times as the rank counts, in
the places cards have always had them, those below the middle upside down. An ace has its suit
once, large. A court card has a tinted panel in a frame, what its rank wears at each end of it (a
plumed cap, a coronet or a crown), and its suit between, so it reads the same from either end.

A suit's picture is layers, each filled shapes or lines in one ink: the suit's own, a second
colour, or the paper's, which cuts into what is under it. It is drawn in a box from -1 to 1 each
way, y up.

Nothing is rendered. A rank is lettered in Fira Sans SemiBold, the client's own font, which
Blender sets and turns into triangles. Outlines are filled by counting crossings along each row
of a grid four times as fine as the picture, which is then averaged down: the same bytes come
out every time."""

import math
import os

import bpy
import numpy as np

import common
import playing_cards

SIZE = playing_cards.BACK_SIZE
WIDTH = playing_cards.WIDTH * 1000.0
HEIGHT = playing_cards.HEIGHT * 1000.0
# The grid the outlines are filled on, against the picture's.
OVER = 4
FONT = os.path.join(common.ASSETS, "fonts", "FiraSans-SemiBold.ttf")

# Colours (sRGB). The paper is the card's own (`look::paper`).
PAPER = playing_cards.PAPER
GOLD = (0.86, 0.66, 0.20)
PANEL = (0.972, 0.932, 0.815)

# The index in a corner: the rank's middle from the card's side, its top from the card's top,
# its height, and the small suit under it.
INDEX_X = 7.2
INDEX_TOP = 4.4
INDEX_CAP = 7.6
INDEX_SUIT = 5.4
INDEX_SUIT_AT = 16.3
# The suits of a number card: their size, the two columns, and the rows.
PIP = 11.6
COLUMN = 12.6
ROW = 27.6
L, C, R = -COLUMN, 0.0, COLUMN
THIRD = ROW / 3.0
PLACES = {
    "2": [(C, ROW), (C, -ROW)],
    "3": [(C, ROW), (C, 0.0), (C, -ROW)],
    "4": [(L, ROW), (R, ROW), (L, -ROW), (R, -ROW)],
    "5": [(L, ROW), (R, ROW), (C, 0.0), (L, -ROW), (R, -ROW)],
    "6": [(L, ROW), (R, ROW), (L, 0.0), (R, 0.0), (L, -ROW), (R, -ROW)],
    "7": [(L, ROW), (R, ROW), (C, ROW / 2), (L, 0.0), (R, 0.0), (L, -ROW), (R, -ROW)],
    "8": [(L, ROW), (R, ROW), (C, ROW / 2), (L, 0.0), (R, 0.0), (C, -ROW / 2), (L, -ROW), (R, -ROW)],
    "9": [(L, ROW), (R, ROW), (L, THIRD), (R, THIRD), (C, 0.0), (L, -THIRD), (R, -THIRD), (L, -ROW), (R, -ROW)],
    "10": [
        (L, ROW), (R, ROW), (C, 2 * THIRD), (L, THIRD), (R, THIRD),
        (L, -THIRD), (R, -THIRD), (C, -2 * THIRD), (L, -ROW), (R, -ROW),
    ],
}  # fmt: skip
# The ranks whose card has its suit once, large (a game may call its lowest card either), and
# how large.
ACES = {"a", "1"}
ACE = 27.0
# A court card: its frame's half size, the corners' radius and its lines' width.
FRAME = (19.2, 31.6)
FRAME_CORNER = 2.4
LINE = 0.42
# Fira's J hangs below the line the other letters stand on, into the suit under it.
HANGS = {"j"}


# --- Outlines, in a box from -1 to 1 each way, y up ------------------------------------------


def curve(p0, p1, p2, p3, steps=20):
    """A cubic Bézier curve's points, without its last."""
    t = (np.arange(steps) / steps)[:, None]
    p0, p1, p2, p3 = (np.array(p, float) for p in (p0, p1, p2, p3))
    return (1 - t) ** 3 * p0 + 3 * (1 - t) ** 2 * t * p1 + 3 * (1 - t) * t**2 * p2 + t**3 * p3


def outline(start, *pieces):
    """An outline (or a path) from `start`: each piece a point (a line to it) or three (a
    curve)."""
    points, at = [], start
    for piece in pieces:
        if isinstance(piece[0], (int, float)):
            points.append(np.array([at], float))
            at = piece
        else:
            points.append(curve(at, *piece))
            at = piece[2]
    points.append(np.array([at], float))
    return np.concatenate(points)


def both_sides(half):
    """An outline the same on both sides, from the half of it to the right of its middle, which
    starts and ends on the middle."""
    return np.concatenate([half, half[-2:0:-1] * (-1.0, 1.0)])


def disc(centre, radius, sides=48):
    a = np.arange(sides) / sides * 2 * math.pi
    return np.stack([centre[0] + radius * np.cos(a), centre[1] + radius * np.sin(a)], -1)


def arc(centre, radius, start, end, steps=24):
    """Part of a circle, as a path: from `start` to `end`, in degrees anticlockwise from +x."""
    a = np.radians(start + (end - start) * np.arange(steps + 1) / steps)
    return np.stack([centre[0] + radius * np.cos(a), centre[1] + radius * np.sin(a)], -1)


def star(points, outer, inner, turn=0.0):
    """A star about the middle, its first point up, turned `turn` degrees anticlockwise."""
    a = np.radians(90.0 + turn) + np.arange(2 * points) * math.pi / points
    r = np.where(np.arange(2 * points) % 2 == 0, outer, inner)
    return np.stack([r * np.cos(a), r * np.sin(a)], -1)


def box(x0, y0, x1, y1):
    return np.array([(x0, y0), (x1, y0), (x1, y1), (x0, y1)], float)


def turned(shape, degrees, about=(0.0, 0.0)):
    """A shape turned anticlockwise about a point."""
    a = math.radians(degrees)
    spin = np.array([[math.cos(a), math.sin(a)], [-math.sin(a), math.cos(a)]])
    return (np.asarray(shape, float) - about) @ spin + about


def mirrored(shape):
    """A shape's other side: as in a mirror standing on the middle."""
    return np.asarray(shape, float) * (-1.0, 1.0)


def moved(shape, by):
    return np.asarray(shape, float) + np.array(by, float)


def rounded_box(half, radius, steps=10):
    out = []
    w, h = half[0] - radius, half[1] - radius
    for corner, (cx, cy) in enumerate(((w, h), (-w, h), (-w, -h), (w, -h))):
        for step in range(steps + 1):
            a = math.pi / 2 * (corner + step / steps)
            out.append((cx + radius * math.cos(a), cy + radius * math.sin(a)))
    return np.array(out)


def filled(*outlines, ink="suit"):
    """A layer of a picture: these outlines, filled with an ink (`suit`, `second` or `paper`)."""
    return {"ink": ink, "outlines": [np.asarray(o, float) for o in outlines]}


def lined(*paths, width, ink="suit", closed=False):
    """A layer of a picture: lines along these paths, `width` wide, their ends round."""
    return {"ink": ink, "paths": [np.asarray(p, float) for p in paths], "width": width, "closed": closed}


# What a court card's rank wears. Each is a list of parts, filled in gold with a line of the
# suit's ink round each, a later part lying over an earlier one.
BAND = np.array([(-0.80, -0.62), (0.80, -0.62), (0.80, -0.34), (-0.80, -0.34)])
CROWN = [
    np.array(
        [
            (-0.80, -0.34), (0.80, -0.34), (0.98, 0.44), (0.60, 0.04), (0.42, 0.60),
            (0.21, 0.08), (0.0, 0.74), (-0.21, 0.08), (-0.42, 0.60), (-0.60, 0.04), (-0.98, 0.44),
        ]
    ),
    disc((-0.98, 0.44), 0.085, 16),
    disc((-0.42, 0.60), 0.085, 16),
    disc((0.42, 0.60), 0.085, 16),
    disc((0.98, 0.44), 0.085, 16),
    # A cross on the middle point.
    np.array(
        [
            (-0.05, 0.70), (0.05, 0.70), (0.05, 0.84), (0.15, 0.84), (0.15, 0.93), (0.05, 0.93),
            (0.05, 1.04), (-0.05, 1.04), (-0.05, 0.93), (-0.15, 0.93), (-0.15, 0.84), (-0.05, 0.84),
        ]
    ),
    BAND,
]  # fmt: skip
CORONET = [
    both_sides(
        outline(
            (0.0, -0.34),
            (0.80, -0.34),
            ((0.98, -0.16), (0.98, 0.20), (0.80, 0.34)),
            ((0.70, 0.12), (0.56, 0.02), (0.44, 0.02)),
            ((0.36, 0.20), (0.24, 0.44), (0.0, 0.62)),
        )
    ),
    disc((-0.80, 0.40), 0.10, 16),
    disc((0.80, 0.40), 0.10, 16),
    disc((0.0, 0.72), 0.12, 20),
    BAND,
]
CAP = [
    # A plume from behind the cap, swept back over it.
    outline(
        (0.62, -0.20),
        ((0.60, 0.40), (0.10, 0.92), (-0.92, 0.98)),
        ((-0.40, 0.70), (0.02, 0.36), (0.16, -0.20)),
    ),
    both_sides(
        outline(
            (0.0, -0.34),
            (0.86, -0.34),
            ((0.86, 0.08), (0.48, 0.36), (0.0, 0.36)),
        )
    ),
    BAND,
    # The clasp that holds the plume.
    disc((0.40, -0.48), 0.09, 16),
]
# By rank: a game may call its court cards by their letters or count on past ten.
WEARS = {"j": CAP, "q": CORONET, "k": CROWN, "11": CAP, "12": CORONET, "13": CROWN}


# --- Filling ----------------------------------------------------------------------------------


class Sheet:
    """A face being drawn: layers of ink over the paper, each a colour and where it is, on the
    fine grid."""

    def __init__(self, paper=PAPER):
        self.paper = paper
        self.per_mm = SIZE[0] * OVER / WIDTH
        self.shape = (SIZE[1] * OVER, SIZE[0] * OVER)
        self.layers = []

    def ink(self, colour):
        """A new layer over the others; returns it, to fill."""
        mask = np.zeros(self.shape, bool)
        self.layers.append((colour, mask))
        return mask

    def fill(self, mask, contours):
        """Fills what the outlines (millimetres) enclose, all of them together: an outline
        inside another is not a hole in it."""
        h, w = mask.shape
        starts, ends = [], []
        for c in contours:
            p = np.stack([(c[:, 0] + WIDTH / 2) * self.per_mm, (HEIGHT / 2 - c[:, 1]) * self.per_mm], -1)
            q = np.roll(p, -1, axis=0)
            # All wound the same way, so that what winds round a point is never undone.
            if np.sum(p[:, 0] * q[:, 1] - q[:, 0] * p[:, 1]) < 0:
                p = p[::-1]
                q = np.roll(p, -1, axis=0)
            starts.append(p)
            ends.append(q)
        a, b = np.concatenate(starts), np.concatenate(ends)
        x0, y0 = (int(v) for v in np.clip(np.floor(a.min(0)), 0, (w, h)))
        x1, y1 = (int(v) for v in np.clip(np.ceil(a.max(0)) + 1, 0, (w, h)))
        if x1 <= x0 or y1 <= y0:
            return
        # Each side crosses the rows between its ends. In a row, +1 or -1 at the first point
        # right of every crossing; summed along the row, that is how many times the outlines
        # wind round each point.
        ax, ay, bx, by = a[:, 0], a[:, 1], b[:, 0], b[:, 1]
        first = np.maximum(np.ceil(np.minimum(ay, by) - 0.5).astype(int), y0)
        last = np.minimum(np.ceil(np.maximum(ay, by) - 0.5).astype(int), y1)
        count = np.maximum(last - first, 0)
        side = np.repeat(np.arange(len(count)), count)
        if len(side) == 0:
            return
        rows = first[side] + np.arange(len(side)) - (np.cumsum(count) - count)[side]
        x = ax[side] + (rows + 0.5 - ay[side]) * (bx[side] - ax[side]) / (by[side] - ay[side])
        columns = np.clip(np.ceil(x - 0.5).astype(int), x0, x1) - x0
        turns = np.zeros((y1 - y0, x1 - x0 + 1), np.int32)
        np.add.at(turns, (rows - y0, columns), np.where(by[side] > ay[side], 1, -1))
        mask[y0:y1, x0:x1] |= np.cumsum(turns, axis=1)[:, :-1] != 0

    def line(self, mask, path, width=LINE, closed=True):
        """A line along a path, or all round an outline: a strip for each of its sides, and a
        round join or end at each of its points."""
        path = np.asarray(path, float)
        ends = np.roll(path, -1, axis=0) if closed else path[1:]
        parts = [disc(point, width / 2, 10) for point in path]
        for a, b in zip(path, ends):
            along = b - a
            length = math.hypot(*along)
            if length < 1e-9:
                continue
            across = np.array([-along[1], along[0]]) / length * width / 2
            parts.append(np.array([a + across, b + across, b - across, a - across]))
        self.fill(mask, parts)

    def pixels(self):
        """The picture, as sRGB values (height, width, 3): each ink over the paper, mixed in
        linear light by how much of each pixel it covers."""
        linear = np.empty((SIZE[1], SIZE[0], 3))
        linear[:] = common.srgb_to_linear(np.array(self.paper))
        for colour, mask in self.layers:
            if not mask.any():
                continue
            cover = mask.reshape(SIZE[1], OVER, SIZE[0], OVER).mean((1, 3))[..., None]
            linear = linear * (1.0 - cover) + common.srgb_to_linear(np.array(colour)) * cover
        return common.linear_to_srgb(linear)


def placed(shape, at, size, upside_down=False):
    """A shape `size` across, about `at`."""
    return shape * (size / 2 * (-1.0 if upside_down else 1.0)) + np.array(at)


class Stamp:
    """A picture to put on a sheet any number of times. Every copy's same layer goes into one
    layer of the sheet's ink, so copies must not lie over one another."""

    def __init__(self, sheet, picture, colours):
        self.sheet, self.picture = sheet, picture
        self.masks = [sheet.ink(colours[layer["ink"]]) for layer in picture]

    def put(self, at, size, upside_down=False):
        for layer, mask in zip(self.picture, self.masks):
            if "outlines" in layer:
                self.sheet.fill(mask, [placed(o, at, size, upside_down) for o in layer["outlines"]])
            else:
                for path in layer["paths"]:
                    self.sheet.line(mask, placed(path, at, size, upside_down), layer["width"] * size / 2, layer["closed"])


def lettering(ranks):
    """Each rank's letters as triangles, one unit to a capital's height, standing on y = 0 about
    x = 0. Blender sets the text; a figure of two digits is set closer and narrower, to go in a
    corner."""
    font = bpy.data.fonts.load(FONT)
    out = {}
    for rank in list(ranks) + ["H"]:
        text = bpy.data.curves.new(f"rank {rank}", type="FONT")
        text.body = rank.upper()
        text.font = font
        text.size = 1.0
        text.resolution_u = 24
        text.space_character = 0.82 if len(rank) > 1 else 1.0
        obj = bpy.data.objects.new(f"rank {rank}", text)
        bpy.context.scene.collection.objects.link(obj)
        bpy.context.view_layer.update()
        mesh = obj.evaluated_get(bpy.context.evaluated_depsgraph_get()).to_mesh()
        mesh.calc_loop_triangles()
        corners = np.array([v.co[:2] for v in mesh.vertices], float)
        triangles = corners[np.array([t.vertices[:] for t in mesh.loop_triangles])]
        if len(rank) > 1:
            triangles[..., 0] *= 0.82
        if rank in HANGS:
            # Made to stand on the line, and as much shorter as that takes: its stem keeps
            # its width, so it is as heavy as the others.
            low, high = triangles[..., 1].min(), triangles[..., 1].max()
            triangles[..., 1] = (triangles[..., 1] - low) * (high / (high - low))
        triangles[..., 0] -= (triangles[..., 0].min() + triangles[..., 0].max()) / 2
        out[rank] = triangles
    # A capital's height is that of one with a flat top and foot.
    cap = out.pop("H")[..., 1].max()
    return {rank: triangles / cap for rank, triangles in out.items()}


def face(rank, picture, colours, letters, ace=ACE, index_suit=INDEX_SUIT, gold=GOLD, panel=PANEL, trim=None):
    """One card's face, as sRGB values. `picture` is its suit's; `colours` are its inks, by
    name (`suit`, `second`, `paper`); `index_suit` is the size of the suit in a corner; `trim`
    is a line round the card, if it has one: how far in from the edge, its corners' radius, its
    width and its colour."""
    sheet = Sheet(colours["paper"])
    if trim:
        inset, corner, width, colour = trim
        sheet.line(sheet.ink(colour), rounded_box((WIDTH / 2 - inset, HEIGHT / 2 - inset), corner), width)
    if rank in WEARS:
        # A tinted panel in a frame, and what the rank wears at each end of it.
        frame = rounded_box(FRAME, FRAME_CORNER)
        sheet.fill(sheet.ink(panel), [frame])
        ink = sheet.ink(colours["suit"])
        sheet.line(ink, frame)
        sheet.line(ink, rounded_box((FRAME[0] - 1.5, FRAME[1] - 1.5), FRAME_CORNER - 0.9), LINE * 0.6)
        for end in (False, True):
            for part in WEARS[rank]:
                part = placed(part, (0.0, -19.0 if end else 19.0), 19.0, end)
                sheet.fill(sheet.ink(gold), [part])
                sheet.line(sheet.ink(colours["suit"]), part, LINE * 0.8)
    suit = Stamp(sheet, picture, colours)

    # The index, and the same turned round in the opposite corner.
    ink = sheet.ink(colours["suit"])
    for turn in (1.0, -1.0):
        corner = np.array([-WIDTH / 2 + INDEX_X, HEIGHT / 2 - INDEX_TOP - INDEX_CAP])
        sheet.fill(ink, list((letters[rank] * INDEX_CAP + corner) * turn))
        # A larger suit keeps its distance from the rank.
        down = INDEX_SUIT_AT + (index_suit - INDEX_SUIT) / 2
        suit.put(np.array([-WIDTH / 2 + INDEX_X, HEIGHT / 2 - down]) * turn, index_suit, turn < 0)

    if rank in ACES:
        suit.put((0.0, 0.0), ace)
    elif rank in WEARS:
        suit.put((0.0, 0.0), 17.0)
        # The suit again, small, in the frame's two free corners.
        for end in (1.0, -1.0):
            suit.put(np.array([FRAME[0] - 5.2, FRAME[1] - 5.6]) * end, 5.0, end < 0)
    for x, y in PLACES.get(rank, []):
        suit.put((x, y), PIP, y < 0)
    return sheet.pixels()


def write_sheet(name, rows, shrink=2, gap=12):
    """Faces on one sheet, a row for each list, to look at (`target/asset-previews/`)."""
    os.makedirs(common.PREVIEWS, exist_ok=True)
    w, h = SIZE[0] // shrink, SIZE[1] // shrink
    sheet = np.full((len(rows) * (h + gap) + gap, max(len(r) for r in rows) * (w + gap) + gap, 3), 0.25, np.float32)
    for row, faces in enumerate(rows):
        for column, pixels in enumerate(faces):
            y, x = gap + row * (h + gap), gap + column * (w + gap)
            sheet[y : y + h, x : x + w] = common.resize_to(pixels.astype(np.float32), w, h)
    common.write_png(os.path.join(common.PREVIEWS, name), sheet)
