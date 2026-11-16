# Bingle 3D

A native 3D client for [Bingle](https://github.com/noderunner/bingle), the server for playing
abstract strategy games, built with [Bevy](https://bevyengine.org/) in Rust for Linux and Windows.

Bingle already has a browser UI. This client plays the same games through the same public
REST/WebSocket API, and draws them as a table in 3D. It has no rules of its own: the server
says what the position is, what the legal moves are and who is to move, and the client decides
only how that looks ([ADR 0124](https://github.com/noderunner/bingle/blob/main/docs/decisions/0124-the-server-says-what-things-are-clients-decide-how-they-look.md)).

> **Status: stage 1 of the [plan](docs/plan.md).** The app opens a window and draws a placeholder
> board in 3D on Linux and Windows. It does not talk to a server yet.

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

Right-drag (or the arrow keys) orbits the camera, and the wheel zooms.

**Windows**: install the [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/)
with the *Desktop development with C++* workload, then rustup, then `cargo run`.

**Linux** needs the development headers Bevy links against: on Debian or Ubuntu,
`libasound2-dev libudev-dev libwayland-dev libxkbcommon-dev`; on Fedora, `alsa-lib-devel
systemd-devel wayland-devel libxkbcommon-devel`.

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

MIT, as Bingle is: see [LICENSE](LICENSE). The games are independent implementations of
published rules; their names and designs belong to their designers and publishers, who are
credited in each game's information on the server.
