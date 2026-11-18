# Agent instructions

MVS Classic is a port of the 2023 MultiVersus open beta (an Unreal Engine 4.26 build), rewritten as one native Rust
program: the Cargo workspace in `crates/` holds the game, the importer that builds it from a player's own copy of the
beta, the renderer on Direct3D 11, the UI, audio and the online code. `crates/README.md` lists the crates;
`docs/development.md` is how to build, import, run and check it.

These rules hold for anyone changing the code, by hand or with an agent.

## What the game is
- The beta's files and observable behaviour define the fighters and how a match plays: the roster, each character's
  attacks, movement and feel, the mechanics, balance and stages, and the visuals and UI the port has from the build.
  Nothing from later versions of MultiVersus is added. External docs and tools are verified against this build.
- Behaviour is derived from the build's data, its Blueprint bytecode and its native code. Video of the beta is the
  last resort and only validates.
- How the program is built is free: language, structure, formats, threading and the render path are chosen for the
  best performance and quality on the widest range of PCs, not kept from the build's engine. The sim's numbers are
  the build's own: float32 and its math routines, the closest to how the beta plays.
- Features of the port's own are welcome around that core: menus, tools and opt-in rules (`docs/experimental.md`).
  A custom rule is off by default, for custom games only, kept in its own code where it can be, and never changes a
  fighter's moves or feel.
- Where the port does not do what the build does, the code says so where it happens: `// Deviation:` says what the
  port does and what the build does; `// Unverified:` says what the port assumes where the build's behaviour is not
  known. A change that makes the port match the build removes its marker.
- The host core (`mvs-host`) holds no game data: it handles ids and seeds only and shares its protocol with the game
  through `mvs-protocol`.

## Never commit the game's material
- The repo holds code, tools and notes only. Players bring their own copy of the beta and the game reads it only
  through the import into the store. Nothing the build supplies is committed, served or bundled: assets, data
  tables, decoded dumps, translated shader code, text, keys, captures, recordings or fixtures made from it. The one
  exception is the README's screenshots of the running game (`docs/images/`).
- Code may hold what a decompiled source would: names (classes, assets, montages, widgets), numeric constants and
  logic ported from the build. Text a player reads (UI strings, Move List text) comes from the player's
  `Game.locres` at run time, never typed into the source; the words of the port's own features, which the beta has
  no line for, are typed in the UI crate's word modules (`mvs-ui` `menu/custom_text.rs`, `hud/custom_text.rs`,
  `launcher/words.rs`).
- `content/` (the developer's pak folder and the store built from it) is ignored by git and stays local.

## The match code
- The match is deterministic: the same inputs give the same match on every machine, which online play's rollback
  relies on. The rules are in `crates/README.md` "Rules" and `crates/mvs-sim/README.md`: math through `mvs-math`,
  no platform float routines, no std `HashMap`, no `unsafe` in a sim crate. Clippy enforces most of them.
- Where you have recorded matches, a change to the sim is checked with its per-tick traces (`sim-trace`,
  `docs/development.md`): a change that is not meant to change behaviour leaves every trace the same.

## Code and commits
- Code, comments and file names follow `docs/code-style.md`. Keep each file's line endings (`.gitattributes`); a new
  file is LF.
- Before a change is done: `cargo fmt --check`, `cargo clippy --release --all-targets` with no new warning, and the
  tests of the crates it touches.
- A commit is one change, its message `type(scope): subject` (`feat`, `fix`, `perf`, `refactor`, `test`, `docs`,
  `build`, `ci`, `style`, `chore`, `revert`; the scope a crate without its `mvs-` prefix), with no full stop, and
  subject and body lines within 72 columns.
