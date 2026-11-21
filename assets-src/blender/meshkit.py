"""Meshes made in code: what the steps that model things share (`garden.py`, `tableware.py`).

Everything is in metres, in Blender's axes (z up; the glTF exporter turns them to the client's,
y up, with Blender's +y becoming the far side of the table). A `Shape` gathers vertices, faces and
each face corner's texture coordinates, and becomes a Blender object with one material slot, named
for the material the client gives it (an environment's file says what each name is made of)."""

import math
import random

import bmesh
import bpy
from mathutils import Matrix, Vector

# How much finer a wood's relief is than its colour, as `look::DETAIL` in the client.
DETAIL = 3.17


class Shape:
    def __init__(self):
        self.verts = []
        self.faces = []
        # One list of (u, v) per face, a pair per corner; None where a face has none yet.
        self.uvs = []

    def add(self, verts, faces, uvs=None, matrix=None):
        base = len(self.verts)
        if matrix is not None:
            verts = [tuple(matrix @ Vector(v)) for v in verts]
        self.verts.extend(tuple(v) for v in verts)
        self.faces.extend(tuple(base + i for i in f) for f in faces)
        self.uvs.extend(uvs if uvs is not None else [None] * len(faces))
        return self

    def merge(self, other, matrix=None):
        return self.add(other.verts, other.faces, other.uvs, matrix)

    def build(self, name, material, smooth=True, sharp=40.0, collection=None):
        mesh = bpy.data.meshes.new(name)
        mesh.from_pydata(self.verts, [], self.faces)
        mesh.update()
        uv = mesh.uv_layers.new(name="UVMap")
        for poly, face_uvs in zip(mesh.polygons, self.uvs):
            if face_uvs is None:
                continue
            for loop, st in zip(poly.loop_indices, face_uvs):
                uv.data[loop].uv = st
        obj = bpy.data.objects.new(name, mesh)
        (collection or bpy.context.scene.collection).objects.link(obj)
        finish(obj, material, smooth, sharp)
        return obj


def finish(obj, material, smooth=True, sharp=40.0):
    """Shading and the material slot of an object made here."""
    mesh = obj.data
    for poly in mesh.polygons:
        poly.use_smooth = smooth
    if smooth and sharp is not None:
        mesh.set_sharp_from_angle(angle=math.radians(sharp))
    mesh.materials.clear()
    mesh.materials.append(slot(material))
    return obj


def slot(name):
    """The material of this name: a plain one here; `matlib` gives it its look for Cycles."""
    return bpy.data.materials.get(name) or bpy.data.materials.new(name)


def box_uv(obj, metres=1.0, detail=False, offset=(0.0, 0.0, 0.0)):
    """Texture coordinates in metres from the side each face looks to, as the client's
    `look::planar_uv` maps a wood (there in its own axes: Blender's y is its -z). With `detail`, a
    second set `DETAIL` times finer, for a wood's relief."""
    mesh = obj.data
    uv = mesh.uv_layers.get("UVMap") or mesh.uv_layers.new(name="UVMap")
    fine = None
    if detail:
        fine = mesh.uv_layers.get("Detail") or mesh.uv_layers.new(name="Detail")
    ox, oy, oz = offset
    for poly in mesh.polygons:
        n = poly.normal
        a = (abs(n.x), abs(n.y), abs(n.z))
        for loop in poly.loop_indices:
            p = mesh.vertices[mesh.loops[loop].vertex_index].co
            x, y, z = p.x + ox, p.y + oy, p.z + oz
            if a[2] >= a[0] and a[2] >= a[1]:
                st = (x, y)
            elif a[0] >= a[1]:
                st = (y, z)
            else:
                st = (x, z)
            st = (st[0] / metres, st[1] / metres)
            uv.data[loop].uv = st
            if fine is not None:
                fine.data[loop].uv = (st[0] * DETAIL, st[1] * DETAIL)
    return obj


def from_bmesh(bm, name, material, smooth=True, sharp=40.0):
    mesh = bpy.data.meshes.new(name)
    bm.to_mesh(mesh)
    bm.free()
    obj = bpy.data.objects.new(name, mesh)
    bpy.context.scene.collection.objects.link(obj)
    return finish(obj, material, smooth, sharp)


