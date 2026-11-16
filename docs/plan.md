# Plan: a 3D client for Bingle

Status: stage 2 done; stage 3 next (2026-11-16). The desktop client comes first: Hex, a game
browser, accounts, friends and voice chat, and VR only after that (decision 6).

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

## Technical choices

| Area | Choice | Why |
|---|---|---|
| Engine | Bevy **0.20.x** (0.20.1 is current), pinned to a minor version | Upgrade deliberately, one Bevy release at a time: each one breaks APIs |
| Language | Rust, edition 2024, `rust-version` = Bevy's MSRV (1.97.1 for 0.20) | Matches Bevy |
| Layout | One crate to start; split into a workspace (`net`, `appearance`, app) once there is a reason | Keep it small until it hurts |
| HTTP | a blocking client (`ureq` 3.5) on a dedicated network thread, with our own `rustls` connector in its chain (`src/net/tls.rs`) | No async runtime to integrate with Bevy's; no OpenSSL to find on Windows. ureq has no setting for a verifier of our own, which trusting a self-signed certificate by fingerprint needs; the connector uses ureq's `unversioned` transport API, so ureq is pinned to 3.5 |
| TLS | `rustls` with *ring*, Mozilla's roots (`webpki-roots`), and certificates the player has trusted, by SHA-256 | *ring* builds with a plain C compiler on Windows; aws-lc-rs needs cmake and nasm. The same configuration will serve the WebSocket |
| WebSocket | `tungstenite` with `rustls`, on the same thread model | Server-push only: one socket per watched room |
| Net ↔ ECS | channels; the network thread sends events, systems turn them into Bevy messages/resources | The ECS never blocks on I/O |
| JSON | `serde` structs written by hand for the parts of the API we use, tolerant of unknown fields | The API adds fields often; we should not break when it does |
| Picking | Bevy's built-in picking | Clicks become gestures; gestures narrow the legal selections |
| Config | `config.toml` in the platform's config directory (server, trusted fingerprints) and `keys.toml` beside it (a guest key per server, owner-only on Linux) | Same places on Linux (XDG) and Windows (`%APPDATA%`); the config can be shared without giving a guest away; a file that cannot be read is reported and never overwritten |
| API types | checked against the JSON Schema Bingle publishes (`schema/bingle-api-1.0.0.schema.json`): a test builds the smallest and the fullest message the schema allows and reads both | Catches a field that is renamed, required or retyped when the schema is updated |
| Text | Fira Sans (SIL OFL), compiled into the binary | Bevy's built-in font is ASCII only, and game names are not ("Hex (11×11)") |
| CI | GitHub Actions on `ubuntu-latest` and `windows-latest`: `fmt`, `clippy -D warnings`, `build`, `test` | The only way to know Windows builds without a Windows machine at hand |

## Stages

Each stage ends with something that runs, and is one pull request or a few.

### Stage 0: this repository (done, #1)

README, this plan, `.gitignore`.

### Stage 1: a window on Linux and Windows (done, #2)

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

### Stage 2: talk to a server (done)

- A network thread, the config file, `POST /api/guest` once and the key kept for later runs.
- A screen listing `GET /api/game-defs` and the open games from `GET /api/games`.
- Loopback HTTP for development against `make build && ./bin/bingle`; HTTPS with normal
  certificates; a self-signed server accepted by fingerprint after the player says so.
- **Done when** the client lists a local server's games, and says plainly when the server is not
  there or its certificate is not trusted.
- Also: `GET /api`'s `api_version` checked first (same major, at least the minor the client was
  built for), with a plain message when the server or the client is too old; an unreachable server
  tried again every 5 s; `--server` for one run. Changing the server from inside the client waits
  for the game browser (stage 5), which needs text entry.

### Stage 3: watch Hex

- Build the board from Hex's game definition: its positions, their hex coordinates and the board's
  layout, with generic defaults for the pieces (shape by type, colour by seat). Nothing in the code
  names Hex: the same path draws any hex board, and square boards and points follow later.
- Start a bot match (`POST /api/bot-matches`), join as a spectator over `GET /ws/{code}`, and
  animate each `move` event; reload state on a `state` event or a `move_seq` gap.
- **Done when** two bots can be watched playing Hex to the end, from nothing but the API.

### Stage 4: play Hex

- Create a game against a built-in bot, read `selections`, and turn clicks into gestures that
  narrow them, with preview and confirm as the web page does; submit the move by notation.
- Highlight legal targets, the last move, whose impulse it is, and the result.
- Join a game by room code; play a person over the WebSocket.
- Resign, the swap rule, and the end of the game as the server reports it.
- **Done when** a whole game of Hex can be played against a bot and against the web page.

### Stage 5: a game browser

- The games the viewer may find (`GET /api/games`, filtered by game with `?family=` and to those
  with a free seat with `?open_seats=1`), refreshed while the screen is open; join a seat or watch.
- Create a game with its options (seats, bots in chosen seats, clocks, a lobby), and wait in the
  lobby until the host starts it.
