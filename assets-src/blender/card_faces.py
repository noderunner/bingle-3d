"""The faces of the usual deck of playing cards: spades, hearts, diamonds and clubs, each from the
ace to the king, as pictures for the card's face (`playing_cards.py`).

- `textures/cards/faces/<suit>_<rank>_albedo.png`, 52 of them (`spades_a`, `hearts_10`,
  `clubs_k`): 640 × 896, which is 5 to 7, the card's paper all over with the face printed on it
  in one ink, red or black, and on a court card gold and a tinted panel besides.

Each is drawn here, with no picture to start from: a suit is an outline of curves, and
`cardkit.py` letters the rank, lays the face out and fills it.

No game names these files: the client finds one by a suit's symbol and a rank's label, as the
server gives them (`props::standard_face`)."""

import bpy

import cardkit
import common
from cardkit import both_sides, disc, filled, outline

SUITS = ["spades", "hearts", "diamonds", "clubs"]
RANKS = ["a", "2", "3", "4", "5", "6", "7", "8", "9", "10", "j", "q", "k"]

# Colours (sRGB).
BLACK = (0.075, 0.075, 0.085)
RED = (0.74, 0.07, 0.10)
INKS = {"spades": BLACK, "clubs": BLACK, "hearts": RED, "diamonds": RED}
# The ace of spades is the deck's largest, as it always is.
ACE_OF_SPADES = 33.0

HEART = both_sides(
    outline(
        (0.0, -0.93),
        ((0.10, -0.70), (0.98, -0.20), (0.98, 0.38)),
        ((0.98, 0.72), (0.76, 0.95), (0.51, 0.95)),
        ((0.28, 0.95), (0.08, 0.80), (0.0, 0.56)),
    )
)
DIAMOND = both_sides(
    outline(
        (0.0, -0.98),
        ((0.22, -0.60), (0.48, -0.26), (0.76, 0.0)),
        ((0.48, 0.26), (0.22, 0.60), (0.0, 0.98)),
    )
)
STEM = both_sides(
    outline(
        (0.0, -0.98),
        (0.44, -0.98),
        ((0.22, -0.88), (0.09, -0.62), (0.07, -0.16)),
        (0.0, -0.16),
    )
)
SPADE = both_sides(
    outline(
        (0.0, -0.46),
        ((0.08, -0.62), (0.26, -0.72), (0.48, -0.72)),
        ((0.76, -0.72), (0.98, -0.52), (0.98, -0.20)),
        ((0.98, 0.26), (0.12, 0.62), (0.0, 0.98)),
    )
)
CLUB = [disc((0.0, 0.52), 0.42), disc((-0.52, -0.16), 0.42), disc((0.52, -0.16), 0.42), disc((0.0, 0.02), 0.26)]
PICTURES = {
    "hearts": [filled(HEART)],
    "diamonds": [filled(DIAMOND)],
    "spades": [filled(SPADE, STEM)],
    "clubs": [filled(*CLUB, STEM)],
}


def name(suit, rank):
    return f"{suit}_{rank}_albedo.png"


def run():
    bpy.ops.wm.read_factory_settings(use_empty=True)
    letters = cardkit.lettering(RANKS)
    rows = []
    for suit in SUITS:
        colours = {"suit": INKS[suit], "paper": cardkit.PAPER}
        rows.append([])
        for rank in RANKS:
            ace = ACE_OF_SPADES if suit == "spades" else cardkit.ACE
            pixels = cardkit.face(rank, PICTURES[suit], colours, letters, ace=ace)
            common.write_png_flat(common.output("textures", "cards", "faces", name(suit, rank)), pixels)
            rows[-1].append(pixels)
    # The whole deck on one sheet, a suit to a row, to look at.
    cardkit.write_sheet("card-faces.png", rows)
