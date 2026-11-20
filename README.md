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

Right-drag (or the arrow keys) orbits the camera, and the wheel zooms (or scrolls the panel, over
it). F5 connects again.

*Play a bot* on a game in the list starts a game in which you have the first seat and the server's
strongest bot the others. When it is your move, the cells you may play on are lit: click one, and
the stone is shown there; click it again, press Enter or choose *Play* to play it, or press Esc
(or Backspace, or *Back*) to take it back. A choice that is not a cell, such as swapping sides in
Hex, is a button on the panel. The client only offers what the server lists as legal, and sends
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
cargo run -- --sign-in                     # the form that signs in to an account
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
as squares, pieces as stones in their seat's colour, and the edges a seat must connect (its
`goals`) as strips in its colour. The client needs a Bingle whose API is 1.6 or newer.

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
- **Signing in** to an account (*Sign in*, under the server's status) keeps the session in
  `keys.toml` too, so the client stays signed in between runs until *Sign out*. Your games as a
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
