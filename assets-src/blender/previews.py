"""Pictures of the assets, rendered in Cycles, to look at before (and beside) the client's own
screenshots: stones on a corner of a board with both stained edges, on the table, in the room's
light. The materials are read from the appearance files, so the picture shows what they say.

They go to `target/asset-previews/`; nothing here is shipped."""

import math
import os
import random
import tomllib

import bpy

import common
import stones

# Each wood's size (how much of a metre its texture covers) and oil finish (sRGB), as in
# `src/look.rs`, and the wood a material naming `board` is drawn in here.
WOODS = {
    "teak": (1.0, (1.0, 0.93, 0.84)),
    "walnut": (1.0, (0.58, 0.44, 0.35)),
    "oak": (1.83, (0.97, 0.88, 0.74)),
    "cherry": (1.0, (0.78, 0.55, 0.46)),
    "maple": (1.0, (1.0, 1.0, 1.0)),
    "felt": (0.285, (1.0, 1.0, 1.0)),
}
BOARD_WOOD = "teak"
# The board's own finish and the table, as in `src/look.rs`.
BOARD = {"wood": "board", "clearcoat": 0.5, "clearcoat_roughness": 0.18}
TABLE = {"wood": "felt", "tint": [0.35, 0.42, 0.40], "reflectance": 0.3}


def appearance(name):
    with open(os.path.join(common.ASSETS, "appearance", f"{name}.toml"), "rb") as f:
        return tomllib.load(f)


def srgb_to_linear(c):
    return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4


def image(path, colour):
    img = bpy.data.images.load(os.path.join(common.ASSETS, path), check_existing=True)
    img.colorspace_settings.name = "sRGB" if colour else "Non-Color"
    return img


def material(name, spec, object_mapped):
    """A Principled BSDF from an appearance file's material."""
    spec = dict(spec)
    if spec.get("wood") == "board":
        spec["wood"] = BOARD_WOOD
    tint = list(spec.get("tint", [1.0, 1.0, 1.0]))
    if "wood" in spec:
        tint = [t * f for t, f in zip(tint, WOODS[spec["wood"]][1])]
        spec["tint"] = tint
    mat = bpy.data.materials.new(name)
    mat.use_nodes = True
    nodes, links = mat.node_tree.nodes, mat.node_tree.links
    bsdf = nodes["Principled BSDF"]
    bsdf.inputs["Roughness"].default_value = spec.get("roughness", 0.5)
    # Bevy's reflectance r gives F0 = 0.16 r²; Blender's level l gives F0 = 0.08 l.
    bsdf.inputs["Specular IOR Level"].default_value = 2.0 * spec.get("reflectance", 0.5) ** 2
    bsdf.inputs["Coat Weight"].default_value = spec.get("clearcoat", 0.0)
    bsdf.inputs["Coat Roughness"].default_value = spec.get("clearcoat_roughness", 0.03)

    def texture(path, colour):
        node = nodes.new("ShaderNodeTexImage")
        node.image = image(path, colour)
        if object_mapped:
            # Woods: mapped by position, in metres, from every side, as the client maps them.
            coords = nodes.new("ShaderNodeTexCoord")
            mapping = nodes.new("ShaderNodeMapping")
            metres = WOODS[spec["wood"]][0]
            mapping.inputs["Scale"].default_value = (1 / metres, 1 / metres, 1 / metres)
            links.new(coords.outputs["Object"], mapping.inputs["Vector"])
            links.new(mapping.outputs["Vector"], node.inputs["Vector"])
            node.projection = "BOX"
            node.projection_blend = 0.15
        return node

    if "wood" in spec:
        w = spec["wood"]
        albedo = texture(f"textures/wood/{w}_albedo.jpg", True)
        arm = texture(f"textures/wood/{w}_arm.jpg", False)
        split = nodes.new("ShaderNodeSeparateColor")
        links.new(arm.outputs["Color"], split.inputs["Color"])
        links.new(split.outputs["Green"], bsdf.inputs["Roughness"])
        normal = texture(f"textures/wood/{w}_normal.jpg", False)
        normal_map = nodes.new("ShaderNodeNormalMap")
        links.new(normal.outputs["Color"], normal_map.inputs["Color"])
        links.new(normal_map.outputs["Normal"], bsdf.inputs["Normal"])
    else:
        albedo = texture(spec["albedo"], True)
    colour = albedo.outputs["Color"]
    if "tint" in spec:
        mix = nodes.new("ShaderNodeMix")
        mix.data_type = "RGBA"
        mix.blend_type = "MULTIPLY"
        mix.inputs["Factor"].default_value = 1.0
        links.new(colour, mix.inputs["A"])
        mix.inputs["B"].default_value = [srgb_to_linear(c) for c in spec["tint"]] + [1.0]
        colour = mix.outputs["Result"]
    links.new(colour, bsdf.inputs["Base Color"])
    return mat


