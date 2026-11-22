"""A tile: a flat hexagon of wood laid over one cell of a hexagonal board (Courtfall's terrain).

- `models/tile_hex.glb`: one tile, made for a 28 mm cell. A hexagonal board's neighbours are a
  pitch apart across flat sides, so the tile is a little narrower than that across its flats,
  and the board's lines still show round it; its points are towards the far and near sides of
  the table (+y and -y here), as the client's cells' are. Its edges are eased. Centred on its
  middle.

It has no material: the appearance file says what each tile is made of (a stained wood)."""

import math

import bmesh
import bpy
from mathutils import Vector

import common
import meshkit

PITCH = 0.028
# Across its flats, against the pitch: inside the cell (0.955 of it), so the line round it shows.
ACROSS = 0.92 * PITCH
THICKNESS = 0.0024
EASE = 0.0005


def tile():
    bm = bmesh.new()
    radius = ACROSS / 2 / math.cos(math.pi / 6)
    ring = [
        Vector((radius * math.cos(a), radius * math.sin(a), 0.0))
        for a in (math.pi / 2 + k * math.pi / 3 for k in range(6))
    ]
    bottom = [bm.verts.new(v) for v in ring]
    top = [bm.verts.new(v + Vector((0, 0, THICKNESS))) for v in ring]
    bm.faces.new(list(reversed(bottom)))
    bm.faces.new(top)
    for i in range(6):
        j = (i + 1) % 6
        bm.faces.new([bottom[i], bottom[j], top[j], top[i]])
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    bmesh.ops.bevel(bm, geom=list(bm.edges), offset=EASE, segments=2, profile=0.5, affect="EDGES")
    obj = meshkit.from_bmesh(bm, "Tile", "piece", sharp=50.0)
    obj = meshkit.triangulate(obj)
    meshkit.finish(obj, "piece", sharp=40.0)
    meshkit.flat_faces_stay_flat(obj)
    meshkit.centre_height(obj)
    return meshkit.box_uv(obj, 1.0, detail=True, offset=(0.41, 0.13, 0.0))


def run():
    bpy.ops.wm.read_factory_settings(use_empty=True)
    meshkit.export_glb(tile(), common.output("models", "tile_hex.glb"))
