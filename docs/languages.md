# Plan: other languages in the client

Status: a plan, for the maintainer to decide whether and how far to go (#75). Nothing here is built and nothing
is translated. What it says of Bevy and its text crates was read from their sources at the versions in
`Cargo.lock` (Bevy 0.20.1, `bevy_text` 0.20.1, `bevy_ui` 0.20.1, `bevy_ui_widgets` 0.20.1, parley 0.11.1,
fontique 0.11.1, taffy 0.14.0, icu_segmenter 2.4.0), each with the file and line it comes from (a path is inside
the crate as published). What it says of Bingle was read from `origin/main` on 2026-11-28. The text engine was
tried in this client on the maintainer's machine (2026-11-27). What could not be checked says so.

## What was tried, and how

- **A count of the client's words**, by a script kept outside the repository (its method is under *1. Which
  words*).
- **Ten languages drawn in a panel of the client's**: a trial module, kept out of the pull request, spawned a
  panel styled as the client's (`browser::colors::PANEL`, 420 px wide, text at 15 px) over the login screen, with a
  sample sentence of the client's own words in each language, machine-drafted, three Arabic lines among them
  that mix in a Latin name, a handle and numbers as the client's built phrases do. Drawn twice: with Fira Sans
  alone, as the client ships; and with Fira Sans first and two candidate fonts after it, as one ordered list of
  fonts (`FontSource::List`, `bevy_text-0.20.1/src/text.rs:301`). And once more with the Arabic rows' nodes
  given `direction: Rtl`. Rendered on the host under `gamescope --backend headless` at 1280×720, a debug build,
  `--frames 200 --screenshot`, pointed at a port nothing listened on.
- **The same samples laid out by parley directly**, in a test of the trial, with the same settings Bevy gives it
  (`WordBreak::Normal`, no locale, 360 px wide), printing each line's text, each run's direction, the font and
  face it was shaped with and its glyph ids. This is what Bevy's own layout holds but keeps private
  (`ComputedTextBlock::layout` is `pub(crate)`, `text.rs:43`), and it lets joining and line breaks be checked
  without reading the script by eye.
- **What was not tried**: an input method (no compositor with one runs headless), the headset, Windows, and a
  release build's size with a font compiled in (the size a font adds is its file's, since `include_bytes!` puts the
  file in as it is).

The pictures are not in the repository (the pull request says where they are). What they show is described where
it matters.

## 1. Which words

### How they were counted

