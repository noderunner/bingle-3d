# Plan: a 3D client for Bingle

Status: stage 1 in progress (2026-11-16).

## Goal

A native client, built with Bevy, that plays every Bingle game through the server's public API on
Linux and Windows, and draws the table in 3D. It should be pleasant for the games that suit 3D
(Tak's stacks, Go's stones, a card table) and still correct for every other game, drawn from
defaults.

## What the client must respect

These come from Bingle's own decisions, and the client is built around them rather than around them
being changed:

- **Every game is playable through the API alone**
  ([game plugins plan](https://github.com/noderunner/bingle/blob/main/docs/history/game-plugins-plan.md#every-game-is-playable-through-the-api-alone)).
  The client needs no rules code. It reads `GET /api/game-defs/{id}` for the board and pieces,
  `GET /api/games/{code}/state` for the position, `GET /api/games/{code}/selections` for the legal
  selections and their gestures, and posts a move by its notation.
- **The server says what things are; each client decides how they look**
  ([ADR 0124](https://github.com/noderunner/bingle/blob/main/docs/decisions/0124-the-server-says-what-things-are-clients-decide-how-they-look.md)).
  Seat colour words (`red`) are identifiers, not colours. Facts such as tints and badges arrive by
  name, with words that explain them; their look is ours to choose. The web page's `web.yaml` is
  the web page's: we may read it for hints, but must not depend on it.
- **Words** ([ADR 0114](https://github.com/noderunner/bingle/blob/main/docs/decisions/0114-impulses-selections-moves-and-gestures.md)):
  impulse, selection, move, ply and gesture mean here what they mean in Bingle, in code and docs.
- **The connection must be secure** ([ADR 0065](https://github.com/noderunner/bingle/blob/main/docs/decisions/0065-the-connection-must-be-secure.md)):
  plain HTTP only to loopback; otherwise HTTPS/WSS, including a server's self-signed certificate,
  which the client should accept by its SHA-256 fingerprint rather than by turning checks off.
- **Commercial games stay out of public repositories**
  ([ADR 0127](https://github.com/noderunner/bingle/blob/main/docs/decisions/0127-commercial-games-in-a-private-overlay.md)).
  Shared client code names no game, and a game's appearance can come from outside this repository.

## Technical choices (proposed)

| Area | Choice | Why |
|---|---|---|
| Engine | Bevy **0.20.x** (0.20.1 is current), pinned to a minor version | Upgrade deliberately, one Bevy release at a time: each one breaks APIs |
| Language | Rust, edition 2024, `rust-version` = Bevy's MSRV (1.97.1 for 0.20) | Matches Bevy |
| Layout | One crate to start; split into a workspace (`net`, `appearance`, app) once there is a reason | Keep it small until it hurts |
| HTTP | a blocking client (`ureq`) with `rustls`, on a dedicated network thread | No async runtime to integrate with Bevy's; no OpenSSL to find on Windows |
| WebSocket | `tungstenite` with `rustls`, on the same thread model | Server-push only: one socket per watched room |
| Net ↔ ECS | channels; the network thread sends events, systems turn them into Bevy messages/resources | The ECS never blocks on I/O |
| JSON | `serde` structs written by hand for the parts of the API we use, tolerant of unknown fields | The API adds fields often; we should not break when it does |
| Picking | Bevy's built-in picking | Clicks become gestures; gestures narrow the legal selections |
| Config | a TOML file in the platform's config directory (server URL, trusted fingerprints) and the guest key beside it | Same places on Linux (XDG) and Windows (`%APPDATA%`) |
| CI | GitHub Actions on `ubuntu-latest` and `windows-latest`: `fmt`, `clippy -D warnings`, `build`, `test` | The only way to know Windows builds without a Windows machine at hand |

## Stages

Each stage ends with something that runs, and is one pull request or a few.

### Stage 0: this repository (done, #1)

README, this plan, `.gitignore`.

### Stage 1: a window on Linux and Windows (in progress)

- `cargo new`-shaped project: `Cargo.toml` with Bevy 0.20 (default features for now), a
  `rust-toolchain.toml`, and the usual dev profile (`opt-level = 1` for our code, `3` for
  dependencies, so a debug build runs at a usable frame rate).
- `src/main.rs` opens a window titled *Bingle* and draws a lit 3D scene: a ground plane, a
  placeholder board (a 7×7 grid of tiles built in code), a few pieces, a camera that orbits with the
  mouse, and an FPS readout in debug builds.
- A `--frames N` flag that quits after N frames, so CI and scripts can run the app and see it exit
  cleanly.
- CI as above, with the Linux job installing `libasound2-dev libudev-dev libwayland-dev
  libxkbcommon-dev`. Stretch: run `--frames 120` under Xvfb with Mesa's software Vulkan (lavapipe)
  and keep a screenshot as an artifact.
- **Done when** `cargo run` shows the scene on the maintainer's Linux desktop (Wayland and X11) and
  on Windows, and CI is green on both.

### Stage 2: talk to a server

- A network thread, the config file, `POST /api/guest` once and the key kept for later runs.
- A screen listing `GET /api/game-defs` and the open games from `GET /api/games`.
- Loopback HTTP for development against `make build && ./bin/bingle`; HTTPS with normal
  certificates; a self-signed server accepted by fingerprint after the player says so.
- **Done when** the client lists a local server's games, and says plainly when the server is not
  there or its certificate is not trusted.

### Stage 3: watch a game, any game

- Build a board from a game definition's positions and layout: squares, hexes and points from the
  board's own coordinates, with generic defaults for pieces (shape by type, colour by seat).
- Stacks drawn as stacks (Tak, and any game whose `cells` hold more than one piece).
- Start a bot match (`POST /api/bot-matches`), join as a spectator over `GET /ws/{code}`, and
  animate each `move` event; reload state on a `state` event or a `move_seq` gap.
- **Done when** two bots can be watched playing Hex, Go and Tak to the end, from nothing but the
  API.

### Stage 4: play

- Create a game against a built-in bot, read `selections`, and turn clicks into gestures that
  narrow them, with preview and confirm as the web page does; submit the move by notation.
- Highlight legal targets, the last move, whose impulse it is, and the result.
- Join a game by room code; play a person over the WebSocket.
- **Done when** a whole game of Hex and of Tak can be played against a bot and against the web page.

### Stage 5: the rest of a game

Clocks, undo and take-back requests, draw offers, resign, chat, move list and replay; reserves and
pools; tints, badges and counters by name; card zones for card games.

### Stage 6: appearance

- A per-game appearance file read by this client only (name and format to be decided: likely
  RON or YAML, data and never code), with models, materials and camera for that game.
- Hand-made looks for a few games that reward it, starting with Tak.
- Appearance for the private overlay's games loaded from a configured folder, without this
  repository naming them (decision 2).

### Later

Accounts and API tokens (`/auth/tokens`) instead of guest keys; release builds attached to GitHub
releases; maybe macOS, gamepad and Steam Deck controls; tutorials and puzzles.

### Later still: SteamVR

Once the flat client has momentum, the same table in VR through OpenXR (SteamVR's runtime), with
hand tracking: pinching a piece is the same gesture as clicking it. Most likely through the
`bevy_mod_openxr` family of crates, whose hand-tracking support follows OpenXR's
`XR_EXT_hand_tracking`; the maintainer is also following a public project adding hand tracking for
the Steam Frame. Nothing is built for it yet, but the flat client is shaped so that it fits later:

- **Real-world scale**: one unit is a metre, the table top is at y = 0 and a board is the size of
  a real one, so in VR the table stands where a table would.
- **Input becomes gestures through one place**: picking turns a pointer ray (mouse today; a
  controller's or a hand's ray, or a fingertip, later) into the same gestures, and nothing else
  reads the mouse directly except the flat camera.
- **The camera is replaceable**: the orbit camera is one plugin, and nothing depends on there being
  exactly one camera or on where it is. A VR rig brings its own.
- **Interface in the world, not only on the screen**: panels that matter during play (whose
  impulse, clocks, the move list) should be able to live on the table as well as in a flat overlay.
- **Bevy versions**: an XR crate usually trails a new Bevy release by weeks or months. When VR
  starts, the client stays on the newest Bevy the XR crate supports, rather than the newest Bevy.

## Decisions

Settled 2026-11-16:

1. **Assets ship with the client.** Models, textures and sounds live in this repository and are
   packaged with the binary. Downloading from the server waits until a data pack needs it.
2. **Private games' appearance is loaded from a folder at run time.** The binary is the same for
   everyone; a private build is the public binary plus a folder of appearance files from
   `bingle-private`, found by a configured path. Shared code still names no game.
3. **Bingle should publish a schema and an API version**, so the client can check the server it
   talks to at start-up and say clearly when it is too old or too new, rather than fail mid-game.
   This is work in the `bingle` repository (to be raised there). Until it lands the client reads the
   API with hand-written, tolerant types.
4. **Platforms: Linux and Windows.** CI builds both; the maintainer runs Windows builds by hand
   from time to time. The Steam Deck and macOS are left out for now but not ruled out.
5. **Development on Linux happens in a toolbox** (`scripts/toolbox-setup.sh`) on the maintainer's
   immutable Fedora desktop: build there, run on the host where the GPU driver is.
