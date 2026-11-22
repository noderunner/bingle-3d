"""The faces of the usual deck of playing cards: spades, hearts, diamonds and clubs, each from the
ace to the king, as pictures for the card's face (`playing_cards.py`).

- `textures/cards/faces/<suit>_<rank>_albedo.png`, 52 of them (`spades_a`, `hearts_10`,
  `clubs_k`): 640 × 896, which is 5 to 7, the card's paper all over with the face printed on it
  in one ink, red or black, and on a court card gold and a tinted panel besides.

Each is drawn here, with no picture to start from. A suit is an outline of curves; a rank is
lettered in Fira Sans SemiBold, the client's own font, which Blender sets and turns into
triangles; a court card has a crown, a coronet or a plumed cap for its rank, twice, so it reads
the same from either end. The outlines are filled by counting crossings along each row of a grid
four times as fine as the picture, which is then averaged down: nothing is rendered, so the same
bytes come out every time.

A face is laid out in millimetres about the card's middle, x to the right and y up: the rank and
its suit in two opposite corners, and between them the suit as many times as the rank counts, in
the places cards have always had them, those below the middle upside down.

No game names these files: the client finds one by a suit's symbol and a rank's label, as the
server gives them (`props::standard_face`)."""

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

SUITS = ["spades", "hearts", "diamonds", "clubs"]
RANKS = ["a", "2", "3", "4", "5", "6", "7", "8", "9", "10", "j", "q", "k"]
# Fira's J hangs below the line the other letters stand on, into the suit under it.
HANGS = {"j"}

# Colours (sRGB). The paper is the card's own (`look::paper`).
PAPER = playing_cards.PAPER
INKS = {
    "spades": (0.075, 0.075, 0.085),
    "clubs": (0.075, 0.075, 0.085),
    "hearts": (0.74, 0.07, 0.10),
    "diamonds": (0.74, 0.07, 0.10),
}
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
# An ace's one suit; the ace of spades is the deck's largest, as it always is.
ACE = 27.0
ACE_OF_SPADES = 33.0
# A court card: its frame's half size, the corners' radius and its lines' width.
FRAME = (19.2, 31.6)
FRAME_CORNER = 2.4
LINE = 0.42


# --- Outlines, in a box from -1 to 1 each way, y up ------------------------------------------


def curve(p0, p1, p2, p3, steps=20):
    """A cubic Bézier curve's points, without its last."""
    t = (np.arange(steps) / steps)[:, None]
    p0, p1, p2, p3 = (np.array(p, float) for p in (p0, p1, p2, p3))
    return (1 - t) ** 3 * p0 + 3 * (1 - t) ** 2 * t * p1 + 3 * (1 - t) * t**2 * p2 + t**3 * p3


def outline(start, *pieces):
    """A closed outline from `start`: each piece a point (a line to it) or three (a curve)."""
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


def rounded_box(half, radius, steps=10):
    out = []
    w, h = half[0] - radius, half[1] - radius
    for corner, (cx, cy) in enumerate(((w, h), (-w, h), (-w, -h), (w, -h))):
        for step in range(steps + 1):
            a = math.pi / 2 * (corner + step / steps)
            out.append((cx + radius * math.cos(a), cy + radius * math.sin(a)))
    return np.array(out)


HEART = [
    both_sides(
        outline(
            (0.0, -0.93),
            ((0.10, -0.70), (0.98, -0.20), (0.98, 0.38)),
            ((0.98, 0.72), (0.76, 0.95), (0.51, 0.95)),
            ((0.28, 0.95), (0.08, 0.80), (0.0, 0.56)),
        )
    )
]
DIAMOND = [
    both_sides(
        outline(
            (0.0, -0.98),
            ((0.22, -0.60), (0.48, -0.26), (0.76, 0.0)),
            ((0.48, 0.26), (0.22, 0.60), (0.0, 0.98)),
        )
    )
]
STEM = both_sides(
    outline(
        (0.0, -0.98),
        (0.44, -0.98),
        ((0.22, -0.88), (0.09, -0.62), (0.07, -0.16)),
        (0.0, -0.16),
    )
)
SPADE = [
    both_sides(
        outline(
            (0.0, -0.46),
            ((0.08, -0.62), (0.26, -0.72), (0.48, -0.72)),
            ((0.76, -0.72), (0.98, -0.52), (0.98, -0.20)),
            ((0.98, 0.26), (0.12, 0.62), (0.0, 0.98)),
        )
    ),
    STEM,
]
CLUB = [
    disc((0.0, 0.52), 0.42),
    disc((-0.52, -0.16), 0.42),
    disc((0.52, -0.16), 0.42),
    disc((0.0, 0.02), 0.26),
    STEM,
]
SHAPES = {"hearts": HEART, "diamonds": DIAMOND, "spades": SPADE, "clubs": CLUB}

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
WEARS = {"k": CROWN, "q": CORONET, "j": CAP}