Every `.rs` file under `src/` was read as tokens: comments skipped, every string literal taken with its line and
the code before it, `#[cfg(test)]` items dropped by matching braces. Left out by rule: literals inside a log or
panic macro (`info!`, `warn!`, `panic!`, `assert!` …), `.expect(…)`, `include_*!`, an entity's `Name`, a lint's
`reason`; literals that are not words (no letter, a path, a URL, an identifier in snake or kebab case, a format
with nothing but placeholders, protocol fragments such as `Bearer {k}` or `&family=`). Left out by hand after
reading them in place: 18 literals that look like words and are not shown (`config.toml`'s values `off`, `low`,
`high` in `graphics.rs:53-108`, materials' and seeds' names). What remained was sorted by where it goes:
**screen** (panels, menus, hints, buttons), **messages** (an error or notice made by the network thread, the
setup file or the voice engine and shown in a panel), **headset bindings** (the OpenXR actions' names,
`vr.rs:467-477`, which SteamVR shows on its bindings page), **keycaps** (the headset keyboard's letters),
**terminal** (`--help` and option errors) and **log** (a game's look, a downloaded look's checks, the config's
problems, the environments' files). A phrase was marked *built from pieces* when it is a format with a
placeholder or a fragment (it starts or ends with a space, a colon or a comma); *number*, *plural*, *time* and
*list* by patterns, then read by hand (below). The heuristics are not exact: one `Debug` string
(`sign_in.rs:75`) was counted as shown, and a phrase spread over two `format!` arguments counts twice.

### What there is

| Where it goes | Phrases | Distinct | Words |
|---|---:|---:|---:|
| Screen | 315 | 270 | 1,041 |
| Messages | 76 | 69 | 572 |
| Headset bindings | 10 | 10 | 37 |
| Keycaps | 29 | 29 | 29 |
| Terminal | 39 | 32 | 846 |
| Log | 139 | 130 | 651 |

What a translation would have to cover is the first three: **about 350 distinct phrases, some 1,650 words.**
The terminal's 846 words are almost all `--help` (`cli.rs`), for whoever runs the client from a shell, and the
log is for whoever makes a look: both can stay English, as Bingle's server messages do (Bingle's ADR 0141,
*What stays as it is*).

On the screen, by module:

| Module | Phrases | Module | Phrases |
|---|---:|---|---:|
| `browser` | 49 | `lobby` | 15 |
| `friends` | 45 | `sign_in` | 15 |
| `game_panel` | 44 | `table` | 11 |
| `escape_menu` | 43 | `vr_panels` | 10 |
| `new_game` | 25 | `call`, `families` | 4 each |
| `voice_chat` | 21 | `hud` | 3 |
| `login` | 18 | eight others | 1 or 2 each |

The messages are mostly `net/session.rs` (33: the server could not be reached, is too old, answered oddly),
the voice engine's devices (10), the setup file (9), the certificate check (`net/tls.rs`, 9) and the address
rule (`net/server_url.rs`, 6). Most of them carry an address or an error from a library inside them
(`"{url} could not be reached: {e}."`, `net/session.rs:1306`), and the library's part would stay English.

Everything merged up to the base of this plan is in the count: the *Graphics* page (`escape_menu.rs:560-631`),
the chess clock's setting (`escape_menu.rs:550-552`), the drag hints (`game_panel.rs:587, 602, 605`) and a
game's own endings (`table.rs:282-305`). The jump arcs and the portal add no words a player reads.

### Built from pieces

About 65 of the screen's phrases are formats or fragments. The ones that must become whole sentences, since
another language orders them differently:

- **Where the game stands** (`table.rs:272-318`): a seat's label, then `" won{how}."`, `" to move."`; and
  `"A draw between "` + names joined by `" and "` + `"{how}."`. `{how}` is the server's ending words or, failing
  them, the reason's id with its underscores made spaces (`table.rs:286-287`).
- **A game in the list** (`browser.rs:897-935`): `"{name}  ·  {room}"`, players joined by `" vs "`, then
  `"waiting for players: {} of {} {} free"` with `seat`/`seats` chosen in the code, or `"{} to move"`; and the
  list's own description, `"{which}, {seats}; read again every {} s."` (`browser.rs:879-893`), where each of
  `{which}` and `{seats}` is itself a phrase.
- **Status lines with a middle dot**: `"Room {}  ·  {doing}"`, `"  · you"`, `"  · won"` (`game_panel.rs:239-269`);
  `"{}  ·  speaking"` (`voice_chat.rs:658-661`); `"Friends  ·  {n} waiting"`, `"Write  ·  {n}"`
  (`friends.rs:593, 720`).
- **Two lines that are one sentence**: `"Somebody can join it with the room code {},"` and `"or on the web at {}"`
  (`game_panel.rs:468, 476`).
- **A label and its value**: `"Fullscreen: On"`, `"Chess clock: Off"`, `"Frame limit: {n}"`, and the
  *Graphics* rows' name with a value from a list of `Off`, `Low`, `Medium`, `High` (`escape_menu.rs:487-631`).
  These are whole strings today, which is right; a translation keeps each whole.

### Numbers, plurals, times, lists

Read by hand from the screen's phrases:

- **Plurals chosen in the code**: `"1 player"` / `"{n} players"` (`browser.rs:940-941`, `families.rs:71-73`,
  with a range `"{lo}–{hi} players"`), and `seat`/`seats` (`browser.rs:917`). Russian has three forms for a
  count and Arabic six, so each becomes a plural key.
- **Counts with no plural today**: `"Friends  ·  {n} waiting"`, `"Write  ·  {n}"`, `"waiting for players: {} of {}"`,
  `"Score: {total}"` (`game_panel.rs:447`), `"{fps} frames a second"` (`frame_pace.rs:129`), `"Frame limit: {n}"`.
- **Decimals and sizes**: `"{:.1} of {:.1} MB"` (`game_panel.rs:66`), `"{fps:.0} fps"` (`hud.rs:66`), a
  resolution `"{}x{}"` (`display.rs:74`), `"MSAA 4×"`. Arabic may write its digits as Arabic-Indic and its decimal
  point as `٫`; Bingle's page leaves that to `Intl` (its `docs/languages.md:46-48`), which the client does not have.
- **Times**: the clock (`clock.rs:57-66`: `4:05`, `1:02:09`, `"{d} d {h} h"`), the time controls offered
  (`new_game.rs:61-84`: `"5 min + 3 s"`, `"1 day a move"`), `"Trying again in {} s."` (`browser.rs:1347`),
  `"read again every {} s."`, and `"Messages are kept for two hours."` (`friends.rs:846`, a number in words).
- **Dates**: none. The client parses the server's RFC 3339 times (`clock.rs:68-`) and shows only durations.
- **Lists**: handles joined by `" and "` (`lobby.rs:163-168`), drawn seats joined by `" and "` (`table.rs:296-300`),
  players joined by `" vs "` (`browser.rs:905-910`). Bingle's page uses `Intl.ListFormat` for these.

### Words not in the code

- **The places' names**: `assets/environments/room.toml:7` (`Table`) and `garden.toml:9` (`Garden`).
- **Words made from ids**: a wood's id capitalised (`wood_choice.rs:72-77`), a status the client does not know
  capitalised (`table.rs:316`), an ending's id with its underscores made spaces (`table.rs:286-287`).
- **The headset keyboard's caps** (29 keys, `vr_keyboard.rs:61-124`) and its labels (`Enter`, `Tab`, `Shift`,
  `Hide`, `vr_keyboard.rs:126-140`): a layout, not words; see *6. Typing*.
