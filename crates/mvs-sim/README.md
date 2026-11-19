# mvs-sim

The match simulation: a `World` made once from the match's bound data (`mvs_data::GameData`), a seed and the set-up
(`MatchSetup`), then stepped once per tick (60 Hz) from the players' inputs. The same inputs give the same World on
every machine, which online play's rollback (`mvs-net`) relies on. Everything a step reads or writes is plain data
in the `World`; nothing is global.

## The module tree

| Module | What |
|---|---|
| `world` | `World` (every field, grouped), `World::new`, `World::step` |
| `world::flow` | the match's flow, the game mode's timers (`TimerCmd`), mutators, the tutorial bus |
| `world::blast` | the blast box, the shrink, rescue and ring-outs |
| `world::spawning` | the owners' spawn requests, the actor families' passes and ends |
| `world::items` | the item pool, reach, pick-ups, the items' pass, the random items |
| `world::pawns` | minions and mode pawns, their controllers |
| `world::stage` | the stage's own state, timers and solids; the stage tables |
| `fighter` | `Fighter`: every field, in groups by subject |
| `fighter::{body, moves, anim, control, match_side}` | the fighter's groups |
| `fighter::tick` | the fighter's calls of the step, the frame's flags |
| `fighter::traces` | the fighter's shape traces against the stage: edges, solids, standing edges |
| `combat` | hit, react, attackers, decay, armor, rage, buffs, holds, shapes |
| `combat::phases` | the combat calls of the step (combo reset, overlaps, the hit passes, dodge) |
| `combat::buff_class` | `BuffClass`: every buff class the build registers |
| `actors` | the actor families: `Projectile`, `Item`, `WorldActor`, the characters' own lists; `ActorRef` |
| `actors::classes` | `ProjectileClass`, `ItemClass`, `WorldActorClass`, `StageActorClass` |
| `ai` | the bots: the behaviour tree runner, its node classes, blackboard and pad; a character's own tasks in `chars::<key>::bots`; the Expert level's look-ahead (the port's own) in `ai::expert` |
| `chars` | `CharState` (a variant per character), each character's module and `register` |
| `modes` | the game modes that run their own code in the World: the tutorials |
| `registry` | the hook tables, `CharHooks`, the entries, the signal enums; `BindError`, `RegistryError` |
| `events` | `Event`: what a step tells the presentation (sounds, effects, HUD) |
| `input`, `setup` | `InputState`, `MatchSetup` and its parts |
| `ids`, `names` | `Id<T>` / `Arena<T>` (generation-checked), `Name` / `Names` (interned strings) |
| `state`, `snapshot` | the derived state walk and hash, the rollback ring |
| `rng` | the random streams (mulberry32) |

`mvs-sim-derive` holds `#[derive(State)]`.

## The step

`World::step` (`world/mod.rs`) calls each phase in order, each a function of the module that owns its behaviour.
The step takes its two fighter lists once: `fs` (the pawns, else the players) ahead of the physics passes, `ps` (the
live pawns, without those still waiting for their first tick) after the owners' first ticks, both into reused
scratch lists. `each` walks a list in place and allocates nothing.

`Fighter::stats.damage` (Comp_Actor_Damage's CurrentDamage) is written by combat only: `combat::hit` for a hit, a
launch and direct damage, `combat::hit::reset_damage` for the spawn, reset and respawn, and `combat::buffs` for a
heal.

## Rules of the code

- Numbers are the build's: `Real` is `f32`, with the build's own float routines through `mvs_sim::math`
  (`mvs-math`). The
  crate's `clippy.toml` (the same file in `mvs-anim` and `mvs-collision`, checked by `tests/state.rs`) refuses the
  standard library's float routines whose results differ by platform (`sin`, `atan2`, `powf`, `min`, `max`,
  `round`, `signum`, `mul_add`, ...) and the std `HashMap` / `HashSet` (their order is random per run): a map is an
  `IndexMap`. `lib.rs` denies the lossy `as` casts: a truncation is `math::to_int32`, a bit reinterpretation
  `cast_signed` / `cast_unsigned`, a narrowing `try_from`; a cast exact by construction carries an `allow` with its
  reason.
- An object is referred to by its id: a fighter `FighterId`, an actor `ActorRef` or its family's id, a notify
  `NotifyRef`, a string `Name`.
- A class family is an enum and a `match`; the registry's entries are `fn` pointers copied out of the
  `&'static Registry` before the call (a hook takes `&mut World`).
- A deferred call kept in the state is a command enum, one per site: `world::TimerCmd` (the game mode's timers),
  `fighter::tick::OnTurn`, `fighter::match_side::FfaDamage`, `chars::shaggy::shockwave::Then`.
- The sim starts no thread of its own. The one work it runs off the calling thread, the Expert bot's decisions,
  goes to the threads a caller hands it (`ai::expert::Workers`) and is taken back at a fixed think, never at a time
  (`ai/expert/worker.rs`); without them it runs inline, with the same result.
- `#[state(skip)]` marks what is not rollback state: the events, the scratch, the set-up's fixed tables, the data,
  the registry, the game mode's hooks. A skipped field is neither copied by a save nor hashed.
- The class data an object reads (an actor's properties, its components, a projectile's boxes, the box samplers) is
  the bound data's, read through the World (`World::data`, `World::fixed`), not state.

### Calls across modules

- **The World is passed, never held.** A function that writes takes `w: &mut World`, one that only reads
  `w: &World`. A fighter's behaviour is a free function of its module taking `(w, f: FighterId, ...)`; an actor is
  passed by its id (`ActorRef`, or its family's id: `ProjectileId`, `ItemId`, `ActorId`, ...), a stage actor by its
  index (`u32`), a notify as `NotifyRef`, a buff as `BuffRef`, an actor's buff as `(ActorRef, u32)`.
- **No reference is held across a call that takes `&mut World`.** Read what you need into locals (copy an id, a
  number, a `Name`; `clone` a small struct), make the call, then look the object up again by its id: a call may
  spawn, end or move anything. A hook is copied out of its table before the call (`fn` pointers are `Copy`).
- **Results are return values or out parameters**: a struct `<Fn>Out`, or `&mut T` when the caller owns it and
  reuses it; a list filled by a call is `&mut Vec<T>`, cleared by its owner.
- **The handler types are the registry's** (`registry/entries.rs`, `registry/char_hooks.rs`), one `Option<fn>` per
  hook: a notify `fn(&mut World, FighterId, NotifyRef, u32)` (begin / end with the notify's index; the actor and
  stage forms take an `ActorRef` / a `u32`), a character hook `fn(&mut World, FighterId, ..)`, a buff
  `fn(&mut World, FighterId, BuffRef, ..)`, an actor's buff `fn(&mut World, ActorRef, u32, ..)`, a world actor
  `fn(&mut World, ActorId, ..)`, a projectile `fn(&mut World, ProjectileId, ..)`, an item `fn(&mut World, ItemId, ..)`,
  a stage actor `fn(&mut World, u32, ..)`, the stage `fn(&mut World, ..)`. A hook that takes a hit takes
  `&HitPacket`. A timer is a command enum entry, run by a `match` in its owner's module.

## Design

- **The state walks are derived.** `#[derive(State)]` writes `Clone` with a field-wise `clone_from` (the standard
  derive's `clone_from` is a whole clone, which allocates) and the desync hash's walk. A save into a ring slot of the
  same shape allocates nothing: `tests/snapshot_alloc.rs` counts it with a global allocator, the one `unsafe` of the
  crate's tree (a test's `unsafe impl GlobalAlloc`; the library is `#![forbid(unsafe_code)]`).
- **The desync hash is FNV-1a 64 over the fields' bits**, in declaration order, an enum's variant index first.
- **Arenas iterate in insertion order** and `retain` keeps it; a freed slot is reused with a new generation, so a
  stale id reads `None`.
- **`World::new` takes `Arc<GameData>`**: the World and every snapshot share the data without a copy. `Names` is
  built at bind from the cast's and the data's keys and is read-only after (a run-time string is a typed key,
  `events::SoundKey`).
- **A fighter's hooks are bound at `World::new`** from its character key; an unknown key is a `BindError` naming
  `MatchSetup.players[i].character`. The hooks are copied with the fighter and not hashed (they follow from its key).
- **The build's class names are enums** (projectiles, items, world actors, stage actors, buffs), with `name()`
  giving the Blueprint class.
- **Objects every fighter shares are the World's**: the combo reset, the shared voice lines, the peers and the stage
  platforms are World fields the fighter code reads through the World.
- **`mvs-anim` runs over `mvs-data`'s records.** Its `MontagePlayer { m: &Montage, s: &mut MontageState<K> }` is
  made per call from the bound data's montage and the owner's state, which keeps the montage as a key (`Name`).
  Nothing in `mvs-anim` copies a record, so the fighter, a mesh actor and a stage actor hold only state.
- **The bot trees are `World::bot_trees: Option<Arc<mvs_data::BotTrees>>`**, filled at set-up when a bot is in the
  match and shared with every snapshot; `None` for a match without bots.
- **Per-object side data is a field of that object**, or an `IndexMap` side table of the World where the key is not
  one object's (`World::actor_delegates`, `PawnControllers::ctl`). Side data keyed by the match's data is computed
  at the bind into `World::fixed`.
- **The code's own literals are `name!("...")`.** An event, class, socket or tag name written in the code is the
  `name!` macro: `build.rs` collects every use under `src/` into `CODE`, which every `Names` interns first, so the
  macro is a constant (`names::code_id`) and a literal is interned whether or not the data holds it. A run-time
  lookup by string (`Names::get`) is for strings that come from the data or the set-up.
- **`HitPacket` is typed**: the build's Struct_HitBoxDamage, every member an `Option`, the Hitframe's extras typed,
  `from` the notify it was built from.
- **A nav or anim set is a `SetRef`** (`Base` or `Named(Name)`): the fighter holds which set it plays, the data holds
  the sets.
- **The box samplers are built at the bind**, one per character of the match's data (the cast, the mode's pawns).

## The per-tick trace

`mvs-sim-trace` writes the World as `mvs-trace`'s record each tick and compares it with a baseline: the sim's own
earlier run, or the recordings of the TS oracle the sim was first checked against bit for bit. Where a field's record
value is computed rather than read (a `Name` as its string, a class enum by `name()`, the montage record from the
player over the bound montage), the crate's `frame_of` holds the mapping.
