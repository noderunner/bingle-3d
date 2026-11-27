# Plan: a 3D client for Bingle

Status: stage 9 under way, with a main screen and an escape menu: a game can be played against the server's bots or a person, found,
set up and started from its lobby; the player can sign in to an account, has friends and can talk
to them; Hex has a look of its own, made by the asset pipeline, and the table can stand in a
garden (2026-11-20); Tak, draughts and the games played with chessmen have their pieces
(2026-11-21). In a build with the `vr` feature the whole client works in a SteamVR
headset, with its controllers (decision 10; 2026-11-21).
Stage 4's and stage 5's last checks, against the web page by hand, are the maintainer's. The desktop client comes first: Hex, a game
browser, accounts, friends and voice chat.

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
| API types | checked against the JSON Schema Bingle publishes (`schema/bingle-api-1.18.0.schema.json`): a test builds the smallest and the fullest message the schema allows and reads both | Catches a field that is renamed, required or retyped when the schema is updated |
| Voice | `rtc` (webrtc-rs's sans-I/O stack, on *ring*) and `rusty-opus`, on a thread of its own; devices through `cpal`; echo cancellation through `sonora` | Speaks what browsers speak with no async runtime and no aws-lc-rs; sound must leave every 20 ms, which the network thread's blocking calls cannot promise (decision 7) |
| Text | Fira Sans (SIL OFL), compiled into the binary | Bevy's built-in font is ASCII only, and game names are not ("Hex (11×11)") |
| Assets | Made by Blender 5.3, run headless from Python scripts (`assets-src/`), from CC0 sources listed with their hashes; the results committed in `assets/` and compiled into the binary | Scripts can be read, reviewed and run again by a person or an agent; a `.blend` file cannot. CC0 ships with no conditions (decision 8) |
| Textures | JPEG, given mipmaps by the client when they load (`src/textures.rs`); the room's light as prefiltered KTX2 cubemaps | Bevy 0.20 makes no mipmaps, and lossless KTX2 with them is ten times the size; Bevy's runtime-filtered environment light filters every frame |
| Appearance | TOML files in `assets/appearance/`, one per game (or family of variants), by game id, each saying the format it is written in (`format`, `appearance::FORMAT`) | Data, never code; `toml` is already a dependency. Replaces "RON or YAML" in the plan before. The format lets a look from a server say what it was written for |
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

### Stage 2: talk to a server (done, #4)

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

### Stage 3: watch Hex (done, #5)

- Build the board from Hex's game definition: its positions, their hex coordinates and the board's
  layout, with generic defaults for the pieces (shape by type, colour by seat). Nothing in the code
  names Hex: the same path draws any hex board, and square boards and points follow later.
- Start a bot match (`POST /api/bot-matches`), join as a spectator over `GET /ws/{code}`, and
  animate each `move` event; reload state on a `state` event or a `move_seq` gap.
- **Done when** two bots can be watched playing Hex to the end, from nothing but the API.
- As built: API 1.1 (for bot matches). The layout coordinates the server sends are used as they
  are, one unit between neighbours, so a hex board, a grid and a board of points all draw from the
  same code; the cells' shape comes from the board's kind (hexagons, squares, round spots) and
  every piece is a stone in its seat's colour, with a word the client knows (`red`) drawn as that
  colour and any other from a palette by seat. Shapes by piece type wait for an appearance file,
  since `pieces.*.shape` is leaving the API (ADR 0124). Each `move` event carries the whole
  position, so the client replaces what it shows and drops in only the pieces that are new, and
  rings the last move. The camera frames each new board. *Watch two bots* on any two-player game
  in the list, *Watch* on a game in progress, and `--watch-bots GAME` / `--watch ROOM`. The bots
  are the server's MCTS bot when it has one (then minimax, tactical, or the first listed).
- **Then**: which edges each seat must join in Hex was only in words in API 1.1 (seat labels,
  `win_rules`); [bingle#216](https://github.com/noderunner/bingle/issues/216) added it as data in
  API 1.2, and stage 4 draws it (below). The socket is reopened after 5 s if it drops mid-game.

### Stage 4: play Hex

- Create a game against a built-in bot, read `selections`, and turn clicks into gestures that
  narrow them, with preview and confirm as the web page does; submit the move by notation.
- Highlight legal targets, the last move, whose impulse it is, and the result.
- Join a game by room code; play a person over the WebSocket.
- Resign, the swap rule, and the end of the game as the server reports it.
- **Done when** a whole game of Hex can be played against a bot and against the web page.
- As built so far (#6: playing the bots). *Play a bot* in the list, or
  `--play-bot GAME`, makes a game with `opponent` set to the server's strongest bot, the player in
  the first seat under the name in `config.toml` (`name`, "Player" by default). The client knows
  its seat from `GET /api/games/{code}`'s `yours`, and opens the game's socket as that seat's
  player (`role=player`, with the guest key), so the seat shows as connected; *Watch* on a game of
  one's own therefore takes the seat again. On its impulse the client reads `selections`, and
  `src/play.rs` narrows them by gestures as the web page's move builder does: a click on a
  position also makes the choices before it that only one selection allows (the stone, in Hex),
  and a choice that is not a position (swapping sides) is a button. Each selection made asks the
  server what may follow (`made`, `ply`); the move that ends the impulse is shown as see-through
  pieces and waits for a second click, Enter or *Play*, and Backspace goes back (Esc did too, until
  the escape menu took it). The move is
  sent whole, by notation, with its `ply`; it arrives on the socket like everyone else's. A move
  refused because the game moved on reads the state again. API 1.1 still covers all of it.
- Each seat's goal on the board: the client targets API 1.2 for the game definition's `goals`
  (bingle#216) and reads the board's `regions`. Every `connect` goal's regions get a strip in the
  seat's colour along their cells' sides that face off the board (a neutral one for a goal every
  seat shares); a side of a corner cell in two regions goes to the one it faces most, so each
  edge shows one colour. The way a region faces is square to the line of its cells, away from the
  board's middle. Nothing names Hex: any board with connect goals is drawn the same way, and a
  goal type the client does not know is skipped. A server older than API 1.2 is now refused.
- Playing a person. *Play a person* in the list, or `--host GAME`, makes a game with no
  `opponent`: the player has the first seat, and the panel shows the room code and the game's
  `invite_url` while it waits. *Join* on a waiting game in the list, or `--join ROOM`, takes a free
  seat (`POST …/join`) when the game is waiting for players and none of its seats is the
  client's already; otherwise the game is only opened, so `--join` on a full game watches it.
  A seat goes by its player's name, so a name already at the table is numbered ("Player 2").
  Who sits where is read again from every `state` event's `seats`, so a person who joins appears
  at once and the game starts on its own. *Resign* asks once more, then `POST …/resign`; the end
  of the game arrives on the socket like a move. A 403 now shows the server's reason; only a
  401 means the server wants an account. Checked against a Bingle on loopback: hosted and joined
  by a guest, joined a guest's game (the swap rule offered on the first reply), and a
  resignation seen ending the game.
- **Next**: a whole game of Hex against the web page, by hand, which is stage 4's test; then
  stage 5. Leaving a waiting game (`…/leave`) and offering a draw are not built.

### Stage 5: a game browser

- The games the viewer may find (`GET /api/games`, filtered by game with `?family=` and to those
  with a free seat with `?open_seats=1`), refreshed while the screen is open; join a seat or watch.
- Create a game with its options (seats, bots in chosen seats, clocks, a lobby), and wait in the
  lobby until the host starts it.
- The game definitions with their names and descriptions, so a game can be chosen before Bingle
  has more than Hex drawn well in 3D (any other game is drawn from defaults).
- **Done when** a game made in the browser can be found and joined from the client, and the other
  way round.
- As built so far (finding games). The list of games being played is read again every 5 s while
  it is on screen (connected, no game open), and only redrawn when it changed. *Only with a free
  seat* asks for `open_seats=1`; *Find games* on a game asks for its `family` (its id when it has
  none), so it lists every size of that game, and *Every game* clears it. A new connection keeps
  the filters. A waiting game with a seat free has *Join*, one being played *Watch*.
- A new game set up by the player (*New game…* on a game, or `--new-game GAME`): the player's
  seat, a person (an open seat) or one of `GET /api/bots` in each other seat (`seat`,
  `seat_bots`), and the time: no clock, a game clock (`clock_seconds`,
  `clock_increment_seconds`) or a time a move (`move_time_seconds`), from a few presets, since
  there is no text entry yet. The game's own setup choices (`options`) are left out: Hex has
  none, and the kinds there are belong to other games.
- Clocks: each seat's time is beside it on the panel. The time of the seat on impulse is
  `impulse_deadline` less `server_time`, so this machine's clock does not matter, and is counted
  down between reports; a game clock's other seats show what they have left. Under 10 s it turns
  amber.
- The lobby: while a game this client has a seat in has not started, the panel offers each open
  seat (*Sit here*, or one of `GET /api/bots` in it), the host *Remove* on every other taken seat,
  and *Leave*, which goes back to the list. A game made with *Start: when I say* on the form
  (`lobby`) waits for its host's *Start*, offered once every seat is taken; one without starts by
  itself when the last seat is. The server decides who may do what (the host is `host_seat`, and
  passes to the next person when the host leaves); the panel only leaves out what it would
  refuse, and shows a refusal. A `state` event does not say which seat is this client's, so it is
  found by its player's name, which only one person at a table goes by: it follows a move to
  another seat, and is gone once this client leaves or is removed. These calls came into Bingle's
  schema in API 1.4 ([bingle#224](https://github.com/noderunner/bingle/issues/224)), which the
  client now needs. A seat's faction (`seat-faction`) is not offered: Hex has none.
- The games to start, grouped by family (48 on Bingle today, in 21 families): one card for each
  game, its sizes, layouts and variants as chips on it, and the card's buttons for the chip
  chosen. The server gives no name for a family, so it is made from the members' names: what
  they share before their parenthesised part ("Hex" for "Hex (7×7)"), each chip then going by
  that part ("7×7"); members that share no such name keep their own, under the name of the member
  whose id is the family's. That member is chosen to begin with. Chips are in natural order (7×7
  before 11×11). The choice is kept while the client runs.
- **Next**: the "done when" check, by hand against the web page both ways.

### Stage 6: accounts

Friends, direct messages and calls need a signed-in account (ADR 0105); a guest key is not enough.

- Sign in with a handle and password (and a second factor when the account has one), keep the
  session between runs beside the config, and sign out.
- **Settled with Bingle**: a sign-in made for native clients that answers with a session token
  ([bingle#227](https://github.com/noderunner/bingle/issues/227), built in bingle#228, API 1.5,
  Bingle's ADR 0138). The other ways were a browser's session cookie (with `X-Bingle-Session`),
  or new scopes on API tokens (made on the web page and pasted in, and not a sign-in).
- As built. *Sign in* under the server's status opens a form in place of the lists: the handle
  and the password (`POST /auth/login` with `token: true`), then, for an account with a second
  factor, the code from its authenticator app or a recovery code (`POST /auth/second` with
  `pending`). The guest key goes with the sign-in as its bearer, so the guest's seats become the
  account's; the key then stops working and is forgotten. The session's token (`bgs_…`) is kept
  in `keys.toml` per server, and from then on every call and the game's socket carry it in place
  of the guest key. The status reads *Signed in as …* with *Sign out* (`POST /auth/logout`; the
  token is forgotten even if the server cannot be told). A token the server no longer knows
  (expired, signed out everywhere, a new password) is forgotten, the client goes on as a guest,
  and says so. Signed in, the client sits down under the account's display name. An account
  that must change its password is told to do so on the web page: the client cannot yet.
- Text entry is Bevy's `TextInput` for the handle and the code. It cannot hide what is typed yet,
  so the password has a field of the client's own (`sign_in.rs`): a dot for each character,
  typing, Backspace and pasting, no cursor to move; the password is never logged and is cleared
  once sent. Tab moves between fields (Bevy's `TabNavigationPlugin`), Enter sends, Escape closes,
  and the camera's arrows and the move keys stand aside while a field has the keyboard. A
  server that admits only accounts offers *Sign in* beside its refusal.
- Not built: signing up, signing in through a provider (OIDC: it needs a browser), changing a
  password. `--sign-in` opens the form at start.

### Stage 7: friends

- The friends list with who is online (`GET /api/friends`), requests in and out, and invitations to
  games; ask, answer, remove, invite to a game the player has a seat in.
- Live changes over `GET /ws-dm` (`friends` events), and the presence setting (ADR 0106).
- Direct messages between friends, over the same socket.
- **Done when** two clients, or the client and the web page, can befriend each other, see each
  other come online, and invite each other to a game.
- As built (friends). While an account is signed in, the network thread opens its own socket,
  `/ws-dm`, with the session token, and reads it in the same loop as the game's (opened again
  5 s after it drops); a `friends` event (it carries nothing) reads the list again, as does the
  socket opening. The thread also says the account is here (`POST /here`) every minute, since
  the server forgets it after two and a half; signing out says it has gone. Whether a friend is
  online is never pushed, only worked out when the list is read, so while the panel is open the
  list is read every 10 s, as Bingle's page does every 8.
- The friends panel stands on the right while signed in, during a game too. Closed, it is one
  button that counts what waits (requests in, invitations not answered). Open: invitations
  (*Join* accepts and joins the game, as *Join* on the list does; *Decline*; *Dismiss* for news
  of one answered), requests in (*Accept*, *Decline*), friends with a dot for online (*Remove*,
  asked twice; *Invite* while this client has a seat in a game that has not started), requests
  out (*Cancel*), a handle field to ask somebody, and the three settings (`show_presence`,
  `accept_invites`, `mail_requests`). `--friends` opens it at start.
- Direct messages. *Write* beside a friend opens the conversation in the panel (*Back to
  friends* returns): what the server keeps (`POST /dm/history`: the last 200 of a pair, for two
  hours), with messages arriving on the account's socket (`dm` events) as they are sent, from
  either side and from anywhere the account is signed in; the socket's `ready` gives the
  account's own address, which tells its messages from the friend's. A field (500 characters,
  the server's limit) and Enter or *Send* write (`POST /dm`); a refusal is shown and the text
  kept. A friend who cannot be reached (`can_write` false) is said to be away before writing.
  The server keeps no record of what was read, so unread messages are those that arrived while
  their conversation was not open; they count on *Write* (*Write · 2*) and in the closed panel's
  *waiting*. `--talk HANDLE` opens a conversation at start.
- A seat kept for a friend (Bingle's ADR 0107). Signed in, each seat of the new game form also
  offers the account's friends (`@bob`); a friend has one seat at most. The game is made with
  `reserved_seats`; the server then keeps it waiting for its creator's *Start* (the form shows
  *When I say* chosen and says why) and invites the friend. In the lobby a kept seat reads *kept
  for @bob*, offers nothing (nobody else may sit there), and the host is told it can start once
  they have sat down; the room code is only offered while some open seat is anybody's. The games
  list says *a seat is kept for you*, and *Join* (or *Join* on the invitation) takes it, the
  server giving that seat first. A game whose open seats are all kept for others is only
  watched, rather than refused. Reservations do not come with `state` events: a friend sitting
  down is seen as the seat being taken.
- Not built: the host giving a kept seat up (`POST /games/{code}/release`), which is in Bingle's
  `docs/api.md` but not its schema, so it is for Bingle first.
- **Next**: the "done when", by hand against the web page; then stage 8.

### Stage 8: voice chat

The game's voice chat and calls between friends, interoperating with the web page
([voice.md](https://github.com/noderunner/bingle/blob/main/docs/voice.md)): audio only, peer to peer
over WebRTC, the server relaying only the signalling. Everything a client uses is in Bingle's
schema since API 1.6 ([bingle#229](https://github.com/noderunner/bingle/issues/229)), with what
a browser puts in each message and what the page expects of the other end.

- The call list (`POST /api/games/{code}/voice` as a heartbeat), offers, answers and ICE candidates
  through `voice/signal` and the `only=voice` WebSocket, ICE servers from `GET /api/rtc/config`;
  direct calls through `/api/dm/call`, `/api/dm/signal` and `/ws-dm`.
- Mute, deafen, a volume per person, push to talk, and a choice of microphone and speaker.
- The WebRTC stack is `rtc` with `rusty-opus` (decision 7). In four pull requests: the engine,
  with no Bingle in it; calls between friends; the game's voice chat; then echo cancellation
  (`sonora`, pure Rust), measured once calls work (until then, headsets are best).
- As built (the engine, `src/voice/`). A call is an `Engine`: a thread of its own, told who is in
  the call, who offers to whom and what the other ends sent, and saying what to send them; it
  knows nothing of Bingle. One UDP socket and one peer connection per person, nonblocking and read
  every 5 ms at most; Opus at 48 kHz, mono, 20 ms a frame, sent as the microphone's clock delivers
  it, with up to three lost packets concealed (in-band FEC for the last). It does what the page
  does: one audio track, no data channel, candidates trickled both ways and those that come before
  their description kept; an offer from a new connection of the other end (a new certificate)
  answered on a new one; in a game's call, after a failure, the side that offers restarts ICE
  1.5 s later, up to three times, and the other sends `retry`. Host candidates and STUN's
  server-reflexive ones, and TURN relays (below). The microphone and the
  speaker are cpal's, any rate and channels, resampled to and from the call's; the voices are
  mixed with a little queued each (40 ms) and a volume each. Speaking (the page's level, held
  300 ms) is reported for each person and for this end. Devices are chosen by cpal's id, since
  names repeat under ALSA.
- Tested by two engines talking over loopback (sound both ways, volume, mute, deafen, a `retry`
  bringing up a new connection), and by hand against headless Chromium both ways, the page's
  offer answered and the engine's offer answered (0 packets lost, the tone heard at its level).
  `--voice-check` plays the microphone back through the speaker over a connection on this
  machine, to try the devices.
- As built (calls between friends, `src/call.rs`). *Call* beside each friend who is online
  rings them (`POST /dm/call` with `join`); their client or page rings, and *Answer* is the same
  call. Each end hears the other join as a `dm-call` ring on the account's socket, and the call
  is on: the ICE servers are read (`GET /rtc/config`, for each call, since TURN credentials
  expire), the lower address offers, and the engine's messages go through `/dm/signal` and come
  back as `dm-rtc`, those that come before the connection starts kept for it. The side that
  answers starts only once the server has taken its answer, since a signal before both have
  joined is refused. *Hang up* is `join` false, heard at the other end as a `dm-call` end (with
  `dropped` when they joined another call). The call shows above the friends panel, open or
  closed: *X is calling* with *Answer* and *Decline*, *Calling X…*, *Connecting to X…*, *In a call
  with X* (and *speaking*), with *Mute* and *Hang up*. A `retry` from the other end is answered
  with a new offer; the client does not retry by itself, as the page does not.
- Two things the server does not do, worked around here: declining tells the caller nothing
  (the one declining never joined), so a call rings for at most a minute and then says *No
  answer*; and an event sent to an account before its socket listens is lost, so the network
  thread now opens the account's socket before it says the account is here (a friend who sees
  it online may ring at once).
- Tested: the call's steps without Bevy (ringing, answering, refusal, declining, ringing out,
  a second caller while busy, the order the server must take an answer in), the requests'
  shapes against a fake server, the types against the 1.6 schema, a call starting a real
  connection that offers, and by hand two clients calling each other through a Bingle on
  loopback (`--call`, with `--no-audio-devices`), both connected.
- As built (the game's voice chat, `src/voice_chat.rs`). A player with a seat has *Join the voice
  chat* on the game's panel. Joining is the page's order, on the network thread: the game's voice
  socket (`/ws/{code}?…&only=voice`) first, `joined` awaited, the ICE servers read, then the seat
  announced (`POST /voice` with `active`), again every 15 s; the socket is opened again if it
  drops, and the list read then. Each seat in the call gets one connection, the higher seat
  offering and restarting ICE after a failure (the lower asking with `retry`); who joins and
  leaves comes as `voice` events and with each heartbeat, and a seat that comes back is
  connected again. The WebRTC messages go out through `/voice/signal` and come in as `rtc`.
  *Leave*, leaving the game, or losing the seat leaves (`active` false). One call at a time:
  joining hangs up a friend's call, and a friend's call taking the account out of this one (a
  `voice` leave for its own seat) is said on the panel.
- The panel: who is in the call, each with a dot (connecting, connected, no connection),
  *speaking*, and a volume (− and +, 0 to 200 %, kept by name as the page keeps it); *You* with
  *speaking*, *muted* or *hold V to talk*; *Mute*, *Deafen* (mutes too), *Push to talk (V)* (not
  while typing in a field), *Devices* (each microphone and speaker, numbered where names repeat,
  switched during the call), *Leave*. `--voice` joins once seated. Device choices and volumes last
  for the run; keeping them in `config.toml` waits for a settings screen.
- Tested: the list's changes and who offers, the microphone's state, a call joined connecting to
  the others and offering to the lower seat only, being taken out of the call, the types and
  events against the schema, and by hand against a Bingle on loopback: two clients in one game's
  call, both connected; and **the client and Bingle's page** (headless Chromium, fake
  microphone, driven like Bingle's own `voice_ui_test.go`): the page shows the client's seat
  connected and plays its audio track, the client shows the page's player connected, and sees
  them leave.
- **Done when** a player in the client and one on the web page can talk during a game: done,
  with the page; by ear on two machines is the maintainer's to try.
- As built (echo cancellation, `src/voice/echo.rs`). What the microphone hears is cleaned as the
  page asks the browser to (`echoCancellation`, `noiseSuppression`, `autoGainControl`), by
  `sonora`: WebRTC's own audio processing (AEC3, noise suppression, AGC2) ported to Rust, seven
  crates of its own and nothing else. The mixer keeps a copy of what the speaker pulled, which the
  voice thread hands the canceller before each frame from the microphone, 10 ms at a time; the
  canceller finds the echo's delay itself. It runs only while a microphone is open, cleans frames
  even while muted so it keeps up with the room, and the *speaking* shown for this end is taken
  after it, so one's own speaker no longer counts as speaking. About 0.4 ms of one core per 20 ms
  frame.
- Measured with recorded speech (the speech fixtures that `rusty-opus` and `sonora` ship), played
  and heard back directly and off three reflections, with microphone hiss: the echo lowered by 24
  to 30 dB at delays from 20 to 200 ms (20 to 26 dB with noise suppression and gain control on
  too), steady over 30 s. When both ends speak at once the local voice is ducked, as AEC3 does in a
  browser. A steady pure tone is not cancelled (its echo has no place in time to find), which
  speech never is. Tested by a voice-like signal heard back 50 ms later at half its level (lowered
  by more than 20 dB in ten seconds), a voice passing when nothing plays, and the mixer's copy of
  what it played.
- As built (TURN relays, `src/voice/relay.rs`). The `turn:` servers among the ICE servers, with
  the credentials Bingle makes for each call (coturn's `use-auth-secret`), give each connection
  an allocation on its own socket, through rtc's TURN client (sans I/O, like the rest): the
  relayed address is trickled as a relay candidate, what comes from the relay server is the
  relay's to read and its packets reach the connection as arriving at the relayed address, and
  what the connection sends from that address goes through the relay (the first packets to a new
  address are dropped while the relay is asked to let it through; ICE sends its checks again).
  The relay refreshes the allocation and its permissions, binds channels, and gives the
  allocation back when the call ends. The first relay is used, as browsers use them all; relays
  reached over TCP or TLS (`?transport=tcp`, `turns:`) are not, yet, for networks that let no UDP
  out at all. `--relay-only` offers only the relay's candidate (a browser's
  `iceTransportPolicy: "relay"`), to try a relay or to keep one's address from the others; with
  no relay offered, it says so, and a relay that refuses fails the call.
- Tested: the relays found among the ICE servers (UDP only, with credentials), relay only with no
  relay, a relay refusing, and two engines that may only use a relay talking through a small TURN
  server in the tests (`fake_turn.rs`: a 401 then the allocation, permissions, channels, Send
  and Data indications, refreshes), sound both ways and every packet relayed. Not tried here
  against coturn or a browser through one (the agent's sandbox runs no container): the
  maintainer's to try.
- **Next**: a relay reached over TCP or TLS, if players need it; by ear, a call on speakers and
  one through coturn.

### Stage 9: a table worth looking at

The first hand-made look, Hex's, and the pipeline that makes it, so that more can follow (decision 8).

- The pipeline (`assets-src/README.md`): `bash scripts/assets.sh` runs Blender headless on the
  scripts in `assets-src/blender/`, which fetch the CC0 sources listed in `sources.toml` (checked
  by SHA-256) and write `assets/`. `previews` renders the assets in Cycles for a person to look
  at. Blender is Flathub's, on the host (it renders on the GPU there); the toolbox has none.
- The room: an HDR photograph of a room (Poly Haven, CC0), graded by the pipeline for a table
  top (its floor, below the horizon, made the dim table the board stands on; its colours at half
  saturation) and filtered into the diffuse and specular cubemaps Bevy's `EnvironmentMapLight`
  reads, on every 3D camera. It is the fill light, kept low; a key light from beyond the board,
  with the room's window turned to it, makes the highlights and shadows, so what faces the
  player reflects the dim room and dark polish stays dark. A felt-covered table fading into the
  room with distance.
- Every camera: SSAO (stones grounded, cut lines dark), TAA (fine grain steady, shadows filtered
  over time), contact shadows, bloom on highlights only, a shallow depth of field focused where
  the orbit camera looks.
- Every board is one slab of wood with a rounded rim, its cells cut into it and darkened lines
  between them; the grain is mapped by position, so it runs on across cells, with the grain's
  relief three times finer than its colour. The player chooses the wood: teak, walnut or cherry,
  each with the colour of an oil finish (stopgap buttons at the bottom right). Every
  piece is a lens-shaped stone modelled in Blender, each turned, sized and leaning a little
  differently. These are the generic defaults (`look.rs`), for every game.
- Each goal's edge: the slab's rim along it is stained, and its cells shaded with a soft gradient
  towards it (fading out a third of the way across), in the seat's colour; a corner on two seats'
  edges is split between them.
- The camera pans as well as orbiting (middle-drag, Shift with right-drag, W A S D). The board's
  coordinates can be shown along its edges (C, or a button): where every position is named as
  letters then a number, a column's letters at its two ends and a row's number at its two sides,
  engraved in the board's margin (glyphs of the client's font, rendered by the pipeline into an
  atlas of the cut's colour and its walls' slopes); a board named otherwise shows none.
- Games played on points (Go, Gomoku, Pente, Meridians; their appearance file says
  `[board] on = "points"`, as the web page's `style: points` does for its own look): a plain slab
  with lines cut between neighbouring positions (a square grid, or three ways on a hexagonal
  board), thicker along the edge, and the stones on the crossings; a click lands on an unseen
  disc at each point, which glows as a soft dot when it would be taken. An appearance file may mark
  positions with a dot, per game id (`[board.marks]`): Go's star points, for each size.
- A position a click would be taken on glows (`highlight.rs`): a line of light traced round
  its edge and short streaks of light rising from it, slowly moving, drawn by a shader of the
  client's own and added to the picture; a point's is a ring small enough to read as a dot.
  It replaced an amber stain of the cell's own material; laid under the glow, the stain was
  judged not needed, and is gone. The board only marks what is lit and its outline, so another
  effect can take the glow's place. The glow is tuned twice: for the window's camera, which
  blooms (the chosen look), and for where there is no bloom, a headset's eyes; `--no-bloom`
  shows the window as they see it. Fence sides are no longer lit: the see-through fence on the side aimed
  at shows where one goes.
- Lighting the legal moves is off by default (at the start of most games every position is one,
  and the whole board lit up); *Legal moves* or L turns it on (`toggles.rs`). The see-through
  stone under the pointer still shows where a click would play. Once a piece is picked up, the
  places it may go are lit whatever the setting, as a chess program shows them; it is lifted and
  bobs over its own space (following the pointer instead was tried, and set aside), and a click
  on it again puts it down. A click on where it goes carries it there: across the board at the
  height it was held, then lowered onto it (a third of a second in all), and it stands there
  before the move is sent. Once the move is sent, what it puts down (Amazons' arrow) is shown
  solid at once, without waiting for the server, and is not dropped in again when the server's
  position arrives. Backspace, or a right-click (the right button pressed and let
  go with the pointer all but still, so a right-drag still turns the camera), goes back one
  click at a time. What a move
  then aims (Amazons' arrow) is held under the pointer in place of its arrow, with a see-through
  one on the space a click would shoot at. The server does not say what an aim puts down, so a
  game's appearance names it (`aimed`); asking the API for it belongs in Bingle. Without it,
  nothing is shown where a move aims: an aim may put nothing down (Fist's pin captures the piece
  aimed at where it stands), and it was drawn as the generic stone. A target a click takes a
  piece on (a pin's) glows red instead of in candlelight (`highlight::Tone`). What the move
  being built is sure to take is not drawn until the move is played or taken back, and each
  piece it moves is see-through where it lands, in its place when it takes the piece there,
  whether it takes anything or not (`board::show_taken`, #76; 2026-11-27): the board shows the
  position the confirming click leaves, a pin once chosen, and each piece a jump captures once
  the player says so rather than sparing it. Before the click, the copy under the pointer
  stands over the piece a click would take, which stays drawn: hiding what is under the
  pointer would move what it is over. Both come from what each legal selection captures,
  which the server says from API 1.10 (bingle#269; `Turn::takes_at`, `Turn::taking`), so the
  client reads no game's rules for them; it needs a server of 1.10 or newer (1.14 since Alice
  chess's boards were drawn apart: below). A region the
  board names may have its cells in a material of the look's (`[board.regions]`): Fist's keep
  is in a chessboard's dark squares. A chessboard's
  squares are lit in their own wood, stained amber and glowing: glow alone barely shows on pale
  maple. A game's look may inlay a border round the cells (`[board] border`), a flat band of
  its own material between the cells and the coordinates. Which games want legal moves on from
  the start is for later. A board's cells are the same
  material as the wood round them. The coordinates are smaller, half way across the margin
  between the outermost spaces or lines and the rim, on every board; a column is named only
  where it ends on the first or last row.
- Blooms (`assets/appearance/blooms.toml`): stones of polished minerals drawn with numpy
  (`assets-src/blender/minerals.py`), after photographs of real semi-precious Go stones the
  maintainer owns: red jasper and tiger's eye for Warm, lapis lazuli (with a little of
  sodalite's veining) and black slate for Cool. A small wooden bowl of each colour beside the
  board, a seat's two at its two hands; the bowls are Go's model made smaller (`[bowls] size`),
  stood by the board's own outline so a hexagonal board's narrowing near side brings them in.
  A click on a bowl takes a stone of its colour (the seat's colours, `colors`, and each piece's,
  `pieces[].color`, are read from the game definition); it waits over the bowl, bobbing, while
  the cells it may go to are lit, and a click on one carries it there and sets it down, as a
  piece moved is; the bowl again puts it back. Any game with bowls carries what it places from
  them (Go's stones too). A one-stone turn ends with the server's `done`, a button on the panel;
  `may_end` (a move sent part way) is read too, though only a one-seat game sets it. The lit
  cells of a plain board are stained amber as well as glowing, as a chessboard's are: the glow
  alone barely showed on teak.
- Captures in trays (`trays.rs`): a small tray in front of each seat, between the board and its
  player, of the board's own wood (it changes with the board's), with a square for each capture
  its win wants (`counters[].max`: 15, 20, 25, 40 and 50 on Blooms' five boards). Wide and
  shallow: one row of up to ten, otherwise two rows (8 by 2, 10 by 2, 13 by 2, 20 by 2, 25 by 2;
  first tried in full rows of up to ten, which made the bigger boards' trays deep). The counts are the position's `reserves` (by the seat's colour word, then the
  counter's id). The server does not say which stones were taken (a move's `captured` names one
  cell), so the board sees it: what leaves the board as a seat's count goes up, not of a colour
  it plays, is its capture, carried from where it stood (straight up clear of its neighbours,
  across, and set into the next empty square, one after another), each with a soft wooden
  "tock", a little higher or lower each time, so several in a row do not grate. A game opened
  with captures made has its trays filled at once in the other seat's colours by turns; a count
  past the squares (the last turn may take more) stacks on the first. Any game whose definition
  has a counter with a count wanted gets them, with no look of its own; `[trays]` may give a
  material and a size. The camera frames the board with the player's tray in front of it. No
  number is shown: the squares are the score.
- Tak: what each seat has left to place stands beside the board at its right hand
  (`supply.rs`): its stones on their edges, face to face, a low wall along the board's side from
  the seat's near corner, and its capstones upright nearest it (the definition's `reserves`,
  each piece's `pool`, the position's counts). A click on the player's own takes a flat stone
  (the plain piece of the pool: the first another may go on) or a capstone; it waits over the
  place it came from, bobbing, and the wheel turns a flat stone into a standing one and back
  (any kind of the same pool). A click on a cell carries it there. A stack clicked is picked up
  whole, as much of it as may be carried; the wheel puts its stones back down on it one at a
  time (and takes them up again), until the first drop; each click then drops the bottom stone
  carried where it goes, what is left held over it. The camera does not zoom while the wheel is
  the move's. Its opening places the other seat's stone, shown in that colour (`opening.owner`).
  The road goals every seat shares no longer shade the board's edges in no one's colour: that
  marking says something only where each seat has its own (Hex). Generic: any game whose seats
  keep pieces in reserve gets a supply.
- Fendo (`assets/appearance/fendo.toml`): chess pawns of boxwood and of orange-stained maple, and
  fences of dark walnut (`fences.py`: a slender rail 90 % of a cell's side long, so a square's
  corners show between the fences round it). The fences are the walls the position's view gives
  (`ext.view.walls`, pairs of neighbouring squares, as Bingle's page draws them for any game
  whose rules are written in Go), so any game with walls in its view gets them; one that has
  just appeared drops in. Each seat's pawns in stock stand beside the board at its right hand,
  as Tak's stones do, counted by the view's `stock` counter (`[supply]`: the reserves do not
  say), and a click on the player's own takes one (the `add` choice), which waits over its
  place; a click on a square, or a square clicked at once, adds it there. The fences never run
  out, so they are heaped in one bowl that both players take from, beside the middle of the
  board's side (beyond the other seat's pawns there), where it reads as shared. Once the piece
  has moved, the fence is taken up by itself (`auto_take`: once nothing but a fence may follow)
  and held under the pointer in place of its arrow, as Amazons' arrow is; to leave the piece
  where it stands, it is picked up and the bowl clicked. The sides of the piece's square the
  fence may go on are lit; the one the pointer comes near shows a see-through fence (in place of
  its light), and a click anywhere on the board puts it there, carried and set down. The wheel
  goes round the sides as well (`play::SideAim`), the camera leaving it alone. Nothing is shown
  under the pointer over an empty square before a pawn is taken: a stone there misled (the
  selection does not say what it adds); with a pawn in hand, a see-through pawn.
  Which action builds on which side is the look's (`[fences] sides`: the API names the actions
  but not their sides). The panel's fence buttons still work.
- Games of placing a stone (Go, Gomoku, Pente, Meridians, Hex, Havannah, Mycelium, Othello,
  Yavalath) play alike: each seat's stones in a turned bowl beside the board (Othello's discs
  excepted, below) (the looks:
  `points.toml`, `hex.toml`, and `stones.toml` for the rest, slate and shell, and jade for
  Mycelium's green), and on the player's turn the pointer is the stone, floating and bobbing as
  Amazons' arrow and Fendo's fence are, with a see-through stone on the point or cell under it;
  a click carries it there. This is the default for any game whose every click places one stone
  (`Turn::auto_hand`: the same choices before one click, nothing after, nothing moved), look or
  no look; a pass or a swap stays on the panel. When the pointer leaves the board (for a panel),
  whatever is held under it (stone, arrow or fence) waits floating over the board's edge where
  the pointer is beyond it, and the pointer's own arrow comes back; back over the board, it is
  the stone again. Which games have bowls is each look's: the definitions do not say which games
  are of this kind (two have their rules in Go and name no placing action).
- Courtfall (`assets/appearance/courtfall.toml`): its units are the chessmen, by the piece each
  is in the definition (a Warden is a pawn, an Outrider a knight, an Arbalest a bishop, a
  Juggernaut a rook, the Champion a queen), of boxwood and ebonised walnut. Its terrain is what
  the position's view tints each hex with (`ext.view.tints`, a key of the legend by position,
  as Bingle's page colours them), drawn as the look's `[tints]` say (`tints.rs`): Overgrowth,
  Mire and Hazard as hexagonal tiles of teak stained green, blue and red (`tiles.py`: a tile a
  little narrower than a cell, so the board's lines show round it, 2.4 mm thick), laid on the
  hex, lit with it while a click there would be taken, the units standing on them; the Eye as a
  dot of ink stamped on its hex. A key the look does not name (the deployment zones) is not
  drawn. During the draft the tiles still to lay (the view's `tiles_left`, `[stock]`) wait
  beside the board to the first seat's left, a column of each colour, each tile the size of the
  board's. On the player's draft a click on one takes it up (`Turn::take_to_place`): it
  leaves its column and floats under the pointer, bobbing, as Fendo's fence does, and only the
  hexes that tile may go on are lit; over one of them the tile is shown there see-through, in
  place of the hex's light, following the pointer from hex to hex; a click on one of them makes both of the selection's
  gestures (the hex, then the tile, as Courtfall's selection has them) and the tile is carried
  down onto it, solid, until the move is played; a click on another hex leaves it in hand, and
  another tile clicked takes its place. Chosen the old way (the hex, then the tile's button on
  the panel), it is carried there from its column. Every tile is cut from the same place in the wood: one grain for all,
  for now. Any game whose view tints positions or counts a shared stock gets the same from a
  look of its own.
- A piece whose moves do not have it clicked (Courtfall's unit, once activated: each of its
  selections says it moves from its hex, and is a click on where it goes, then what it does
  there) is picked up by a click on it (`Turn::unclicked_from`, `Turn::lifted`): it floats
  under the pointer in place of its arrow, with the sound of a piece picked up, the hexes it
  may go to or act on are lit, and over an empty one it may go to a see-through copy of it
  stands there (none over a unit it would strike or shove: it does not go there). A click on
  one of them moves it, carried from under the pointer; a click on it again, or Back, puts it
  down; a click where it may not go leaves it in hand. Its second action starts from where
  the first put it: the board follows the piece the move being built has moved
  (`show_turn`'s `origin`), so it is picked up there and carried on from there.
- The game's panel says what the seat on impulse is asked in the server's words, for any game
  whose view says it (`ext.view.impulse`, else `prompt`: Courtfall's "Choose where to lay a
  tile (Overgrowth 4, Mire 3, Hazard 3 left).", "Choose a Court."), as Bingle's page does:
  under "Your move." until the player chooses something (then how to go on from there, as
  before), and under whose move it is for everyone else. A game that says nothing keeps the
  generic hints.
- Commercial games are not shown, even when a server offers them, unless the private folder
  has their look (below, *Private games*; Bingle's ADR 0127). The client may not name them, so
  it asked Bingle to mark them (bingle#260): API 1.8 marks the games a server builds from its
  private overlay `private`, and the client, now requiring 1.8, leaves those without a look out
  of the *New* and *Active* lists, and their tables (`session::public`). A public server marks
  none.
- A board's lines are as thick round the outside as between its cells: the darkened layer under
  the cells is grown by a cell's margin, since round the outside only one cell's margin made the
  line (half as thick). The cells stand only 0.3 mm proud of the lines, not 1.2 mm: the deeper
  trenches between cells were darkened by shadow and ambient occlusion, so the lines round the
  outside, walled on one side only, still looked thinner. A points board's thicker edge lines are
  a Go board's, on purpose.
- Sounds as a seat is played (`sounds.rs`), the same for every game by default (made for
  Amazons first, and the default for now); a game's appearance may give any of them another
  (`[sounds]`): a piece picked up, a piece set down (when a carried piece touches the board, or
  as one is placed or an arrow shot), and the move sent. A light tap of wood, a soft thump with a
  quick decay (a die dropped on felt; a ringing clack of wood on wood was too much), and a soft
  key press on a well-lubricated mechanical keyboard as the move is sent (two marimba notes were
  too much). The game's end, for the player's own seat, as it happens (not a game opened over):
  a crowd's short cheer as it is won, with applause, or a groan as it is lost (a loss with every
  seat too); a draw is silent. The crowd is synthesised as the rest are: a score of voices of
  their own pitch and start, each a glottal buzz through a vowel's three formants, gliding
  ("eh" to "ah", rising, for the cheer; "aw" to "oh", falling, for the groan). Only the
  player's own; the opponent's moves are silent for now, but for what they capture.
  With `--no-audio-devices` Bevy's audio plugin is left out, so such a run (an agent's, a
  screenshot's) opens no speaker; before this Bevy opened the default output at every start.
  This way of picking up and moving pieces is the desktop client's for every game; the headset
  may want another.
- Texture resolution, judged on the GPU: walnut at 4K against 2K was not worth four times the
  memory, so the woods stay at 2K. Oak was tried and left out: its open grain caught too much
  glare.
- The appearance file (`appearance.rs`, decided: TOML): per game id, the pieces' model, what each
  seat's pieces and rim are made of, and each seat's colour (in the panels, and shading its
  cells). Hex's: slate and clamshell stones (the shell a little translucent), the slate side's rim
  stained ebony and the shell side's in maple.
- As built. JPEG textures get mipmaps and a repeating, anisotropic sampler when they load, and
  each material using one is built again (Bevy does not rebuild a material when its image
  changes). Bevy's `jpeg` feature is the only new code (zune-jpeg). PCSS soft shadows were left
  out: they are behind an experimental feature, noisy and costly; TAA's filtering does enough.
- A model for each kind of piece (2026-11-21). An appearance file names a model for each piece
  id, how it `stands` (a stone lies loose; a turned piece upright; a block square to the board,
  a quarter turn any way, and as a hand sets it down, up to 5° askew and 1.3 mm aside of the
  middle each way, so the blocks of a stack do not line up; a piece with a face looks across
  the board from its seat's side) and,
  for a piece that is nobody's, a material of its own. A kind it does not name is the generic
  stone. A piece is stood by its model's foot, and what is on it in a stack is raised by its
  model's height, both read from the model's file, so a stack may be of several kinds: Tak's
  flat stones under a wall or a capstone. A colour word no seat plays may have a look too (the
  pawn that is neither side's in Fist), and one with none still gets a colour. The move being
  built shows the piece it would put down (the selection's `piece`, or what stands where it
  moves `from`), and nothing over a position a click would only pick a piece up from.
- The pieces, made by the pipeline:
  - **Chessmen**: king, queen, bishop, knight, rook and pawn of the usual pattern, the king
    46 mm tall for a 28 mm cell. Five are turned from profiles; the knight's head is an outline
    of its side given a carved head's thickness. Pale and near-black wood. Chess and its
    variants (Chess960, Crazyhouse, Antichess, Atomic, Alice: `chess.toml`) are played with the
    whole set on Amazons' chessboard of walnut and maple squares, a king in check stamped red on
    its square (the view's `check` tint). Alice's two boards are two boards side by side, each
    its own slab, inlay and coordinates, a cell's width and a little table between them: the
    server lists each board's positions from API 1.14 (`boards`, bingle's ADR 0148), and
    `Layout::of` sets them apart, each as large as one board alone. A game on several boards
    needs nothing of its own in the client. Breakthrough (pawns), Amazons (queens on a checkered board, and a red checker for an
    arrow), Brandubh (pawns and a king) and Fist (the whole set, its ownerless pawn a queen of
    green copper) are played with them.
  - **Checkers**: a wooden draughtsman with a milled edge, stained red or black; a king is two.
  - **Tak**: a square block (a flat stone) and the same block on its edge, corner to corner (a
    wall), in pale and dark wood. Its capstone is the chessmen's pawn, in the same wood (the
    maintainer's choice, over a capstone of Tak's own that was made and dropped).
  - **A playing card**: a blank one of the usual size with a place on each side for a picture
    (a JPEG or a PNG of 5 to 7), and a back to give it until a game brings its own.
  - **The usual deck's faces**: a PNG for each of the 52 cards of spades, hearts, diamonds and
    clubs, ace to king, drawn by the pipeline with no picture to start from (`card_faces.py`):
    the rank and suit in two corners, the suit as many times as the rank counts in the places
    cards have them, and for a court card a crown, a coronet or a plumed cap in a frame, twice,
    so it reads the same from either end. The client finds one by the suit's symbol and the
    rank's label as the server gives them (`props::standard_face`), so no game is named. With
    them, the 16 a deck numbered from 1 to 13 has where the usual one has its aces and courts
    (2026-11-25): a 1, an ace with 1 in its corners, and an 11, a 12 and a 13, each its figure,
    large, between two suits, since that many suits are not counted at a glance. 1.3 MB
    together. No jokers: no game deals one yet.
  - **A die**: six sides, pips hollowed out, turned to show a number (`props::die_showing`).
  - **A two-sided disc and a ring** (2026-11-23): a disc 15 mm across, light on one side and
    dark on the other (its parts `light` and `dark`, each a face and half the edge), shown
    either side up by turning it over (`props::disc_shows`); and a ring 25 mm across and 7 mm
    tall that a disc lies inside, in a seat's material. Made for the games played with rings
    and markers that turn over; their looks (and the game itself, a private one) come later,
    from outside this repository (decision 2). Generic ivory and near-black for now
    (`look::disc_light`, `disc_dark`).
  - **Reversi's discs** (2026-11-23): the same disc 24 mm across (`disc_large.glb`), to fill a
    28 mm square. A piece's look may give it two sides (`sides`, the material of each of its
    model's two parts, and `up`, which side each seat's lie up); the board draws any piece of
    such a kind, wherever it is made, as its upper half and a child for its lower
    (`board::dress_sides`, after everything that makes a piece), the halves' materials swapped
    for a seat whose second side is up, since the halves are each other's mirror image. A disc
    that changes hands is the same disc turned over where it lies (`sides.rs`): raised, half a
    turn away from the move played, and down again, each further from the move a little later
    than the one before, with the soft sound of a capture as each lands. No bowls: a heap of
    stones would not do for discs, and a disc in hand comes from under the pointer as before.
- A patience's cards are drawn (below); no game's dice, discs or rings yet. `props.rs` holds how
  these models are read. `--showcase` lays every piece model, the discs and rings, the dice and
  some cards on the table,
  to look at an asset in the client's own light; `--card-face` and `--card-back` try a picture
  on a card, and `--view` takes a place on the table to look at.
- Cost: 3 MB of models. A chessman is 3,700 to 6,500 triangles, a checker 5,000, a die 2,900,
  a disc 3,000 (3,900 the large one) and a ring 4,600;
  a board full of them (Fist's thirty) is over 100,000, and held the display's 60 frames a
  second on the maintainer's GPU. Not measured on the Steam Frame.
- Open:
  - **Pictures as SVG.** The client reads JPEG and PNG (Bevy does), not SVG, and Bingle's page
    draws its cards as SVG in code. To put an SVG on a card it must be made a picture: ahead of
    time, by a tool the pipeline does not have (Blender's own import reads filled shapes, but
    no text and no strokes), or in the client by a rasteriser such as `resvg`, a dependency to
    weigh. Not decided.
  - A card with no picture: a card of the usual deck now has one. The generic face of any
    other card, drawn from what the server says it is (its suit's symbol and its rank), still
    belongs with card zones; so do a table's card sets (Bingle's ADR 0085), whose art is each
    client's to draw.
  - The court cards are plain: what a rank wears, not a figure. Whether they want figures, as
    printed cards have, is the maintainer's to say.
  - A board of squares is plain wood: light and dark squares, where a game is known by them
    (draughts), would be an appearance's to ask for.
- **Next**: the maintainer's eye on it, on the GPU; pictures on these games' cards in the main
  screen; then the card zones of games that are not a patience.

### The main screen and the escape menu (UI basics)

- The list panel is now the main screen, laid out as the web page's home: the server's status
  above, then tabs (*Games*; *Puzzles*, *Ratings*, *Tournaments* and *Global Chat* are shown
  greyed until they are built), and in *Games* a toggle between *New* and *Active*.
- *New* is the web page's gallery: a card for each family (`families.rs`), with its picture, its
  title, its marks (*Prototype*, else *Beta* while the default member's `version` is below 1.0,
  and *Tutorial* when a member has one; `version` and `tutorials` are API 1.6's, so no new
  schema) and how many play it ("2–4 players", every member counted). A card opens the game's
  setup in place of the cards (*← All games* returns): a larger picture, the members as chips,
  the description, and *Play a bot*, *New game…*, *Watch two bots* and *Find games* (which shows
  *Active* filtered to the game).
- A card's picture is the appearance file's (`[card.pictures]`, by game id), so shared code
  names no game; a game without one has a blank square. Go, Gomoku and Pente show a teak board of
  lines with slate and shell stones, rendered by the pipeline (`cards` step); Meridians, on a
  hexagonal board, has none yet, rather than a square board's.
- In a game, Esc opens the escape menu (`escape_menu.rs`): *Resume*, *Main Menu* (as the game
  panel's *Back*: the game goes on at the server), *Settings*, *Game Options* and *Profile*
  (greyed until built) and *Quit*. It covers the window, so clicks reach neither the board nor
  the panels, and the move keys and the camera stand aside. A text field with the keyboard is
  let go of by the first Esc. Backspace now takes a gesture back, which Esc did before.
- *Settings* is built (2026-11-23): fullscreen or a window, and the resolution (`display.rs`),
  kept in `config.toml` (`fullscreen`, `resolution`). Fullscreen is the default (decision 14).
  Esc on *Settings* goes back to the menu.
- Found while building it: the interface is blended in linear light (the cameras are HDR), so a
  faint white over the dark panels shows much lighter, and a nearly opaque panel much more
  see-through, than the same colours on the web page. The new parts use opaque colours.
- The pointer is the client's own (`cursor.rs`), never the system's cursor, which some
  platforms showed again now and then while it was meant to be hidden under a piece in hand: a
  small glove of capsules, drawn by a camera of its own into an image shown at the pointer, over
  every panel. It is open over nothing, relaxed palm down over what a click picks up (a piece
  to move, a bowl, a seat's supply, a stock's tile, a card to lift), and points over what a
  click chooses (a lit position, a pile a card may go on, a button). It is hidden while what
  the move aims is held in its place, as the system's arrow was. The table says what a click
  would do (`play::feel`, `card_table`'s `feel`), from the server's legal selections, as a
  click would. A drawn pointer trails the system's by a frame; the hand is not drawn in a
  headset, whose pointers are rays.

### The login screen

- The client starts on a login screen (`login.rs`) rather than connecting at once: the server's
  address, the sign-in's username and password (and a second factor's code), *Log in*,
  *Continue as guest*, an optional guest key, and *Auto-login*.
- The address is checked when the player leaves its field, presses Enter in it, and at start:
  the network thread switches its session to it (`use_server`) and reads `GET /api`, with the
  same version check as connecting, and who the kept session is there (`Request::Probe`). The
  light beside it: green (usable), yellow (a Bingle whose API and the client's do not go
  together: either is out of date), red (nothing usable: not there, not Bingle, a bad address,
  or a certificate not trusted, which can be trusted there by its fingerprint). No guest key is
  made to look.
- *Continue as guest* ends any account's session kept for that server, then takes the guest
  key typed once the server says it knows it (a key it does not know is refused, and the kept
  one stays), else the kept one, else a new one. A kept session still good on the server is
  offered as *Continue as <handle>*.
- *Auto-login* (`auto_login` in `config.toml`) goes straight to the main screen at the next
  start, with the session token or guest key kept in `keys.toml`. **No password is ever kept**:
  an account's session is what lasts. Options that act on the server at start (`--watch`,
  `--play-bot`, `--new-game`, `--friends`, …) go straight in too; `--sign-in` shows the login
  screen whatever the config says. An address the player typed is remembered once in.
- Esc's menu is now also on the main screen (*Main Menu* greyed there), with *Logout*: out of
  any game, the account's session ended, auto-login turned off, back to the login screen with
  the username filled in. *Sign in* on the main screen goes to the login screen too; its old
  form on the list panel is gone.

### Environments: where the table stands

A place to play in, in place of the plain table: the player's own choice, which the server knows
nothing of (decision 9). First draft, for the maintainer's eye.

- An environment is a file (`assets/environments/<id>.toml`, `environment.rs`): its model, its
  sun, its sky's light and what is seen behind everything, its mist, the camera's exposure and
  reach, cushions, and the materials its model's parts are named for. The plain table
  (`room.toml`) is one too: what `scene.rs` had as constants is now its data. `scene.rs` builds
  whichever is chosen and takes the last one down. The table top stays at y = 0 with its middle
  at the origin, so boards, pieces and the camera know nothing of where they are.
- *Place* (stopgap buttons, with the woods) chooses; `environment` in `config.toml` keeps the
  choice, `--environment` chooses for a run, and `--view` puts the camera somewhere for a
  picture.
- **The garden**: a low walnut table on four and a half tatami mats, on a platform of hinoki over
  raked gravel; rocks on islands of moss with the furrows going round them; a maple in autumn
  between the table and a low sun, so its shade falls across the board; a stone lantern; a pond
  with lily pads and carp, and a stone basin fed by a bamboo spout; bamboo, clipped shrubs, a
  bamboo fence, and trees and hills beyond it. Late on a misty afternoon.
- A cushion lies on the floor for each seat of the game (two when there is none), the first on
  the player's side and the others evenly round the table.
- Games of stones keep them in bowls (`bowls.rs`): an appearance file's `[bowls]` puts a turned
  wooden bowl at each seat's right hand, by the board's corner, heaped with stones in that seat's
  material. Go, Gomoku, Pente and Meridians have them, on the plain table too. Only to look at.
- How it is made (`assets-src/README.md`). The garden is modelled by a script (`garden.py`) and
  exported as one glTF file with no materials; a part's material is a name, which the
  environment's file gives a look, read by the client and by Cycles alike. Everything is made in
  code but the rocks (a scanned CC0 set, made lighter): the maple is a branching trunk with
  sprays of leaves on cards, the gravel's furrows are in its texture and the mesh's mapping bends
  them round the islands, and what stands beyond the fence is two rings of pictures.
- Its light is made from the garden itself: Cycles renders all round from just above the table,
  under the same sun and sky as the client's, and that panorama is filtered into the cubemaps as
  the room's photograph is. So the board is lit by the maple's red and the moss's green as well
  as the sky, and a stone reflects the garden. What polished things reflect is dimmed to under
  half (a polarising filter): under an open, misty sky a lacquered board was veiled in white.
  The sky is a photograph with its sun clipped out, since the client's sun is a light of its own.
- Daylight needed an exposure of its own (EV 11.8, against a room's 9.7), four shadow cascades
  reaching 30 m where the room has two reaching 4 m, and a smaller aperture, so the garden is
  seen and not only the board. The camera goes back 2.2 m at most there, under the maple's
  boughs.
- Cost, on the maintainer's GPU: about 117,000 triangles in 77 meshes, 8 MB of textures, and the
  same GPU load as the plain table within what could be measured (the post-processing is most of
  both). The client grew by 17 MB.
- Not built: sound (water, wind, a bird); anything that moves but the water's ripples (leaves
  falling, the carp, a deer-scarer); a tea house to close the south side, where there is only
  the fence and a gap for a gate; settings for the hardware (shadow distance, the effects), which
  wait for more environments to measure; a light in the lantern after dark.
- **Next**: the maintainer's eye on it; then what it says.

### Patience: cards on the table (2026-11-22)

Solitaire and its variants (draw three, Spider of one, two and four suits, FreeCell) are
played, from Bingle's patience vocabulary (ADR 0122) and nothing of any one game's: any game
that declares a patience plays the same way.

- A game is played as a patience when it has cards and an action of the kind `patience`
  (`patience::plays`). Its board (one cell) stands aside, and its cards lie on a dark green
  cloth (`card_table.rs`): a row beyond with the stock and the waste, or the free cells, to the
  left and the foundations, or the finished runs, to the right, and the columns below it,
  fanned towards the player, closer when long. The vocabulary names the piles' zones
  (`t1`…, `f1`…, `c1`…, `stock`, `waste`, `done`), so the layout is read from the zones the
  position has: a game dealt whole (FreeCell) shows no stock. Each pile's place is printed on
  the cloth.
- A card's face is the usual deck's (`props::standard_face`, by its suit's symbol and rank);
  one this client may not see lies face down. The waste fans its top three.
- **Click and swap**: a click on a card a move starts with lifts it and the cards on it. They
  hover just beyond the pointer, bobbing, held near their edge so what they are aimed at stays
  in sight, and the pointer's arrow is hidden. Each pile they may go on glows; the one under
  the pointer shows them see-through where they would land. A click there puts them down and
  plays the move. A click on the pile they came from, on the bare cloth, a right-click or
  Backspace puts them back; a click on another card that moves takes it instead. A card sent
  home (`to-f`) is shown going to its suit's foundation when the view lists the suits
  (`patience::home`), since the server, not the click, chooses which. The stock draws, turns
  the waste back or deals, whichever is legal; *Finish* is the panel's button.
- A game of one seat has *Play* on its setup (`browser::setup_actions`), which makes the game
  with this client in its seat, and it starts at once; *New game* at its end makes another.
- The gestures the view says are drawn on the table (`panel_actions`) are no buttons, and the
  panel says to click where the cards go.
- Cards go where the position puts them by the hand's own carrying, one after another (a row
  dealt, a run off the table, *Finish*), over the piles and down; a card that was face down
  where it came from (drawn, turned up in its column, dealt) turns over on the way.
- Not built: dragging (the server's own words mention it; a press, move and release could
  make the same two gestures); the suit's mark on an empty foundation; lighting the cards that
  move (L lights a board's legal moves, not a patience's); the web page's flight home and
  celebration when a game is won; a themed card set (ADR 0085); a headset's ray pointer is not
  tried. (Bowling Solitaire, below.)
- **Next**: the maintainer's eye on it, and a game played through.

### Bowling Solitaire: a card game laid by its look (2026-11-23)

Bowling Solitaire is not a patience: its rules are written in Go, and a play is pins, then the
ball that knocks them down (`pin-e`, `pin-f`, `pin-g`, `ball-1`). So its look lays it out, and
shared code names no game:

- An appearance file's `[cards]` (`bowling-solitaire.toml`) says where each zone's pile lies
  (in metres, the whole centred on the table by itself) and how its cards lie, the choice a
  click on it makes, which choices are made `together`, and the view's score sheet. Any card
  game may have one; a game with cards and one is laid on the cloth as a patience is
  (`card_table::on_table`), its cards from the view's `table` when the position keeps none of
  its own (`Position::card_zones`).
- The pins are a rack, the back row farthest from the player; the three ball piles stand to
  the right in line with the front pin, and what is out of play is fanned beyond them.
- A click on a pin chooses it, or lets it go, in any order (`card_table::Together`, kept by
  the client, as the web page keeps it); a chosen pin hovers over its place. The ball piles
  whose top card knocks down exactly the pins chosen glow, and a click on one throws it: the
  play's gestures are sent in the order the server lists them (`throw`, `Turn::enter_all`,
  since `Turn` alone takes a pin clicked first as ambiguous when one play reaches it straight
  away and another after another pin). The ball and the pins are carried out of play, and the
  next ball turns up. A click on the bare cloth, or Backspace, lets the pins go. *End this
  ball* is the panel's button.
- The panel shows the score sheet (the view's `bowling`): ten frames with their marks and
  totals, the one being played outlined, and the score.
- A game of one seat is started with *Play* (the patience's work), so it is started from its
  card in the main screen.
- Not built: which pins a ball could take, lit before any is chosen (L); the score sheet's
  strike and spare marks are the server's, drawn as text.
- **Next**: the maintainer's eye on it, and a game bowled through.

### Private games: the private folder (2026-11-24)

The games Bingle builds from its private overlay (ADR 0127) are drawn from a folder of looks
this repository may not hold (decision 2). This repository names none of them, here too.

- **The private folder** is laid out as `assets/` is: `appearance/*.toml`, and any models,
  textures, sounds and pictures of its own. `--private-dir`, else `BINGLE3D_PRIVATE_DIR`, else
  `config.toml`'s `private_dir` names it. Its files are read once at start-up and found by the
  same paths as ours (`assets::all`), so a private look names our models (the disc and the
  ring) or its own the same way. A file in place of one of ours is left out, and logged: a
  private look adds to the client, and never changes what another game draws.
- **A private game is shown when the folder has its look** (`Appearances::private_games`,
  `session::public`); without one it stays hidden, as before. The binary is the same for
  everyone.
- **Its home is the overlay**: `bingle-3d/assets/` in `bingle-private`, beside the server's
  code for the same games, with an asset pipeline of its own there when a game needs models of
  its own. `cargo test` with `BINGLE3D_PRIVATE_DIR` set also checks the folder's looks (they
  read, and name only files and materials that exist), for the overlay's CI.
- **The first** (its look in the overlay) is a game of rings and two-sided markers on a points
  board: the ring and disc models (#48), each seat's side of a marker up and a marker turned
  over where it lies (`sides.rs`), and the view's tints stamped round what stands there. It is
  watched and played from the generic code, with no change to it.
- **Its pieces cut from marble** (2026-11-25): the disc and the ring are flat on top with sharp
  edges, as cut from a slab (Reversi's larger disc stays a turned counter), and the ring has
  a second part, `trim`, a band round each edge of its top. Two polished marbles, white and
  black, are the pipeline's (`minerals.py`), each seamless so that a piece mapped in metres
  is cut from it at its real size. A material may be a metal (`metallic`), and **a seat's
  look may give a part of its pieces' models a material of its own** (`seats.<word>.parts`):
  the look trims one seat's rings with polished gold and the other's with polished silver. A
  part that neither the piece's look nor its seat's names is drawn in the piece's own
  material, so a model that gains a part is whole under a look written before it had one.
  Every piece of one kind is cut from the same place in the marble (a model has one mapping),
  told apart only by the way each is turned.
- Not built for it, and generic when they are: what a selection turns over, shown before the
  move is sent (the server says only what it takes, `captures`: a Bingle issue); a count of
  the view's in trays (they follow `GameInfo.counters`); what a seat has to place at setup
  beside the board when nothing counts it; and the piece held under the pointer when a
  selection does not name it (a disc is shown where a ring is placed); and bingle#293's
  `converts`, now asked for.
- **The second** is played with move cards: each card shows where a piece may go, and a seat
  chooses one of its own, then a piece and where it goes. **Move tiles** (`move_tiles.rs`,
  the look's `[move_tiles]`): each card of the view's is a tile of pale wood beside the board,
  the grid cut into it and its squares stamped in ink (the piece's in black, where it may go in
  vermilion), its name engraved under the grid with the coordinates' glyphs and a seal in its
  stamp's seat's colour beside it. Everything on a tile is drawn from the view (its moves, as
  its holder sees them, and its name), so no card is in this repository. A seat's tiles lie in
  front of it, reading from its side, and the side's at the right hand of the seat on impulse,
  which takes it next; the camera frames them with the board. A click on a tile the seat may
  choose (they glow) makes the look's choice and the card's id; the one chosen is lifted until
  the move is played; when the cards change hands each tile travels to its new place, rising
  over the board on the way. The tile and the ink stamp are the pipeline's (`move_tiles`).
- Not built for it: a hint on the tile of where the chosen card would take the piece under the
  pointer, and the tiles in the showcase. (The squares the view tints are its look's stamps.)
- **The third** has no board: tiles laid on the table, which grow as they are placed and moved,
  and climb on each other. **A game with no board** (the look's `[board] on = "table"`, as the
  web page's `style: free`; the API does not say it) has no slab, lines or coordinates: its
  positions are unseen cells on the table, each lit when a click there would be taken, and
  raised to the top of what stands on it, so a climb is lit where it is seen (`raise_cells`).
  The server's field is far wider than play, so the camera, and what stands beside the board
  (the supply), keep to the cells within five of the middle (`table_frame`); the field keeps
  the layout's pitch, so its tiles are a little smaller than a boxed set's. **A piece's own
  parts** (the look's `parts`): its model's parts after the first in materials of their own,
  the same on every seat's piece, as a child of the piece made wherever the piece is
  (`dress_parts`, as `dress_sides` dresses a disc): an engraving's paint in a tile of its
  seat's colour. The tiles are the overlay's own models, made by a pipeline script there that
  uses this one's helpers.
- Not built for it: a seat's supply lying face up (it stands on edge, as any flat piece's
  does), and the parts in the showcase.

### Downloaded looks: a game's look from the server (2026-11-26)

A server may offer the look of a game this client does not ship: an install pack carries a
folder for each client, `clients/bingle-3d/`, laid out as `assets/` is, and the server lists and
serves it without reading it (Bingle's ADR 0147, *Every client's files*; API 1.17, bingle#302).
The client fetches it, checks it, keeps it, and draws the game with it exactly as with a
built-in or private-folder look, so one build of the client plays a private game on any server
that offers its look (bingle#303). This repository still names no game: its test pack's look is
for Bingle's own Draughts fixture.

- **Knowing what a server offers**: a game's `clients` (in the list and in its info) name the
  clients the server has files for. A game the server marks `private` is shown when it offers
  this client a look, or the private folder has one (`session::public`). The client requires
  API 1.17.
- **Fetching** is the network thread's (`downloads/fetch.rs`): the index first
  (`GET /api/game-defs/{id}/clients/bingle-3d`), then only the files the cache lacks, each a
  mebibyte at a time by byte range (`If-Range` its hash), so the thread answers everything else
  in between, a download broken off goes on from where it stopped, and one abandoned stops at
  once. A game's whole look is fetched when the player opens it, unless the client has a look
  of its own for it (built in, or in the private folder: how a look is tried before it goes
  into a pack); the game waits to be drawn, its panel says how far the fetching has got, and
  *Play without it* draws it from the defaults at once. What happened in the game meanwhile is
  read again once it is drawn. The list asks, for each family whose look the server offers, for
  its appearance file and its card's picture only.
- **Keeping** (`downloads/cache.rs`): files in the platform's cache directory, each by its
  SHA-256, so a file shared by two variants, two versions or two servers is fetched once; the
  index each server last gave for each game beside them (used when the server cannot be asked).
  The cache keeps 2 GiB, and drops what was used longest ago. `BINGLE3D_CACHE_DIR` names another
  folder.
- **Trust** (`downloads/check.rs`): everything from a server is hostile until checked, against
  the client's own limits, whatever the server's were:
  - each file matches its hash when it is fetched and each time it is read;
  - only appearance files, binary glTF, PNG, JPEG, KTX2 and Ogg Vorbis are taken; nothing else
    is fetched. A look is data and never code: no shader, no script;
  - at most 1,024 files and 256 MiB a look, 32 MiB a model or picture, 8 MiB a sound and 1 MiB an
    appearance file;
  - what a decoder is given: pictures at most 4,096 pixels a side (a model's own too), a model's
    meshes at most 500,000 vertices, no list in it over 4 million, and no buffer or picture it
    names outside itself (which the asset server would follow); a sound at most 60 s;
  - paths keep to a look's folder. A fetched file is found under `downloads/<sha256>.<kind>`,
    never by its path, and the look that names it is renamed to match, so it adds to the client
    and never replaces a file the client ships or another game's. A file a look names (a model,
    a picture) by one of the client's own paths or its private folder's is not taken: the
    client's own is used;
  - a look that cannot be read is left out, as a whole, and logged, and the game is drawn from
    the defaults: one written for a later format, one that does not say its format, one that
    names a file it did not bring (or that was refused) and the client does not have.
- **Drawing**: the look is an appearance file like any other, for the game it came with alone,
  whatever its `games` say (a pack's game ids carry its version), and its card's picture is the
  one it gives that game, or its only one. It may name the client's own models, materials and
  sounds, as a private look does. Of several appearance files in a look, the one that names the
  game is read, else the only one.
- **The appearance file has a format number** (`format`, now 1): a look from a server must say
  it; a built-in or private one may leave it out.
- **Tried** against a Bingle built from its main branch with the Draughts fixture pack, marked
  private, and this repository's `tests/pack/clients/bingle-3d/` as its folder
  (`scripts/pack-server.sh`): the game is listed with the pack's picture, and opening it fetches
  its look (jade and carnelian checkers, its own textures) and draws it.
- Not built: fonts or environments in a look; what was refused shown anywhere but the log.

### A server's setup file (2026-11-26)

A Bingle server can hand its players this client and a setup file made for the server and for
whoever asks (Bingle's ADR 0151, bingle#309): which server, and who to be there. A client run
for the first time then starts as that player with nothing to type (#82).

- **The file** is `bingle-3d-setup.toml`: `format = 1`, `server`, and, as the player was in the
  browser, an account's `name` and `sign_in_code`, or a guest's `guest_key`; and
  `certificate_sha256` from a server with a certificate it made for itself. `setup.rs` reads
  it, with no Bevy in it.
- **Found** by `--setup FILE`, else beside the program, which is where a player unpacking a
  build can put it and where the Steam Frame's upload takes it from.
- **Taken into the config store before anything connects**: the server, the name and the
  certificate (a `trusted` entry for the server's origin) to `config.toml`, a guest's key to
  `keys.toml` (and a session kept for that server forgotten here: the file says who to be),
  and `auto_login` on for a guest. An account's is turned on once its code has signed in, with
  what the login screen remembers: a code the server turns down must not leave the client
  going in by itself as a new guest.
- **A sign-in code is kept nowhere.** It is the run's first request (`Request::SetupSignIn`,
  `POST /auth/login {code}`, API 1.18), sent from the login screen: signed in, the main screen
  follows as after a sign-in typed there; refused, the login screen says what the server said,
  with the server filled in. The guest key goes with it as with any sign-in; one the server no
  longer knows is refused before the code is looked at, so the code is then tried alone.
- **Deleted once used up**, since it holds a credential: at once without a code, and when the
  code is spent or refused with one. Not reached, the server leaves the code good, so the file
  stays for the next start. A file that cannot be taken (another format, a server the client
  may not use, a config that cannot be written) is left where it is and the player is told, on
  the login screen too, which now shows the client's notices.
- **Its server is the run's**, whatever `--server` says: the code is good nowhere else.
- **`release.json`** is published with each release (`scripts/release-json.sh`: the version and
  `API_VERSION`), for a server to offer only the builds its API can be used by.
- Not built: a private folder found beside the program by itself; a link that opens the
  installed client with the same code and no file (#77).

### A card game of several seats: a table laid round its seats (2026-11-25)

The first game with hands of cards that each player keeps to themselves, and the first for
more than two seats, is one of the overlay's (so it is not named here, and its look is there).
What it needed is generic, and any card game of several seats is drawn the same way from a
look of its own:

- **A seat's own view.** A game whose rules are written in Go (`engine: "ruleset"`) may keep
  secrets from a seat, so the server sends everybody a public view of the position, on the
  socket too. A client that holds a seat now reads the position as that seat
  (`/state?player_name=`, as the web page does): when the game is opened, when it is read
  again, and for every event, on the network thread, before the event reaches the table
  (`net::own_view`), so the player's hand is never drawn face down and then turned up. A
  guest who holds several seats (a game played against oneself) plays, and sees as, the one
  on impulse (`Table::yours`).
- **A table laid round its seats** (`Layout::seated`, the look's `[cards.piles.<zone>] seat`):
  a zone each seat has lies in front of its seat, on the seat's own terms (to its right, and
  from the middle towards it), the cloth is round, and the whole is seen from the seat this
  client plays, with play going to the left. A hand `held` is the player's own fanned in
  front of them and tilted up to them, each card over the one to its left so its corner is
  read, and anybody else's **standing in the air beyond the table's edge, its back to the
  table**. A pile `played` (a trick) lies in the middle, each card a little towards the seat
  that played it, as the view's captions say who did. A pile shown `unless` another has
  cards (the trick before) stays where it was played until the next is led, and a card it
  shows is not also drawn where else the position has it. A `row` shows what a seat has
  taken. A card the client may not know shows its back from both sides.
- **A click on a card of the hand plays it** (`[cards] play`: the look's choice, then the
  card's kind; the game says whether that plays, discards or passes it). The cards that may be
  played stand a little out of the fan while not all may be, and the one under the pointer
  further; the pointer's hand points at one.
- **A board of choices** (`choice_grid.rs`, the look's `[grid]`): the view's lines of boxes
  as a board of wood on the table, a tile for each box with the look's picture for it and
  each line's picture at its head. A click on a box makes its choice. A box a seat crossed
  has that seat's marker on it, small, carried there from in front of the seat; a box out of
  the game is covered; the box left on a line has a brass rim; with the legal moves lit (L)
  the boxes that may be crossed are.
- **Seat markers** (`seat_markers.rs`, the look's `[markers]`): a piece for each seat, by its
  colour word, standing in front of the seat and looking to the middle, the part the look
  names in the seat's own colour. Once the view puts the seats on sides, the markers of one
  side are made of one material and the other's of another (pale wood and dark, as a chess
  set's), so partners are seen to be; the seat the view names has a ring round its marker's
  foot. Clicks on the markers make the choices that name seats (a team picked by its
  members): one that no other choice names more seats than is made at once, a seat that may
  be one of several hovers until another is clicked or the player's own marker settles it,
  and the player's own marker alone makes the choice that names nobody. The panel's buttons
  make the same choices, in the view's words for them (`view.labels`).
- **A move of several selections is drawn as the server previews it** (`WatchStatus::shown`):
  the seat that picks the team goes on to choose a card in the same impulse, so nothing is
  sent yet, and the cards, the grid and the markers are drawn from the preview of the
  selections made (the team's woods, the ring, the card gone from the hand) until the move is
  played or taken back.
- **The panel counts for each seat** what the look names of the view's counters
  (`[panel] counters`: points, tricks taken), under the seat's name, with the view's labels.
- **A deck numbered from 1 to 13** has faces too (`card_faces.py`): `1`, `11`, `12` and `13`
  of each suit beside the usual 52, found the same way (`props::standard_face`).
- The pictures and the markers' models are the look's own, made by scripts in the overlay
  that use this pipeline's helpers.
- **After the maintainer's first games (2026-11-25)**:
  - A player's hand could not be read once the team had been picked. A move event's `seq`
    counts selections and a state's `move_seq` counts moves, and the client took them for
    one number: after the first move of two selections (a team, then a card) the seat's own
    view never matched the event it was read for, and the public view, with the hand face
    down, was kept. An event's state now says the moves played after it (from its `ply` and
    `ends_impulse`), and the table follows the socket's count of selections apart
    (`Table::seq`). A turn is no longer asked again halfway through a move of its own.
  - A box of the grid that may be crossed is lit while the pointer is over it, and the
    pointer's hand points at it. The pointer over a box, or a line's head, shows a card of
    words beside it: what the line decides, and what that box would make the rule, as the
    view says (`label`, `hint`), so no picture has to be guessed at.
  - A marker may have several parts in its seat's colour (`seat_parts`: its plinth, its eyes).
- Not built: a card dragged to the middle; the cards a seat may play lit (they stand out
  instead); a place printed for a pile on a round table; a taken trick carried to its
  winner's pile (it leaves the middle when the next is led, and the pile has grown already);
  the round table in a headset is not tried.
- **Next**: the maintainer's eye on it, and a game played through with other people.

### After that

- The rest of a game: clocks, undo and take-back requests, draw offers, chat, move list and replay;
  reserves and pools; tints, badges and counters by name; card zones for card games that are not
  a patience.
- More games: boards that are neither a grid nor hexagons, reserves beside the board (Tak's
  stones in hand), drawn from their definitions.
- Appearance (begun in stage 9): a camera per game; hand-made looks for a few more games that
  reward it; more of the private overlay's games (*Private games*).
- Maybe macOS, gamepad and Steam Deck controls; tutorials and puzzles. (Release builds:
  decision 11.)

### VR: usable (2026-11-21)

The aim is the same table in SteamVR, through OpenXR: first with the headset's controllers, a
ray and a trigger being the same gesture as a click, and later with hand tracking, pinching a
piece. It was to wait for the desktop client (decision 6); decision 10 starts it beside it.

What the desktop client has kept for it:

- **Real-world scale**: one unit is a metre, the table top is at y = 0 and a board is the size of
  a real one.
- **Input becomes gestures in one place**: nothing but picking and the flat camera reads the mouse.
- **The camera is replaceable**: nothing depends on there being exactly one camera or on where it
  is.

Steps:

1. **A spike** (built; it runs in the maintainer's headset through SteamVR on Windows):
   `--vr`, in a build with the `vr` feature, starts an OpenXR session, and the environment
   chosen is drawn for each eye with the player sat at the table (`src/vr.rs`). An eye has the
   environment's light and MSAA, and none of the flat camera's effects. The window keeps the
   interface and the flat camera. With no runtime it says so and runs in the window alone.
2. **A ray from each controller** (built; it works in the maintainer's headset): each is one of Bevy's
   pointers (`src/ray_pointer.rs`), so its trigger on a cell or a piece is the click the mouse
   makes, through `play.rs` unchanged. A face button plays the move and another goes back.
   The sticks move the player about: ahead and back and sideways, a turn, up and down.
3. **A menu in the headset** (built; it works in the maintainer's headset): the left hand's
   menu button opens a panel in front of the player (`src/vr_panels.rs`), Bevy UI drawn into
   an image by a camera of its own and pressed with the same beam (`src/world_panel.rs`,
   `ray_pointer::Panel`): the move, sitting again, what is shown, the place, quit.
4. **The game's panel in the world** (built; it works in the maintainer's headset): the window's own panel, given to a
   panel beside the table while a headset's session runs, and back to the window after. And
   **starting in the headset by itself** when a VR runtime is already running (its server
   process is looked for: SteamVR's `vrserver`, Monado's, WiVRn's), as games do; `--vr` and
   `--desktop` decide instead, and a run that `--frames` or `--screenshot` limits does not
   look.
   Bindings are suggested for the controllers OpenXR's core names, and SteamVR maps the
   headset's own onto them.
5. **The rest of the interface in the world** (built; it works in the maintainer's headset): every UI root of the
   window's but the game's panel is given to a screen across the table while a session runs
   (the login screen, the main screen, friends), shown while no game is watched; the right
   stick scrolls a panel the beam is on; and a keyboard of our own (`src/vr_keyboard.rs`)
   stands before the player while a field that takes typing has the focus, sending what the
   real keyboard would. SteamVR's own keyboard belongs to OpenVR, not OpenXR: using it would
   mean loading Valve's `openvr_api` library beside OpenXR, so it is set aside. **The escape
   menu** has a panel of its own, opened in front of the player by pressing the right stick
   (or Esc), or the Steam Frame's right *Menu* button. SteamVR shows the Frame's controllers to
   an OpenXR game as Touch controllers, with no right Menu button, unless the game asks for
   Valve's `XR_VALVE_frame_controller_interaction`; the client asks for it (a runtime without
   it goes on without), and suggests bindings for the Frame's own profile where it is taken.
6. **On the Steam Frame itself** (built; it runs on the maintainer's Frame, with no PC): a
   build for arm64 Linux, made on x86-64 against Valve's Steam Runtime 4 arm64 SDK
   (`scripts/frame-sysroot.sh`, `scripts/release-build.sh steam-frame`), with the OpenXR loader
   built for it in `lib/` beside the binary, uploaded by Valve's SteamOS Devkit Client
   (README). It is far too slow to play for now: the Frame's graphics are a phone's, and the
   scene was tuned on a desktop card. Next: a lighter standalone preset (each eye's
   resolution, shadows, MSAA, a lighter garden), measured on the device.
7. Sitting at the seat played; comfort and settings; the window showing what the headset sees.
8. Hand tracking, and picking a piece up.

Open: the headset sets the room's floor aside (the head is put at a sitter's eye point, so any
chair does); whether the floor should count where the player really kneels. CI does not build
the `vr` feature yet (the release workflow does, on a tag).

A note on hands: [Frametop](https://github.com/Frametop/frametop) tracks hands on the
Steam Frame itself and publishes them to files rather than through OpenXR, so using it would mean
running the client on the headset. It is young and changing; look at it again then.

## Decisions

Settled 2026-11-16:

1. **Assets ship with the client, and a game's look may come from the server.** Models,
   textures and sounds live in this repository and are packaged with the binary. A server may
   offer the look of a game the client does not ship (an install pack's, Bingle's ADR 0147),
   which the client fetches, checks and keeps (*Downloaded looks*, 2026-11-26).
2. **Private games' appearance is loaded from a folder at run time.** The binary is the same for
   everyone; a private build is the public binary plus a folder of appearance files from
   `bingle-private`, found by a configured path. Shared code still names no game. (Built
   2026-11-24: a private game is shown when that folder has its look; *Private games*.) It
   stays beside looks from a server: for the overlay's games, and for making a look before it
   goes into a pack, since a look in the folder is used before one the server offers.
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

Settled 2026-11-20:

7. **Voice is WebRTC through `rtc` 0.21 with `rusty-opus`**, on a thread of its own. A spike put
   both `rtc` and `str0m` against Chromium, each sending and receiving Opus with nothing lost.
   `str0m` was set aside: its pure-Rust crypto still enables a certificate generator that pulls in
   aws-lc-rs, which the client keeps out (it needs cmake and nasm on Windows). `rtc` builds on
   *ring*, `rustls` and `rcgen` alone (77 crates more, none of them C but *ring*). Both do no I/O
   of their own, so the client owns the sockets. The voice thread is the one exception to all
   network I/O being on the network thread: a frame must go every 20 ms, and that thread waits on
   blocking calls. It shares nothing with the network thread but channels; the signalling still
   goes through the network thread, as every call to the server does.

Settled 2026-11-20, with the maintainer:

8. **Assets are made by scripts, from CC0 sources.** Blender (pinned to 5.3) runs headless on
   Python scripts in `assets-src/`; textures and light are scanned CC0 material (Poly Haven),
   fetched by URL and SHA-256, never committed as sources; models and procedural textures are
   made by the scripts. Sounds are synthesised by a script of their own
   (`assets-src/sounds/sounds.py`: Python's standard library, then ffmpeg for Ogg Vorbis, which
   Bevy's default audio already reads, so no crate is added), nothing recorded. What they make is
   committed in `assets/` and compiled into the binary (decision 1). Appearance files are TOML. Hex's look: slate and clamshell stones on a wooden
   board of the player's choosing, each side's goal edges coloured to match its stones.
9. **Where the table stands is the client's, and each player's.** An environment is chosen on
   this client and kept in its config; the server is not told and the game does not depend on
   it, so two players at one table may be in different places. An environment is data and a
   model made by the pipeline (decision 8), never code, and whatever it is the table top is at
   y = 0 with its middle at the origin. The first is a Japanese garden.
10. **VR starts now, beside the desktop client, behind a feature.** Decision 6 kept it until the
    desktop client worked; with environments built, the interface, the cameras' effects and
    gestures are better shaped knowing what a headset needs. It is OpenXR through
    `bevy_mod_openxr`, the only maintained way from Bevy to SteamVR, an optional dependency of
    the `vr` feature so that the usual build and CI are as before. Until it is released for
    Bevy 0.20 it is taken by revision from the pull request that ports it
    (awtterpip/bevy_oxr#212); move to the release when there is one.

Settled 2026-11-21, with the maintainer:

11. **Releases are cut by tags, with builds for Windows, Linux and the Steam Frame.** Pushing
    `vX.Y.Z` (Cargo.toml's version) runs `.github/workflows/release.yml`, which builds the three
    with the `vr` feature and publishes them as a GitHub release; a merge cuts nothing. Linux
    is built on the oldest supported Ubuntu (glibc 2.35) and ships the OpenXR loader beside
    the binary, found through its runpath (`$ORIGIN/lib`); Windows links the loader in. The
    Frame's build is cross-compiled against Valve's Steam Runtime 4 arm64 SDK sysroot, pinned
    by version and SHA-256, and runs in the Frame's `steamrt4 arm64` runtime. The first is
    0.0.1.

Proposed 2026-11-22, for the maintainer to settle (the patience pull request):

12. **A patience's move plays as soon as it is whole.** Every other game previews the move
    that ends the impulse and waits for Enter or a second click; in a patience the click that
    puts the cards on a pile is that second click (the pile showed them there, see-through,
    before it), and a game is hundreds of moves. That holds for a move the server says may
    end the impulse as well as one that must (one that turns no card up may be followed by
    more in the same impulse, ADR 0121): each is sent as it is made. Nothing is taken for the
    player either,
    however few moves there are (`Turn::clicks_only`): a lone move is not made by itself.

Proposed 2026-11-23, for the maintainer to settle (the frame limit's pull request):

13. **The window draws at most 60 frames a second by default.** A board game needs no more,
    and the monitor's pace (vsync) on a laptop panel can be 144 or 240: at 240 a frame every
    4 ms, each the whole table with its effects, kept two and a half cores and the GPU busy for
    nothing. Measured on the maintainer's 2080 Ti, at the 1280×720 window, showcase: uncapped
    ~250 fps, 2.6 cores, 160 W at the GPU; at 60, 0.65 of a core and 71 W, against 59 W idle.
    The effects themselves are cheap (about 1.1 ms of GPU a frame, SSAO the largest at 0.22),
    so they stay. `max_fps` in `config.toml` sets it (0 is the monitor's pace), `--max-fps`
    for a run; a settings screen will show it. In a headset there is no limit of ours: the
    runtime holds each frame for the headset's display, and a wait of our own would only make
    frames miss their turn. What is chosen there is the headset's own display rate
    (`vr_refresh_rate`, `--vr-refresh`: 72, 90, 120…, the nearest of those the session
    offers), asked of the runtime through `XR_FB_display_refresh_rate`, so it is the rate of
    the headset streamed to, never the PC monitor's. A runtime without the extension (SteamVR,
    as far as is known; the Frame's runtime is untried) keeps its own setting, and the log says
    so. On a 144 Hz monitor 60 does not divide the refresh and a camera turning may judder:
    72 or 0 there.

Proposed 2026-11-23, for the maintainer to settle (the fullscreen pull request):

14. **The client starts fullscreen, at the desktop's resolution.** That is borderless
    fullscreen on the monitor the window opens on: nothing is switched, and the resolution is
    the one the monitor is already in, so it needs no detecting before the window exists. A
    resolution chosen in *Settings* is, in fullscreen, an exclusive mode of the monitor's own
    of that size (its highest refresh rate), greyed where the monitor lists no such mode; where
    the platform cannot switch (Wayland), winit falls back to borderless at the desktop's
    resolution. In a window it is the window's size; *Any size* leaves it to the player.
    Resolutions larger than the monitor are not offered. A run limited by `--frames` or
    `--screenshot` stays a 1280×720 window (`--fullscreen` to override), so a script's
    picture is the same everywhere; a headset leaves the window alone.

Proposed 2026-11-25, for the maintainer to settle (the upload memory's pull request):

15. **Uploads wait in system memory, never in a small window into the card's.** On the
    maintainer's 2080 Ti the client drew 10 frames a second on the desktop, at any resolution,
    with 4 ms of the card's work in a frame. wgpu's allocator puts the staging buffers every
    upload goes through in memory that is on the card and mapped for the processor: on a card
    without a resizable BAR, a window of 256 MB (a heap of 246 MB) the whole desktop shares.
    It is taken 64 MB at a time, then 128; the textures loaded at start fill the first block,
    the second takes the window past its budget (196 MB, 50 of them other programs'), and from
    then on a write to it runs at about 18 MB a second: a 256 KB buffer took 28 ms to make,
    against 0.08 ms. A frame uploads a megabyte or so (the lights' clusters, the interface's
    vertices), so it took 100 ms. Three runs in four went that way; under gamescope, and in a
    run whose first block was enough, the same build drew 60. The allocator falls back to
    ordinary memory when the card refuses a block, so the client asks for blocks of 256 MB,
    which that window can never hold (`src/gpu_memory.rs`, wgpu's `MemoryHints::Manual`): the
    same buffer then takes 0.03 ms, and eight runs in eight drew 60 at 3440×1440. A card whose
    whole memory is mapped keeps its uploads there. The cost is 256 MB of system memory taken
    in one piece at the first upload, where wgpu's own choice starts at 64. A headset's device
    is made by the OpenXR plugin with wgpu's defaults: not changed, and to be watched there.
    `--frame-times` logs a frame's time and each drawing pass's, which is how this was found
    (the passes were fast; the time was in Bevy's own systems writing buffers).
