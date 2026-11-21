"""The garden's light (`environment/garden_{diffuse,specular,sky}.ktx2`), and pictures of it.

The client lights a scene with a sun and with what surrounds the table, as two cubemaps
(`environment.py`). The room's are a photograph; the garden's are a picture of the garden itself:
Cycles renders everything round the table from just above its top, the garden lit by the same sun
and under the same sky as the client's (`assets/environments/garden.toml`), and that panorama is
filtered. So a polished stone reflects the maple and the gravel, and the board is lit by the red
of the leaves and the green of the moss as well as the sky.

The sky is a photograph of one (CC0), turned so that its sun stands where the garden's does. The
sun itself is taken out of it (the brightest of it is clipped): the client's sun is a light of its
own, and would otherwise shine twice.

`previews` here renders the garden from where a player sits, to look at beside the client's own
screenshot; they go to `target/asset-previews/`."""

import math
import os

import bpy
import numpy as np
import OpenImageIO as oiio
from mathutils import Vector

import common
import environment
import garden
import matlib
import previews

SKY = "kloofendal_28d_misty_puresky_2k.hdr"
SCRATCH = os.path.join(common.REPO, "target", "garden")
# The sky no brighter than this many times its own average: what is cut off is the sun.
SUN_CLIP = 5.0
# What polished things reflect, against the light things are lit by. Under an open, misty sky a
# polished board, a bowl or a pond gives back so much white that it is veiled; this is the
# polarising filter a photographer would put on.
SPECULAR_GAIN = 0.45
SKY_SIZE = 512
PANORAMA = (2048, 1024)


def write_exr(path, pixels):
    h, w, c = pixels.shape
    buf = oiio.ImageBuf(oiio.ImageSpec(w, h, c, oiio.FLOAT))
    buf.set_pixels(oiio.ROI(0, w, 0, h, 0, 1, 0, c), np.ascontiguousarray(pixels, np.float32))
    if not buf.write(path):
        raise SystemExit(f"{path}: {buf.geterror()}")


def to_blender(v):
    """A direction in the client's axes (y up, z towards the player) in Blender's (z up)."""
    return Vector((v[0], -v[2], v[1]))


