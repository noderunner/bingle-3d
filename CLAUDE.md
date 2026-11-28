# Working on bingle-3d, for an agent

bingle-3d is a native client for [Bingle](https://github.com/noderunner/bingle), built with Bevy in
Rust for Linux and Windows. It plays Bingle's games through the server's public API and draws them
in 3D. It has no rules of its own. [README.md](README.md) covers building; [docs/plan.md](docs/plan.md)
holds the stages, the decisions made so far and the open questions. This file is the working
agreement, adapted from Bingle's own `CLAUDE.md`, which wins where the two disagree about how work
reaches GitHub.

## Secrets and host paths

Credentials and identities come from the environment, and are referred to **only** by the name of
the variable that holds them. **Never write a token, a key, an account name, a key's path or a path
on the maintainer's machine into a file in this repository**: not in a comment, a test fixture, a
commit message or a pull request. If you need one of these and the variable is unset, say so and
stop; do not guess, and do not read it out of a dotfile into anything you commit.

| Variable | What it holds |
|---|---|
| `BINGLE_GH_APP_ID` | the client ID of the GitHub App that agent work acts as (Bingle's; it is installed here too) |
| `BINGLE_GH_APP_KEY` | the path to that app's private key |
| `BINGLE_GH_REMOTE` | a repository's HTTPS URL. It is set for `bingle`; **override it for this repository** (below) |

Repository administration uses a separate maintainer-held credential that agent work never needs
and should never look for.

## Acting on GitHub

**Agent work acts on GitHub only as the GitHub App**, through Bingle's `scripts/ghapp`. Never
through a personal account: not the maintainer's, and not a second account made for an agent (one
was suspended; Bingle's `CLAUDE.md` explains why). Do not `git push` over SSH, even though `origin`
is an SSH URL: fetching that way is fine, pushing is not.

The helper lives in a checkout of Bingle beside this one. Build it once per session into a scratch
directory, and point it at this repository:

```sh
(cd ../bingle && go build -o "$SCRATCH/ghapp" ./scripts/ghapp)       # $SCRATCH: any temp dir
export BINGLE_GH_REMOTE=https://github.com/noderunner/bingle-3d.git
R=noderunner/bingle-3d
"$SCRATCH/ghapp" api GET "/repos/$R/pulls?state=open"                 # any REST call, as the app
"$SCRATCH/ghapp" api POST "/repos/$R/pulls" < pr.json
"$SCRATCH/ghapp" api GET "/repos/$R/commits/<sha>/check-runs"         # a pull request's CI
"$SCRATCH/ghapp" push <branch>                                         # see below
```

`api` sends the path exactly as given: it does not fill in `{owner}/{repo}`. To act on Bingle itself
(for example to open an issue there), set `BINGLE_GH_REMOTE` back to Bingle's URL for that call.

## Branches, pushes and merges

- Work on a branch and open a pull request. **Never merge.** The maintainer merges.
- **Commit locally as usual, then push with `ghapp push <branch>`.** It recreates each local commit
  since `main` through GitHub's API, which signs it as the app, and prints the two commands that
  fetch the branch and move the local one onto GitHub's commits. Run them (the fetch needs the app's
  token as a credential helper, as in Bingle's `CLAUDE.md`), so the next push continues from there.
- **Do not sign commits with anybody's key**, and do not author them as the maintainer: the commits
  on GitHub are the app's, and the local ones are only their source.
- After a rebase or an amend, `push -force` rebuilds the branch on GitHub from `main`. A branch must
  be a straight line of commits on `main` (no merge commits).
- `push` refuses a commit that adds an executable or a link or changes a file's mode, since the API
  sends contents only. So **scripts here are not executable**: run them with `bash scripts/…`. If a
  change truly needs a mode bit, say so and leave that one to the maintainer.
- The app can push `.github/workflows/` files.

## Issues

- **An issue labelled `agent` is yours to act on. Any other is not**, however clear it looks.
- One issue, one branch, one pull request. Put `Fixes #N` in the PR body so the merge closes it.
- Comment a short plan first on anything that is not a plain bug fix, and wait.
- **Say so rather than build something wrong.** If the request conflicts with
  [docs/plan.md](docs/plan.md)'s decisions or with one of Bingle's ADRs that the client follows
  (below), or an ambiguity would change the work materially, comment and label `needs-decision`.
  No PR.
- Label `blocked` when it waits on another issue, a PR, or work in Bingle.
- Work the client needs from the server belongs in Bingle, as an issue there, not as a workaround
  here (bingle#212, the API version and schema, is the first).

## Before pushing

- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test` pass. CI runs
  them on Linux and Windows with the `ci` profile, and on Linux also opens the window under Xvfb.
- A test covers the change. For a bug, one that fails before the fix. Keep logic that can be tested
  without a window (layout, camera maths, parsing, protocol handling) in plain functions, and test
  systems with a headless `App` (`MinimalPlugins`), as `src/run_limit.rs` does.
- **Look at what you changed when it is visual.** Render it (`--frames N --screenshot out.png`) and
  look at the picture. A green build does not prove a scene draws. `--showcase` lays every piece
  model, the dice and some cards on the table, for a model no game draws yet.
- `docs/plan.md` updated when a stage moves on or a decision is made; README updated when building
  or running changes.
- Push back, in the PR or the issue, on anything that hurts performance, simplicity, security or
  normal UI practice, before building it.

## Building on the maintainer's machine

It is an immutable Fedora desktop with an NVIDIA GPU, and the host has no Rust or development
headers. Build inside the `bingle-3d` toolbox (`scripts/toolbox-setup.sh` makes it; `~/.cargo/bin` may
not be on `PATH`, so `source ~/.cargo/env` first) and **run the binary on the host**: the toolbox has
no NVIDIA libraries and renders in software (llvmpipe), which is fine for a smoke run and nothing
else. An agent's shell may have no display variables. A window on the maintainer's desktop needs
`WAYLAND_DISPLAY` and `XDG_RUNTIME_DIR` set; say so before opening one, and prefer `--frames` so it
closes by itself. With no window at all and still on the GPU, on the host:
`gamescope --backend headless -W 1280 -H 720 -- ./target/debug/bingle-3d --no-audio-devices --frames 200 --screenshot out.png`
(add `--view YAW,PITCH,RADIUS` to look from somewhere). Headless, run it as CI does: in the toolbox,
`WAYLAND_DISPLAY= xvfb-run -a -s '-screen 0 1280x720x24' ./target/debug/bingle-3d --frames 120 --screenshot out.png`.

**Against a real server**: build Bingle from `origin/main` into a scratch directory (`git archive
origin/main` from `../bingle`, then `go build`), without touching that checkout, which may be on
the maintainer's branch. Run it on loopback with its own database and ports, since the maintainer
may have a Bingle running on the defaults (`BINGLE_ADDR=127.0.0.1:18080`, `BINGLE_METRICS_ADDR`,
`BINGLE_DB_PATH`; add `BINGLE_TLS_SELFSIGNED=1` and `BINGLE_TLS_CACHE` to try a self-signed
certificate). Point the client at it with `--server`, and **always set `BINGLE3D_CONFIG_DIR` to a
scratch directory**, so the maintainer's own config and guest keys are never read or written.

## What the client must respect

These are Bingle's decisions; the client is built around them, not around changing them.

- **API only.** The server is the authority: the client asks for the legal selections and narrows
  them as the player clicks; it never decides legality, and it holds no rules.
- **The server says what things are; the client decides how they look**
  ([ADR 0124](https://github.com/noderunner/bingle/blob/main/docs/decisions/0124-the-server-says-what-things-are-clients-decide-how-they-look.md)).
  A seat's color is a name, not a value to draw with (Words, below). Every game must draw from its meaning alone,
  with generic defaults; a per-game appearance improves that and is never required.
- **No game is named in shared code.** Game-specific looks live in per-game appearance files.
- **Commercial games stay out of this repository**
  ([ADR 0127](https://github.com/noderunner/bingle/blob/main/docs/decisions/0127-commercial-games-in-a-private-overlay.md)). A server marks the games it builds from its
  private overlay `private` (API 1.8); the client leaves them out of every list
  (`session::public`) unless the server offers their look (API 1.17, `downloads/`) or the
  private folder has it. That folder is
  `bingle-3d/assets` in the overlay, read at run time (plan, decision 2; `--private-dir`,
  `BINGLE3D_PRIVATE_DIR`, `private_dir`), and their looks go there. Never add their names,
  art, models or rules text here, not even in a test: a test makes up a game id.
- **The connection must be secure**
  ([ADR 0065](https://github.com/noderunner/bingle/blob/main/docs/decisions/0065-the-connection-must-be-secure.md)):
  plain HTTP only to loopback; a self-signed server is trusted by its fingerprint after the player
  agrees, never by turning certificate checks off.

## Words

[ADR 0154](https://github.com/noderunner/bingle/blob/main/docs/decisions/0154-a-color-is-a-name-and-paint-is-each-clients-own.md)
defines these four, used the same way here as in Bingle:

| Word | Meaning |
|---|---|
| **seat** | a place at the table: its turn in the order, its clock, its result |
| **color** | a name the rules give a set of pieces (`white`, `yellow`, `grey`): an identifier from the API, never a value to draw with |
| **label** | what players call a seat ("White", "Warm", "Attackers") |
| **paint** | what this client draws a color as: an sRGB value, a material |

A field, variable or parameter called `color` holds a name, and a value to draw with is called
`paint`; Bevy's `Color` type keeps its name. **One resolver turns a color into paint**
(`look::seat_paint`, the standard look, and `appearance::seat_paint`, a game's look over it), and
no other code reasons about hue. Wherever the client names a seat or a color, the paint stands
beside the name: a dot for each color the seat plays (`Appearances::seat_paints`, `browser::said`
for a line of words that names seats). The word is spelled `color` everywhere, in code, comments, docs and
data files; `tests/spelling.rs` fails on the other spelling and lists the few places that keep it
(a copy of Bingle's schema, and the old key a data file is still read with).

[ADR 0114](https://github.com/noderunner/bingle/blob/main/docs/decisions/0114-impulses-selections-moves-and-gestures.md)
defines these; use them the same way in code, docs and names.

- An **impulse** is the choices one seat makes before the game needs something from elsewhere; the
  **impulse seat** is the seat whose impulse the game is waiting for.
- A **selection** is one self-contained choice within an impulse; a **move** is the selections of one
  impulse, a **pending move** one not yet submitted, and a **ply** one impulse.
- A **gesture** is one thing a player does in an interface: a click, a tap, a drag, a key, a pinch.
  The rules never depend on gestures. In this client every input device (mouse today; controllers
  and hands in VR later) becomes gestures in one place.

A game's own rules and its players' words keep their terms.

## The shape of the code

- `src/main.rs`: options, then the app and its plugins. `src/cli.rs`: command-line options.
- `src/net/`: the server. **All network I/O is on one thread** (`mod.rs`), with blocking calls (a call's sound excepted: `src/voice/`); systems
  send it `Request`s and draw from the `ServerStatus` and `WatchStatus` resources (the watched game,
  and the turn of a seat this client plays). The watched game's
  WebSocket (`socket.rs`, tungstenite over our own TLS) is read in the same loop with a short
  timeout, so it stays one thread. `session.rs` makes the calls and turns every failure into a
  `Problem` worded for the player; `api.rs` holds the API's types and the version check;
  `server_url.rs` the address rule (plain HTTP only to loopback); `tls.rs` the certificate check
  and the connector ureq uses. **A game that may keep secrets from a seat**
  (`GameInfo::may_keep_secrets`: a hand of cards) is sent to everybody as a public view, on
  the socket too: a client that holds a seat reads the position again as that seat
  (`Session::state` with the player's name) before an event reaches the table (`own_view`).
  `WatchStatus::shown` is the position to draw what lies on the table from: the game's, or
  the server's preview of the move being built.
- `src/table.rs`: the watched game as the server last reported it. Every event carries the whole
  position, so it replaces what is known; `move_seq` decides what is stale and what is a gap
  (reload). **A move event's `seq` counts selections, of which a move may be several; a
  state's `move_seq` counts moves**: the table keeps the first apart (`Table::seq`) to tell a
  stale event or a gap, and an event's state says the moves played after it. Of several seats
  this client holds (a game against oneself), `yours` is the one on impulse.
- `src/config.rs`: `config.toml` and `keys.toml` (guest keys, and the session token of an account
  signed in). Keys never go in `config.toml`, and a file that
  cannot be read is never overwritten.
- `src/setup.rs`: a server's setup file (`bingle-3d-setup.toml`, Bingle's ADR 0151; `--setup`,
  or beside the program), taken into the config store at start, before anything connects: the
  server, the name, a certificate's fingerprint, a guest's key. An account's sign-in code is
  kept nowhere: the network thread spends it first (`Request::SetupSignIn`), from the login
  screen. **The file is deleted once it is used up, and never when the code may still be
  good** (the server was not reached). No Bevy in it. **Never log the code or the key.**
- `src/browser.rs`: the main screen: the server's status, the web page's tabs (only *Games* built),
  and in *Games* the cards of the games to start (*New*, with a game's setup when one is picked)
  or the games being played (*Active*). Its `Action` is what every panel's buttons do. The
  interface is blended in linear light: prefer opaque colors to faint see-through ones.
  `src/families.rs`: the games to start grouped by family, the names of a family and its
  members, and a card's marks and players, with no Bevy in it. `src/game_panel.rs`: the panel
  beside a watched game: what the seat on impulse is asked, in the server's words when its
  view says (`Position::impulse_words`), else generic hints, as also once the player has
  chosen something (`hint`). A choice the view says is drawn on the table
  (`Position::panel_actions`: a patience's cards) is no button. A game whose look names a
  score sheet in the view (`[cards] sheet`) has it drawn there, a box for each frame; one
  whose look names counters of the view (`[panel] counters`) has them under each seat's
  name; a choice is labelled in the view's words where it has them (`Position::choice_words`).
- `src/escape_menu.rs`: the menu Esc opens over a game or the main screen (resume, back to the
  main screen, settings, logout to the login screen, quit; game options and profile greyed). While it is open, play's keys and the camera
  stand aside (`escape_menu::closed`). Its *Settings* page (`Page::Settings`) sets the display and the chess clock, and leads to *Graphics*
  (`Page::Graphics`), a row for each effect that steps to its next value, and the frame limit.
- `src/display.rs`: fullscreen (the default: borderless at the desktop's resolution) or a
  window, and the resolution (`DisplaySettings`; `offered`, `mode_for` and `window_mode` have no
  Bevy systems in them), kept as `fullscreen` and `resolution` in `config.toml`. A run limited
  by `--frames` or `--screenshot` is a 1280×720 window unless `--fullscreen`; a headset leaves
  the window alone.
- `src/graphics.rs`: which effects the flat cameras and the sun have (`Graphics`: ambient
  occlusion, anti-aliasing, depth of field, bloom, shadows, contact shadows), the *Graphics*
  page's to change (`Graphics::cycle`; MSAA turns ambient occlusion off, since neither it nor TAA
  goes with MSAA), kept in `config.toml` as `ambient_occlusion`, `anti_aliasing`, `depth_of_field`,
  `bloom`, `shadows` and `contact_shadows`. `scene::set_effects` puts them on a camera and takes
  them off, `scene::retune` and `tune_sun` again at once on a change, and bloom's flips
  `scene::BloomOn`. A run limited by `--frames` or `--screenshot` has the defaults whatever is
  kept, and `--no-bloom` bloom off; a headset's eyes are never touched (`HeadsetRun`: the page is
  greyed there). **A new effect is a field, a row of the page and an arm in `set_effects`.**
- `src/new_game.rs`: the form that sets up a new game (seats, bots, time, whether it waits for the
  player's start), drawn on the list panel in place of the lists while it is open; `Form` has no
  Bevy in it.
- `src/lobby.rs`: a game not yet started that this client has a seat in, on the game's panel:
  sit, put a bot in a seat, remove, leave, start; a seat kept for a friend waits for them. `offers` has no Bevy in it; the server decides
  who may do what.
- `src/friends.rs`: the signed-in account's friends panel, on the right, and its direct
  messages (kept per friend address, unread counted here: the server keeps no record of it). The list is filled by
  the network thread, which also keeps the account's own socket (`/ws-dm`) open and says it is
  here every minute (`AccountLink` in `net/mod.rs`).
- `src/login.rs`: the login screen at start (`Screen::Login`; `Screen::Main` is the rest): the
  server's address, checked by `Request::Probe` with a light (green, yellow for an API that does
  not go together, red), the sign-in's fields, *Continue as guest* with an optional guest key
  (checked with the server before it is kept), and auto-login (`config.toml`'s `auto_login`:
  what is kept is used, never a password). What only stands beside the main screen is marked
  `MainOnly`. The game list's reading and a lost server's retry run only on the main screen.
- `src/sign_in.rs`: signing in and out. `Form` has no Bevy in it, and its fields are drawn on
  the login screen; the password has a masked
  field of its own, since Bevy's `TextInput` cannot hide what is typed. While signed in,
  `session.rs` sends the session's token wherever it would send the guest key. **Never log a
  password or a token.**
- `src/clock.rs`: time in a timed game: the server's RFC 3339 timestamps, the time each seat has
  left (counted down between the server's reports) and how it reads.
- `src/chess_clock.rs`: a chess clock beside the board in a game of two seats with a game clock
  (`stands_for`), at the first seat's right hand clear of what lies there (`place`, from
  `board::BesideBoard`, which the board fills with what it lays beside itself), each face at its
  seat's end (`End::of_seat`). A face shows twelve less the time left (`hands`); the lamp under
  the face whose time runs is lit, tuned by `scene::BloomOn`; it only shows. Its model is the one
  of several nodes (`assets::model_nodes`): the case, and each hand, flag and button turned about
  its own. `ChessClockOn` (*Settings*' row, `chess_clock` in `config.toml`; on in a scripted run).
- `src/fonts.rs`: Fira Sans, compiled in. Use `UiFonts` for every text; Bevy's built-in font is
  ASCII only.
- `src/environment.rs`: where the table stands, as data (`assets/environments/<id>.toml`): the
  plain table in a dim room (`room`), and places modelled by the pipeline (`garden`). Its sun,
  its sky's light, mist, exposure, the camera's reach, cushions, and the materials its model's
  parts are named for. The player's choice (`Chosen`; stopgap buttons in
  `src/environment_choice.rs`, kept as `environment` in `config.toml`), which the server knows
  nothing of. No Bevy systems in it.
- `src/scene.rs`: builds the environment chosen and takes the last one down: a sun for shadows
  and the sky's light (`EnvironmentMapLight`), and on every 3D camera SSAO, TAA (both need
  `Msaa::Off`), bloom on highlights only (`BloomOn`), depth of field focused by the orbit
  camera, and fog.
  A model is drawn node by node from a glTF file with no materials: **a part's material is a
  name the environment's file gives a look**, so textures go through `textures.rs`. A cushion is
  laid for each seat of the game. In the room the key light (and the room's window, turned to
  it) is beyond the board: what faces the player reflects the dim room, so dark polish stays
  dark. **One unit is one metre, and the table top is at y = 0 with its middle at the origin,
  whatever the environment**, so boards are their real size (this is what makes VR work later)
  and nothing on the table knows where it stands.
- `src/bowls.rs`: a bowl of each color a seat plays beside the board (one at its right hand,
  or one at each hand), for games whose appearance has `[bowls]`, placed by the board's own
  outline; `places` and `width_at` have no Bevy in them. A click on a bowl makes its choice (a
  stone of its color: `play::Choice`); the stone waits over the bowl and is carried to the cell
  it is placed on (`board::bowl_stones`).
- `src/supply.rs`: what each seat has left to place (the definition's `reserves` and each
  piece's `pool`; the position's counts; or, for a game whose rules are written in Go, the
  view's counter its look names, `[supply]`, taken with the choice it names), in a line along the board's side at the seat's right
  hand: a flat piece on its edge (or face up, where its look says `[supply] lie = "flat"`), a
  tall one upright. `pools`, `Stance` and `line` have no Bevy in them. A click on the player's
  own, found on a box of its model's size (`SimplifiedMesh`: Bevy's ray cast passes through a
  model's smallest triangles, in metres), takes its plain piece (`board::show_supply`); it waits
  over its place and is carried where it goes, as from a bowl (`board::bowl_stones`, whose
  sources are both). In an opening where seats place each other's pieces (`opening.owner`), the
  piece is shown in the other's color.
- `src/fences.rs`: the walls the view says stand between positions (`Position::walls`; Fendo's
  fences), drawn on the side two squares share; where one stands has no Bevy in it. A game whose
  look has `[fences]` has a bowl of them both seats take from, just off the board's side to the
  first seat's left where its supply leaves room (`bowl_place`) (`play::TakeUp`: a click takes
  one up, `Turn::take_up`; or it is taken up by itself once nothing else may follow,
  `Turn::auto_take`). It waits lifted over the bowl, the sides it may go on lit;
  the side nearest the pointer, or turned to with the wheel, is aimed at (`play::SideAim`,
  `board::fence_aim`: a see-through fence there), and a click on the board makes the choice
  that builds it there, and it is carried there (`board::show_fences`). Which choice builds on which side is the look's (`sides`): the API
  does not say.
- `src/tints.rs`: the positions the view tints with a key of the game's legend
  (`Position::tints`; Courtfall's terrain and its Eye), drawn as the look's `[tints]` say: a
  piece of the look laid there (a tile, `Stands::Fitted`, which what stands there stands on:
  `Kit::floor`), or a stamp of a material printed there; a key the look does not name is not
  drawn. And what both seats have still to place, counted in the view (`[stock]`, a draft's
  tiles: `Position::view_counts`), in a column of each kind beside the board to the first
  seat's left; a click on one takes it to place (`play::TakeToPlace`, `Turn::take_to_place`:
  lifted over its place in the stock, the hexes it may go on lit, the one under the pointer showing it
  see-through in place of its light (`board::TileGhost`), and a click on one makes the click and
  the tile's choice that follows it), and it is carried where it goes (`board::FromStock`).
  `Tinted`, `shown`, `stock` and `takes` have no Bevy in them; `board::show_tints` draws them.
- `src/move_tiles.rs`: a game played with move cards, for a game whose look has `[move_tiles]`:
  each card the view lists (`view`: those each seat holds and the one at the side, each its
  id, name, moves as its holder sees them, and its stamp's seat) is a tile of wood beside the
  board (`models/move_tile.glb`), the grid cut into it, its squares stamped in ink
  (`textures/stamp/square_albedo.png`), its name engraved (`coordinates::glyph_mesh`) and a
  seal in its stamp's seat's color. A seat's tiles in front of it, the side's at the right
  hand of the seat on impulse; a click on one makes the look's `choice` and its id
  (`play::Choice`), those that may be chosen glow, the one chosen is lifted, and they travel
  over the board as the cards change hands. `dealt`, `reach`, `stamps`, `grid`, `places` and
  `reaches` (what the camera frames, `board.rs`) have no Bevy in them; no card is named.
- `src/trays.rs`: a tray in front of each seat for a game that counts what each captures
  (`GameInfo.counters` with a `max`: a square for each capture wanted; the counts are the
  position's `reserves`). `grid`, `places` and `fill` have no Bevy in them: `fill` says which
  pieces that left the board each seat's rising count took (the server does not say). A game
  whose look has `[score]` keeps a seat's score the same way with its own pieces instead: a
  dot on the board's margin at its right hand for each point (`marks`; the view's counter and
  its `max`), each taking one of its pieces of the look's kind as they leave the board.
  `board::fill_trays` carries them there (`hand::Route`: a wait, straight up, across, and a
  sound of their own as they land: a point scored on a dot is cheered as a win is).
- `src/assets.rs`: every file of `assets/` (but the fonts), compiled in and loaded as
  `embedded://<path>`; a test checks the list against the folder. The private folder's files
  (`read_private`) are read at start-up and found by the same paths (`all`), never in place
  of ours. It also reads a model's
  height and its parts' names from the file's own JSON. `src/textures.rs`: mipmaps for
  our JPEG textures when they load (Bevy 0.20 makes none), and the materials using them built
  again. The files are made by the asset pipeline: **Blender 5.3 headless, run from scripts in
  `assets-src/`** (its README), on the host, never by hand; CC0 sources only, listed with their
  hashes. Look at its Cycles previews (`bash scripts/assets.sh previews`) and at the client's
  screenshot before calling an asset done. A face is seen from one side in the client and
  from both in Cycles: only the client's screenshot shows a card or a fence wound the wrong way.
- `src/appearance.rs`: per-game appearance files (`assets/appearance/*.toml`, by game id): each
  kind of piece's model, how it stands (`Stands`: a stone lies loose, a block square to the
  board but never quite, a piece with a face looks across it) and a material of its own if it
  has one, or two sides (`sides`, `up`: a disc light on one side and dark on the other, each
  seat's its own side up; `src/sides.rs`, no Bevy systems, which half is which and how one
  changing hands is turned over, `board::dress_sides` and `flip_pieces` drawing it); what a
  seat's pieces and goal edges are made of, a seat's paint in the panels (`seats.<color>.paint`), the picture on its
  card in the main screen, sounds of its own as it is played in place of every game's
  (`[sounds]`), parts of its model in materials of their own (`parts`: an engraving's paint,
  every seat's alike; or a seat's own for a part, `seats.<word>.parts`: a ring's trim, gold on
  one seat's and silver on another's; a part given neither is of the piece's own material;
  `board::dress_parts`), the piece a move of it leaves where it started
  (`leaves`: a disc dropped in a ring that moves on as the ring is picked up, and shown
  see-through in a ring the pointer is over; the server does not say), a wider margin (`[board] margin`), a chessboard's two
  materials (`[board] squares`), its inlaid
  border (`[board] border`), a material for the cells of a region the board names
  (`[board.regions]`: Fist's keep), and the piece
  what a move aims is shown as while it is built (`aimed`: the server does not say; without it nothing is shown where a move aims). A file is for the games that mean the same by a piece's id.
  A game's name appears only there, never in shared code. The private folder's files are
  read the same way (`Appearances::all`), and say which private games are shown
  (`private_games`); with `BINGLE3D_PRIVATE_DIR` set, `cargo test` checks them too.
- `src/board.rs`: the watched game's board and pieces, from the game definition's board kind and
  layout coordinates and the position's cells; names no game. Each kind of piece is its
  appearance's model, or the generic stone: stood by the model's foot, and what is on it in a
  stack raised by the model's height (`assets::model_span` reads both from the file, so nothing
  else says how tall a model is). The board is one slab
  (`src/slab.rs`: its outline, rounded rim, grid lines, and the shading of a goal's cells,
  geometry with no systems) with the cells cut into it (a slab for each board of a game on
  several side by side, the API's `boards`: Alice chess's two, set apart by `Layout::of`, all
  else keeping clear of the whole), or, for a game whose appearance says
  `on = "points"` (`Surface`), lines cut into it and the pieces on their crossings, or for
  `on = "table"` no board at all: the cells unseen on the table, raised to the top of what
  stands on them (`raise_cells`), the camera and the supply kept to the middle (`table_frame`); `BoardWood` is its wood, chosen by `src/wood_choice.rs`
  (stopgap buttons), and the materials made of it are built again when it changes.
  `src/look.rs`: generic defaults for how things look (a seat's paint in the standard look, the woods, the stone),
  and `MaterialSpec`, a material as data (its `wood` may be `board`, the board's, or `contrast`,
  one that stands out against it: an inlay; `metallic` makes it a metal); a wooden mesh needs texture coordinates in metres
  (`world_uvs`). Do not read `shape`, `style` or other fields ADR 0124 moves to `web.yaml`: they
  are leaving the API. Each seat's `connect` goals are found along their regions' outer sides
  (`goal_strips`): the rim there is stained, and the cells shaded, in its color; a goal every
  seat shares (Tak's roads) is nobody's to color, and is not marked. The board also
  marks the positions a click would be taken on `highlight::Lit` (always while a piece is in
  hand), each with its `Outline`, and shows the
  move being built: what it places as see-through pieces (solid once the move is sent), and a
  piece it moves held over its space or carried to where it goes. **Anything in hand is lifted
  over where it came from and bobs there, and the pointer stays a hand; a click never puts
  anything under the pointer** (plan, 2026-11-27): what a move aims (Amazons' arrow) is only
  shown see-through where it would land. `src/hand.rs` moves them every frame (a piece held
  bobs; one put down goes across, then is lowered: `travel`); `hand::Held`, something under the
  pointer in place of the pointer's hand, waiting over the board's edge where the pointer is
  beyond it (`hand::within`, `BoardEdge`), is what a drag holds (`board::hold_dragged`: the
  piece in hand, out of picking's way while it is held). Where every click
  places one stone (Go, Hex, Othello: `Turn::auto_hand`), the stone waits over its bowl by
  itself and is carried from there to where it is placed; with no bowl, nothing is held. The pointer's place is read
  from a board's `Spot`s and from anything marked `hand::Surface` (a card, the cloth).
- `src/cursor.rs`: the pointer, which is the client's own: the system's cursor is never shown
  over the window. A small glove of capsules (`pose`, `bones`: no Bevy systems), drawn by a
  camera of its own (`scene::Apart`, so it gets none of the table's light) into an image shown
  at the pointer over every panel: open over nothing, palm down to pick up over what a click
  picks up, pointing over what a click chooses (a button, here); over a field that takes typing
  it gives way to a text cursor, an I-beam of nodes (`beam_parts`). What a click on the table
  would do is the table's to say: a system in `cursor::Sense` reads `Under` and writes
  `TableFeel` (`play::feel` for the board, `card_table`'s `feel` for cards), deciding as its
  click would. Hidden while anything is held under the pointer (`hand::Held`: only what a drag holds), but over the interface (never
  by whether the pointer is on the board, which flickers from cell to cell), and not added in a
  headset.
- `src/patience.rs`: a patience (Bingle's ADR 0122), which any game may declare
  (`patience::plays`: it has cards and actions of the kind `patience`): the vocabulary's piles
  by their zones (`t1`…, `f1`…, `c1`…, `stock`, `waste`, `done`), its gestures (`from-t3-2`,
  `to-t5`, `to-f`; the waste is `w`), where each pile lies on the table (`Layout`: a row of
  the stock, waste and free cells left and the foundations right, beyond the columns, which
  fan towards the player), which gesture a click makes (`click`), and the foundation a card
  goes home to when the view lists the suits (`home`). No Bevy systems in it.
  `src/card_layout.rs`: where a card game's piles lie and each card of a pile (`Layout`:
  squared, fanned, or down towards the player); a patience's from its vocabulary, any other
  game's where its look puts them (`Layout::placed`), and a game of several seats' round the
  table (`Layout::seated`): a zone each seat has in front of its seat, on the seat's own
  terms (`seat_turn`, `from_seat`: the viewer's seat on the player's side, play going left),
  a hand `Held` (the viewer's fanned up to them, another's standing in the air, its back to
  the table), a trick `Played` (each card towards who played it) and a `Row`.
  `src/card_table.rs`: a card game's cards on a cloth in place of its board (the board stands
  aside: `sync_board`), for a patience or a game whose look has `[cards]` (`on_table`), from
  the position's card zones (`Position::card_zones`: a declared game's own, else the view's
  `table`) and the faces of the usual deck (`props::standard_face`). A game laid by its look
  plays by the choice its look gives each pile (`look_click`), or, round a table of several seats, by a click on a card of the
  player's hand (`[cards] play`, `card_choice`: the cards that may be played stand out of the
  fan, `stand_out`), the trick's cards laid towards who played them by the view's captions
  (`caption_seat`), a pile shown `unless` another has cards (`piles`), seen from the seat
  this client plays (`viewer`). Choices made `together` (a
  rack's pins) are chosen in any order, kept by the client (`Together`) and hover; the piles
  that would play exactly them (the balls) glow, and a click there sends the move's gestures
  in the server's order (`throw`, `Turn::enter_all`). In a patience, a click on a card a move starts with lifts it and
  those on it: they hover over their pile, the pointer staying a hand (an anchor they would
  follow under the pointer is `hand::Held` only for a drag); the piles they may go on glow, the one under the
  pointer shows them see-through where they would land, and a click there plays the move at
  once (`Turn::clicks_only`). Or they are dragged, by `drag.rs`'s mechanism: a press on a
  card or a pile's place is a `drag::CardPress` (`on_press`, `follow_cards`), the pointer
  leaving it lifts what a click would and holds the anchor under the pointer (`hold_cards`),
  let go over a pile a click would put them on it is that click (`click_pile`, the one
  function for a click there, which every click on a patience's cards is), anywhere else the
  turn is as it was. Cards go where the position puts them by `hand::Carry::Put`,
  one after another, turned over on the way when they were face down where they came from.
- `src/choice_grid.rs`: the view's board of choices (lines of boxes), for a game whose look
  has `[grid]`: a board of wood on the table, a tile for each box with the look's picture for
  it; a click on a box makes its choice, a box a seat crossed has that seat's small marker on
  it (carried there from the seat's own), a box out of the game is covered, the one left on a
  line is rimmed. A box that may be crossed is lit while the pointer is over it and the hand
  points at it (`light`, `sense`); the pointer over a box or a line's head shows a card of
  words beside it, what the view says it means (`explain`: its `label` and `hint`). `read`,
  `places`, `words` and `card_at` have no Bevy in them.
- `src/seat_markers.rs`: a piece that stands for each seat in front of it, for a game whose
  look has `[markers]` (`Marker`, dressed by `dress` wherever one is put): of its side's
  material once the view puts seats on sides, the look's `seat_parts` in its seat's color, a
  ring round the one the view names. Clicks on them make the choices that name seats (a team
  picked by its members: `choices`, `click`, no Bevy in them; those chosen so far are
  `Picked`, kept by the client).
- `src/sounds.rs`: the sounds a seat makes as it plays (a piece picked up, set down or
  captured, the move sent, the game won or lost), the same for every game (`Cue::default_file`) unless its
  appearance gives others (`[sounds]`). `cues` and `ended`
  read them from the turn's and the table's changes, with no Bevy in them; a carried piece sounds as it lands (`hand.rs`). The files are synthesised
  by `assets-src/sounds/sounds.py` (Python's standard library and ffmpeg, on the host), never
  recorded. `--no-audio-devices` leaves Bevy's audio out, so no speaker is opened.
- `src/props.rs`: a playing card, a die, and a two-sided disc and its ring, as the pipeline
  makes them: a model's parts by name (a card's `face`, `back` and `edge`; a disc's `light` and
  `dark`; a ring's `piece` and `trim`), a card turned over, a die turned to show a number, a disc showing either side,
  and the picture on the face of a card of the usual deck, found by its suit's symbol and its
  rank's label (`standard_face`; the usual 52, and `1`, `11`, `12` and `13` for a deck
  numbered to thirteen, are the pipeline's, `card_faces.py`).
  A patience's cards are drawn with them (`card_table.rs`); no game's dice, discs or rings yet. `src/showcase.rs`: `--showcase`, every
  piece model the appearance files name, with discs and rings, cards and dice, laid out to look at; it names no
  game either.
- `src/play.rs`: playing a seat (Enter plays, Backspace or a right-click goes back; Esc is the
  escape menu's).
  `Turn` is the move being built from the server's legal selections, narrowed by gestures, with no Bevy in it
  (`in_hand` is a piece taken to place; `may_end` a move a one-seat game may send part of;
  `stack_moves` a stack picked up, by its gestures: a position, a count, a position for each
  piece dropped; a stack clicked is taken whole; `lifted` the piece every click moves, where
  its selections do not have it clicked (Courtfall's activated unit), picked up by a click on
  it and lifted over its hex; `clicks_only` a turn where nothing is chosen for the player
  and a whole move plays itself, a card game's; `enter_all` a whole selection's gestures at
  once, put together by the player in an order of their own; `preview` the position after the
  selections made, as the server's answer has it (`Selections.state`): what its view asks next
  is the panel's words, what it stamps is stamped, and a piece of two sides it turns over is
  shown turned, until the move is played or taken back, and one it then takes away as the
  last preview to have it had it: `previews`; `lines` the selections chosen by what they take,
  each clicked only where it takes and taking more (a line of discs removed, named by its
  ends): the one through the piece under the pointer whose middle is nearest it is aimed at
  (`nearest_line`, `LineAim`, set by `board::line_aim`), its pieces lit, and a click there
  makes all its clicks at once); its systems turn clicks on a `Spot` (a board cell or a
  piece on it, a press and a release that `drag.rs` reads), keys, the wheel (`Turn::wheel`: how many of a stack are carried, or another
  kind of piece of the same pool in hand; the camera leaves it alone then, `camera::WheelTaken`)
  and the panel's buttons into gestures and commands. Picking is Bevy's mesh
  picking with markers: only what is `Pickable` is picked, through a `MeshPickingCamera`.
- `src/drag.rs`: a piece on the board dragged: pressed, carried under the pointer while the
  button is down, let go where it goes; and what lies beside it (a `drag::Source`: a bowl, a
  supply, the bowl of fences, the stock, what waits in hand over them), taken up as the pointer
  leaves it and let go as the click on the board there (`let_go_beside`). **The one place that knows a drag is under way**
  (`Drag`: the press, the turn as it was before it, whether the pointer has left the space
  pressed, what is carried). Every click on the board's spaces is a press and a release here
  too, made by `play::click_at`, the one function for what a click there does; `on_click` in
  `play.rs` keeps what lies beside the board. A press becomes a drag when the pointer leaves
  the space pressed (`Turn::grabs`: what a press there may carry); let go over a space the
  piece goes to (`Turn::carries_to`) it is the click there, on its own space it stays lifted,
  anywhere else (or a right-click, Backspace, the escape menu: `cancel`) the turn is as it was.
  Bevy's `PointerPress` and the pointer's own `PointerInput`, so a headset's ray drags as the
  mouse does. Where a piece stands part-way through a chain is `Turn::legs` and
  `Turn::stands_at` (each selection's `path`), the piece in hand on the board `Turn::held_at`,
  a Tak stack carried `Turn::stack_at`.
- `src/coordinates.rs`: the board's coordinates along its edges (each board's own, for a game
  on several), toggled with C, from position names that are letters then a number
  (`edge_labels`, no Bevy); none for any other naming.
  Engraved in the board's margin, from the glyph atlas the pipeline renders (`engraving.py`;
  its order and grid are in both places).
- `src/highlight.rs`: how a lit position looks, which the board does not know: today a glow
  (`Effect::Glow`), a line of light traced round the outline and short streaks rising from it,
  drawn by a shader of our own (`src/highlight/cell_glow.wesl`, compiled in as
  `embedded://shaders/cell_glow.wesl`; Bevy 0.20's shaders are WESL), its light added to the
  picture; a point gets a ring small enough to read as a dot, or, with a piece on it, a ring
  round the piece at its top (`board::point_outline`). A lit position's `Tone` (the
  board's to say) is plain, or red where a click takes the piece standing there
  (`Turn::takes_at`, from API 1.10's `captures`: Fist's pin). What the move being built takes
  (`Turn::taking`: a pin chosen, a jump's pieces captured) is see-through until the move is
  played (`board::show_taken`). Tuned twice, for a camera that
  blooms and one that does not (`scene::BloomOn`: off in a headset, and with `--no-bloom`). **Another effect is
  another `Effect` and its arm in `light`**, nothing else. The outline's geometry (`walk`, `ring`, `ribbon`) has no Bevy systems in it.
- `src/toggles.rs`: stopgap buttons and keys for the table's settings: coordinates (C) and
  lighting the legal moves (L, `board::ShowLegalMoves`, off by default; `--legal-moves` starts
  with it on).
- `src/camera.rs`: the orbit camera, which also pans (middle-drag, Shift with right-drag, W A S D). The left mouse button is kept for picking; nothing else reads
  the mouse directly, and nothing assumes there is exactly one camera. How far back it goes is
  the environment's (`Reach`); `--view` puts it somewhere for a picture (`StartView`), looking at
  the table's middle or at a place on it.
- `src/hud.rs`: text over the scene. Text that is only words is `Pickable::IGNORE`, so the wheel
  and clicks reach the table through it; the camera leaves the wheel to any UI node under the
  pointer.
- `src/call.rs`: calls between friends. `Call` is where one stands (the page's `js/dm-call.js`,
  with no Bevy and no sound), returning the `Effect`s to carry out; `DirectCalling` holds it and
  the voice engine, and turns them into requests. Its default opens no devices; only the plugin
  chooses them (`--no-audio-devices` keeps them off, for runs like an agent's).
- `src/voice_chat.rs`: a game's voice chat, on the game's panel: one connection per seat in the
  call, the higher seat offering. Joining, the heartbeat and the call's own socket
  (`only=voice`) are the network thread's (`VoiceLink` in `net/mod.rs`); `changes` and `role`
  have no Bevy in them. Its default opens no devices, as `DirectCalling`'s.
- `src/voice/`: calls (stage 8), with no Bingle in it. `Engine` is a call: **a thread of its own**,
  the one exception to all network I/O being on the network thread (a frame of sound must leave
  every 20 ms; plan, decision 7). It is told who is in the call, who offers, and what the other
  ends signal (`Command`), and says what to signal back (`Event`); carrying those through the
  server is the network thread's. `peer.rs` is one person: a UDP socket and rtc's peer
  connection, following what Bingle's page does (`docs/voice.md` there), so a browser can be on
  the other end. `signal.rs` is the messages in a browser's form; `audio.rs` the microphone's
  buffer, the mix (keeping a copy of what it played) and a resampler, with no devices; `echo.rs`
  the microphone's sound cleaned as a browser cleans it (echo cancellation, noise suppression,
  gain), with what was played as the reference; `devices.rs` cpal's microphone and speaker;
  `relay.rs` a TURN relay
  for one connection, on its socket (rtc's TURN client), `fake_turn.rs` a TURN server for the
  tests; `check.rs` `--voice-check`. Tests connect two engines over loopback with no devices: **never
  open a real microphone or speaker from a test or an agent's run** (the toolbox reaches the
  host's sound): run clients with `--no-audio-devices`.
- `src/vr.rs`: the table in a headset (`--vr`, only in a build with the `vr` feature: OpenXR
  through `bevy_mod_openxr`). The plugin makes a camera for each eye under its tracking root;
  `seat_root` (no Bevy systems, tested without the feature) puts the root so the head is at a
  sitter's eye point. An eye is marked `scene::Headset`: lit, with none of the flat camera's
  effects. A controller is a `RayPointer` at its aim; the four things asked of one (`Input`)
  and what is suggested for each kind of controller (`BINDINGS`) are data.
  `src/ray_pointer.rs`: a pointer that is a ray in the world, as one of Bevy's pointers: its
  own ray cast against what is `Pickable`, its trigger the primary button, so clicks reach
  `play.rs` as the mouse's do, and points at a `Panel`, a UI drawn into an image and hung in
  the world (compiled and tested without the feature). `src/world_panel.rs` hangs one: a
  camera drawing UI into an image, and the quad showing it. `src/vr_panels.rs`: the headset's
  menu, opened by `vr.rs` in front of the player; the game's panel, given to a panel beside
  the table while a session runs (`InHeadset`); and every other UI root of the window's,
  given to a screen across the table then; the escape menu has a panel of its own. **A new window UI root goes to that screen in a
  headset by itself**; one with a `UiTargetCamera` of its own does not. `src/vr_keyboard.rs`:
  a keyboard in the world while a field that takes typing has the focus, sending
  `KeyboardInput` as the real one does. **Run `cargo clippy --all-targets
  --features vr -- -D warnings` too when touching it**: CI does not build the feature. **A
  build with it starts in the headset by itself when a VR runtime is running**
  (`vr::runtime_running`), except in a run `--frames` or `--screenshot` limits: pass
  `--desktop` to any other run. An agent cannot see into the headset: starting `--vr` with
  SteamVR running draws in the headset the maintainer may be wearing, so say so first; with
  `XR_RUNTIME_JSON` pointing at a manifest of a runtime that is not there, it only tries the
  fallback.
- `src/run_limit.rs`: `--frames` and `--screenshot`.
- `src/gpu_memory.rs`: what the renderer's allocator is asked (wgpu's `MemoryHints`): blocks
  too large for the 256 MB window a card without a resizable BAR maps for the processor, so
  uploads wait in system memory. In that window, once it is crowded, a write runs hundreds of
  times slower and the client draws 10 frames a second (plan, decision 15). **When the client
  is slow, run it with `--frame-times` on the maintainer's desktop before changing the
  scene**: under gamescope the same build may be fast.
- `src/frame_pace.rs`: the most frames a second the window draws (60 by default; `max_fps` in
  `config.toml`, `--max-fps`; stepped by *Settings, Graphics*, `next_limit`; a run a script limits
  ignores the config's): each frame ends waiting for the next to be due. Not in a headset,
  whose runtime paces it to the headset's display: there `vr_refresh_rate` (`--vr-refresh`) is
  the rate asked of the headset itself (`HeadsetRate`, asked by `vr.rs` as the session starts,
  where the runtime has `XR_FB_display_refresh_rate`), never the PC monitor's.
- `scripts/release-build.sh`: a release for one platform (Linux, Windows, the Steam Frame), as
  `.github/workflows/release.yml` runs it on a tag (plan, decision 11). The Frame's is
  cross-built against Valve's sysroot (`scripts/frame-sysroot.sh`); `scripts/openxr-loader.sh`
  builds the OpenXR loader a Linux build ships in `lib/`. `scripts/release-json.sh` writes the
  release's `release.json` (the version and `API_VERSION`), which a server that offers the
  builds reads. **Cutting a release is pushing a tag: only when the maintainer asks for one.**

## Dependencies and Bevy

- **Keep the dependency list short**, as Bingle does. Say why before adding a crate. No async runtime
  (networking runs on its own thread with blocking clients); `rustls` with *ring*, never OpenSSL or
  aws-lc-rs (which needs cmake and nasm on Windows). Today: `bevy`, `ureq` (pinned to 3.5, for the
  `unversioned` transport API `src/net/tls.rs` uses), `rustls`, `ring`, `webpki-roots`, `serde`,
  `serde_json`, `toml`, `tungstenite` (only its `handshake` feature: TLS is ours), and for voice
  `rtc` (its `crypto-ring` feature only), `rusty-opus`, `cpal` (which Bevy's audio already builds),
  `bytes` (rtc's packets) and `sonora` (WebRTC's audio processing in Rust: echo cancellation); and, only
  with the `vr` feature, `bevy_mod_openxr` and `bevy_mod_xr` (by git revision until released for
  this Bevy: plan, decision 10) and `openxr`, which they already build.
  Bevy's `jpeg` feature reads the scanned textures.
- **The API's schema**: `schema/` holds the JSON Schema of the API version the client targets
  (`API_VERSION` in `src/net/api.rs`), copied from Bingle's `docs/api.schema.json`, and a test reads
  the smallest and fullest messages it allows with the client's types. When the client starts using
  something a newer minor version added, copy that version's schema and raise `API_VERSION` with it:
  the client then refuses servers older than that. A new major version is a deliberate upgrade.
- **Bevy is pinned to one minor version** (0.20). An upgrade is its own pull request. Bevy's API
  changes every release, so check a type or field in the crate's source under
  `~/.cargo/registry/src/` rather than from memory; the compiler's suggestions are usually right.
- `rust-toolchain.toml` pins the compiler for every machine and CI; `rust-version` in `Cargo.toml`
  is Bevy's minimum.
