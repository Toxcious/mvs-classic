# Developing MVS Classic

How to build the game from source, build its content from your copy of the beta, run it and check a change. The
crates are listed in `crates/README.md`; the rules for a change are in `AGENTS.md`.

## What you need
- Windows 10 or 11, 64-bit, with a Direct3D 11 graphics card.
- Rust (stable) and the Visual Studio C++ build tools.
- The 2023 open beta, downloaded through the Steam console as the README's "Install" shows. The tools read its paks
  where Steam put them and never change them.

## Point the tools at your beta
Create `content/local.json` (the `content/` folder is ignored by git):

```json
{ "paks": "C:/Program Files (x86)/Steam/steamapps/content/app_1818750/depot_1818751/MultiVersus/Content/Paks" }
```

with the path of your own `depot_1818751`. The importer reads the paks' key from the beta's exe beside them; it is
never typed in or printed. `mvs-import setup <paks dir>` checks that a folder holds the build the port expects.

## Build the store
The game never reads the paks while it runs: the import builds everything it uses into a store, one pack per kind.
A player's launcher does this once; a developer builds the dev store in `content/.store`:

```
cargo run --release -p mvs-import -- import --store content/.store
```

A later run rebuilds only the kinds whose code or source changed (`--kind <id>` builds one, `--force` all). The tools
and tests find the dev store by themselves; `CONTENT_STORE=<dir>` points them at another one.

## Run it
- `play.bat` builds the launcher and the game and opens the launcher on the dev store, at its Home: PLAY starts the
  game. `play.bat --play` goes straight into the game. Arguments the launcher does not know go on to the game, for
  example `play.bat --size 1920x1080 --no-audio`.

It builds the `trace` profile: release code built incrementally, so a rebuild after a small change takes seconds,
not minutes. `--release` is for timings and what ships.

`tools/cargo-slot.ps1` runs cargo when one of a few build slots is free, at low priority, so several builds at
once (worktrees, agents) queue instead of taking the whole machine. Plain `cargo` works too.

## Check a change
- `cargo fmt --check` and `cargo clippy --release --all-targets`: no new warning.
- `cargo test --profile trace --no-fail-fast -p <crate>` for each crate you touched. Tests that read the store use
  the dev store.
- The match code (`mvs-sim` and the crates it uses) is also checked with per-tick traces: `sim-trace` (in
  `mvs-sim-trace`) replays recorded matches from their inputs and compares the whole World, field by field, every
  tick. The recordings hold the build's numbers, so they are not in the repo; you record your own from your store.
  The matches are defined in `crates/mvs-sim-trace/matches.json` (setups, seeds and the rules that make each
  player's inputs). Before the change, on a clean checkout:

  ```
  cargo run --profile trace -p mvs-sim-trace --bin sim-trace -- --record .scratch/baseline
  ```

  After it, the same with `-- --all`: each match is stepped again with the recorded inputs and every field the
  change moved is named, with the tick and both values. A change not meant to change how a match plays leaves every
  match `same`. `.scratch/baseline` is where a plain run and the trace tests of `mvs-net` look by default; record
  it again after the store is imported again. `sim-trace --help` lists the other modes.

## Where things are
- `crates/`: the program, one crate per part (`crates/README.md`).
- `docs/code-style.md`: how code and comments are written.
- `docs/experimental.md`: the custom lobby's opt-in rules.
- `docs/relay.md`: running a relay.
- `online.json`: the default relay the game connects to.
