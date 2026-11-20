"""The woods (and the table's felt): scanned CC0 textures, resized to what the client needs and
saved as JPEG.

Each wood is three images, as glTF and Bevy's `StandardMaterial` read them:
- `albedo`: the colour, in sRGB, resized in linear light;
- `normal`: OpenGL convention (green up), as Bevy expects;
- `arm`: ambient occlusion in red, roughness in green, metalness in blue, so one image serves as
  both the occlusion and the metallic-roughness texture.

The client makes their mipmaps when it loads them (`src/textures.rs`)."""

import common

# name: (source prefix, source resolution, albedo size, normal size, arm size). The client knows
# how much of a metre each covers (`look::WOODS`).
WOODS = {
    # Boards, to choose from: golden teak, dark walnut, warm cherry.
    "teak": ("teak_veneer", "2k", 2048, 1024, 1024),
    "walnut": ("black_walnut_veneer_01", "2k", 2048, 1024, 1024),
    "cherry": ("cherry_veneer", "2k", 2048, 1024, 1024),
    # Pale wood, for the light side of a board.
    "maple": ("white_maple_veneer", "1k", 1024, 1024, 1024),
    # Not a wood: the table's felt, made the same way.
    "felt": ("scuba_suede", "2k", 1024, 1024, 1024),
}


def find(prefix, kind, resolution):
    name = f"{prefix}_{kind}_{resolution}.jpg"
    if not any(s["file"] == name for s in common.sources()):
        raise SystemExit(f"no source {name} in sources.toml")
    return common.source(name)


def run():
    for name, (prefix, res, albedo_size, normal_size, arm_size) in WOODS.items():
        albedo = common.srgb_to_linear(common.read_image(find(prefix, "diff", res))[..., :3])
        albedo = common.linear_to_srgb(common.resize(albedo, albedo_size))
        common.write_jpeg(common.output("textures", "wood", f"{name}_albedo.jpg"), albedo, 88)

        normal = common.resize(common.read_image(find(prefix, "nor_gl", res))[..., :3], normal_size)
        common.write_jpeg(common.output("textures", "wood", f"{name}_normal.jpg"), normal, 92)

        arm = common.resize(common.read_image(find(prefix, "arm", res))[..., :3], arm_size)
        common.write_jpeg(common.output("textures", "wood", f"{name}_arm.jpg"), arm, 90)
