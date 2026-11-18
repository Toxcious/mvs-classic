# mvs-sim-trace

The Rust sim's side of the per-tick trace. `mvs-trace` holds the record and the file; this crate maps the World
onto it. The sim crates know nothing of the trace: every Rust to record mapping of a struct is in one place here.

## What it holds
- `frame_of(world, book, inputs, events)`: the frame the oracle's writer writes after
  a step, the same bytes; with the Rust values it has no rule for (written as null), each named.
- `load_frame(world, book, frame)`: the inverse. The actor lists are rebuilt in the record's order, each fighter is
  set by its id, then the World's rows and rest. It returns the frame's inputs and events as Rust values, and what it
  could not set, each named by its record path.
- `world_for(header, store)`: the World a header was made with (`new World(data, worldSeed, setup)`), the stage's
  and characters' data from the store.
- `owner(path)`: the Rust module that owns a record field (`fighters[0].hitstun` is `combat::hit`). A
  runner line names it.
- `run`: the round trip, the run and the seeded run (below), which the binary and the test use.
- `harness`: a World for a test of a path no trace runs: `harness::at(match, frame)` loads a baseline frame (or
  `fresh(setup, seed)` makes one), the test changes it where the path starts and steps it (`step_recorded` with
  the trace's inputs, `step` with its own); `record` writes it as the oracle would and `diffs` compares it with the
  trace's frame. `tests/harness.rs` checks that a loaded World steps as the trace.
- The `Book` passed to `frame_of` remembers the id of every actor it has seen live or in the World's gone lists (`cx.rs`
  `*_seqs`; a projectile spawned and removed within one step is only in `Actors::gone_motion`). A writer that
  starts on a World already mid-match writes -1 for a reference to an actor gone before its first frame, where the
  oracle writes that actor's id. Prime the book first: write the straight run's frames before the start into it, as
  `crates/mvs-net/tests/trace_session.rs` does.

The modules: `conv` holds the two mapping traits (`Rec` for a typed record, `Plain` for a value of a rest bag or an
event) and their macros. `cx` holds the write and load contexts (names, the id of each actor). Then `parts` (the
montage, hits, buffs, inputs), `fighter`, `actors` (projectiles, items, stage actors), `world`, `events`, `hit` (the
hit packet) and `owners`.

