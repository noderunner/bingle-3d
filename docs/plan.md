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
| API types | checked against the JSON Schema Bingle publishes (`schema/bingle-api-1.6.0.schema.json`): a test builds the smallest and the fullest message the schema allows and reads both | Catches a field that is renamed, required or retyped when the schema is updated |
| Voice | `rtc` (webrtc-rs's sans-I/O stack, on *ring*) and `rusty-opus`, on a thread of its own; devices through `cpal`; echo cancellation through `sonora` | Speaks what browsers speak with no async runtime and no aws-lc-rs; sound must leave every 20 ms, which the network thread's blocking calls cannot promise (decision 7) |
| Text | Fira Sans (SIL OFL), compiled into the binary | Bevy's built-in font is ASCII only, and game names are not ("Hex (11×11)") |
| Assets | Made by Blender 5.3, run headless from Python scripts (`assets-src/`), from CC0 sources listed with their hashes; the results committed in `assets/` and compiled into the binary | Scripts can be read, reviewed and run again by a person or an agent; a `.blend` file cannot. CC0 ships with no conditions (decision 8) |
| Textures | JPEG, given mipmaps by the client when they load (`src/textures.rs`); the room's light as prefiltered KTX2 cubemaps | Bevy 0.20 makes no mipmaps, and lossless KTX2 with them is ten times the size; Bevy's runtime-filtered environment light filters every frame |
| Appearance | TOML files in `assets/appearance/`, one per game (or family of variants), by game id | Data, never code; `toml` is already a dependency. Replaces "RON or YAML" in the plan before |
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
  disc at each point, which glows softly when it would be taken. An appearance file may mark
  positions with a dot, per game id (`[board.marks]`): Go's star points, for each size.
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
  game's appearance names it (`aimed`); asking the API for it belongs in Bingle. A chessboard's
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
    of its side given a carved head's thickness. Pale and near-black wood. Bingle has no chess,
    but Breakthrough (pawns), Amazons (queens on a checkered board, and a red checker for an
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
    rank's label as the server gives them (`props::standard_face`), so no game is named. 1 MB
    together. No jokers: no game deals one yet.
  - **A die**: six sides, pips hollowed out, turned to show a number (`props::die_showing`).
- No game's cards or dice are drawn yet (card zones, below): `props.rs` holds only how the two
  models are read. `--showcase` lays every piece model, the dice and some cards on the table,
  to look at an asset in the client's own light; `--card-face` and `--card-back` try a picture
  on a card, and `--view` takes a place on the table to look at.
- Cost: 3 MB of models. A chessman is 3,700 to 6,500 triangles, a checker 5,000, a die 2,900;
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
  screen; then card zones.

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
- Found while building it: the interface is blended in linear light (the cameras are HDR), so a
  faint white over the dark panels shows much lighter, and a nearly opaque panel much more
  see-through, than the same colours on the web page. The new parts use opaque colours.

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

### After that

- The rest of a game: clocks, undo and take-back requests, draw offers, chat, move list and replay;
  reserves and pools; tints, badges and counters by name; card zones for card games.
- More games: boards that are neither a grid nor hexagons, reserves beside the board (Tak's
  stones in hand), drawn from their definitions.
- Appearance (begun in stage 9): a camera per game; hand-made looks for a few more games that
  reward it; the private overlay's games' appearance loaded from a configured folder, without
  this repository naming them (decision 2).
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
