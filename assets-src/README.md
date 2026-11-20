# The asset pipeline

Everything in `assets/` but the fonts is made here, by scripts, so it can be read in a review,
changed by a person or an agent, and made again (plan, decision 8).

```sh
bash scripts/assets.sh                 # every step: environment, woods, stones
bash scripts/assets.sh stones          # one step
bash scripts/assets.sh previews        # render the assets in Cycles, to look at
```

The script runs Blender with no window (`--background`) on `blender/build.py`, which:

1. fetches each source in `sources.toml` into `target/asset-sources/` (once) and checks its
   SHA-256;
2. runs the steps asked for, each writing into `assets/`.

Commit what changes in `assets/` with the scripts that made it. A new file there must also be
listed in `src/assets.rs` (a test says so), which compiles it into the binary.

## Blender

Blender **5.3**, which `build.py` checks. The script finds it as `$BLENDER`, else `blender` on
`PATH`, else Flathub's `org.blender.Blender`:

```sh
flatpak install --user flathub org.blender.Blender
```

On the maintainer's machine it runs on the host, not in the toolbox: Flatpak's Blender sees the
NVIDIA GPU there, and Cycles renders the previews with OptiX. Its sandbox has its own `/tmp`, so
the scripts read and write only inside the repository (`target/` for what is not committed).

Blender's Python brings what the steps use: numpy, OpenImageIO (reading and writing images),
zstandard (KTX2) and tomllib. Nothing is installed into it.

## The steps

| Step | Makes | From |
|---|---|---|
| `environment` | `environment/room_{diffuse,specular}.ktx2`: the room's light for Bevy's `EnvironmentMapLight` | an HDR panorama, graded for a table top (the floor below the horizon made the dim table the board stands on, colours at half saturation), then GGX-filtered per roughness level, and irradiance from spherical harmonics |
| `woods` | `textures/wood/<wood>_{albedo,normal,arm}.jpg`: four board woods (teak, walnut, oak, cherry), walnut again at 4K to compare, pale maple, and the table's felt | scanned wood and cloth, resized |
| `stones` | `models/stone.glb`, `textures/stone/{slate,shell}_albedo.jpg` | the stone's profile, turned; the colours drawn with numpy |
| `engraving` | `textures/engraving/glyphs_{albedo,normal}.png`: the coordinates' glyphs as cuts in wood, an atlas of 8 × 5 cells | each glyph of Fira Sans set as a Blender text object and rendered from above in Cycles; the cut's colour (walls darkening to its bottom) and its walls' slopes worked out from that shape |
| `previews` | `target/asset-previews/*.png`, not shipped | Cycles, with the materials of `assets/appearance/hex.toml` |

How the client reads them:

- **Woods** are mapped by position, in metres (`look::world_uvs`), so a wood's texture must be
  seamless and its real size known (`look::WOODS`), with the colour an oil finish gives the raw
  veneer that was scanned. Its normal map is read three times finer than its colour, for the
  pores a close look sees. `arm` is ambient occlusion, roughness and metalness in red, green and
  blue, as glTF reads them.
- **Textures** are JPEG, and the client makes their mipmaps when they load
  (`src/textures.rs`); a colour texture's name ends in `_albedo`, which is how it is read as
  sRGB.
- **Models** are glTF, in metres, centred on their middle, made for a 28 mm cell; the first
  mesh is used.
- **Materials** are not in the models: an appearance file (`assets/appearance/*.toml`) or the
  generic look (`src/look.rs`) says what each thing is made of, and the previews read the same
  file.

## Adding a source

Only CC0 sources, so the client can ship what is made from them with no conditions. Add an entry
to `sources.toml` with the file's URL and SHA-256 (`sha256sum` it once downloaded), the asset's
name, page and authors, and a line to `assets/CREDITS.md`.
