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
  look at the picture. A green build does not prove a scene draws.
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
closes by itself. Headless, run it as CI does: in the toolbox,
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
  A seat's colour word is an identifier, not a colour. Every game must draw from its meaning alone,
  with generic defaults; a per-game appearance improves that and is never required.
- **No game is named in shared code.** Game-specific looks live in per-game appearance files.
- **Commercial games stay out of this repository**
  ([ADR 0127](https://github.com/noderunner/bingle/blob/main/docs/decisions/0127-commercial-games-in-a-private-overlay.md)).
  Their appearance is loaded from a folder at run time (plan, decision 2). Never add their names,
  art, models or rules text here, not even in a test.
- **The connection must be secure**
  ([ADR 0065](https://github.com/noderunner/bingle/blob/main/docs/decisions/0065-the-connection-must-be-secure.md)):
  plain HTTP only to loopback; a self-signed server is trusted by its fingerprint after the player
  agrees, never by turning certificate checks off.

## Words

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
  and the connector ureq uses.
- `src/table.rs`: the watched game as the server last reported it. Every event carries the whole
  position, so it replaces what is known; `move_seq` decides what is stale and what is a gap
  (reload).
- `src/config.rs`: `config.toml` and `keys.toml` (guest keys, and the session token of an account
  signed in). Keys never go in `config.toml`, and a file that
  cannot be read is never overwritten.
- `src/browser.rs`: the panel listing the server's games; it becomes the game browser. Its `Action`
  is what every panel's buttons do. `src/families.rs`: the games to start grouped by family, and
  the names of a family and its members, with no Bevy in it. `src/game_panel.rs`: the panel beside a watched game.
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
- `src/sign_in.rs`: signing in and out. `Form` has no Bevy in it; the password has a masked
  field of its own, since Bevy's `TextInput` cannot hide what is typed. While signed in,
  `session.rs` sends the session's token wherever it would send the guest key. **Never log a
  password or a token.**
- `src/clock.rs`: time in a timed game: the server's RFC 3339 timestamps, the time each seat has
  left (counted down between the server's reports) and how it reads.
- `src/fonts.rs`: Fira Sans, compiled in. Use `UiFonts` for every text; Bevy's built-in font is
  ASCII only.
- `src/scene.rs`: the table and its lights. **One unit is one metre, and the table top is at
  y = 0**, so boards are their real size (this is what makes VR work later).
- `src/board.rs`: the watched game's board and pieces, from the game definition's board kind and
  layout coordinates and the position's cells; names no game. `src/look.rs`: generic defaults for
  how things look (a seat's colour). Do not read `shape`, `style` or other fields ADR 0124 moves to
  `web.yaml`: they are leaving the API. Each seat's `connect` goals are strips along their regions'
  outer sides, in the seat's colour (`goal_strips`). The board also lights the cells a click would be taken on
  and shows the move being built as see-through pieces.
- `src/play.rs`: playing a seat. `Turn` is the move being built from the server's legal selections,
  narrowed by gestures, with no Bevy in it; its systems turn clicks on a `Spot` (a board cell or a
  piece on it), keys and the panel's buttons into gestures and commands. Picking is Bevy's mesh
  picking with markers: only what is `Pickable` is picked, through a `MeshPickingCamera`.
- `src/camera.rs`: the orbit camera. The left mouse button is kept for picking; nothing else reads
  the mouse directly, and nothing assumes there is exactly one camera.
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
- `src/run_limit.rs`: `--frames` and `--screenshot`.

## Dependencies and Bevy

- **Keep the dependency list short**, as Bingle does. Say why before adding a crate. No async runtime
  (networking runs on its own thread with blocking clients); `rustls` with *ring*, never OpenSSL or
  aws-lc-rs (which needs cmake and nasm on Windows). Today: `bevy`, `ureq` (pinned to 3.5, for the
  `unversioned` transport API `src/net/tls.rs` uses), `rustls`, `ring`, `webpki-roots`, `serde`,
  `serde_json`, `toml`, `tungstenite` (only its `handshake` feature: TLS is ours), and for voice
  `rtc` (its `crypto-ring` feature only), `rusty-opus`, `cpal` (which Bevy's audio already builds),
  `bytes` (rtc's packets) and `sonora` (WebRTC's audio processing in Rust: echo cancellation).
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
