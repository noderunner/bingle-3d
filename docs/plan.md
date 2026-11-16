# Plan: a 3D client for Bingle

Status: planning in progress (2026-11-16).

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

### Stage 0: this repository (done here)

README, this plan, `.gitignore`.

### Stage 1: a window on Linux and Windows

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
- A way for the private overlay to supply appearance for its games without this repository naming
  them (see the open questions).

### Later

Accounts and API tokens (`/auth/tokens`) instead of guest keys; release builds attached to GitHub
releases; maybe macOS, gamepad and Steam Deck controls; tutorials and puzzles.

## Open questions

1. **Assets**: does the client ship its own, download them from the server, or both? Bingle left
   this for when the client is built. Proposal: ship them with the client, and leave downloading
   until a data pack needs it.
2. **The private games**: how their appearance reaches a private build. Options: a Cargo feature
   that pulls an appearance crate from `bingle-private`, or appearance files loaded from a directory
   at run time. The second keeps the binary the same for everyone.
3. **API drift**: the client follows the server's API by hand. Should Bingle publish a JSON schema
   for `internal/wire`, or a version the client can check, so a mismatch is found at start-up rather
   than mid-game?
4. **Platforms**: Linux and Windows to begin. macOS and the Steam Deck are possible later; say if
   either matters sooner, since the Deck in particular changes how input should be designed.