## The matches: `matches.json`
Each line defines a match, all of it our own: its setup (fighters, stage, rules), world seed, length and where each
player's inputs come from; the store supplies the game's data. `masher` gives each player a seed for a random
button masher; `combat` a seed for a player that walks to the nearest foe, attacks, gets back on stage and picks
up items, choosing from the last frame; `listed` the inputs tick by tick (the tutorials' scripts). A bot slot
thinks as in a match (`labBots` makes a slot a Lab bot; `botTrees` loads the behaviour trees for a match with no
bot slot, for a minion's controller). `until` stops a match at the first event that matches it; `calls` makes the
app's calls into the World (the tutorials' steps) at their ticks. `--record` reads it, `matches::record` steps one
match.

## The runner: `sim-trace`
`cargo run --profile trace -p mvs-sim-trace --bin sim-trace -- [mode] [options]` (behind `tools/cargo-slot.ps1`).

| Mode | What it does | A line per match |
|---|---|---|
| (none) | each match stepped from its header with the recorded inputs, each frame compared with the oracle one | `same`, or the first differing tick, field, both values and the owner |
| `--seeded` | each tick t loaded from oracle frame t - 1, stepped once with frame t's inputs and compared with frame t | the matching ticks counted, the first few mismatches, the paths differing most |
| `--round-trip` | each frame loaded and written back | the byte-equal frames counted, the paths that differ, what the loads could not set and what the writer could not write |
| `--bot-log <dir>` | each match stepped as a plain run steps it, nothing compared; its bots' log written to `<dir>/<name>.botlog` | the frames stepped and the thinks logged |
| `--record <dir>` | each match of `matches.json` (or those `--match` names) stepped from its definition on the store's data and written to `<dir>/<name>.trace`: a baseline that a plain run with `--traces <dir>` compares with after a change | the ticks recorded and why it stopped |

The options:
- `--match <name>` can be repeated. Without it the run checks the kept set (below, "Coverage map") and the traces
  the map does not know; `--all` checks every trace of the folder; `--changed [<rev>]` the matches a diff reaches.
- `--changed [<rev>]` picks the matches whose sim code `git diff <rev>` changes (the working tree and untracked
  files included; `<rev>` defaults to HEAD's merge base with `main`), prints them and why on one line, and runs
  them as a plain run does. A changed line counts in the innermost function of the map that holds it; a line between
  functions (a type, a constant, an attribute) counts for every function of its file; a hunk of comments and blank
  lines counts for none. It falls back to the default set, saying why, for a sim file the map does not hold (a new
  file), a change to the record or the trace tools (`mvs-trace`, `mvs-sim-trace`'s source), to a sim crate's
  manifest or proc macro (`mvs-sim-derive`), to the workspace's `Cargo.toml` or `Cargo.lock`, or a file changed
  between `<rev>` and the map's commit when the map is not the older of the two (its lines cannot be placed). A
  change only outside the sim's crates and the trace tools, or only in code no match runs, runs nothing and says so.
  A trace the map does not know is added to every pick.
- `--jobs <n>` sets the matches run at once. The default is half the machine's threads, fewer when the jobs' peak
  memory (`machine::JOB_BYTES` each, the largest match's) would pass a quarter of its physical memory. The process
  lowers its own priority below normal at start; the largest traces start first (the lines are still printed in name
  order); its last line, on stderr, gives the matches, the jobs, the time and the peak memory. A plain run holds the
  Rust trace's frames only with `--out`.
- `--until <tick>` stops a match there. A run that matched up to that tick is `same`.
- `--out <dir>` writes the Rust traces there (run mode); `trace-diff` reads them.
- `--traces <dir>` sets where the oracle traces are read from. The default is `MVS_TRACES`, else `.scratch/traces` of
  the main tree: the parent of `git rev-parse --git-common-dir`, so a worktree reads main's.
- `--fields <n>` sets how many differences or paths are printed.
- `--owner <module prefix or source file>` keeps only the differences of the fields that module owns (`fighter::body`,
  `combat`); repeated, those any of them owns (a track's files in one run). A buff entry's extension is owned by
  its class's module too (`owners.rs` `BUFF_EXT`, else combat::perks for a perk class), besides combat::buffs. A source file under `crates/mvs-sim/src` (`crates/mvs-sim/src/chars/creature/tether.rs` or
  `chars/creature/tether.rs`, `/` or `\`) names its module (`chars::creature::tether`; a `mod.rs` its folder's); a file
  that does not exist is refused, and a file need not own a record field. The count is then of the ticks where those
  fields match. The unwritten values name no field, so they are printed but not counted. A prefix that no owner starts
  with and that is no module of the sim's source tree is refused (owners are module paths with `::`), and so is `--owner` with `--round-trip`, which compares whole
  frames. In a seeded run a tick whose step panicked at a place outside the owner's module (the file in the panic's
  place) is counted apart as blocked, not as a mismatch: the summary reads `n of m ticks match (a mismatched, b
  blocked)`, and each distinct place prints once at the end as `blocked by <file:line> <message>: n ticks`. A panic
  inside the owner stays a mismatch. Blocked ticks do not fail the exit code (the owner's check passes without
  them). A plain run still ends at its first panic.
- `--not <record path prefix>` (repeatable, run and seeded) drops the differences whose path starts with it
  (`fighters[0].rest.buffs`, with or without the `world.`), counted as `--owner` counts (the unwritten values are
  then printed but not counted). `--not '*.<key>'` drops a key at any depth (`*.pitch`: every item's and
  projectile's pitch). It is refused with `--round-trip`.
- `--stage-b` (run and seeded) counts only the differences of the stage-B class (`mvs_trace::stable::STABLE`: a
  table of record path patterns and rules), what the sim on the build's float32 and math routines must not move: the
  events (kinds, order, which fighter or actor, their integers; not their floats), every string and flag (states,
  the movement mode, montage names and sections, notify identities, buff classes), the numbers that are counts,
  ids, ticks and enums, the inputs, a list's length or a value on one side only, and the damage as the HUD's
  integer. Every other number is a float and is not compared. `trace-diff --stage-b` compares two trace files so.
  It is refused with `--round-trip`.
- `--help` prints the options.
- `--from <tick>` (seeded) starts at that tick, loaded from the frame before it; `--show <n>` (seeded) prints the
  first n mismatching ticks (3 by default).
- `--all-paths` counts the known gaps too. By default the paths of `run::GAPS` (the table below), the misses they
  make and the unwritten queued taunt are left out of the count and the print.
- `--ignore-header` runs a trace whose header names another store. By default such a trace is not run: its build id
  or a kind's version differs from the store's, and the line names which.

A frame's inputs are written back as the trace recorded them: they are what the step took, not what it made.

A tick matches when its bytes do, or when its differences are all filtered out, and nothing was left unwritten:
the writer writes a value with no rule (one of the `opaque!` types, a stale reference) as null or a stand-in, so
such a frame is not the oracle one even when the bytes agree. A mismatching tick prints, in order: the panic, what the
load before it could not set (a bad load, not the step's fault), what the writer could not write, then the
differences.

Exit 0 when every match is the same, 1 on a difference, 2 on an error. A step that panics (an `unported!` call
reached) is caught and printed with its message and place. A run stops there. A seeded run counts the tick as a
mismatch (blocked with an `--owner` the panic is outside of), puts the World back to its state before that step, and goes on.

### The bot log
A difference in a bot's input (an `inputs[i].*` line) is placed by the bot log: per frame, each controller's think
(`think p<slot>`, or `m<j>` for the j-th pawn), the tasks it executed (`exec <node id>`) and aborted, the tree's
loop (`tree finished`), each decorator's condition check with its result (`dec <id> <0|1>`) and each service's
schedule in ticks (`svc <id> <ticks>`), each line led by its frame. The sim makes the lines (`mvs_sim::ai::log`,
off unless a tool starts it on its thread; it only reads). The oracle writes the same lines from its replay: from its
trace recorder run with its bot-log option on the same matches (BtBot's methods wrapped by the recorder, the final
hash still checked). `bot-log-diff <a> <b> [--lines <n>]` (a bin of this crate) compares two
logs or two folders of them: the first line that differs, its frame and whose think, the think's lines before it.
Both sides step their own sim from the same header and recorded inputs, so a difference may come from the World
drifting before the think (a float32 compare since stage B): the first frame the trace differs at says which.

### The bot arena
`bot-arena --a <fighter|all> --b <fighter|all>` (a bin of this crate) plays many headless bot-vs-bot matches on the
dev store (1v1, to `--kos` ring-outs, `--level-a` / `--level-b` 0..3 the beta's levels and 4 Expert, `--stage`,
`--games`, `--jobs`, `--seed`) and prints per pairing the wins, the games that ran out of time or crashed, and the
mean and p99 microseconds of the World's step, A's think and B's, and a `clone_from` of the World. It is the Expert bot's yardstick
(`docs/expert-bot-plan.md`) and a crash finder: a game whose sim panics is counted, and the first panic is named
with its seed.

### What the seeded run cannot see
- A difference that builds up over ticks: each tick starts from the oracle state, so a drift too small to show in one
  step never adds up. The plain run is the check that a match plays the same.
- State the record does not hold: Rust-only fields and the timers' payloads (below) come from the Rust World's own
  last step, not from the oracle. A bug there shows only when it reaches a recorded field.
- What a load cannot set (below): such a field stays at the Rust World's value from its last step.
- The order things happen in within a tick. The record holds the state at the tick's end, and its events in order.
  A module's seeded check therefore holds only once the phases that run before it in the step are ported: until
  then its inputs within the tick are wrong.

## Coverage map
`coverage/map.txt` maps each trace match to the sim code it runs: each function of the sim's crates
(`coverage::SIM_CRATES`: `mvs-sim` and the workspace crates it depends on) with its first and last line at the
map's `commit` (the last commit that changed those crates when the map was made) and the set of matches that run it.
`coverage/kept.txt` is the default set: a greedy cover over the code regions the matches run (the match with the most
regions not yet covered first, ties to the shorter; then a kept match whose every region another kept match runs is
dropped), so each kept match runs a region no other kept match runs. Both are our code's line numbers and the
matches' names only.

A coverage-instrumented run makes them: it builds sim-trace with `-C instrument-coverage` into
`target/coverage`, runs each match alone (`--match <m> --jobs 1`, its own `LLVM_PROFILE_FILE`) a few at once below
normal priority, reduces each profile with `llvm-profdata` and `llvm-cov export` (`rustup component add llvm-tools`)
into `.scratch/sim-coverage/runs/` (a stopped run started again goes on from the matches not done), then writes the two
files. The sim's crates must be unchanged against HEAD. It sets `CONTENT_STORE` to the oracle-format store when the
environment names none, and reads the main tree's Rust baseline (`--traces` another folder).

The map is remade with a new baseline, a new match, and after a batch that adds sim files: a file the map does not
hold sends `--changed` to the default set. Between remakes `--changed` still places a change to a file edited since the
map's commit, by the diff from that commit (a superset of the diff from `<rev>`).

## What a load leaves
- **Events.** A frame's events are the step's output. The loaded ones are returned, not put in the World.
- **Timers.** A timer is loaded by its kind, its key and its due tick. The command's payload (the fighter, step,
  winner or team it closes over) is not in the record. The World's own timer with the same kind, key and tick is
  kept. Else the command is made: `removeCameraFocus` takes its fighter from the key, as the oracle keys it; the others
  get a stand-in payload, and the load names it.
- **Rust-only state.** It is never in the record and stays as it was: the stage's run, the item spawn area, the bot
  trees, the caches. The writer lists the World-level ones as unrecorded; they do not fail a tick.
- **The families with no Rust list yet.** These are Morty's spawned actors (marker, ship, mortar, aim punch,
  accelerator). A reference to one is written as `spawned` -1 and named. The tutorial UI event is named the same
  way, and a pop's parameters are written but not loaded.
- **Fields the record cannot hold.** The load and the writer name each one. Today they are:

| Record field | What is missing |
|---|---|
| `fighters[].rageInput` past its main slot | the record holds the main instance only (`run::GAPS`) |
| an active hit's own packet | the record names the notify only (`run::GAP_UNMAPPED`) |
| a timer's payload | the record holds its kind, key and due tick (`run::GAP_MISSES`; "Timers" above) |

`tests/round_trip.rs` checks every frame of every baseline trace (about 15 min in the trace profile). A trace whose
header names another store runs all the same, with a note (as `sim-trace --ignore-header`); a run that checks no trace
at all while the baseline holds some fails. It fails on a differing path, a miss or an unwritten value outside
`run::GAPS`, `run::GAP_MISSES` and `run::GAP_UNMAPPED` (this table).
The runner's `--round-trip` prints the same counts.
