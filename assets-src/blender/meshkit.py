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


def spline(points, steps=6):
    """A smooth line through points, of any dimension (a centripetal Catmull-Rom spline, which
    never overshoots into a loop): `steps` points for each stretch between two of them."""
    pts = [Vector(p) for p in points]
    if len(pts) < 3:
        return [tuple(p) for p in pts]
    ext = [pts[0] * 2 - pts[1]] + pts + [pts[-1] * 2 - pts[-2]]
    out = []
    for i in range(1, len(ext) - 2):
        p0, p1, p2, p3 = ext[i - 1 : i + 3]
        t0 = 0.0
        t1 = t0 + max((p1 - p0).length, 1e-9) ** 0.5
        t2 = t1 + max((p2 - p1).length, 1e-9) ** 0.5
        t3 = t2 + max((p3 - p2).length, 1e-9) ** 0.5
        for s in range(steps):
            t = t1 + (t2 - t1) * s / steps
            a1 = p0 * ((t1 - t) / (t1 - t0)) + p1 * ((t - t0) / (t1 - t0))
            a2 = p1 * ((t2 - t) / (t2 - t1)) + p2 * ((t - t1) / (t2 - t1))
            a3 = p2 * ((t3 - t) / (t3 - t2)) + p3 * ((t - t2) / (t3 - t2))
            b1 = a1 * ((t2 - t) / (t2 - t0)) + a2 * ((t - t0) / (t2 - t0))
            b2 = a2 * ((t3 - t) / (t3 - t1)) + a3 * ((t - t1) / (t3 - t1))
            out.append(tuple(b1 * ((t2 - t) / (t2 - t1)) + b2 * ((t - t1) / (t2 - t1))))
    out.append(tuple(pts[-1]))
    return out


class Arc(list):
    """Part of a circle as a run of a `profile`: its points are used as they are."""


def arc(centre, radius, start, end, count):
    """Points round (radius, z) `centre` from angle `start` to `end`, in degrees from the
    outward direction, anticlockwise (90 is straight up)."""
    out = Arc()
    for i in range(count + 1):
        a = math.radians(start + (end - start) * i / count)
        out.append((centre[0] + radius * math.cos(a), centre[1] + radius * math.sin(a)))
    return out


def profile(runs, scale=1.0, steps=3):
    """A profile to turn (`lathe`), from runs of (radius, z) points: each run is a smooth curve
    through its points (a straight line through two), and two runs meet at a corner. A run starts
    where the one before ended, so that point is given once, by the run before. A run may begin
    with a number: how many points to draw between each two of its own, in place of `steps`."""
    out = []
    for run in runs:
        scaled = [(r * scale, z * scale) for r, z in (p for p in run if not isinstance(p, int))]
        if isinstance(run, Arc):
            out.extend(scaled)
            continue
        fine = run[0] if isinstance(run[0], int) else steps
        curve = spline(([out[-1]] if out else []) + scaled, fine)
        out.extend(curve[1:] if out else curve)
    # Nothing left of the axis, whatever a curve did on its way to it.
    return [(max(r, 0.0), z) for r, z in out]


def turned(name, material, outline, segments, angles=None):
    """A profile turned into a closed object standing on z = 0, its faces looking outwards.
    `angles`: where round the axis its vertices go, in place of `segments` even steps."""
    verts, faces, uvs = lathe(outline, segments) if angles is None else lathe_at(outline, angles)
    obj = Shape().add(verts, faces, uvs).build(name, material, sharp=None)
    apply_modifier(obj, "WELD", merge_threshold=0.000001)
    return outward(obj)


def lathe_at(outline, angles):
    """`lathe`, with a vertex at each of `angles` (radians, increasing, less than a full turn)."""
    verts, faces = [], []
    n = len(angles)
    for r, z in outline:
        for a in angles:
            verts.append((r * math.cos(a), r * math.sin(a), z))
    for i in range(len(outline) - 1):
        for s in range(n):
            t = (s + 1) % n
            faces.append((i * n + s, i * n + t, (i + 1) * n + t, (i + 1) * n + s))
    return verts, faces, None


def outward(obj):
    """Makes every face of a closed mesh look outwards: the client draws a face from one side."""
    bm = bmesh.new()
    bm.from_mesh(obj.data)
    bmesh.ops.dissolve_degenerate(bm, dist=1e-7, edges=bm.edges)
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    bm.to_mesh(obj.data)
    bm.free()
    obj.data.update()
    return obj