def box(name, size, at, mat):
    bpy.ops.mesh.primitive_cube_add(size=1.0, location=at)
    obj = bpy.context.object
    obj.name = name
    obj.scale = size
    bpy.ops.object.transform_apply(scale=True)
    obj.data.materials.append(mat)
    return obj


def scene():
    bpy.ops.wm.read_factory_settings(use_empty=True)
    look = appearance("hex")
    mats = {k: material(k, v, "wood" in v) for k, v in look["materials"].items()}

    world = bpy.data.worlds.new("Room")
    world.use_nodes = True
    env = world.node_tree.nodes.new("ShaderNodeTexEnvironment")
    env.image = bpy.data.images.load(common.source("lythwood_room_2k.hdr"))
    world.node_tree.links.new(env.outputs["Color"], world.node_tree.nodes["Background"].inputs[0])
    bpy.context.scene.world = world

    height = 0.016
    box("Table", (1.2, 1.2, 0.02), (0, 0, -0.01), material("table", TABLE, True))
    box("Board", (0.20, 0.16, height), (0, 0, height / 2), material("board", BOARD, True))
    seats = look["seats"]
    box("Far edge", (0.20, 0.008, height + 0.001), (0, 0.084, height / 2), mats[seats["red"]["goal"]])
    box("Right edge", (0.008, 0.16, height + 0.001), (0.104, 0, height / 2), mats[seats["blue"]["goal"]])

    rng = random.Random(3)
    pitch = 0.028
    spots = [(-1, 1), (0, 1), (1, 0), (-2, 0), (0, 0), (1, 1), (-1, -1), (2, -1), (0, -1), (1, -2)]
    for i, (gx, gy) in enumerate(spots):
        obj = stones.make_stone_object(f"Stone {i}")
        kind = seats["red"]["pieces"] if i % 2 == 0 else seats["blue"]["pieces"]
        obj.data.materials.append(mats[kind])
        obj.location = (gx * pitch + 0.5 * pitch * (gy % 2), gy * pitch * 0.866, height + stones.HALF_THICKNESS)
        obj.rotation_euler = (0, 0, rng.uniform(0, 2 * math.pi))

    cam = bpy.data.objects.new("Camera", bpy.data.cameras.new("Camera"))
    cam.data.lens = 60
    cam.data.clip_start = 0.005
    bpy.context.scene.collection.objects.link(cam)
    cam.location = (0.03, -0.24, 0.17)
    target = bpy.data.objects.new("Target", None)
    bpy.context.scene.collection.objects.link(target)
    target.location = (0.0, 0.0, height)
    track = cam.constraints.new("TRACK_TO")
    track.target = target
    bpy.context.scene.camera = cam


def render(path, samples=128):
    s = bpy.context.scene
    s.render.engine = "CYCLES"
    prefs = bpy.context.preferences.addons["cycles"].preferences
    for kind in ("OPTIX", "CUDA", "HIP", "ONEAPI"):
        try:
            prefs.compute_device_type = kind
            prefs.get_devices()
            if any(d.type == kind for d in prefs.devices):
                for d in prefs.devices:
                    d.use = d.type == kind
                s.cycles.device = "GPU"
                break
        except TypeError:
            continue
    s.cycles.samples = samples
    s.cycles.use_denoising = True
    s.render.resolution_x, s.render.resolution_y = 1280, 800
    s.render.filepath = path
    bpy.ops.render.render(write_still=True)
    print(f"rendered {os.path.relpath(path, common.REPO)} on {s.cycles.device}")


def run():
    os.makedirs(common.PREVIEWS, exist_ok=True)
    scene()
    render(os.path.join(common.PREVIEWS, "hex.png"))
    # Close, to see the stones' surfaces.
    cam = bpy.context.scene.camera
    cam.location = (0.0, -0.07, 0.07)
    cam.data.lens = 85
    bpy.data.objects["Target"].location = (0.0, -0.005, 0.02)
    render(os.path.join(common.PREVIEWS, "hex-close-slate.png"))
    cam.location = (0.03, -0.035, 0.07)
    bpy.data.objects["Target"].location = (0.028, 0.02, 0.02)
    render(os.path.join(common.PREVIEWS, "hex-close-shell.png"))
