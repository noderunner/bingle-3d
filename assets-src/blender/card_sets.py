"""Themed sets of cards: a set gives a deck of plain suits and ranks another look and changes
nothing of how it plays, as Bingle's own sets do (its ADR 0085). A set has eight suits and a back
of its own. A game's suits wear the set's by place, in the game's own order of suits, so a game
of four suits wears the first four; its ranks are the game's own, whatever it calls them.

- `textures/cards/sets/<set>/<suit>_<rank>_albedo.png`: a face for each suit and each rank a
  game here may ask for: an ace by either name (`a`, `1`), 2 to 10, and the court cards by
  their letters (`j`, `q`, `k`) or counted on past ten (`11`, `12`, `13`). 640 × 896.
- `textures/cards/sets/<set>/back_albedo.png`: the set's back, the same either way up.

One set so far, **Ancient Empires** (`empires`): Greece, Egypt, Rome, China, the Aztecs, the
Zulu, Persia and Japan, each a picture of its own in a colour of its own, with gold for a second
ink (turquoise on the Aztecs' stone), on paper the colour of parchment. The first and third are warm colours and the second and
fourth cool ones, so a game that tells its suits apart as red and black (a patience) still can.

Everything is drawn here, as `card_faces.py` draws the usual deck, with `cardkit.py`. The client
finds a face by the set's id, the suit's place and the rank's label (`props::set_face`)."""

import math

import bpy
import numpy as np

import cardkit
import common
from cardkit import arc, both_sides, box, disc, filled, lined, mirrored, moved, outline, star, turned

