# The asset pipeline

Everything in `assets/` but the fonts is made here, by scripts, so it can be read in a review,
changed by a person or an agent, and made again (plan, decision 8).

```sh
bash scripts/assets.sh                 # every step (the table below)
bash scripts/assets.sh stones          # one step
bash scripts/assets.sh previews        # render the assets in Cycles, to look at
bash scripts/assets.sh piece_previews  # the same for the chessmen, checkers, Tak, cards, dice and discs
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
| `stones` | `models/stone.glb`, `textures/stone/{slate,shell}_albedo.jpg`, and polished minerals for coloured stones: `textures/stone/{lapis,jasper,tiger_eye,black_slate}_albedo.jpg` | the stone's profile, turned; the colours drawn with numpy (the minerals in `minerals.py`, numpy alone, so they can be tried outside Blender: lapis's cloud, calcite and pyrite flecks, jasper's veins, tiger's eye's bands and silk, slate's layers) |
| `engraving` | `textures/engraving/glyphs_{albedo,normal}.png`: the coordinates' glyphs as cuts in wood, an atlas of 8 × 5 cells | each glyph of Fira Sans set as a Blender text object and rendered from above in Cycles; the cut's colour (walls darkening to its bottom) and its walls' slopes worked out from that shape |
| `cards` | `pictures/cards/points.jpg`: the picture on the cards of the games played on a square board's points, in the client's main screen | Cycles, 512 × 512: a teak board of nine lines by nine with its star points, and slate and shell stones from `assets/appearance/points.toml`, from the player's side |
| `surfaces` | `textures/garden/*`: what the garden is made of | scanned surfaces resized as the woods are (tatami, hinoki, linen, moss, cut stone, bark, a bamboo fence, the rocks' own); and, drawn with numpy, the raked gravel (a scan of gravel with a furrow's shape laid over it), the water's ripples, a bamboo cane, clipped shrubs' leaves, sprays of maple and bamboo leaves cut out (PNG with alpha), and the trees and hills beyond the fence |
| `tableware` | `models/bowl.glb`, `models/bowl_stones.glb`: a turned bowl, and the heap of stones in it | the bowl's profile, turned; the board's stone with few triangles, three dozen of them heaped |
| `chessmen` | `models/chess/{king,queen,bishop,knight,rook,pawn}.glb`: a set of chessmen of the usual pattern, the king 46 mm tall | five turned from profiles written in `chessmen.py`, with what a lathe cannot do added after (the rook's battlements, the queen's coronet, the cut in the bishop's mitre, the king's cross); the knight's head from an outline of its side, given a carved head's thickness, with a mane, an eye and a mouth cut in |
| `checkers` | `models/checkers/{man,king}.glb`: a wooden draughtsman, and a king, which is two | a profile turned, its edge milled |
| `tak` | `models/tak/{flat,wall}.glb`: a square block lying flat, and the same block on its edge (Tak's capstone is the chessmen's pawn) | a box with its edges eased |
| `fences` | `models/fence.glb`, `models/bowl_fences.glb`: a slender rail of wood 90 % of a 28 mm cell's side long, so the squares' corners show between fences (Fendo's), and a heap of them for the bowl | a box with its edges eased; the heap plain boxes dropped every way |
| `tiles` | `models/tile_hex.glb`: a hexagonal tile of wood laid over one cell of a hexagonal board (Courtfall's terrain), a little narrower than a 28 mm cell across its flats, so the board's lines show round it; its points to the far and near sides, as the client's cells' are | a hexagonal prism with its edges eased |
| `playing_cards` | `models/card.glb`: a blank playing card with a place on each side for a picture; `textures/cards/back_albedo.jpg`: a back for it | the card's outline, its `face`, `back` and `edge` each a part; the back drawn with numpy |
| `card_faces` | `textures/cards/faces/<suit>_<rank>_albedo.png`: the faces of the usual deck's 52 cards (`spades_a`, `hearts_10`, `clubs_k`), 640 × 896 | drawn with numpy: a suit is an outline of curves, a rank is lettered in Fira Sans (set by Blender, turned into triangles), a court card has a crown, a coronet or a plumed cap in a frame; the outlines are filled on a grid four times as fine and averaged down |
| `dice` | `models/die.glb`: a die of six sides | a cube with its edges rounded, a ball cut out of it for each pip (`body` and `pips` are its parts) |
| `discs` | `models/disc.glb`: a disc 15 mm across and 3.6 mm thick, light on one side and dark on the other; `models/ring.glb`: a ring 25 mm across and 7 mm tall that a disc lies inside | each turned from a profile, the disc's faces dished inside a rim; the disc's faces with the half of the edge beside each are its two parts, `light` (up, as it lies) and `dark` |
| `garden` | `environments/garden.glb`: the whole garden, one file | modelled in `garden.py` with the helpers of `meshkit.py`; the rocks are a scanned set, made lighter |
| `garden_light` | `environment/garden_{diffuse,specular,sky}.ktx2`: the garden's light, and its sky | Cycles renders the garden from just above the table, all the way round, under the sun and sky that `assets/environments/garden.toml` gives the client; that panorama is filtered as the room's photograph is. The sky is a photograph (pure sky, no ground), turned to put its sun where the garden's is, with the sun itself clipped out |
| `previews` | `target/asset-previews/*.png`, not shipped | Cycles, with the materials of `assets/appearance/hex.toml` |
| `piece_previews` | `target/asset-previews/{chess,checkers,tak,blooms,fendo,cards-dice,discs,…}.png`, not shipped | Cycles, each piece made as its step makes it, in the materials its appearance file gives it (the card, the die and the disc in their generic look, `src/look.rs`, with two of the usual deck's faces); `PIECE_VIEWS=chess,tak` renders only those |
| `garden_previews` | `target/asset-previews/garden-*.png`, not shipped | Cycles, the garden from the client's camera at a few places, and from high above (`GARDEN_VIEWS=garden-seat,garden-plan` renders only those) |

The sounds (`sounds/{tick,thump,key,tock,cheer,groan}.ogg`) are not Blender's: `python3
assets-src/sounds/sounds.py`, on the host, synthesises them (wood's modes, decaying sine waves
over a burst of noise; a crowd as a score of voices, each a buzz through a vowel's formants, with
applause and a little of a hall; with Python's standard library) and encodes them as Ogg Vorbis with
ffmpeg, bit-exactly, so the same script makes the same files.

How the client reads them:

- **Woods** are mapped by position, in metres (`look::world_uvs`), so a wood's texture must be
  seamless and its real size known (`look::WOODS`), with the colour an oil finish gives the raw
  veneer that was scanned. Its normal map is read three times finer than its colour, for the
  pores a close look sees. `arm` is ambient occlusion, roughness and metalness in red, green and
  blue, as glTF reads them.
- **Textures** are JPEG, and the client makes their mipmaps when they load
  (`src/textures.rs`); a colour texture's name ends in `_albedo`, which is how it is read as
  sRGB.
- **Models** are glTF, in metres, made for a 28 mm cell; the first mesh is used. The client
  reads how low and how high a model reaches from its file (`assets::model_span`): a piece is
  stood by its foot, and what is on it in a stack is raised by its height, so nothing else
  says how tall it is. The pieces here are centred on their middle. One with a face (a knight)
  looks to the far side of the table, +y in Blender: the client turns it for its seat.
- **A model of several parts** (a card's face, back and edge; a die's body and pips; a disc's
  light and dark sides) has a
  material slot for each and is exported with them (`meshkit.export_glb(…, materials=True)`),
  so each part carries a name: the client gives each name a material (`props::spawn`).
- **Materials** are not in the models: an appearance file (`assets/appearance/*.toml`) or the
  generic look (`src/look.rs`) says what each thing is made of, and the previews read the same
  file.

## A piece

A game's appearance file (`assets/appearance/*.toml`) names a model for each kind of piece, by
the id the game gives it, and how it `stands`: `loose` (a stone: turned any way, leaning a
little), `upright` (a turned piece), `square` (a block set down by hand: square to the board, a
quarter turn any way, a little askew and a little aside of the middle, so a stack's blocks do
not line up), `forward` (a piece with a face) or `fitted` (a tile: true to the board, never
turned). A kind it does not name is the generic stone. A seat's
pieces are all of that seat's material, unless a piece has a `material` of its own.

A piece is mapped in metres from every side, with the finer second set (`meshkit.box_uv(…,
detail=True)`), so it can be made of a wood; each kind from its own place in the wood
(`offset`). **A mesh cut with a boolean is put in order after** (`meshkit.boolean` does it,
with `meshkit.settle`): the cut leaves its vertices in whatever order its threads finished, and
the file would differ from run to run.

## A card and a die

- **The card** lies face up about the origin, its top to the far side of the table. A side's
  picture fills it: **any image of 5 to 7, as a JPEG or a PNG**. On the face the picture's top
  is the card's top; the back is mapped the other way across, so its picture reads rightly when
  the card is turned over about its long axis (`props::card_lies`). The client reads no SVG: one
  is made a PNG or a JPEG first (Blender's own SVG import reads filled shapes, but no text and
  no strokes, so it will not do for a card in general).
- **The usual deck's faces** (`card_faces.py`) are such pictures, one for each of its 52 cards:
  the card's paper with the face printed on it. The client finds one by the suit's symbol and
  the rank's label, as the server gives them (`props::standard_face`); a card of any other deck
  has no picture here. The step also lays the whole deck on one sheet to look at,
  `target/asset-previews/card-faces.png`. A PNG, since a JPEG rings round flat ink on flat
  paper; the 52 are 1 MB together. There are no jokers: no game deals one yet.
- **The die** has 1 up and 6 down, 2 towards the player and 5 away, 3 to the right and 4 to
  the left, as dice are made; `props::die_showing` turns it to show a number.
- Their generic looks are in `src/look.rs` (`paper`, `card_side`, `die`, `pips`), which
  `piece_previews.py` repeats.

To look at any of these in the client, which draws a face from one side only:

```sh
cargo run -- --showcase --view 15,38,0.5 --frames 200 --screenshot showcase.png
cargo run -- --showcase --card-face my-face.png --view 0,62,0.22,0,0.11   # a picture on a card
```

`--showcase` lays every piece model the appearance files name on the table, in each material it
is given, with the dice and a few cards (four of the usual deck face up, one face down and a
pile); `--view`'s last two numbers aim the camera at a place
on the table.

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
