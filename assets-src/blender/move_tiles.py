"""A move tile: a flat tile of wood that shows, as squares stamped on a grid cut into it, where a
piece may go (a game played with move cards: the client draws each one from what the server says
of it), and the ink stamp those squares are printed with.

- `models/move_tile.glb`: one tile, 64 mm wide and 70 mm deep and 4.5 mm thick, its corners
  rounded in plan and its edges eased. Centred on its middle. One part, with no material: the
  appearance file says what the tiles are made of. The client cuts the grid into its top, and
  stamps the squares, the piece's square, a seal and the card's name on it.
- `textures/stamp/square_albedo.png`: one square stamped in ink on wood, 128 × 128: white where
  the ink lies (the client tints it), its alpha the ink's cover. The edge is ragged as a rubber
  stamp's is, the ink is thinner where it is thin, and the wood's grain shows through in specks.
  The client turns it a quarter turn or more from stamp to stamp, so no two look alike."""

import math

import bmesh
import bpy
import numpy as np
from mathutils import Vector

import common
import meshkit

MM = 0.001
WIDTH = 64 * MM
DEPTH = 70 * MM
THICKNESS = 4.5 * MM
# The corners' radius in plan, and how far the top and bottom edges are eased.
CORNER = 4.0 * MM
CORNER_SEGMENTS = 8
EASE = 0.7 * MM

STAMP_SIZE = 128
# The square's side against the image's: the rest is room for the ragged edge.
STAMP_SQUARE = 0.84
STAMP_SEED = 20261124


def outline():
    """The tile's outline in plan, counter-clockwise: a rectangle with its corners rounded."""
    hx, hy = WIDTH / 2 - CORNER, DEPTH / 2 - CORNER
    points = []
    for cx, cy, start in ((hx, hy, 0.0), (-hx, hy, 90.0), (-hx, -hy, 180.0), (hx, -hy, 270.0)):
        for k in range(CORNER_SEGMENTS + 1):
            a = math.radians(start + 90.0 * k / CORNER_SEGMENTS)
            points.append(Vector((cx + CORNER * math.cos(a), cy + CORNER * math.sin(a), 0.0)))
    return points


def tile():
    bm = bmesh.new()
    ring = outline()
    bottom = [bm.verts.new(v) for v in ring]
    top = [bm.verts.new(v + Vector((0, 0, THICKNESS))) for v in ring]
    caps = [bm.faces.new(list(reversed(bottom))), bm.faces.new(top)]
    n = len(ring)
    for i in range(n):
        j = (i + 1) % n
        bm.faces.new([bottom[i], bottom[j], top[j], top[i]])
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    # Only the edges round the top and the bottom are eased: those between the sides' segments
    # meet at so small an angle that easing them would only add faces.
    rims = [e for e in bm.edges if any(f in caps for f in e.link_faces)]
    bmesh.ops.bevel(bm, geom=rims, offset=EASE, segments=3, profile=0.5, affect="EDGES")
    obj = meshkit.from_bmesh(bm, "Move tile", "piece", sharp=50.0)
    obj = meshkit.triangulate(obj)
    meshkit.finish(obj, "piece", sharp=40.0)
    meshkit.flat_faces_stay_flat(obj)
    meshkit.centre_height(obj)
    return meshkit.box_uv(obj, 1.0, detail=True, offset=(0.23, 0.57, 0.0))


def smooth_noise(rng, size, cells):
    """Noise that varies smoothly over the image: random values on a coarse grid of `cells`,
    blended between them."""
    grid = rng.random((cells + 1, cells + 1))
    t = np.linspace(0.0, cells, size, endpoint=False)
    i = t.astype(int)
    f = t - i
    f = f * f * (3.0 - 2.0 * f)
    rows = grid[i][:, i] * (1 - f)[None, :] + grid[i][:, i + 1] * f[None, :]
    below = grid[i + 1][:, i] * (1 - f)[None, :] + grid[i + 1][:, i + 1] * f[None, :]
    return rows * (1 - f)[:, None] + below * f[:, None]


def stamp():
    """The square stamp: its cover (alpha) in 0..1, and the ink's density in its color."""
    rng = np.random.default_rng(STAMP_SEED)
    n = STAMP_SIZE
    # Distance in from the square's edge, in pixels (positive inside).
    c = (np.arange(n) + 0.5) - n / 2
    half = n * STAMP_SQUARE / 2
    inside = half - np.maximum(np.abs(c)[None, :], np.abs(c)[:, None])
    # The edge wanders, in long and short waves, as a rubber stamp's does.
    wander = (smooth_noise(rng, n, 6) - 0.5) * 3.2 + (smooth_noise(rng, n, 24) - 0.5) * 1.6
    edge = np.clip((inside + wander) / 1.4 + 0.5, 0.0, 1.0)
    # Thinner where less ink reached the wood, and the grain showing through in specks along it.
    thin = smooth_noise(rng, n, 5)
    grain = smooth_noise(rng, n, 64)
    streaks = np.repeat(smooth_noise(rng, n, 40)[:1, :], n, axis=0)
    specks = (grain * 0.6 + streaks * 0.4) < (0.16 + 0.22 * (1.0 - thin))
    cover = edge * np.where(specks, 0.25, 1.0) * (0.82 + 0.18 * thin)
    density = 0.86 + 0.14 * smooth_noise(rng, n, 9)
    pixels = np.ones((n, n, 4))
    pixels[..., :3] = density[..., None]
    pixels[..., 3] = cover
    return pixels


def run():
    bpy.ops.wm.read_factory_settings(use_empty=True)
    meshkit.export_glb(tile(), common.output("models", "move_tile.glb"))
    common.write_png(common.output("textures", "stamp", "square_albedo.png"), stamp())