# --- Filling ----------------------------------------------------------------------------------


class Sheet:
    """A face being drawn: layers of ink, each a colour and where it is, on the fine grid."""

    def __init__(self):
        self.per_mm = SIZE[0] * OVER / WIDTH
        self.shape = (SIZE[1] * OVER, SIZE[0] * OVER)
        self.layers = []

    def ink(self, colour):
        """A new layer over the others; returns it, to fill."""
        mask = np.zeros(self.shape, bool)
        self.layers.append((colour, mask))
        return mask

    def fill(self, mask, contours):
        """Fills what the outlines (millimetres) enclose: a point is inside when the outlines
        wind round it, so one inside another, turning the other way, is a hole."""
        h, w = mask.shape
        pts = [np.stack([(c[:, 0] + WIDTH / 2) * self.per_mm, (HEIGHT / 2 - c[:, 1]) * self.per_mm], -1) for c in contours]
        every = np.concatenate(pts)
        x0, y0 = (int(v) for v in np.clip(np.floor(every.min(0)), 0, (w, h)))
        x1, y1 = (int(v) for v in np.clip(np.ceil(every.max(0)) + 1, 0, (w, h)))
        if x1 <= x0 or y1 <= y0:
            return
        # In each row, +1 or -1 at the first point right of every crossing; summed along the
        # row, that is how many times the outline winds round each point.
        turns = np.zeros((y1 - y0, x1 - x0 + 1), np.int16)
        for p in pts:
            for (ax, ay), (bx, by) in zip(p, np.roll(p, -1, axis=0)):
                if ay == by:
                    continue
                top, bottom = min(ay, by), max(ay, by)
                rows = np.arange(max(math.ceil(top - 0.5), y0), min(math.ceil(bottom - 0.5), y1))
                if len(rows) == 0:
                    continue
                x = ax + (rows + 0.5 - ay) * (bx - ax) / (by - ay)
                columns = np.clip(np.ceil(x - 0.5).astype(int), x0, x1) - x0
                turns[rows - y0, columns] += 1 if by > ay else -1
        mask[y0:y1, x0:x1] |= np.cumsum(turns, axis=1)[:, :-1] != 0

    def line(self, mask, contour, width=LINE):
        """A line along a closed outline: a strip for each of its sides, and a round join."""
        for a, b in zip(contour, np.roll(contour, -1, axis=0)):
            along = b - a
            length = math.hypot(*along)
            if length < 1e-9:
                continue
            across = np.array([-along[1], along[0]]) / length * width / 2
            self.fill(mask, [np.array([a + across, b + across, b - across, a - across])])
            self.fill(mask, [disc(a, width / 2, 10)])

    def pixels(self):
        """The picture, as sRGB values (height, width, 3): each ink over the paper, mixed in
        linear light by how much of each pixel it covers."""
        linear = np.empty((SIZE[1], SIZE[0], 3))
        linear[:] = common.srgb_to_linear(np.array(PAPER))
        for colour, mask in self.layers:
            cover = mask.reshape(SIZE[1], OVER, SIZE[0], OVER).mean((1, 3))[..., None]
            linear = linear * (1.0 - cover) + common.srgb_to_linear(np.array(colour)) * cover
        return common.linear_to_srgb(linear)


def placed(shape, at, size, upside_down=False):
    """A shape's outlines `size` across, about `at`."""
    k = size / 2 * (-1.0 if upside_down else 1.0)
    return [part * k + np.array(at) for part in shape]


