# Code style

These rules hold for all code in the repository: the Rust crates (`crates/`) and the tools in `tools/`. The match
code's rules beyond style (numerics, `unsafe`, maps) are in `crates/README.md` "Rules" and `crates/mvs-sim/README.md`.

## Comments
- A comment says what the code does and why, when the code alone does not make that plain. Where the code mirrors
  the beta build, the comment names the build's own thing so a reader with the beta can check it:
  - the class, function, asset, property or constant (`Comp_Pawn_ChargeAttack TotalChargedTime`);
  - a Blueprint function and its bytecode offset (`Pawn_C015 ubergraph @1234`);
  - an exe function (`FParticleEmitterInstance::Tick`), with its address only when it has no symbol.
- A comment never cites:
  - issue ids or ledger rows;
  - batches, tracks, prep commits, waves or agents;
  - dates, people, commit hashes or Ghidra query numbers;
  - paths outside the repository or in an ignored folder (`.scratch/`, a scratchpad).

  History goes in the commit message. Open work goes in an issue, which names the file.
- A comment that only repeats the code goes. A comment that is no longer true is fixed or removed.
- Comment density follows the code: a decoded formula or an engine quirk gets its explanation; plain code gets none.

## File header
A file opens with one to three lines that say what it holds and, when it mirrors the build, which build class or
system. It has no history and no list of the files that use it (the imports show that).

## Deviations and guesses
Two markers, written at the code they concern:
- `// Deviation: ...` marks where we do differently from the build on purpose (speed, platform limits) and says how.
- `// Unverified: ...` marks a model not yet checked against the build and says what it assumes.

## Names
- Rust crates are `mvs-<subject>` (`mvs-collision`), their modules and files `snake_case` (`collision_profiles.rs`).
  Tools keep their language's convention (kebab-case for JS, `snake_case` for Python).
- Use readable names, not build codes: `iron_giant/actors.rs`, not `c017actors.rs`. Name a stage after the build's
  map display name. The build code goes in the file header.
- A Rust folder's module file is `mod.rs` or the folder's name beside it, one way per crate.
- A test is named after what it tests (`tests/knockback.rs`).
- Rust identifiers follow Rust's conventions. A name that mirrors the build keeps the build's words in the
  language's case (`total_charged_time` for `TotalChargedTime`), and the comment names the build's spelling where
  a search needs it.

## Files and folders
- A file holds one subject. At about 1,500 lines, look for a seam; past 3,000 lines, split it.
- A type too big for one file keeps its fields in one file and its `impl` blocks split by subject in files beside it
  (or plain functions that take it first: `fn step_ring_outs(w: &mut World, ...)`). No trait just to spread a type
  over files.
- A folder with more than about 30 files directly in it gets subfolders by subject.
- Code that two crates share lives in a crate both depend on (`mvs-math`, `protocol`), never in a `#[path]` or a
  relative include that reaches out of a crate.
