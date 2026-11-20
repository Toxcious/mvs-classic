//! The port's own Waves mode in the record: the World's `rest` key `waves` (the run, `mvs_sim::modes::waves::
//! WavesRun`), present only in a World with a Waves run. The oracle has no Waves mode: these keys are the Rust
//! writer's alone, and a Waves trace's baseline is the Rust side's own record.

use mvs_sim::modes::waves::{WavesEnd, WavesRun};
use mvs_sim::world::World;
use mvs_trace::{Obj, Value};

use crate::conv::{Plain, plain_struct, text};
use crate::cx::{Lx, Wx};

impl Plain for WavesEnd {
    fn to_value(&self, _: &Wx) -> Value {
        Value::Str(
            match self {
                WavesEnd::Cleared => "cleared",
                WavesEnd::PlayerOut => "playerOut",
            }
            .to_owned(),
        )
    }
    fn load_value(&mut self, v: &Value, l: &mut Lx) {
        if let Some(e) = Self::from_value(v, l) {
            *self = e;
        }
    }
    fn from_value(v: &Value, l: &mut Lx) -> Option<Self> {
        match text(v, l)? {
            "cleared" => Some(WavesEnd::Cleared),
            "playerOut" => Some(WavesEnd::PlayerOut),
            s => {
                l.miss(format_args!("a Waves end {s:?} (no rule to read it)"));
                None
            }
        }
    }
}

plain_struct!(WavesRun {
    "begun" = begun,
    "end" = end,
    "endTick" = end_tick,
    "enemies" = enemies,
    "gone" = gone,
    "nextSpawn" = next_spawn,
    "out" = out,
    "player" = player,
    "points" = points,
    "ringOuts" = ring_outs,
    "spawned" = spawned,
});

/// The World's rest key of a Waves run (none outside one).
pub fn rest_put(w: &World, x: &Wx, o: &mut Vec<(&'static str, Value)>) {
    if let Some(run) = &w.waves {
        o.push(("waves", run.to_value(x)));
    }
}

/// [`rest_put`]'s inverse: the run's state where the World has a run.
pub fn rest_load(w: &mut World, ob: &Obj, l: &mut Lx) {
    let v = ob.get("waves");
    if v.is_some() != w.waves.is_some() {
        l.miss(format_args!("the Waves run in the record {}, in the World {}", v.is_some(), w.waves.is_some()));
    }
    if let (Some(v), Some(run)) = (v, w.waves.as_deref_mut()) {
        l.at("waves", |l| run.load_value(v, l));
    }
}

// ---- headless runs (the tuning arena `waves-arena` and the tests)

/// A Waves set-up with a bot in the player's seat at `level` (the tuning arena's and the tests' player; the game's
/// player is the local one, `mvs_sim::modes::waves::setup`).
pub fn bot_setup(
    variant: mvs_sim::setup::WavesVariant,
    character: &str,
    level: i32,
    cast: Vec<String>,
) -> mvs_sim::setup::MatchSetup {
    let player = mvs_sim::setup::MatchPlayer {
        character: Some(character.to_owned()),
        difficulty: Some(mvs_sim::real(level)),
        ..mvs_sim::setup::MatchPlayer::default()
    };
    let mut s = mvs_sim::modes::waves::setup(variant, player, cast);
    s.players[0].bot = true;
    s
}

/// The data of a Waves run of `setup` on `stage` from `store`.
pub fn data_for(
    store: &mvs_store::Store,
    setup: &mvs_sim::setup::MatchSetup,
    stage: &str,
) -> Result<std::sync::Arc<mvs_data::GameData>, String> {
    let keys = crate::keys_of(setup);
    let refs: Vec<&str> = keys.iter().map(String::as_str).collect();
    mvs_data::GameData::load(store, &refs, stage)
        .map(std::sync::Arc::new)
        .map_err(|e| format!("the gamedata kind: {e:?}"))
}

/// A World of a Waves `setup` with the bot trees set, as the app makes one.
pub fn world(
    data: &std::sync::Arc<mvs_data::GameData>,
    setup: &mvs_sim::setup::MatchSetup,
    seed: i32,
) -> Result<World, String> {
    let mut w = World::new(data.clone(), seed, setup).map_err(|e| format!("the bind: {e:?}"))?;
    w.bot_trees = Some(std::sync::Arc::new(data.bot_trees.clone()));
    Ok(w)
}

/// One step of a Waves World with no input pressed (a bot holds the player's seat), its events dropped.
pub fn step(w: &mut World) {
    w.step(&[mvs_sim::InputState::default()]);
    w.events.clear();
}