- **Words in the world**: the coordinates engraved on a board, and the move tiles' names, are drawn from a sheet
  of 36 glyphs, `0-9a-z` (`coordinates.rs:116`); any other character is dropped (`coordinates.rs:127-132`). The
  panels hung in a headset are the window's own UI drawn into an image (`world_panel.rs`), so they are whatever
  the window's text is.

## 2. Where the words live

### A catalog of the client's own, in the binary

- **The format is Bingle's** (its ADR 0141, *Decision*, first point): a JSON file per language, a key's words with
  `{name}` where a value goes, an object of plural forms (`one`, `few`, `many`, `other` …) for a key given a count,
  English complete and any key another catalog lacks read in English. The same codes: `en`, `es`, `fr`, `de`,
  `pt-BR`, `zh-Hans`, `ja`, `ko`, `ru`, `ar`. Keeping the format means the same tools, tests and drafting habits
  serve both, and a reviewer of a language reads both the same way.
- **Compiled in** (`include_str!`), as the fonts are (`fonts.rs:12-19`): the login screen speaks before any
  server is reached, and a client is not tied to the server's version. About 350 phrases is some 30 KB a
  language, 300 KB for ten.
- **Served by the server** was considered and is not recommended: the client's words are the client's, they
  change with its releases rather than the server's, and the login screen and the connection's errors have
  to be readable with no server at all.
- **What the client lacks that the page has**: `Intl`. A small module of its own: a plural rule per language
  (CLDR's rules for these ten are short; Chinese, Japanese and Korean have one form, Russian three, Arabic
  six), checked in a test against the forms Bingle's catalogs give each plural key (21 of them,
  `internal/i18n/catalogs/en.json`); a list joined per language; a number with the language's digits and
  decimal mark. `icu_plurals` would do the plural rules but is one more crate; the rules are a few dozen lines.

### Sharing with Bingle

- **Bingle's catalogs** have 1,802 keys each, the same in all ten, every other language's marked
  *Machine-drafted; not yet reviewed by a speaker of the language* in its `_review` entry
  (`internal/i18n/catalogs/*.json`). Of the client's 270 distinct screen phrases, **65 are word for word an
  English value of Bingle's** (`Resign`, `Join the voice chat`, `Pick a game to set up a match.`, `{} is calling`,
  `Waiting for players.` …); of the 69 messages, one. Those 65 can take Bingle's drafts as they are, with the key
  they came from recorded, so the two say the same thing the same way. Each needs a look: the same English can be
  a different sense (`Back` is the tour's in Bingle, `tour.back`).
- **The glossary** (`docs/glossary/<code>.md`) is the stronger link: the platform's terms (seat, room, lobby, bot,
  guest, resign, tutorial, friends, global chat, username …), each with a language's word for it, checked against
  Bingle's catalogs by `TestGlossary_EachLanguageHasEveryTermAsItsCatalogShowsIt` (Bingle's
  `docs/languages.md:129-134`). The client would copy the glossary's files (as it copies the API's schema into
  `schema/`) and a test would check that each of its keys that names a term says the glossary's word.
- **At run time the client reads nothing of Bingle's words.** Linking the two at build time (a git submodule,
  fetching at build) would make building the client depend on Bingle's repository; copying the few files, with
  a test that says when they differ, is how `schema/` already works.

## 3. Which language is used

What the server offers: `/auth/me` has `language` (what the interface speaks to this request in) and
`language_choice` (the one chosen, absent for the browser's own) since API 1.9.0 (Bingle's
`internal/auth/handlers.go:396-403`; ADR 0141, *Decision*). The server works `language` out as the account's
choice, else the `bingle_lang` cookie, else `Accept-Language`, else English (`internal/i18n/i18n.go:162-187`).
The client reads neither field today (`Me`, `src/net/api.rs:85-97`), and sends no `Accept-Language` and keeps no
cookie (its agent is set in `net/session.rs:1585`). So today the server answers a guest from this client in
English, and an account in its chosen language, and an account that chose *Browser* in English.

Proposed, in this order:

1. **A setting of the client's**, `language` in `config.toml`, with *Automatic* as its first choice, as the
   page's setting has *Browser (…)* (Bingle's `docs/languages.md:21-23`). It is this machine's, like the
   resolution.