def triangulate(obj):
    """Every face a triangle, so that its tangents can be worked out when it is exported."""
    bm = bmesh.new()
    bm.from_mesh(obj.data)
    bmesh.ops.triangulate(bm, faces=bm.faces)
    bm.to_mesh(obj.data)
    bm.free()
    obj.data.update()
    return obj


def height(obj):
    """How far above z = 0 an object reaches."""
    return max(v.co.z for v in obj.data.vertices)


def centre_height(obj):
    """Moves a mesh standing on z = 0 down by half its height, as the client expects a piece: its
    middle at the origin. Returns the height."""
    h = height(obj)
    for v in obj.data.vertices:
        v.co.z -= h / 2.0
    obj.data.update()
    return h


def boolean(obj, cutter, operation="DIFFERENCE", keep_materials=False):
    """Cuts `cutter` out of `obj` (or joins or intersects them), and takes the cutter away. With
    `keep_materials`, the faces the cutter leaves keep its material: a slot of their own."""
    mod = obj.modifiers.new("Boolean", "BOOLEAN")
    mod.operation = operation
    mod.object = cutter
    mod.solver = "EXACT"
    if keep_materials:
        mod.material_mode = "TRANSFER"
    bpy.context.view_layer.objects.active = obj
    bpy.ops.object.modifier_apply(modifier=mod.name)
    bpy.data.objects.remove(cutter, do_unlink=True)
    return settle(obj)


def settle(obj, grain=1e-7):
    """Puts a mesh's vertices and faces in an order that depends only on where they are (to a
    tenth of a micrometre). A boolean cut leaves them in whatever order its threads finished,
    and the file made from them would differ from run to run."""
    mesh = obj.data
    keys = [tuple(round(c / grain) for c in v.co) for v in mesh.vertices]
    order = sorted(range(len(keys)), key=keys.__getitem__)
    index = {old: new for new, old in enumerate(order)}
    faces = []
    for poly in mesh.polygons:
        corners = [(keys[v], index[v]) for v in poly.vertices]
        first = corners.index(min(corners))
        corners = corners[first:] + corners[:first]
        faces.append(([k for k, _ in corners], [i for _, i in corners], poly.material_index))
    faces.sort()
    settled = bpy.data.meshes.new(mesh.name)
    settled.from_pydata([tuple(k * grain for k in keys[old]) for old in order], [], [f[1] for f in faces])
    for material in mesh.materials:
        settled.materials.append(material)
    for poly, (_, _, part) in zip(settled.polygons, faces):
        poly.material_index = part
    settled.update()
    obj.data = settled
    bpy.data.meshes.remove(mesh)
    return obj


def export_glb(objs, path, materials=False):
    """Writes objects as one glTF file, y up. Without `materials` it has none, and its first
    mesh is the whole thing; with them each material slot is a part of the mesh, named, in the
    file (the file holds no look: the client gives each name one)."""
    import os

    objs = objs if isinstance(objs, (list, tuple)) else [objs]
    bpy.ops.object.select_all(action="DESELECT")
    for obj in objs:
        obj.select_set(True)
    bpy.context.view_layer.objects.active = objs[0]
    bpy.ops.export_scene.gltf(
        filepath=path,
        export_format="GLB",
        use_selection=True,
        export_tangents=True,
        export_materials="EXPORT" if materials else "NONE",
        export_image_format="NONE",
        export_extras=False,
        export_yup=True,
    )
    size = os.path.getsize(path)
    print(f"wrote {path.split('/assets/')[-1]} ({size / 1024:.0f} KiB, {triangles(objs)} triangles)")


def flat_faces_stay_flat(obj):
    """Weights each vertex's normal by its faces' areas, so a large flat face beside a rounded
    edge is lit as flat, and only the rounding shades."""
    return apply_modifier(obj, "WEIGHTED_NORMAL", keep_sharp=True, weight=100)


def check_parts(path, parts):
    """Checks that a glTF file's first mesh has these parts, by their materials' names: the
    client finds a part by its name."""
    import json
    import struct

    with open(path, "rb") as f:
        data = f.read()
    length = struct.unpack_from("<I", data, 12)[0]
    doc = json.loads(data[20 : 20 + length])
    names = [doc["materials"][p["material"]]["name"] for p in doc["meshes"][0]["primitives"]]
    if names != list(parts):
        raise SystemExit(f"{path}: its parts are {names}, not {list(parts)}")
