# Plan: a 3D client for Bingle

Status: stage 7 under way: a game can be played against the server's bots or a person, found,
set up and started from its lobby; the player can sign in to an account, and has friends
(2026-11-19).
Stage 4's and stage 5's last checks, against the web page by hand, are the maintainer's. The desktop client comes first: Hex, a game
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
| API types | checked against the JSON Schema Bingle publishes (`schema/bingle-api-1.6.0.schema.json`): a test builds the smallest and the fullest message the schema allows and reads both | Catches a field that is renamed, required or retyped when the schema is updated |
| Voice | `rtc` (webrtc-rs's sans-I/O stack, on *ring*) and `rusty-opus`, on a thread of its own; devices through `cpal`; echo cancellation through `sonora` | Speaks what browsers speak with no async runtime and no aws-lc-rs; sound must leave every 20 ms, which the network thread's blocking calls cannot promise (decision 7) |
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
  pieces and waits for a second click, Enter or *Play*, and Esc or Backspace goes back. The move is
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