2. **The account's choice** (`language_choice`), once signed in, when the setting is *Automatic*. The client
   follows it and never sets it: `POST /auth/language` changes the account's language for the web page and its
   mail too (Bingle's `handlers.go:533-562`), which a setting of the client must not do behind the player's back.
3. **The system's language**: `LANG`/`LC_MESSAGES` on Linux, the user's locale on Windows. `sys-locale` does both
   and is already built (Bevy's text depends on it, `Cargo.lock`), but would be a direct dependency; reading the
   two by hand is a few lines.
4. **English.**

A guest has no account choice, so it gets the setting or the system's language. The login screen, before any
server, uses 1 then 3. The client should also **send `Accept-Language`** with the language it shows: then a guest's
`/auth/me` answers in it, and whatever the server later translates (bingle#253 says its game information "take[s]
the reader's language (`Accept-Language` or a parameter)") arrives in it.

## 4. Letters

### What the typeface covers

The client draws every text with Fira Sans, Regular and SemiBold, compiled in (`fonts.rs:12-19`; 456,996 and
489,028 bytes, SIL OFL). Bevy's build has no system fonts: `bevy_text`'s `system_font_discovery` feature turns on
parley's (`bevy_text-0.20.1/Cargo.toml`, `[features]`), the client's `bevy` dependency does not ask for it, and
fontique is built without its system back-ends (its dependencies in `Cargo.lock` are no font-config library). So
the client has only the fonts it registers, and a character none of them has is drawn as `.notdef`.

Seen with Fira Sans alone (`fira.png`): English, Spanish, French (with `« Œuvre »`), German (with `ß` and a long
compound), Portuguese (`ação, não`) and Russian are drawn whole. Chinese, Japanese, Korean and Arabic are rows of
empty boxes; Arabic's boxes already come in right-to-left order, since the order is the layout's, not the font's.

### What the others need, and what it weighs

| Font | Covers | Licence | File | Binary | Found on this machine |
|---|---|---|---:|---:|---|
| Noto Sans Arabic (variable, all weights) | Arabic | SIL OFL 1.1 | 239,324 B | +0.24 MB | `google-noto-sans-arabic-vf-fonts` |
| Noto Sans CJK Regular (`.ttc`, JP KR SC TC HK and Mono) | Chinese, Japanese, Korean | SIL OFL 1.1 | 19,484,784 B | +19.5 MB | `google-noto-sans-cjk-fonts` 2.004 |
| Noto Sans SC Regular (subset OTF) | Chinese (Simplified) | SIL OFL 1.1 | 8,331,336 B | +8.3 MB | publisher's repository¹ |
| Noto Sans JP Regular (subset OTF) | Japanese | SIL OFL 1.1 | 4,533,028 B | +4.5 MB | publisher's repository¹ |
| Noto Sans KR Regular (subset OTF) | Korean | SIL OFL 1.1 | 4,644,748 B | +4.6 MB | publisher's repository¹ |

¹ Sizes from `notofonts/noto-cjk`'s `Sans/SubsetOTF/` as GitHub lists them; not downloaded. The Medium weights
(the nearest to SemiBold) are each about the same size again. Licences are as the Fedora packages state them
(`rpm -q --qf %{LICENSE}`).

- **Memory**: a font's bytes are kept once (`Font::from_bytes` makes a `Blob`, `bevy_text-0.20.1/src/font.rs:39-41`,
  and fontique registers that same blob, `font.rs:93-99`), plus each glyph drawn in an atlas. Resident memory at
  frame 150 was 1,708,008 kB with Fira alone and 1,735,752 kB with the two Noto files loaded: about 28 MB more for
  19.7 MB of fonts and the glyphs drawn. One run each; the debug build's resident memory moves by a few MB between
  runs, so read it as "about the font's size".
- **The binary**: Fira's two faces are 0.95 MB today. Arabic adds a quarter of a megabyte. CJK as one collection
  adds 19.5 MB; as the three regional subsets, 17.5 MB for one weight. Loading **only the face of the language
  shown** (one of the three, read from the binary when that language is chosen) keeps memory at one font's size
  even if all three are compiled in.

### Shaping, joining and order (Arabic)

Seen in the picture with Noto Sans Arabic (`noto-zoom.png`), and checked in the layout report:

- **Joining forms are applied.** `ب` alone is glyphs `[314, 14]` (the dotless body and its dot), and `ببب` is
  `[314, 15, 314, 16, 314, 19]`: three different bodies for the final, medial and initial forms. In the picture
  every word is drawn connected, `لا` included.
- **Runs are in right-to-left order.** In `انقر على قطعة … اللاعبين:` the layout's first run on the line, leftmost,
  is the colon and space from the end of the text, and the first word is rightmost; the full stop and the `3`
  of the next line end up at its left, as they should.
- **A Latin name inside Arabic follows the bidirectional algorithm, which is not always what is meant.** In
  `فاز @alice بالاستسلام في Hex (11×11).` the runs come out as `alice`, then ` @`, then `فاز`: the `@` is a neutral
  between Arabic and Latin, so it goes with the Arabic, and the handle reads **`alice@`**. With the handle and the
  game's name wrapped in first-strong isolates (U+2068 … U+2069) the same sentence gives `@`, `alice`, in that
  order: parley honours isolates (`parley-0.11.1/src/bidi.rs:166-196`), and the two marks drew nothing visible.
  So **every value put into a translated phrase goes inside isolates**, as Bingle's page puts names in `<bdi>`
  (its `docs/languages.md:80-81, 98`).
- **The paragraph's direction is guessed from its first strong letter.** Bevy cannot set it: parley is asked with
  no base level (`parley-0.11.1/src/analysis/mod.rs:538-546`, the `None`), and so takes the first strong character
  (`bidi.rs:83-85`). `Hex  ·  ABCD: في انتظار @alice` is laid out left to right, since `H` comes first. An Arabic
  phrase that may begin with a name needs a right-to-left mark (U+200F) in front: with it the report shows the
  line right to left, `Hex  ·  ABCD` at its right.
- **Alignment follows that guess, not the panel.** `Justify::Start` aligns a right-to-left paragraph to the right
  (`bevy_text-0.20.1/src/text.rs:250-251`), and in `noto-rtl-zoom.png` the two Arabic-first rows are right-aligned
  and the `Hex`-first row is left-aligned in the same panel. The client sets no `Justify` anywhere today, so it
  has the default, `Left` (`text.rs:237-238`).
- **Not judged**: whether the shapes are those an Arabic reader expects (the kashida, the lam-alef's form, the
  spacing of the dots), and whether the machine-drafted sentences are good Arabic. Needs a reader of the
  language.

### Line breaks (Chinese, Japanese, Korean)

Bevy lays text out with `WordBreak::Normal` for the client's `LineBreak::WordBoundary`, the default
(`bevy_text-0.20.1/src/pipeline.rs:197-210`), and parley breaks lines with ICU4X's line segmenter, which follows
Unicode's line-breaking rules (`parley-0.11.1/src/analysis/mod.rs:58-75`). Read from the report at 360 px:

- **Chinese** breaks between characters: `…玩家：3个座位中` / `还有2个空位。…`. No line begins with `。`, `：` or
  `）`.
- **Japanese** breaks between characters and keeps the closing marks off a line's start: `…動かしてください。`
  ends one line with its `。`; `…空いていま` / `す。ちょっと…` puts `す` before the `。` down with it. A line made to
  break where `っ`, `。`, `ー`, `、` follow each other (`…わを` / `んっ。ー、「かっこ」…`) moved `ん` down to the next
  line rather than begin one with a small `っ`: the strict rule, which is Japanese's usual.
- **Korean** breaks at spaces where it can (`…기다리는` / `중: …`), and inside a word between syllables where it
  cannot (`플레이어를기` / `다리는…` at 150 px). Korean usually wants breaks only at spaces (CSS's `keep-all`).
  parley has it (`WordBreak::KeepAll`, `analysis/mod.rs:71-75`), but Bevy's `LineBreak` has no value that asks
  for it (`pipeline.rs:197-210`). Short panels lines rarely need a break inside a word; a long hint might.
- **A message on every layout of Chinese or Japanese**: `ICU4X data error: No segmentation model for complex
  script: Chinese/Japanese` (`icu_segmenter-2.4.0/src/complex/mod.rs:183`), from parley's word segmenter built
  without its dictionaries (parley's `complex-scripts` feature, which Bevy's `complex_script_segmentation`
  turns on, `bevy_text-0.20.1/Cargo.toml`; `analysis/mod.rs:46-54`). It is about words, not lines: the line
  breaks above were right without it. It would matter for moving the caret by words in a typed field, and it is
  printed to the console each time.
- **Han characters are drawn with the first face of the collection, which is Japanese's.** The report shows every
  Chinese run shaped with face 0 of the `.ttc`, which `fc-scan` names *Noto Sans CJK JP*. Bevy registers every
  face of a file under one family (`font.rs:93-99`) and passes no language to parley, so parley's choice of face
  by locale cannot happen. Some characters (`骨`, `直`, `角` were in the sample) have different forms in Chinese
  and in Japanese; whether the picture shows the Japanese ones was not judged by eye. **Each language needs the
  font made for it**, loaded alone: the regional subsets above, not the collection.
- **Not judged**: whether the Chinese, Japanese and Korean read well at 15 px, and the punctuation's place in its
  box (a Chinese and a Japanese font put `：` and `，` differently). Needs a reader of each language.

## 5. Right to left

What Bevy offers, from its source and the trial:

- **A node has a direction**: `Node::direction`, `Ltr` or `Rtl` (`bevy_ui-0.20.1/src/ui_node.rs:636-639, 875-883`),
  given to taffy (`layout/convert.rs:113, 293-297`), whose flexbox, block and grid layouts mirror for `Rtl`
  (`taffy-0.14.0/src/compute/flexbox.rs:565-584`, `block.rs:670, 765`). In the trial a row with `direction: Rtl`
  put its label at the right and its text to the left (`noto-rtl.png`). Each node's default is `Ltr`
  (`ui_node.rs:831`) and Bevy converts each node's style on its own (`convert.rs:113`), so a mirrored panel
  sets it on every node that lays out a row, not once at its root. A helper that every panel's rows go through
  does that.
- **Margins, padding and positions are physical**: `UiRect` has `left`, `right`, `top` and `bottom` and nothing
  logical (`geometry.rs:693-701`). The client uses a physical left or right in 17 places in 10 files (a margin, a
  panel's position on the screen). Each becomes a function of the direction, the counterpart of the page's
  `margin-inline-start`.
- **Text aligns by its own guess** (section 4): every translated text would take `Justify::Start` and an Arabic
  phrase that may start with a name a right-to-left mark.
- **Arrows and symbols**: `← All games` (`browser.rs:686`) points the other way in Arabic; the play hints' keys
  (`Enter`, `Backspace`) stay.

What stays left to right, as on Bingle's page (its `docs/languages.md:51-54`): the board, its engraved coordinates,
the cards on the table, notation and the chess clock, which are things on a table, not text. The panels'
places in the world are physical too: in a headset the game's panel hangs beside the table
(`vr_panels.rs`) and the keyboard in front, and a panel's own contents mirror while where it hangs does not. Since
the headset's panels are the window's UI drawn into an image, the same mirroring serves both; it was not looked at
in a headset.

## 6. Typing

### In the window

Every field but the password is Bevy's `TextInput` with an `EditableText` (`sign_in.rs:355-367`, `friends.rs:876`).
Bevy 0.20's text input takes an input method's composition: it turns the window's `Ime::Preedit` and
`Ime::Commit` into edits of the field (`bevy_ui_widgets-0.20.1/src/text_input.rs:499-560`), asks the window for an
input method while a field has the focus (`text_input.rs:624-650`, `window.ime_enabled`), and tells the system
where to put the candidates' box (`text_input.rs:571-612`); winit passes both to the system
(`bevy_winit-0.20.1/src/system.rs`). So with the system's input method (Fcitx or IBus on Linux, Microsoft's
on Windows), Chinese, Japanese and Korean should be typed in the window **with no work of the client's**.
**Not tested**: a headless compositor has no input method, and the maintainer's desktop was not used. Worth a
look on both systems before counting on it: the preedit's underline, the candidates' box placed under the field
in a fullscreen window. The masked password field reads keys itself (`sign_in.rs:8`) and takes no composition,
which is usual for a password.

### In a headset

The headset's keyboard (`vr_keyboard.rs`) is one QWERTY layout of ASCII: digits, `a`-`z`, a few symbols, Shift
for capitals (`vr_keyboard.rs:61-124`), each key sending what the real keyboard would (`KeyboardInput` with its
text, `vr_keyboard.rs:270-290`). It cannot type `é`, `ñ`, `ß`, `ç` or `ã`, let alone Cyrillic or Arabic. What each
script would need:

- **Spanish, French, German, Portuguese**: accented letters, by a long press showing a key's variants or by
  per-language layouts (AZERTY, QWERTZ). Data and one gesture.
- **Russian, Arabic**: a layout of their own (ЙЦУКЕН; Arabic's), switched by a key. Data; Arabic typed into a
  field also shows the field's caret and selection in right-to-left text, which parley's editor handles
  (`parley-0.11.1/src/editing/cursor.rs`), not tried.
- **Korean**: a Hangul layout (2-set) plus composing jamo into syllables, which is a small algorithm (no
  dictionary).
- **Japanese and Chinese**: kana or pinyin into characters needs a converter with a dictionary of tens of
  megabytes (as Mozc's or librime's). Not something to build into the client. In a headset, SteamVR's own
  keyboard (an OpenVR overlay, not OpenXR) and the Steam Frame's system keyboard may offer it; neither was
  looked at, and using either means leaving OpenXR's API.

What a player types in this client is short: a server's address, a handle, a password, a guest key, a room code,
and direct messages to friends. All but the messages are Latin by nature. So the headset's keyboard matters
for messages only.

## 7. What the client cannot translate

The server sends these, and they stay as they come:

- **Game names, families, descriptions and seats' labels** (`GameInfo`, families' titles in `families.rs`), and the
  badges' names: English until bingle#253 is built, which is open and blocked (it waits on #251's work, its body
  says), and for a game it has not reached after it.
- **What a seat is asked and what a choice is called**, when the view says (`Position::impulse_words` and
  `choice_words`, `game_panel.rs:456, 519-535`): the game's own words.
- **How a game ended** (`table::ending_words`, `table.rs:342-`), or failing that the reason's id.
- **The server's errors**, put into the client's sentence (`"The server turned the request down ({status}):
  {message}"`, `net/session.rs:143`), and a library's error inside the client's messages (`{e}`). Bingle keeps its
  server messages English by decision (ADR 0141, *What stays as it is*).
- **Engraved words**: the move tiles' names are the server's card names in `0-9a-z` only (section 1).
- **Chat**: Bingle translates a game's chat and direct messages for a reader who asks (bingle#252, ADR 0143;
  `/auth/me`'s `translation` and `translate_chat`, `handlers.go:404-409`). The client shows direct messages as
  they are sent; using the server's translation is a separate piece of work.

**How it reads**: a translated panel with English islands in it, mostly the game's own words. In Arabic, the
game's panel would say, in Arabic, whose move it is, then the game's English instruction beneath, left to right
in a right-to-left panel; the list of games would have Arabic around English names. This is how Bingle's page
reads today in the same languages (its `docs/languages.md:96-97`: "A game's own words in these … stay as the game has
them"), so a player who knows the page knows it. It is not worse than the page, and it is not a finished
translation either, until #253.

## 8. Keeping it up

### What it adds to each pull request

Bingle's rule (its `CLAUDE.md`, *Before pushing*; `docs/languages.md:56-60`): a word added or changed goes
through `t()`, with its key in `en.json` and a machine draft in the nine other catalogs, in the same pull request.
Here the same would mean, for a pull request that adds or changes a phrase:

- the phrase as a key in `en.json` and the call that shows it;
- nine drafts, in the glossary's words for the platform's terms, and plural forms where it counts;
- for an Arabic layout, a look at the panel in Arabic if the change is to layout, not only to words.

How often: of the 61 pull requests merged from 2026-11-21 to this plan's base, **21 added a phrase-like literal**
to `src/` (a rough count by pattern, `"Capitalised words …"` in added lines, which also catches some tests'
strings). For one or two phrases the drafts are minutes of an agent's time and touch ten files; for a new panel
(the *Graphics* page and the settings round it are 25 phrases, `escape_menu.rs:550-631`) they are the larger part of the review. **Reviewers**, when there
are any, see every change twice: once in English and once in each language they review. A reviewed language goes
stale phrase by phrase, so each draft must say whether a speaker has seen it since English last changed.

### What CI checks

Tests in `cargo test`, as `tests/spelling.rs` is one today:

1. **Every key the code uses is in `en.json`**, read from the source as the spelling test reads it, and every
   key in `en.json` is used (no dead words).
2. **Every catalog has every key** of English's, and each translation carries the same `{values}` as English's
   (Bingle's `TestCatalogs_MatchEnglish`). A plural key has the forms its language needs (`one`, `few`, `many`,
   `other` for Russian; all six for Arabic) and no others.
3. **A stale draft fails**: each catalog keeps, beside a key's words, a short hash of the English they were
   drafted from. A pull request that changes English's words without redrafting fails until each language's
   hash matches again. This is stricter than Bingle's catalogs, which do not record it.
4. **No word escapes the catalog**: the count's rules made a test. A string literal with words in it passed to
   the UI's helpers (`Text::new`, `line`, `button`, a `format!` that reaches them) outside `t(…)` fails, with an
   allow-list for what is meant to stay (the brand, a resolution's `×`). The rules above, which found about 350
   phrases with a handful of mistakes, are good enough for a test with an allow-list; they are not good enough to
   count on without one.
5. **The glossary's terms**: a key that names a platform term says the word Bingle's glossary gives it, from a
   copy of `docs/glossary/` kept like `schema/`, with a test that fails when the copy is older than Bingle's
   (read at a pinned commit, as `schema/` is pinned to an API version).
6. **A pseudo-language** (`en-XA`, as Bingle's: accented, 40% longer, in brackets) for a picture of each panel in
   CI's window run, to find what overflows. Looked at by a person, not asserted.

## 9. Tiers

### Tier 0: English only, and the web page for other languages

- **What a player sees**: the client in English. When the account's `language` (or, for a guest and on the login
  screen, the system's) is not English, one line on the main screen beside the server's status, in English and
  naming the language in English: *"Bingle's web page is also in Japanese: {address}"*, with the address the
  client already shows elsewhere (`game_panel.rs:476`). Once per server, gone when closed. Written in the
  player's language it would be friendlier, but for Chinese, Japanese, Korean and Arabic that one line would
  need the fonts of tiers 2 and 3; for the five languages Fira Sans writes it could be in their own words, ten
  drafts of one key, Bingle's where one fits.
- **Stages**: read `language` from `/auth/me` (one field), the line and its setting to hide it. **One pull
  request**, small.
- **Risks**: none to speak of; the cost is that the client stays a client for English readers.

### Tier 1: the languages Fira Sans writes (Spanish, French, German, Portuguese, Russian)

- **Stages, in pull requests**:
  1. The machinery: the catalog format and loader, `t()` and the plural, list and number helpers, the setting
     and the order of section 3, `Accept-Language`, the tests 1, 2, 3 and 5 of section 8, the pseudo-language.
     English only in it. *One pull request, medium.*
  2. The words moved into `en.json`, module by module, with the phrases built from pieces made whole and the
     plurals made plural keys: three or four pull requests by area (the login and the main screen; the game's
     panel, the lobby and new game; friends, calls and voice; the escape menu, *Graphics* and messages).
     Test 4 comes with the last.
  3. The five languages' drafts, the 65 phrases Bingle has taken from its catalogs, the rest machine-drafted,
     each catalog marked unreviewed. *One pull request.*
  4. The headset's keyboard: accented letters and a Cyrillic layout. *One pull request.*
- **Size**: six to eight pull requests. Most of the work is stage 2, which every later tier needs too.
- **Risks**: German and French run 20 to 40% longer than English: the panels have fixed widths (the trial's
  panel showed German's compound wrapping cleanly, but buttons do not wrap). Machine drafts unreviewed, as Bingle's
  are. Every later change pays section 8's cost from stage 1 on.

### Tier 2: and Chinese, Japanese, Korean

- **Adds to tier 1**: the three regional fonts (17.5 MB for one weight, 35 MB for Regular and Medium), only the
  one shown loaded; their drafts; an input method checked on Linux and Windows (Bevy's, no work expected);
  Korean's line breaking at spaces (a change to Bevy, or text in Korean left to break between syllables).
- **Stages**: the fonts and the face-per-language loading (one pull request); the three catalogs (one); a pass
  over each panel in each language for height and widths, since CJK text at 15 px needs more line height (one).
- **Size**: three more pull requests after tier 1.
- **Risks**: the binary grows by 18 to 35 MB (the release's present size was not measured for this plan); a font's glyphs in a headset's panel at its texture's resolution were not looked at;
  Han forms are right only if each language has its own font (section 4); typing in a headset is ASCII and
  Hangul at best, and Chinese and Japanese not at all.

### Tier 3: all ten, with Arabic

- **Adds to tier 2**: Noto Sans Arabic (0.24 MB); mirroring: `direction` on every row node, the 17 physical
  margins and positions made a function of the direction, `Justify::Start` on translated text, isolates round
  every value put into a phrase and a right-to-left mark where a phrase may start with a name, the arrows
  turned; an Arabic layout for the headset's keyboard; a check of the caret in right-to-left text in a field.
- **Stages**: the isolates and justification (they help every language and should come with tier 1's stage 2,
  whatever is decided); mirroring the panels (two pull requests: the screen's panels, then the escape menu,
  friends and the headset's panels); the catalog and the keyboard (one).
- **Size**: three or four more pull requests.
- **Risks**: mirroring by hand, node by node, is the largest risk of the whole plan: a panel built later that
  forgets it is half mirrored, and nothing in Bevy or in a test catches it unless a test of the panels' nodes is
  written for it. Bingle's page does it with CSS's logical properties and a test that checks the page is mirrored
  (`TestWebLanguages_*`, its `docs/languages.md:71-72`); the client would need the like.

## 10. What decides it

- **Who uses the client, in which languages**: the server knows an account's chosen language
  (`accounts.language`) and its browser's (`accounts.browser_language`, Bingle's `internal/store/store.go:328-332`),
  and each signed-in session's user agent (`sessions.agent`, `internal/store/accounts.go:334-336`), which for this
  client begins `bingle-3d/` (`net/session.rs:1585`). So whoever runs a server can count, in its database, the
  accounts that signed in with this client and the languages those accounts read the page in. Nothing in the API
  or the metrics says it, and a guest's sessions are not recorded so. That count, on the maintainer's server, is
  the number this decision most wants, and it is not in this plan.
- **What the client offers that the page does not**: the table in three dimensions and in a headset, with
  sounds and the pieces' models; the same games, lobby, friends, calls and voice chat as the page. Everything a
  player does in the client can be done on the page, which already speaks all ten languages and lets a player
  type with their system's input method everywhere. The headset is the one thing only the client has, and it is
  also where typing in other scripts is hardest.
- **What the same work would buy elsewhere**: tier 1 is some eight pull requests, tiers 2 and 3 about seven more,
  and every later pull request carries drafts. The same effort is about the size of a stage of `plan.md`: the rest
  of a game's controls (*After that*: undo and take-back requests, draw offers, a game's chat, the move list and
  replay), or bingle#253 itself, which translates the games' own words for the page and, through the API, for the
  client too.

## Recommendation

For the maintainer to decide; this is the plan's view.

1. **Tier 0 now**: read `language` from `/auth/me`, and when it is not English tell the player, in English, that
   the web page speaks it. One small pull request.
2. **Whatever is decided, make the phrases built from pieces whole and isolate the values put into them** when
   those panels are next touched: it is the part of tier 1's stage 2 that also makes the English better
   ("waiting for players: 2 of 3 seats free" chosen in code is already a plural rule), and it is cheap once and
   costly to do across the client at once later.
3. **Tier 1 only when** the count in section 10 shows players of those five languages using the client, since its
   cost is mostly the lasting one of section 8. It needs no font.
4. **Tiers 2 and 3 not before bingle#253**: until a game's own words are translated, a translated client is a
   frame of Chinese or Arabic around English instructions, at 18 to 35 MB of fonts and the hardest layout work
   of the plan, while the page offers the same games in the same languages, with typing that works. In a headset,
   Chinese and Japanese could not be typed at all.
