"""The woods (and the table's felt): scanned CC0 textures, resized to what the client needs and
saved as JPEG.

Each wood is three images, as glTF and Bevy's `StandardMaterial` read them:
- `albedo`: the colour, in sRGB, resized in linear light;
- `normal`: OpenGL convention (green up), as Bevy expects;
- `arm`: ambient occlusion in red, roughness in green, metalness in blue, so one image serves as
  both the occlusion and the metallic-roughness texture.

The client makes their mipmaps when it loads them (`src/textures.rs`)."""

import common

# name: (source prefix, albedo size, normal and arm size). The client knows how much of a metre
# each covers (`look::WOODS`).
WOODS = {
    # Boards, to choose from: golden teak, dark walnut, open-grained oak, warm cherry.
    "teak": ("teak_veneer", 2048, 1024),
    "walnut": ("black_walnut_veneer_01", 2048, 1024),
    "oak": ("oak_veneer_01", 2048, 1024),
    "cherry": ("cherry_veneer", 2048, 1024),
    # Pale wood, for the light side of a board.
    "maple": ("white_maple_veneer", 1024, 1024),
    # Not a wood: the table's felt, made the same way.
    "felt": ("scuba_suede", 1024, 1024),
}


def find(prefix, kind):
    for s in common.sources():
        if s["file"].startswith(f"{prefix}_{kind}_"):
            return common.source(s["file"])
    raise SystemExit(f"no source for {prefix} {kind} in sources.toml")


def run():
    for name, (prefix, albedo_size, detail_size) in WOODS.items():
        albedo = common.srgb_to_linear(common.read_image(find(prefix, "diff"))[..., :3])
        albedo = common.linear_to_srgb(common.resize(albedo, albedo_size))
        common.write_jpeg(common.output("textures", "wood", f"{name}_albedo.jpg"), albedo, 88)

        normal = common.resize(common.read_image(find(prefix, "nor_gl"))[..., :3], detail_size)
        common.write_jpeg(common.output("textures", "wood", f"{name}_normal.jpg"), normal, 92)

        arm = common.resize(common.read_image(find(prefix, "arm"))[..., :3], detail_size)
        common.write_jpeg(common.output("textures", "wood", f"{name}_arm.jpg"), arm, 90)
