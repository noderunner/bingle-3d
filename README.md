# Bingle 3D

A native 3D client for [Bingle](https://github.com/noderunner/bingle), the server for playing
abstract strategy games, built with [Bevy](https://bevyengine.org/) in Rust for Linux and Windows.

Bingle already has a browser UI. This client plays the same games through the same public
REST/WebSocket API, and draws them as a table in 3D. It has no rules of its own: the server
says what the position is, what the legal moves are and who is to move, and the client decides
only how that looks ([ADR 0124](https://github.com/noderunner/bingle/blob/main/docs/decisions/0124-the-server-says-what-things-are-clients-decide-how-they-look.md)).

> **Status: stage 4 of the [plan](docs/plan.md).** The app connects to a Bingle server, lists its
> open games and the games it can play, and shows any game being played, live, on a table in 3D:
> two of the server's bots, or a game in progress. A game can be played against the server's bots;
> resigning, joining a game by its room and playing a person come next.

> **AI-generated code.** As with Bingle, most of this project is expected to be written by an AI
> assistant (Claude, by Anthropic) working with the project's author, who directs the design and
> tests the result. Review it as you would any code you did not write yourself.

## How it fits together

```
 bingle-3d (this repo)                       bingle (Go server)
 ┌──────────────────────────────┐            ┌──────────────────────────────┐
 │ Bevy app                     │  HTTPS     │ /api       REST: game defs,  │
 │  ├─ scene: board, pieces,    │◄──────────►│            games, selections,│
 │  │   camera, picking         │            │            moves             │
 │  ├─ net: HTTP + WebSocket    │  WSS       │ /ws/{code} live move and     │
 │  │   on its own thread       │◄───────────│            state events      │
 │  └─ appearance: defaults,    │            │                              │
 │      optional per-game files │            │ rules engine, bots, SQLite   │
 └──────────────────────────────┘            └──────────────────────────────┘
```

- **The server is the authority.** The client never decides whether a move is legal: it asks
  for the legal selections and narrows them as the player clicks, as the web page does.
- **Any game can be drawn from its meaning alone**: board positions and adjacency from the game
  definition, a palette by seat, a shape by piece type. A game may later get a hand-made
  appearance in this client, but never needs one.
- **No game is named in shared code.** Game-specific appearance lives in per-game files, so
  games kept out of public repositories (Bingle's private overlay) can bring their own.

## Downloads

Builds for Windows, Linux and the Steam Frame are on the
[releases page](https://github.com/noderunner/bingle-3d/releases): unpack one and run the
program inside, no compiling needed. All have VR in them: they start in the headset when SteamVR
is running, and in a window otherwise.

- **Windows**: run `bingle-3d.exe` (the program is unsigned, so Windows may warn: *More info*,
  *Run anyway*).
- **Linux** (x86-64, glibc 2.35 or newer): run `./bingle-3d`. The OpenXR loader is in `lib/`
  beside it.
- **Steam Frame**, on the headset itself with no PC: see [below](#on-the-steam-frame).

## Building

You need [rustup](https://rustup.rs/); `rust-toolchain.toml` picks the compiler (Rust 1.100), and
rustup fetches it on the first build. The first build compiles Bevy and takes several minutes;
later ones take seconds.

```sh
cargo run                                   # a window with the scene
cargo run -- --frames 120 --screenshot a.png  # render 120 frames, save a PNG, quit
cargo test                                  # unit tests, no window needed
cargo clippy --all-targets -- -D warnings   # as CI runs it
```

Right-drag (or the arrow keys) orbits the camera, middle-drag or Shift with right-drag (or W, A,
S, D) pans it across the table, and the wheel zooms (or scrolls the panel, over it). C (or the
*Coordinates* button) shows the board's coordinates, engraved along its edges. F5 connects again.

The client starts on a login screen: the server's address, with a light beside it once it has
been checked (when you leave the field, or press Enter): green when the client can use the
server, yellow when a Bingle answered but its version and the client's do not go together (one
of them needs updating), red when nothing usable answered. Then *Log in* with a username and
password, or *Continue as guest*, with a guest key you were given before (optional: left empty,
the one kept for that server is used, or the server gives a new one). *Auto-login* skips this
screen at the next start, with the session or guest key kept (never a password); *Logout* in the
escape menu (Esc) comes back to it and turns auto-login off.

Once connected, the main screen is laid out as Bingle's web page: tabs (only *Games* works yet;
*Puzzles*, *Ratings*, *Tournaments* and *Global Chat* are shown greyed), and in *Games* a choice
of *New* (a card for each game, with its picture, marks such as *Beta* or *Prototype*, and how
many play it; click one to set it up) or *Active* (the games being played or waiting for
players). Esc opens a menu, on the main screen or in a game: *Resume*, *Main Menu* (back to the
main screen from a game; the game goes on), *Logout*, *Quit* (to the desktop), and *Settings*,
*Game Options* and *Profile*, not built yet.

*Play a bot* on a game in the list starts a game in which you have the first seat and the server's
strongest bot the others. When it is your move, a see-through stone follows the pointer where a
click would be taken (L, or *Legal moves*, also lights every such place): click one, and the
stone is shown there; click it again, press Enter or choose *Play* to play it, or right-click
or press Backspace (or *Back*) to take it back, one click at a time. A piece that moves is
picked up with a click and hovers over its space, the places it may go lit; a click on one
carries it there and sets it down (in Amazons the arrow is then held under the pointer).
In a game where each turn is one stone placed (Go, Hex, Reversi and the like), the pointer is
that stone, floating over the board; over a panel it waits at the board's edge. Picking up,
setting down and playing each make a sound, in every game, and a crowd cheers when
you win and groans when you lose (none with `--no-audio-devices`). A choice that is not a cell, such as
swapping sides in Hex, is a button on the panel. The client only offers what the server lists as legal, and sends
the move by its notation. *Watch* on a game of your own in the list (one you left) takes your seat
again.

*New game…* on a game in the list sets one up: your seat, who plays each of the others (a person,
who joins later, or one of the server's bots), and the time (no clock, a game clock such as
5 min + 3 s, or a time for each move). While a game waits for people the panel shows the room code
(and the game's web address) to send them. In a timed game each seat's time is shown beside it and
counts down. *Join* on a game in the list
that is waiting for players takes a free seat in it. Your name is `name` in `config.toml`
("Player" by default); if somebody at the table already has it, you join as "Player 2". *Resign*
gives up a game you are playing, after asking once more.

The games being played are read again every few seconds while the list is open. *Only with a free
seat* narrows it to games you could join, and *Find games* on a game to that game in every size.

*Watch two bots* on a game in the list starts a game between two of the server's bots and shows
it as they play; *Watch* on an open game shows that game. From the command line:

```sh
cargo run -- --watch-bots hex-11           # a new bot game of Hex (11×11), by its id
cargo run -- --watch 7ZKMXP                # the game in this room
cargo run -- --play-bot hex-7              # a new game of Hex (7×7) against the server's bot
cargo run -- --host hex-7                  # a new game of Hex (7×7), waiting for a person
cargo run -- --join 7ZKMXP                 # a free seat in this room's game (or watch it)
cargo run -- --new-game hex-7              # the form that sets up a new game of Hex (7×7)
cargo run -- --sign-in                     # the login screen, even with auto-login on
cargo run -- --friends                     # the friends panel open (signed in)
cargo run -- --talk ana                    # the conversation with the friend @ana
```

In a game you play, *Join the voice chat* on the game's panel talks with the others at the table,
in the client or on Bingle's web page: each has a volume, and there are *Mute*, *Deafen*, push to
talk (hold V), the microphone and speaker to use (*Devices*), and *Leave*. `--voice` joins at
start.

Signed in, *Call* beside a friend who is online rings them, in the client or on Bingle's web page;
the call shows at the top of the friends panel, with *Answer* and *Decline* when somebody rings
you, and *Mute* and *Hang up* while you talk. It uses your default microphone and speaker. What
your microphone picks up of the speaker is taken out (echo cancellation, as a browser does, with
the background noise lowered and the level evened out), so speakers work; it takes a few seconds
to settle at the start of a call. `--voice-check` tries them: for ten seconds you
hear yourself, through a voice connection on your own machine, with no server and no window. It
lists the devices it can see.

Players connect directly where their networks allow it. Where one does not (a strict NAT, some
mobile and office networks), the call goes through the server's TURN relay, when the server has
one (Bingle's `BINGLE_RTC_TURN_URLS`; relays reached over UDP for now). `--relay-only` sends
every call through the relay, as a browser's `iceTransportPolicy: "relay"`: to try it, or to keep
your address from the others in the call.

```sh
cargo run -- --host hex-7 --voice          # host a game, and join its voice chat once seated
cargo run -- --call ana                    # ring the friend @ana once she is online (or answer her)
cargo run -- --voice-check                 # hear your microphone through your speaker
cargo run -- --host hex-7 --voice --relay-only   # the same call, only through the relay
```

Any game is drawn from what the server says about it: a hex board's cells as hexagons, a grid's
as squares, cut into one slab of wood on a felt table, lit by a real room's light; pieces as
stones in their seat's colour, and the edges a seat must connect (its `goals`) with the board's
rim stained and the cells along them shaded in its colour. The buttons at the bottom right
choose the board's wood: teak, walnut or cherry.
A game may have a look of its own (`assets/appearance/`): Hex is played with slate and
clamshell stones, and Go, Gomoku, Pente and Meridians with the same stones on the points where
the board's lines cross, each player's in a wooden bowl beside the board. Tak has its blocks,
walls and capstones, stacked as they are played, and what each player has left stands beside
the board at their right hand: click it to take a stone (the wheel turns it from flat to
standing), then click where it goes. A stack clicked is picked up whole; the wheel puts its
stones back down one at a time, and each click then drops one where it goes. Fendo has pale
and orange pawns, the ones each player has left in stock standing beside the board, and dark
wooden fences on the squares' sides: click a pawn in stock to add one, or move a piece; the
pointer then becomes a fence, which shows where it would go as it comes near a side of the
piece's square (the wheel goes round them too), and a click puts it there. To build without
moving, click the piece, then the bowl of fences both players share.
Draughts wooden checkers; Breakthrough,
Brandubh and Fist turned chessmen; and Amazons its queens on a chessboard of pale and dark
wood with a dark border, and red checkers for arrows. Blooms' stones are polished minerals (red
jasper and tiger's eye for Warm, lapis lazuli and slate for Cool), each colour in a small wooden
bowl beside the board: click a bowl to take a stone, then click where it goes. What each player
captures is carried to a small wooden tray in front of them, with a square for every capture the win
needs. The client needs a Bingle whose API is 1.10 or newer. Games a server builds from its private
overlay (commercial games, marked `private`) are not shown: they are for a private build of the
client.

Where the table stands is yours to choose, and nobody else's business: the server knows nothing
of it, and each player at a table may be sitting somewhere else. *Place*, at the bottom right,
chooses between the plain *Table* and the *Garden*: a platform of tatami among raked gravel,
rocks on moss, a pond and a maple in autumn, with a cushion for each seat of the game. The
choice is kept (`environment` in `config.toml`); `--environment garden` chooses for one run.

```sh
cargo run -- --environment garden --play-bot go-9x9
cargo run -- --environment garden --view 60,15,2 --frames 200 --screenshot garden.png
```

`--view YAW,PITCH,RADIUS` puts the camera somewhere for a picture: degrees round the table (0 is
your side), degrees above it, and metres from its middle. Two more numbers, `,X,Z`, aim it at a
place on the table instead: metres to the right of the middle, and towards you.

The positions a click would be taken on glow: a line of light traced round each, with short
streaks rising from it. `--legal-moves` starts with every such position lit, and `--no-bloom`
without the camera's bloom, as a headset sees the table.

The window draws at most 60 frames a second, which a board game does not need more than, so the
GPU and its fans can rest. `max_fps` in `config.toml` changes it (0 draws at the monitor's pace),
and `--max-fps N` for one run. On a 144 Hz monitor, 72 keeps in step with it.

`--showcase` lays every piece's model on the table, with the dice and a few playing cards of the
usual deck, and nothing else: to look at them. `--card-face FILE` and `--card-back FILE` try a picture (a JPEG or
a PNG, 5 to 7) on the cards' sides.

```sh
cargo run -- --showcase --view 15,38,0.5
```

**In a headset**: the place and the table at their real size, controllers that point and click,
the login and main screens on a screen across the table, the game's panel beside it, a menu,
and a keyboard. Build with the `vr` feature. A build with it starts in the headset by itself when
SteamVR (or Monado, or WiVRn) is already running, and in the window otherwise; `--vr` starts in
the headset whatever is running (which starts SteamVR), and `--desktop` stays in the window.

```sh
cargo run --features vr -- --environment garden --play-bot go-9x9
```

What it needs besides the usual build:

- **On Windows, CMake**, since the OpenXR loader is built from source there:
  `winget install Kitware.CMake`, then a new terminal.
- **On Linux, the OpenXR loader** (`libopenxr_loader.so.1`; Fedora's `openxr-libs`), at run time.
  A release has its own in `lib/`.
- **SteamVR set as the OpenXR runtime** (in SteamVR's settings, under OpenXR).

You are sat at the table when the headset is
first tracked; press Home in the window to be sat again from where your head is. The window
keeps the interface. Without a runtime the client says so and runs in its window alone.

Each controller casts a thin beam. Pull the trigger on a cell or a piece to click it, as the
mouse does. The lower face button (A, or X on the left) plays the move being built, as Enter
does, and the upper one (B or Y) goes back. The left stick walks you ahead or back and slides you
sideways, and pressing it sits you at the table again; the right stick turns you and lifts or
lowers you, and pressing it opens the escape menu (main menu, logout, quit) in front of you.
The left controller's menu button (the Steam Frame's *View* button) opens a menu in front of
you: play the move, go back, sit again, show the legal moves or the coordinates,
change the place, quit. The login screen and the main screen stand across the
table as a screen the size of the window; in a game, its panel (the players, whose move it is,
the clock, its buttons) stands at your front left instead. With the right hand's beam on one of
them, the right stick scrolls it. While a field that takes typing has the focus, a keyboard
stands before you; *Hide* puts it away. All of it is back in the window when you leave the
headset. SteamVR's controller bindings can change all of these.

On the Steam Frame, the client asks SteamVR for the Frame's own controllers (Valve's
`XR_VALVE_frame_controller_interaction`), so the right *Menu* button opens the escape menu and
the left *View* button the menu; A and B play the move and go back, as the D-pad's bottom and
top do.

### On the Steam Frame

The Frame runs Linux on arm64, and a build for it is made on an x86-64 Linux machine against
Valve's Steam Runtime 4 arm64 SDK (its files, about 750 MB, fetched once into `target/`). It
needs clang and CMake (in the toolbox: `sudo dnf install cmake`).

```sh
rustup target add aarch64-unknown-linux-gnu
bash scripts/frame-sysroot.sh                 # Valve's files, checked by their SHA-256
bash scripts/release-build.sh steam-frame     # into target/dist/bingle-3d-steam-frame/
```

To put it on the headset:

1. On the Frame, *Steam Settings > System > Enable Developer Mode*; then, under *Developer*,
   *Set User Password*.
2. On the PC, install the **SteamOS Devkit Client** from Steam (it is under *Software*). On the
   Frame, *Settings > Developer > Pair new host*; in the client's *Devkits* tab, *Register* it.
3. In the client's *Title Upload*: a name, the folder `target/dist/bingle-3d-steam-frame` (or a
   release's unpacked), start command `bingle-3d`, and the runtime **steamrt4 arm64**. Valve's
   Frame pages say "Sniper": that arm64 runtime was retired, and on the Frame "Sniper" is the
   x86 one, which runs the program through an emulator that refuses it, silently.
4. Upload. It is then in the Frame's library under *Non-Steam*, started from the Steam menu.
   Give it `%command% --vr` as its launch options.

Its log is Steam's, on the Frame (`ssh steamos@frame`, then `~/.local/share/Steam/logs`).
It is slow on the Frame for now: the scene was made for a desktop graphics card.

### Releases

A release is cut by a tag, never by a merge: set `version` in `Cargo.toml` (in a pull request),
and once that is merged, tag main's commit `v` and that version (`v0.0.1`) and push the tag.
[The release workflow](.github/workflows/release.yml) builds the three archives with
`scripts/release-build.sh` and publishes them as a GitHub release; it refuses a tag that
disagrees with `Cargo.toml`. A pull request that changes the release's scripts runs the builds
without publishing.

The models, textures and light are made by Blender, run from scripts, from CC0 sources; how to
make them again, or add to them, is in [assets-src/README.md](assets-src/README.md). Building
the client does not need Blender: what the scripts make is committed.

## Connecting to a server

The client connects to `http://127.0.0.1:8080` unless told otherwise, which is where a Bingle
built from source listens (`make build && ./bin/bingle` in a checkout of Bingle).

```sh
cargo run -- --server localhost:9000             # another server, for this run only
cargo run -- --server https://games.example.com
```

To change it for good, set `server` in `config.toml`, in `~/.config/bingle-3d/` on Linux
(`$XDG_CONFIG_HOME/bingle-3d/`) or `%APPDATA%\bingle-3d\` on Windows:

```toml
server = "https://games.example.com"
name = "Ana"    # the name you sit at a table under ("Player" if not set)
```

- **Plain `http://` only reaches this machine.** Any other server must use `https://`, as Bingle
  itself requires ([ADR 0065](https://github.com/noderunner/bingle/blob/main/docs/decisions/0065-the-connection-must-be-secure.md)).
  An address without a scheme gets `http://` for this machine and `https://` otherwise.
- **A server with its own certificate** (`BINGLE_TLS_SELFSIGNED=1`) is not trusted at first. The
  client shows the certificate's SHA-256 fingerprint; compare it with the one the server printed
  when it started, and if they are the same, choose *Trust this certificate*. That certificate,
  and only that one, is trusted for that server from then on (it is kept in `config.toml`).
- **A guest key** is made the first time the client meets a server and kept in `keys.toml`,
  beside `config.toml` and readable only by you, so the server knows it is the same guest next
  time. Delete the file to start again as a new guest.
- **Logging in** to an account (on the login screen, or *Sign in* under the server's status) keeps the session in
  `keys.toml` too, so the client stays signed in between runs until *Sign out* or *Logout*. Your games as a
  guest on that server become the account's. Accounts are made on the server's web page.
  Signed in, the *Friends* panel on the right lists your friends and who of them is online,
  requests, and invitations to games, and *Write* opens a conversation with a friend; your
  friends see you online while the client runs.
- `BINGLE3D_CONFIG_DIR` points the client at another directory, to keep a development setup apart.

**Windows**: install the [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/)
with the *Desktop development with C++* workload, then rustup, then `cargo run`.

**Linux** needs the development headers Bevy links against: on Debian or Ubuntu,
`libasound2-dev libudev-dev libwayland-dev libxkbcommon-dev` (and `libxkbcommon-x11-0` to run
under X11, which a desktop already has); on Fedora, `alsa-lib-devel systemd-devel wayland-devel
libxkbcommon-devel`.

**Immutable Fedora (Bazzite, Silverblue, Kinoite)**: rather than layering packages onto the
system, build in a [toolbox](https://containertoolbx.org/), a container that shares your home
directory, display and devices:

```sh
bash scripts/toolbox-setup.sh               # once: creates the `bingle-3d` toolbox
toolbox run -c bingle-3d bash -lc 'cargo build'
./target/debug/bingle-3d                    # run on the host
```

Run the binary on the host if you use NVIDIA's driver: the toolbox has no NVIDIA libraries and
would draw in software. `cargo run` inside the toolbox still works, just slowly.

CI builds and tests on Linux and Windows, and on Linux also opens the window under Xvfb, renders
120 frames in software and keeps a screenshot (the `smoke-screenshot` artifact).

## License

MIT, as Bingle is: see [LICENSE](LICENSE). The interface's typeface is Fira Sans, under the SIL
Open Font License ([assets/fonts/OFL.txt](assets/fonts/OFL.txt)). The games are independent implementations of
published rules; their names and designs belong to their designers and publishers, who are
credited in each game's information on the server.