def rounded_box(name, size, at, material, radius=0.004, segments=2, metres=1.0, detail=False):
    """A box with its edges rounded, its middle at `at`, mapped from every side."""
    bm = bmesh.new()
    bmesh.ops.create_cube(bm, size=1.0)
    for v in bm.verts:
        v.co = Vector((v.co.x * size[0], v.co.y * size[1], v.co.z * size[2]))
    if radius > 0.0:
        bmesh.ops.bevel(
            bm,
            geom=list(bm.edges),
            offset=min(radius, min(size) * 0.45),
            segments=segments,
            profile=0.5,
            affect="EDGES",
        )
    for v in bm.verts:
        v.co += Vector(at)
    obj = from_bmesh(bm, name, material, sharp=50.0)
    return box_uv(obj, metres, detail)


def lathe(profile, segments, v_scale=1.0, u_repeats=1.0, cap_start=False, cap_end=False):
    """A profile of (radius, z) turned about z. Returns verts, faces and uvs for a `Shape`: u goes
    round, v along the profile's length (in metres, over `v_scale`)."""
    verts, faces, uvs = [], [], []
    along = [0.0]
    for (r0, z0), (r1, z1) in zip(profile, profile[1:]):
        along.append(along[-1] + math.hypot(r1 - r0, z1 - z0))
    for r, z in profile:
        for s in range(segments):
            a = 2.0 * math.pi * s / segments
            verts.append((r * math.cos(a), r * math.sin(a), z))
    for i in range(len(profile) - 1):
        for s in range(segments):
            n = (s + 1) % segments
            a, b = i * segments + s, i * segments + n
            c, d = (i + 1) * segments + n, (i + 1) * segments + s
            faces.append((a, b, c, d))
            u0, u1 = s / segments * u_repeats, (s + 1) / segments * u_repeats
            v0, v1 = along[i] / v_scale, along[i + 1] / v_scale
            uvs.append([(u0, v0), (u1, v0), (u1, v1), (u0, v1)])
    for cap, ring, flip in ((cap_start, 0, True), (cap_end, len(profile) - 1, False)):
        if not cap:
            continue
        loop = [ring * segments + s for s in range(segments)]
        r = max(profile[ring][0], 1e-6)
        st = [
            (0.5 + 0.5 * verts[i][0] / r / v_scale * r, 0.5 + 0.5 * verts[i][1] / r / v_scale * r)
            for i in loop
        ]
        if flip:
            loop, st = loop[::-1], st[::-1]
        faces.append(tuple(loop))
        uvs.append(st)
    return verts, faces, uvs


def tube(points, radii, sides, v_scale=0.5, u_repeats=1.0, cap_end=True):
    """A tube along a line of points, with a radius at each: a branch, a cane, a pipe. u goes
    round, v along it."""
    verts, faces, uvs = [], [], []
    pts = [Vector(p) for p in points]
    # A frame carried along the line, so the tube does not twist.
    tangent = (pts[1] - pts[0]).normalized()
    side = tangent.orthogonal().normalized()
    along = 0.0
    lengths = []
    for i, p in enumerate(pts):
        if 0 < i:
            along += (p - pts[i - 1]).length
        lengths.append(along)
        if 0 < i < len(pts) - 1:
            t = (pts[i + 1] - pts[i - 1]).normalized()
        elif i == 0:
            t = tangent
        else:
            t = (p - pts[i - 1]).normalized()
        side = (side - t * side.dot(t)).normalized()
        up = t.cross(side)
        for s in range(sides):
            a = 2.0 * math.pi * s / sides
            verts.append(tuple(p + (side * math.cos(a) + up * math.sin(a)) * radii[i]))
    for i in range(len(pts) - 1):
        for s in range(sides):
            n = (s + 1) % sides
            faces.append((i * sides + s, i * sides + n, (i + 1) * sides + n, (i + 1) * sides + s))
            u0, u1 = s / sides * u_repeats, (s + 1) / sides * u_repeats
            v0, v1 = lengths[i] / v_scale, lengths[i + 1] / v_scale
            uvs.append([(u0, v0), (u1, v0), (u1, v1), (u0, v1)])
    if cap_end:
        # A fan about the end's middle (triangles, so its tangents can be worked out).
        last = (len(pts) - 1) * sides
        verts.append(tuple(pts[-1]))
        for s in range(sides):
            faces.append((last + s, last + (s + 1) % sides, len(verts) - 1))
            uvs.append([(0.4, 0.4), (0.6, 0.4), (0.5, 0.6)])
    return verts, faces, uvs


def quad(centre, right, up, size, uv=((0, 0), (1, 0), (1, 1), (0, 1))):
    """A card: a flat rectangle about `centre`, `size` (width, height) along `right` and `up`."""
    c, r, u = Vector(centre), Vector(right).normalized(), Vector(up).normalized()
    w, h = size[0] / 2.0, size[1] / 2.0
    verts = [tuple(c - r * w - u * h), tuple(c + r * w - u * h), tuple(c + r * w + u * h), tuple(c - r * w + u * h)]
    return verts, [(0, 1, 2, 3)], [list(uv)]