def lettering():
    """Each rank's letters as triangles, one unit to a capital's height, standing on y = 0 about
    x = 0. Blender sets the text; a figure of two digits is set closer and narrower, to go in a
    corner."""
    font = bpy.data.fonts.load(FONT)
    out = {}
    for rank in RANKS + ["H"]:
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
        # All wound the same way, so that together they are one outline's inside.
        a, b, c = triangles[:, 0], triangles[:, 1], triangles[:, 2]
        area = (b[:, 0] - a[:, 0]) * (c[:, 1] - a[:, 1]) - (b[:, 1] - a[:, 1]) * (c[:, 0] - a[:, 0])
        triangles[area < 0] = triangles[area < 0][:, ::-1]
        triangles = triangles[np.abs(area) > 1e-12]
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


def face(suit, rank, letters):
    """One card's face, as sRGB values."""
    sheet = Sheet()
    shape, colour = SHAPES[suit], INKS[suit]
    if rank in WEARS:
        court(sheet, suit, rank)
    ink = sheet.ink(colour)

    # The index, and the same turned round in the opposite corner.
    for turn in (1.0, -1.0):
        corner = np.array([-WIDTH / 2 + INDEX_X, HEIGHT / 2 - INDEX_TOP - INDEX_CAP])
        sheet.fill(ink, list((letters[rank] * INDEX_CAP + corner) * turn))
        at = np.array([-WIDTH / 2 + INDEX_X, HEIGHT / 2 - INDEX_SUIT_AT]) * turn
        for part in placed(shape, at, INDEX_SUIT, turn < 0):
            sheet.fill(ink, [part])

    if rank == "a":
        size = ACE_OF_SPADES if suit == "spades" else ACE
        for part in placed(shape, (0.0, 0.0), size):
            sheet.fill(ink, [part])
    for x, y in PLACES.get(rank, []):
        for part in placed(shape, (x, y), PIP, y < 0):
            sheet.fill(ink, [part])
    return sheet.pixels()


def court(sheet, suit, rank):
    """A court card's middle: a tinted panel in a frame, what the rank wears at each end of it,
    and the suit between."""
    shape, colour = SHAPES[suit], INKS[suit]
    frame = rounded_box(FRAME, FRAME_CORNER)
    sheet.fill(sheet.ink(PANEL), [frame])
    ink = sheet.ink(colour)
    sheet.line(ink, frame)
    sheet.line(ink, rounded_box((FRAME[0] - 1.5, FRAME[1] - 1.5), FRAME_CORNER - 0.9), LINE * 0.6)
    for end in (False, True):
        for part in placed(WEARS[rank], (0.0, -19.0 if end else 19.0), 19.0, end):
            sheet.fill(sheet.ink(GOLD), [part])
            sheet.line(sheet.ink(colour), part, LINE * 0.8)
    ink = sheet.ink(colour)
    for part in placed(shape, (0.0, 0.0), 17.0):
        sheet.fill(ink, [part])
    # The suit again, small, in the frame's two free corners.
    for end in (1.0, -1.0):
        at = np.array([FRAME[0] - 5.2, FRAME[1] - 5.6]) * end
        for part in placed(shape, at, 5.0, end < 0):
            sheet.fill(ink, [part])


def name(suit, rank):
    return f"{suit}_{rank}_albedo.png"


def run():
    bpy.ops.wm.read_factory_settings(use_empty=True)
    letters = lettering()
    faces = {}
    for suit in SUITS:
        for rank in RANKS:
            faces[suit, rank] = face(suit, rank, letters)
            common.write_png(common.output("textures", "cards", "faces", name(suit, rank)), faces[suit, rank])
    # The whole deck on one sheet, a suit to a row, to look at.
    os.makedirs(common.PREVIEWS, exist_ok=True)
    small = {key: common.resize_to(pixels.astype(np.float32), SIZE[0] // 2, SIZE[1] // 2) for key, pixels in faces.items()}
    gap = 12
    w, h = SIZE[0] // 2 + gap, SIZE[1] // 2 + gap
    sheet = np.full((len(SUITS) * h + gap, len(RANKS) * w + gap, 3), 0.25, np.float32)
    for row, suit in enumerate(SUITS):
        for column, rank in enumerate(RANKS):
            y, x = gap + row * h, gap + column * w
            sheet[y : y + h - gap, x : x + w - gap] = small[suit, rank]
    common.write_png(os.path.join(common.PREVIEWS, "card-faces.png"), sheet)
