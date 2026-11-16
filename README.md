# Bingle 3D

A native 3D client for [Bingle](https://github.com/noderunner/bingle), the server for playing
abstract strategy games, built with [Bevy](https://bevyengine.org/) in Rust for Linux and Windows.

Bingle already has a browser UI. This client plays the same games through the same public
REST/WebSocket API, and draws them as a table in 3D. It has no rules of its own: the server
says what the position is, what the legal moves are and who is to move, and the client decides
only how that looks ([ADR 0124](https://github.com/noderunner/bingle/blob/main/docs/decisions/0124-the-server-says-what-things-are-clients-decide-how-they-look.md)).

> **Status: planning.** There is no code yet. [docs/plan.md](docs/plan.md) lays out the stages,
> starting with an application that opens a window and draws a scene on Linux and Windows.

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

Not yet: stage 1 of the [plan](docs/plan.md) adds the Cargo project. It will need a Rust
toolchain (via [rustup](https://rustup.rs/)) and, on Linux, the development headers Bevy links
against (ALSA, udev, Wayland/X11, xkbcommon); on Windows, the MSVC build tools.

## License

MIT, as Bingle is: see [LICENSE](LICENSE). The games are independent implementations of
published rules; their names and designs belong to their designers and publishers, who are
credited in each game's information on the server.
