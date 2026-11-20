"""The woods: scanned CC0 textures, resized to what the client needs and saved as JPEG.

Each wood is three images, as glTF and Bevy's `StandardMaterial` read them:
- `albedo`: the colour, in sRGB, resized in linear light;
- `normal`: OpenGL convention (green up), as Bevy expects;
- `arm`: ambient occlusion in red, roughness in green, metalness in blue, so one image serves as
  both the occlusion and the metallic-roughness texture.

The client makes their mipmaps when it loads them (`src/textures.rs`)."""

import common

# name: (source prefix, albedo size, normal and arm size). Teak and maple cover 1 m, the dark
# wood 2 m: the client scales them to that (`src/look.rs`).
WOODS = {
    # The board: a golden, fine and even grain.
    "teak": ("teak_veneer", 2048, 1024),
    # Pale wood, for the light side of a board.
    "maple": ("white_maple_veneer", 1024, 1024),
    # The table: dark, reddish, under everything.
    "dark": ("dark_wood", 2048, 1024),
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
