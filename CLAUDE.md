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
- `src/scene.rs`: the table and the placeholder board. **One unit is one metre, and the table top
  is at y = 0**, so boards are their real size (this is what makes VR work later).
- `src/camera.rs`: the orbit camera. The left mouse button is kept for picking; nothing else reads
  the mouse directly, and nothing assumes there is exactly one camera.
- `src/hud.rs`: text over the scene. Bevy's built-in font is ASCII only.
- `src/run_limit.rs`: `--frames` and `--screenshot`.

## Dependencies and Bevy

- **Keep the dependency list short**, as Bingle does. Say why before adding a crate. No async runtime
  (networking runs on its own thread with blocking clients); `rustls`, never OpenSSL.
- **Bevy is pinned to one minor version** (0.20). An upgrade is its own pull request. Bevy's API
  changes every release, so check a type or field in the crate's source under
  `~/.cargo/registry/src/` rather than from memory; the compiler's suggestions are usually right.
- `rust-toolchain.toml` pins the compiler for every machine and CI; `rust-version` in `Cargo.toml`
  is Bevy's minimum.