def sky(sun_from):
    """The sky's photograph with its sun where the garden's is (in Blender's own mapping of a
    panorama: the middle column is +x, and +y is a quarter of the way to its left), and the sun
    clipped out."""
    img = common.read_image(common.source(SKY))[..., :3].astype(np.float32)
    h, w = img.shape[:2]
    lum = img @ np.array([0.2126, 0.7152, 0.0722], np.float32)
    # The sun: the brightest patch, found in a blurred copy so one hot pixel does not decide.
    k = 16
    coarse = lum[: h // k * k, : w // k * k].reshape(h // k, k, w // k, k).mean((1, 3))
    row, col = np.unravel_index(coarse.argmax(), coarse.shape)
    found = (col + 0.5) / coarse.shape[1]
    elevation = 90.0 - (row + 0.5) / coarse.shape[0] * 180.0
    d = to_blender(sun_from).normalized()
    wanted = 0.5 - math.atan2(d.y, d.x) / (2.0 * math.pi)
    img = np.roll(img, int(round((wanted - found) * w)), axis=1)
    lum = np.roll(lum, int(round((wanted - found) * w)), axis=1)
    upper = lum[: h // 2]
    cap = SUN_CLIP * float(np.median(upper))
    clipped = img * np.minimum(1.0, cap / np.maximum(lum, 1e-6))[..., None]
    print(
        f"  the sky's sun is {elevation:.0f} degrees up (the garden's: "
        f"{math.degrees(math.asin(d.z)):.0f}); its sky is {float(np.median(upper)):.2f} at the median, "
        f"clipped at {cap:.2f}"
    )
    return img, clipped


def light(env):
    """The world and the sun, as the client's file has them."""
    sun_from = env["sun"]["from"]
    whole, clipped = sky(sun_from)
    os.makedirs(SCRATCH, exist_ok=True)
    path = os.path.join(SCRATCH, "sky.exr")
    write_exr(path, clipped)
    world = bpy.data.worlds.new("Sky")
    world.use_nodes = True
    tex = world.node_tree.nodes.new("ShaderNodeTexEnvironment")
    tex.image = bpy.data.images.load(path)
    world.node_tree.links.new(tex.outputs["Color"], world.node_tree.nodes["Background"].inputs[0])
    bpy.context.scene.world = world

    lamp = bpy.data.lights.new("Sun", "SUN")
    # A value of 1 in the sky is `intensity` cd/m² in the client, and the sun `illuminance` lux.
    lamp.energy = env["sun"]["illuminance"] / env["sky"]["intensity"]
    lamp.color = env["sun"]["colour"]
    lamp.angle = math.radians(2.5)
    obj = bpy.data.objects.new("Sun", lamp)
    bpy.context.scene.collection.objects.link(obj)
    obj.rotation_euler = (-to_blender(sun_from)).to_track_quat("-Z", "Y").to_euler()
    return whole, clipped


def scene():
    seat = garden.build()
    garden.seats(seat)
    env = matlib.environment("garden")
    missing = matlib.build_all(env["materials"])
    if missing:
        print(f"  no material in garden.toml for: {missing}")
    # What casts no shadow in the client casts none here.
    unshadowed = set(env.get("shadows", {}).get("none", []))
    for obj in bpy.data.objects:
        if obj.type == "MESH" and any(m and m.name in unshadowed for m in obj.data.materials):
            obj.visible_shadow = False
    skies = light(env)
    return env, skies


def camera(location, target, fov=45.0):
    cam = bpy.data.objects.get("Camera")
    if cam is None:
        cam = bpy.data.objects.new("Camera", bpy.data.cameras.new("Camera"))
        bpy.context.scene.collection.objects.link(cam)
        bpy.context.scene.camera = cam
    cam.data.type = "PERSP"
    cam.data.sensor_fit = "VERTICAL"
    cam.data.angle_y = math.radians(fov)
    cam.data.clip_start = 0.02
    cam.data.clip_end = 400.0
    cam.location = Vector(location)
    cam.rotation_euler = (Vector(target) - Vector(location)).to_track_quat("-Z", "Y").to_euler()
    return cam


def orbit(yaw, pitch, radius, focus=(0.0, 0.0, 0.0)):
    """Where the client's orbit camera is, in Blender's axes (yaw 0 looks from the player's side)."""
    yaw, pitch = math.radians(yaw), math.radians(pitch)
    return (
        focus[0] + radius * math.cos(pitch) * math.sin(yaw),
        focus[1] - radius * math.cos(pitch) * math.cos(yaw),
        focus[2] + radius * math.sin(pitch),
    )


def run_previews():
    os.makedirs(common.PREVIEWS, exist_ok=True)
    scene()
    s = bpy.context.scene
    s.view_settings.view_transform = "AgX"
    s.view_settings.exposure = -0.6
    views = {
        "garden-seat": (0, 32, 1.5),
        "garden-table": (0, 49, 0.8),
        "garden-east": (-70, 22, 2.2),
        "garden-west": (75, 24, 2.2),
        "garden-south": (180, 20, 2.2),
        "garden-above": (0, 80, 2.2),
    }
    only = os.environ.get("GARDEN_VIEWS")
    for name, (yaw, pitch, radius) in views.items():
        if only and name not in only.split(","):
            continue
        camera(orbit(yaw, pitch, radius), (0, 0, 0))
        previews.render(os.path.join(common.PREVIEWS, f"{name}.png"), samples=64, size=(1280, 720))
    # From high up, the whole of it.
    if not only or "garden-plan" in only:
        camera((0.0, -3.0, 30.0), (0, 0, GROUND_TARGET), fov=45.0)
        previews.render(os.path.join(common.PREVIEWS, "garden-plan.png"), samples=48, size=(1024, 1024))


GROUND_TARGET = garden.GROUND


def run():
    env, (whole, clipped) = scene()
    s = bpy.context.scene
    cam = camera((0.0, 0.0, 0.12), (0.0, 1.0, 0.12))
    cam.data.type = "PANO"
    cam.data.panorama_type = "EQUIRECTANGULAR"
    s.render.image_settings.file_format = "OPEN_EXR"
    s.render.image_settings.color_depth = "32"
    path = os.path.join(SCRATCH, "panorama.exr")
    previews.render(path, samples=128, size=PANORAMA)
    pixels = common.read_image(path)[..., :3]
    print(f"  the panorama's average is {float(pixels.mean()):.3f}")
    # To look at: the panorama as a picture.
    os.makedirs(common.PREVIEWS, exist_ok=True)
    common.write_png(
        os.path.join(common.PREVIEWS, "garden-panorama.png"),
        common.linear_to_srgb(common.resize_to(pixels / (1.0 + pixels), 1024, 512)),
    )
    environment.build_from("garden", pixels, SPECULAR_GAIN)

    # The sky alone, for behind everything: the photograph in the same turn as the panorama (its
    # middle the far side), the sun left in but not at its full strength, and mist below.
    h, w = whole.shape[:2]
    lum = whole @ np.array([0.2126, 0.7152, 0.0722], np.float32)
    cap = 4.0 * SUN_CLIP * float(np.median(lum[: h // 2]))
    backdrop = whole * np.minimum(1.0, cap / np.maximum(lum, 1e-6))[..., None]
    backdrop = np.roll(backdrop, w // 4, axis=1)
    fog = np.array([common.srgb_to_linear(np.array(c)) for c in env["fog"]["colour"]], np.float32)
    horizon = backdrop[h // 2 - 8 : h // 2].mean((0, 1))
    lat = (0.5 - (np.arange(h) + 0.5) / h) * 180.0
    t = np.clip(-lat / 4.0 + 0.5, 0.0, 1.0)[:, None, None]
    backdrop = backdrop * (1.0 - t) + (horizon * 0.5 + fog * float(horizon.mean()) * 0.5)[None, None, :] * t
    pano = environment.Panorama(backdrop)
    level = environment.specular_level(pano, SKY_SIZE, 0.0)
    common.write_ktx2_cube_rgba16f(common.output("environment", "garden_sky.ktx2"), [level])