- The game definitions with their names and descriptions, so a game can be chosen before Bingle
  has more than Hex drawn well in 3D (any other game is drawn from defaults).
- **Done when** a game made in the browser can be found and joined from the client, and the other
  way round.

### Stage 6: accounts

Friends, direct messages and calls need a signed-in account (ADR 0105); a guest key is not enough.

- Sign in with a handle and password (and a second factor when the account has one), keep the
  session between runs beside the config, and sign out.
- **To settle with Bingle first**: API tokens' scopes (`play`, `create_game`, `read_metrics`) do not
  cover friends or direct messages, and `/ws-dm` is authenticated by the browser's session. Either
  the client holds a session cookie like a browser (sending `X-Bingle-Session` on state-changing
  requests), or Bingle gains a token scope or a sign-in flow for native clients. Raise it as a
  Bingle issue before building this stage.

### Stage 7: friends

- The friends list with who is online (`GET /api/friends`), requests in and out, and invitations to
  games; ask, answer, remove, invite to a game the player has a seat in.
- Live changes over `GET /ws-dm` (`friends` events), and the presence setting (ADR 0106).
- Direct messages between friends, over the same socket.
- **Done when** two clients, or the client and the web page, can befriend each other, see each
  other come online, and invite each other to a game.

### Stage 8: voice chat

The game's voice chat and calls between friends, interoperating with the web page
([voice.md](https://github.com/noderunner/bingle/blob/main/docs/voice.md)): audio only, peer to peer
over WebRTC, the server relaying only the signalling.

- The call list (`POST /api/games/{code}/voice` as a heartbeat), offers, answers and ICE candidates
  through `voice/signal` and the `only=voice` WebSocket, ICE servers from `GET /api/rtc/config`;
  direct calls through `/api/dm/signal` and `/ws-dm`.
- Mute, deafen, a volume per person, push to talk, and a choice of microphone and speaker.
- **Needs a WebRTC stack in Rust**, and this is the stage's biggest decision: it must speak what
  browsers speak (ICE, DTLS-SRTP, Opus). The first candidate is `str0m`, which does no I/O of its
  own and so fits the client's thread model without an async runtime; with an Opus codec and the
  audio devices through `cpal` (which Bevy's audio already pulls in). To be decided, with its
  dependencies, when the stage starts.
- **Done when** a player in the client and one on the web page can talk during a game.

### After that

- The rest of a game: clocks, undo and take-back requests, draw offers, chat, move list and replay;
  reserves and pools; tints, badges and counters by name; card zones for card games.
- More games: square boards, points and stacks (Go, Tak), drawn from their definitions.
- Appearance: a per-game appearance file read by this client only (name and format to be decided:
  likely RON or YAML, data and never code), with models, materials and camera for that game;
  hand-made looks for a few games that reward it; the private overlay's games' appearance loaded
  from a configured folder, without this repository naming them (decision 2).
- Release builds attached to GitHub releases; maybe macOS, gamepad and Steam Deck controls;
  tutorials and puzzles.

### Much later: VR

Not before the desktop client above works (decision 6). The aim is the same table in SteamVR,
through OpenXR, with hand tracking: pinching a piece is the same gesture as clicking it. Nothing is
built for it, but the desktop client keeps these so that it fits later at little cost:

- **Real-world scale**: one unit is a metre, the table top is at y = 0 and a board is the size of
  a real one.
- **Input becomes gestures in one place**: nothing but picking and the flat camera reads the mouse.
- **The camera is replaceable**: nothing depends on there being exactly one camera or on where it
  is.

A note for when it starts: [Frametop](https://github.com/Frametop/frametop) tracks hands on the
Steam Frame itself and publishes them to files rather than through OpenXR, so using it would mean
running the client on the headset. It is young and changing; look at it again then.

## Decisions

Settled 2026-11-16:

1. **Assets ship with the client.** Models, textures and sounds live in this repository and are
   packaged with the binary. Downloading from the server waits until a data pack needs it.
2. **Private games' appearance is loaded from a folder at run time.** The binary is the same for
   everyone; a private build is the public binary plus a folder of appearance files from
   `bingle-private`, found by a configured path. Shared code still names no game.
3. **Bingle publishes a schema and an API version** (done in
   [bingle#212](https://github.com/noderunner/bingle/issues/212), Bingle's ADR 0134), so the client
   checks the server it talks to at start-up and says clearly when it is too old or too new, rather
   than failing mid-game. The client's types stay hand-written and tolerant, and are tested against
   the schema of the API version they target.
4. **Platforms: Linux and Windows.** CI builds both; the maintainer runs Windows builds by hand
   from time to time. The Steam Deck and macOS are left out for now but not ruled out.
5. **Development on Linux happens in a toolbox** (`scripts/toolbox-setup.sh`) on the maintainer's
   immutable Fedora desktop: build there, run on the host where the GPU driver is.
6. **The desktop client first, and Hex first.** One game, Hex, played end to end, and the platform
   around a game (a game browser, accounts, friends, voice chat), before more games or VR. VR waits
   until that works; the desktop client only keeps the few habits above that make it possible.