def blob_outline(rng, radius, wobble, count=40, lobes=(2, 3, 5)):
    """A closed outline about the origin: a circle whose radius wanders, as a pond's edge or a
    patch of moss does. Returns (x, y) points, anticlockwise."""
    phases = [(k, rng.uniform(0, 2 * math.pi), rng.uniform(0.4, 1.0) / k) for k in lobes]
    total = sum(w for _, _, w in phases)
    out = []
    for i in range(count):
        a = 2.0 * math.pi * i / count
        r = radius * (1.0 + wobble * sum(w * math.sin(k * a + p) for k, p, w in phases) / total)
        out.append((r * math.cos(a), r * math.sin(a)))
    return out


def inside(outline, x, y):
    """Whether a point is inside a closed outline."""
    hit = False
    n = len(outline)
    for i in range(n):
        (x0, y0), (x1, y1) = outline[i], outline[(i + 1) % n]
        if (y0 > y) != (y1 > y) and x < x0 + (y - y0) / (y1 - y0) * (x1 - x0):
            hit = not hit
    return hit


def mound(outline, height, rings=5, metres=1.0, rng=None, roughness=0.0, skirt=0.0):
    """A low mound filling an outline: highest in the middle, falling to nothing at its edge (and
    `skirt` below it, so it sinks into the ground). Mapped from above, in metres."""
    verts, faces, uvs = [(0.0, 0.0, height)], [], []
    n = len(outline)
    for ring in range(1, rings + 1):
        t = ring / rings
        for x, y in outline:
            # A dome that meets the ground at a slope, not a lip.
            z = height * (1.0 - t * t) ** 1.5 - (skirt if ring == rings else 0.0)
            if rng is not None and ring < rings:
                z += rng.uniform(-roughness, roughness) * height
            verts.append((x * t, y * t, z))
    st = lambda i: (verts[i][0] / metres, verts[i][1] / metres)
    for i in range(n):
        j = (i + 1) % n
        faces.append((0, 1 + i, 1 + j))
        uvs.append([st(0), st(1 + i), st(1 + j)])
    for ring in range(rings - 1):
        a0 = 1 + ring * n
        b0 = a0 + n
        for i in range(n):
            j = (i + 1) % n
            f = (a0 + i, b0 + i, b0 + j, a0 + j)
            faces.append(f)
            uvs.append([st(k) for k in f])
    return verts, faces, uvs


def place(obj, at, yaw=0.0, scale=1.0):
    obj.location = Vector(at)
    obj.rotation_euler = (0.0, 0.0, yaw)
    obj.scale = (scale, scale, scale) if not isinstance(scale, (tuple, list)) else scale
    return obj


def instance(obj, name, at, yaw=0.0, scale=1.0, tilt=(0.0, 0.0)):
    """Another of the same thing: it shares the mesh, so the client draws them in one batch."""
    twin = bpy.data.objects.new(name, obj.data)
    bpy.context.scene.collection.objects.link(twin)
    twin.location = Vector(at)
    twin.rotation_euler = (tilt[0], tilt[1], yaw)
    twin.scale = (scale, scale, scale) if not isinstance(scale, (tuple, list)) else scale
    return twin


def apply_modifier(obj, kind, **settings):
    mod = obj.modifiers.new(kind, kind)
    for key, value in settings.items():
        setattr(mod, key, value)
    bpy.context.view_layer.objects.active = obj
    bpy.ops.object.modifier_apply(modifier=mod.name)
    return obj


def join(objs, name):
    """Several objects made one mesh (they must share a material to stay one draw)."""
    bpy.ops.object.select_all(action="DESELECT")
    for o in objs:
        o.select_set(True)
    bpy.context.view_layer.objects.active = objs[0]
    bpy.ops.object.join()
    objs[0].name = name
    objs[0].data.name = name
    return objs[0]


def apply_transform(obj):
    bpy.ops.object.select_all(action="DESELECT")
    obj.select_set(True)
    bpy.context.view_layer.objects.active = obj
    bpy.ops.object.transform_apply(location=True, rotation=True, scale=True)
    return obj


def triangles(objs=None):
    total = 0
    for o in objs or bpy.data.objects:
        if o.type == "MESH":
            total += sum(len(p.vertices) - 2 for p in o.data.polygons)
    return total


def rotation(axis, angle):
    return Matrix.Rotation(angle, 4, axis)


def translation(v):
    return Matrix.Translation(Vector(v))


def rng(seed):
    return random.Random(seed)
