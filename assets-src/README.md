# The asset pipeline

Everything in `assets/` but the fonts is made here, by scripts, so it can be read in a review,
changed by a person or an agent, and made again (plan, decision 8).

```sh
bash scripts/assets.sh                 # every step (the table below)
bash scripts/assets.sh stones          # one step
bash scripts/assets.sh previews        # render the assets in Cycles, to look at
bash scripts/assets.sh garden_previews # the same for the garden, from where a player sits
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
| `woods` | `textures/wood/<wood>_{albedo,normal,arm}.jpg`: three board woods (teak, walnut, cherry), pale maple, and the table's felt | scanned wood and cloth, resized |
| `stones` | `models/stone.glb`, `textures/stone/{slate,shell}_albedo.jpg` | the stone's profile, turned; the colours drawn with numpy |
| `engraving` | `textures/engraving/glyphs_{albedo,normal}.png`: the coordinates' glyphs as cuts in wood, an atlas of 8 × 5 cells | each glyph of Fira Sans set as a Blender text object and rendered from above in Cycles; the cut's colour (walls darkening to its bottom) and its walls' slopes worked out from that shape |
| `cards` | `pictures/cards/points.jpg`: the picture on the cards of the games played on a square board's points, in the client's main screen | Cycles, 512 × 512: a teak board of nine lines by nine with its star points, and slate and shell stones from `assets/appearance/points.toml`, from the player's side |
| `surfaces` | `textures/garden/*`: what the garden is made of | scanned surfaces resized as the woods are (tatami, hinoki, linen, moss, cut stone, bark, a bamboo fence, the rocks' own); and, drawn with numpy, the raked gravel (a scan of gravel with a furrow's shape laid over it), the water's ripples, a bamboo cane, clipped shrubs' leaves, sprays of maple and bamboo leaves cut out (PNG with alpha), and the trees and hills beyond the fence |
| `tableware` | `models/bowl.glb`, `models/bowl_stones.glb`: a turned bowl, and the heap of stones in it | the bowl's profile, turned; the board's stone with few triangles, three dozen of them heaped |
| `garden` | `environments/garden.glb`: the whole garden, one file | modelled in `garden.py` with the helpers of `meshkit.py`; the rocks are a scanned set, made lighter |
| `garden_light` | `environment/garden_{diffuse,specular,sky}.ktx2`: the garden's light, and its sky | Cycles renders the garden from just above the table, all the way round, under the sun and sky that `assets/environments/garden.toml` gives the client; that panorama is filtered as the room's photograph is. The sky is a photograph (pure sky, no ground), turned to put its sun where the garden's is, with the sun itself clipped out |
| `previews` | `target/asset-previews/*.png`, not shipped | Cycles, with the materials of `assets/appearance/hex.toml` |
| `garden_previews` | `target/asset-previews/garden-*.png`, not shipped | Cycles, the garden from the client's camera at a few places, and from high above (`GARDEN_VIEWS=garden-seat,garden-plan` renders only those) |

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

## An environment

A place to play in (`src/environment.rs`) is two files: `assets/environments/<id>.toml`, which
is the client's (its sun, sky, mist, camera, cushions and **materials**), and a model the
pipeline makes.

- **The model is one glTF file with no materials of its own.** Each mesh's material slot is only
  a name; the TOML file says what that name is made of (`look::MaterialSpec`), and the client and
  the pipeline both read it (`matlib.py` builds the same material for Cycles). So a look is
  changed by editing data, and a preview shows what the client will.
- **The table top is at height 0 with its middle at the origin**, in metres. In Blender x is to
  the player's right, y away from the player, z up.
- **Things that repeat share a mesh** (`meshkit.instance`, `garden.put`): the client draws each
  mesh's copies in one batch. The garden is about 117,000 triangles in 77 meshes.
- **Texture coordinates** are the mesh's own. A material with `metres` expects them in metres
  (`meshkit.box_uv`, or a floor mapped from above); a `wood` also needs the finer second set
  (`box_uv(..., detail=True)`). Every mesh needs some, for its tangents.
- **A face is seen from one side** in the client, though Cycles shows both: check the winding
  of anything flat (a card, a fence) in the client's own screenshot.
- **The cushion** is one node, standing on z = 0 about the origin; the client lays one out for
  each seat (`[seating]`). `garden.seats` lays two out for the previews.
- **The light is made from the model**, so run `garden_light` after `garden` when what is seen
  from the table changes much. Its Cycles render is noisy, so its files' bytes differ from run
  to run; the other steps give the same bytes each time.

To look: `bash scripts/assets.sh garden_previews`, then the client,
`--environment garden --view 60,15,2 --frames 200 --screenshot garden.png`.

## Adding a source

Only CC0 sources, so the client can ship what is made from them with no conditions. Add an entry
to `sources.toml` with the file's URL and SHA-256 (`sha256sum` it once downloaded), the asset's
name, page and authors, and a line to `assets/CREDITS.md`.