RANKS = ["a", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13", "j", "q", "k"]

GOLD = (0.80, 0.60, 0.16)
TURQUOISE = (0.04, 0.52, 0.54)


# --- Ancient Empires --------------------------------------------------------------------------

# Greece: a temple's front.
COLUMNS = (-0.66, -0.22, 0.22, 0.66)
GREECE = [
    filled(
        np.array([(-1.0, 0.42), (1.0, 0.42), (0.0, 0.98)]),
        box(-0.90, 0.20, 0.90, 0.36),
        *[box(x - 0.095, -0.60, x + 0.095, 0.20) for x in COLUMNS],
        *[box(x - 0.15, 0.12, x + 0.15, 0.20) for x in COLUMNS],
        *[box(x - 0.15, -0.60, x + 0.15, -0.53) for x in COLUMNS],
        box(-0.90, -0.76, 0.90, -0.62),
        box(-1.0, -0.96, 1.0, -0.80),
    ),
    filled(disc((0.0, 0.60), 0.105, 24), ink="second"),
]

# Egypt: the eye of Horus.
EGYPT = [
    # The brow, the eye's lids and the line drawn out from its corner.
    lined(outline((-0.96, 0.52), ((-0.42, 0.98), (0.40, 0.92), (0.98, 0.50))), width=0.17),
    lined(
        outline(
            (-0.84, 0.18),
            ((-0.40, 0.62), (0.30, 0.62), (0.72, 0.20)),
            ((0.30, -0.14), (-0.40, -0.16), (-0.84, 0.18)),
        ),
        width=0.13,
        closed=True,
    ),
    lined(outline((0.70, 0.20), ((0.82, 0.14), (0.92, 0.12), (1.0, 0.12))), width=0.11),
    # The mark below it, and the curl that ends in a coil.
    filled(np.array([(-0.20, -0.12), (0.06, -0.12), (-0.02, -0.98), (-0.12, -0.98)])),
    lined(
        outline(
            (0.14, -0.10),
            ((0.24, -0.52), (0.50, -0.84), (0.78, -0.80)),
            ((0.98, -0.76), (1.0, -0.48), (0.82, -0.44)),
            ((0.70, -0.42), (0.64, -0.56), (0.76, -0.60)),
        ),
        width=0.11,
    ),
    filled(disc((-0.04, 0.24), 0.25, 32)),
    filled(disc((-0.04, 0.24), 0.10, 20), ink="second"),
]


def leaf(at, towards, length=0.36, width=0.095):
    """A leaf from a point, lying `towards` degrees anticlockwise of +x."""
    shape = outline(
        (0.0, 0.0),
        ((length * 0.25, width), (length * 0.70, width), (length, 0.0)),
        ((length * 0.70, -width), (length * 0.25, -width), (0.0, 0.0)),
    )[:-1]
    return moved(turned(shape, towards), at)


def branch():
    """The right half of a wreath: a stem round a circle, with a leaf each side of it all the
    way up and one at its end."""
    radius, leaves = 0.70, []
    for step in range(7):
        a = -78.0 + step * 24.0
        at = (radius * math.cos(math.radians(a)), radius * math.sin(math.radians(a)))
        leaves += [leaf(at, a + 90.0 - 38.0), leaf(at, a + 90.0 + 38.0, length=0.30)]
    a = -78.0 + 7 * 24.0
    leaves.append(leaf((radius * math.cos(math.radians(a)), radius * math.sin(math.radians(a))), a + 90.0))
    return leaves


# Rome: a wreath of laurel, tied with a ribbon.
ROME = [
    lined(arc((0.0, 0.0), 0.70, -84.0, 90.0), mirrored(arc((0.0, 0.0), 0.70, -84.0, 90.0)), width=0.055),
    filled(*branch(), *[mirrored(l) for l in branch()]),
    lined(
        outline((0.0, -0.70), ((0.10, -0.80), (0.20, -0.90), (0.34, -0.96))),
        outline((0.0, -0.70), ((-0.10, -0.80), (-0.20, -0.90), (-0.34, -0.96))),
        width=0.10,
        ink="second",
    ),
    filled(disc((0.0, -0.70), 0.105, 20), ink="second"),
]


def roof(y, eave, top, rise=0.26):
    """A roof with its eaves turned up: its foot at height `y`, `eave` out to each side, and
    `top` wide where the next storey stands on it."""
    return both_sides(
        outline(
            (0.0, y + rise),
            (top, y + rise),
            ((top + 0.06, y + 0.06), (eave - 0.22, y + 0.02), (eave, y + 0.15)),
            ((eave - 0.10, y + 0.02), (eave - 0.24, y - 0.02), (eave - 0.34, y - 0.02)),
            (0.0, y - 0.02),
        )
    )


# China: a pagoda.
CHINA = [
    filled(
        box(-0.62, -0.98, 0.62, -0.86),
        box(-0.44, -0.86, 0.44, -0.56),
        roof(-0.56, 0.98, 0.34),
        box(-0.34, -0.30, 0.34, -0.08),
        roof(-0.08, 0.80, 0.25),
        box(-0.25, 0.18, 0.25, 0.36),
        both_sides(
            outline(
                (0.0, 0.74),
                ((0.06, 0.52), (0.36, 0.40), (0.62, 0.50)),
                ((0.52, 0.38), (0.40, 0.34), (0.30, 0.34)),
                (0.0, 0.34),
            )
        ),
    ),
    lined(np.array([(0.0, 0.70), (0.0, 0.98)]), width=0.06),
    filled(disc((0.0, 0.84), 0.075, 16), ink="second"),
    # Its door, and a window in each storey above.
    filled(
        both_sides(outline((0.0, -0.86), (0.12, -0.86), (0.12, -0.72), ((0.12, -0.64), (0.06, -0.60), (0.0, -0.60)))),
        box(-0.07, -0.25, 0.07, -0.13),
        box(-0.05, 0.22, 0.05, 0.32),
        ink="paper",
    ),
]

# The Aztecs: the stone of the sun.
AZTEC = [
    filled(star(8, 1.0, 0.66), star(8, 0.84, 0.66, 22.5), disc((0.0, 0.0), 0.72)),
    lined(disc((0.0, 0.0), 0.60), width=0.045, ink="paper", closed=True),
    filled(*[disc(turned((0.0, 0.48), 22.5 + 45.0 * k), 0.055, 12) for k in range(8)], ink="paper"),
    filled(disc((0.0, 0.0), 0.37), ink="paper"),
    filled(disc((0.0, 0.0), 0.30), ink="second"),
    # The sun's face: its eyes, and its tongue put out.
    filled(
        disc((-0.12, 0.07), 0.055, 12),
        disc((0.12, 0.07), 0.055, 12),
        np.array([(-0.10, -0.07), (0.10, -0.07), (0.0, -0.24)]),
    ),
]

# The Zulu: a shield of hide on its stick, a spear and a club crossed behind it.
SHIELD = both_sides(
    outline(
        (0.0, -0.90),
        ((0.34, -0.78), (0.58, -0.40), (0.58, 0.0)),
        ((0.58, 0.40), (0.34, 0.78), (0.0, 0.90)),
    )
)
SPEARHEAD = outline(
    (0.0, 0.0),
    ((0.10, 0.06), (0.11, 0.20), (0.0, 0.40)),
    ((-0.11, 0.20), (-0.10, 0.06), (0.0, 0.0)),
)[:-1]
ZULU = [
    lined(np.array([(-0.80, -0.92), (0.66, 0.66)]), np.array([(0.80, -0.92), (-0.70, 0.74)]), width=0.075),
    filled(moved(turned(SPEARHEAD, -42.7), (0.64, 0.64)), disc((-0.74, 0.78), 0.14, 24)),
    lined(SHIELD, width=0.12, ink="paper", closed=True),
    filled(SHIELD),
    lined(np.array([(0.0, -1.0), (0.0, 1.0)]), width=0.07),
    # The slits its lacing goes through, in two rows down the middle.
    lined(
        *[np.array([(side * 0.09, y), (side * 0.26, y)]) for side in (-1.0, 1.0) for y in np.arange(-0.54, 0.55, 0.18)],
        width=0.055,
        ink="paper",
    ),
    lined(np.array([(0.0, -0.62), (0.0, 0.62)]), width=0.05, ink="second"),
]

# Persia: the winged disc.
WING = np.array(
    [
        (0.16, 0.42), (1.0, 0.50), (0.98, 0.28), (0.78, 0.26), (0.76, 0.08),
        (0.56, 0.06), (0.54, -0.12), (0.16, -0.14),
    ]
)  # fmt: skip
FEATHERS = [np.array([(0.24, 0.27), (0.78, 0.27)]), np.array([(0.24, 0.07), (0.56, 0.07)])]
TAIL = np.array([(-0.24, -0.10), (0.24, -0.10), (0.42, -0.78), (0.0, -0.96), (-0.42, -0.78)])
STREAMER = outline(
    (0.26, -0.16),
    ((0.42, -0.50), (0.60, -0.66), (0.80, -0.62)),
    ((0.96, -0.58), (0.94, -0.36), (0.78, -0.38)),
)
PERSIA = [
    lined(STREAMER, mirrored(STREAMER), width=0.075),
    filled(WING, mirrored(WING), TAIL),
    lined(*FEATHERS, *[mirrored(f) for f in FEATHERS], width=0.04, ink="paper"),
    lined(
        np.array([(0.0, -0.24), (0.0, -0.90)]),
        np.array([(0.10, -0.24), (0.21, -0.80)]),
        np.array([(-0.10, -0.24), (-0.21, -0.80)]),
        width=0.04,
        ink="paper",
    ),
    lined(disc((0.0, 0.15), 0.36), width=0.05, ink="paper", closed=True),
    filled(disc((0.0, 0.15), 0.33)),
    filled(disc((0.0, 0.15), 0.21), ink="paper"),
    filled(disc((0.0, 0.15), 0.13), ink="second"),
]

# Japan: a cherry blossom.
PETAL = both_sides(
    outline(
        (0.0, 0.10),
        ((0.34, 0.26), (0.52, 0.66), (0.28, 0.94)),
        ((0.22, 1.01), (0.08, 0.94), (0.0, 0.80)),
    )
)
JAPAN = [
    filled(*[turned(PETAL, 72.0 * k) for k in range(5)]),
    lined(*[turned(np.array([(0.0, 0.0), (0.0, 0.36)]), 72.0 * k) for k in range(5)], width=0.035, ink="paper"),
    filled(*[turned(disc((0.0, 0.40), 0.05, 12), 72.0 * k) for k in range(5)], ink="paper"),
    filled(disc((0.0, 0.0), 0.15, 24), ink="second"),
]

# A set: its id, its paper, its second ink, its suits in order (each its name, its ink, its
# picture, and a second ink of its own if it has one), the line round a face, the size of the
# suit in a corner, a court card's panel, and its back.
EMPIRES = {
    "id": "empires",
    "paper": (0.955, 0.918, 0.812),
    "second": GOLD,
    "panel": (0.975, 0.895, 0.715),
    "trim": (2.3, 1.9, 0.26, (0.66, 0.50, 0.24)),
    "index_suit": 6.4,
    "suits": [
        ("greece", (0.74, 0.29, 0.10), GREECE),
        ("egypt", (0.10, 0.30, 0.62), EGYPT),
        ("rome", (0.58, 0.07, 0.17), ROME),
        ("china", (0.07, 0.46, 0.32), CHINA),
        ("aztec", (0.62, 0.40, 0.02), AZTEC, TURQUOISE),
        ("zulu", (0.17, 0.13, 0.11), ZULU),
        ("persia", (0.42, 0.16, 0.52), PERSIA),
        ("japan", (0.80, 0.29, 0.47), JAPAN),
    ],
}
SETS = [EMPIRES]

# The back of Ancient Empires: a key pattern round a dark field, and in the middle a sun of
# eight points, with a stud round it for each empire.
NIGHT = (0.085, 0.125, 0.27)
DUSK = (0.15, 0.20, 0.38)
BRONZE = (0.84, 0.66, 0.28)


def key_band(length, unit=1.0):
    """A key pattern along +x, about x = 0, its foot on y = 0: hooks standing on a line."""
    count = int(length // (4 * unit))
    start = -count * 2 * unit
    paths = [np.array([(start, 0.0), (-start, 0.0)])]
    for i in range(count):
        x = start + i * 4 * unit + 0.5 * unit
        hook = [(0.0, 0.0), (0.0, 3.0), (3.0, 3.0), (3.0, 1.2), (1.4, 1.2)]
        paths.append(np.array(hook) * unit + (x, 0.0))
    return paths


def empires_back():
    sheet = cardkit.Sheet(EMPIRES["paper"])
    w, h = cardkit.WIDTH / 2, cardkit.HEIGHT / 2
    panel = cardkit.rounded_box((w - 3.2, h - 3.2), 2.0)
    sheet.fill(sheet.ink(NIGHT), [panel])
    # A lattice over the field, faint, kept inside the key pattern.
    lattice = sheet.ink(DUSK)
    for k in np.arange(-150.0, 150.0, 5.2):
        for slope in (1.0, -1.0):
            sheet.line(lattice, np.array([(k - 100.0, -100.0 * slope), (k + 100.0, 100.0 * slope)]), 0.22, closed=False)
    inside = np.zeros_like(lattice)
    sheet.fill(inside, [cardkit.rounded_box((w - 10.6, h - 10.6), 1.2)])
    lattice &= inside
    middle = sheet.ink(NIGHT)
    sheet.fill(middle, [disc((0.0, 0.0), 17.2, 96)])

    gold = sheet.ink(BRONZE)
    sheet.line(gold, cardkit.rounded_box((w - 4.3, h - 4.3), 1.3), 0.32)
    sheet.line(gold, cardkit.rounded_box((w - 10.6, h - 10.6), 1.2), 0.32)
    # The key pattern: along the top and up the right side, and the same turned half round.
    across = [moved(p, (0.0, h - 9.2)) for p in key_band(2 * w - 24.0)]
    up = [moved(turned(p, -90.0), (w - 9.2, 0.0)) for p in key_band(2 * h - 24.0)]
    for path in across + up:
        sheet.line(gold, path, 0.42, closed=False)
        sheet.line(gold, -path, 0.42, closed=False)
    for x in (-1.0, 1.0):
        for y in (-1.0, 1.0):
            sheet.fill(gold, [moved(star(4, 2.0, 0.75), (x * (w - 7.45), y * (h - 7.45)))])
    # The sun, and the eight studs.
    sheet.line(gold, disc((0.0, 0.0), 15.4, 96), 0.45)
    sheet.line(gold, disc((0.0, 0.0), 10.2, 96), 0.3)
    sheet.fill(gold, [star(8, 9.6, 4.2), star(8, 6.6, 4.0, 22.5)])
    sheet.fill(gold, [moved(disc((0.0, 0.0), 0.95, 20), turned((0.0, 12.8), 22.5 + 45.0 * k)) for k in range(8)])
    sheet.fill(sheet.ink(NIGHT), [disc((0.0, 0.0), 2.9, 40)])
    sheet.fill(sheet.ink(BRONZE), [disc((0.0, 0.0), 1.7, 32)])
    return sheet.pixels()


EMPIRES["back"] = empires_back


def name(suit, rank):
    return f"{suit}_{rank}_albedo.png"


def run():
    bpy.ops.wm.read_factory_settings(use_empty=True)
    letters = cardkit.lettering(RANKS)
    for card_set in SETS:
        rows = []
        for suit, ink, picture, *second in card_set["suits"]:
            colours = {"suit": ink, "second": (second or [card_set["second"]])[0], "paper": card_set["paper"]}
            rows.append([])
            for rank in RANKS:
                pixels = cardkit.face(
                    rank,
                    picture,
                    colours,
                    letters,
                    index_suit=card_set["index_suit"],
                    gold=card_set["second"],
                    panel=card_set["panel"],
                    trim=card_set["trim"],
                )
                common.write_png_flat(common.output("textures", "cards", "sets", card_set["id"], name(suit, rank)), pixels)
                rows[-1].append(pixels)
        back = card_set["back"]()
        common.write_png_flat(common.output("textures", "cards", "sets", card_set["id"], "back_albedo.png"), back)
        # To look at: the whole set on one sheet, a suit to a row; and its aces and its back,
        # larger.
        cardkit.write_sheet(f"card-set-{card_set['id']}.png", rows, shrink=4, gap=6)
        aces = [row[0] for row in rows]
        cardkit.write_sheet(f"card-set-{card_set['id']}-aces.png", [aces[:4] + [back], aces[4:]])
