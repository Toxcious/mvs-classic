//! Batcave (`stages/batcave/`, Map_Batcave): its level script and placed actors: the vent cycle and the two fogs
//! (`vents`), the two Batmobiles (`car`), the two bat swarms (`bats`). The floor is a static solid; the fans are the
//! drawing's.

mod bats;
mod car;
mod vents;

pub use bats::BatcaveBats;
pub use car::BatcaveCar;
pub use vents::{BatcaveFog, VentsState};

use crate::Real;
use crate::State;
use crate::actors::StageActorClass;
use crate::registry::entries::{StageActorEntry, StageEntry};
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use crate::world::stage::{StageOwn, stage_delay};
use crate::world::wall_bp::WallActor;
use indexmap::IndexMap;

/// `BatcaveState`: the entry's own state (`StageRun.s`), its `StageOwn` variant: the
/// level script's (vents) and the placed actors' owner numbers (`StageData.placed` index) by class, in level order.
#[derive(State, Debug, Default)]
pub struct BatcaveOwn {
    pub vents: VentsState,
    pub cars: Vec<i32>,
    pub bats: Vec<i32>,
    /// The swarms that tick now, in level order (`StageEntry::tickers`).
    pub tick: Vec<i32>,
    /// The fighters' PF_CapsuleHalfHeight by id (BaseFighter @33775: the pawn's tick saves its capsule's half height
    /// while walking), for the cars' Box1 end overlap (`car::car_frame`); taken in the leftover pass (unverified, see
    /// there). By id: a minion's (World.spawnMinion) from 100.
    pub pf: IndexMap<i32, Real>,
}

// PandaGameMode's BeginPlay Delay(0.2) -> StartNewMatch, which broadcasts ResetLevelObjects (the only caller;
// TrainGameMode runs the parent's BeginPlay, so the Lab too): the game mode's own Delay, owner GAME_MODE_OWNER (no
// placed actor, not the level script's LEVEL_OWNER).
const GAME_MODE_OWNER: i32 = -2;
const EV_START_NEW_MATCH: i32 = car::CAR_EV + 99;

/// The stage's own state.
fn own(w: &World) -> &BatcaveOwn {
    match &w.stage_run.as_ref().expect("a stage run").s {
        StageOwn::Batcave(s) => s,
        s => panic!("not Batcave's stage state: {s:?}"),
    }
}

fn own_mut(w: &mut World) -> &mut BatcaveOwn {
    match &mut w.stage_run.as_mut().expect("a stage run").s {
        StageOwn::Batcave(s) => s,
        s => panic!("not Batcave's stage state: {s:?}"),
    }
}

/// `actorOf`: the slot (`StageState::actors` place) of the placed actor with owner number `owner`.
fn slot_of(w: &World, owner: i32) -> Option<u32> {
    let r = w.stage_run.as_ref()?;
    r.actors
        .iter()
        .position(|a| i64::from(a.index) == i64::from(owner))
        .map(|i| u32::try_from(i).expect("a list index"))
}

fn class_at(w: &World, slot: u32) -> StageActorClass {
    w.stage_run.as_ref().expect("a stage run").actors[slot as usize].cls
}

/// Map_Batcave ResetLevelObjects @2337: both fogs' DeactivateFog(false) @2380-2417 (when the right one is valid:
/// `vents::vents_reset_level_objects`), then BPI_Reset on the right car @2533 and the left one @2645
/// (`car::car_reset`).
fn reset_level_objects(w: &mut World) {
    vents::vents_reset_level_objects(w);
    let cars = own(w).cars.clone();
    let name_of = |w: &World, i: i32| {
        usize::try_from(i).ok().and_then(|i| w.data.stage.placed.as_ref()?.get(i)).map(|d| d.name.clone())
    };
    for n in ["BatcaveWallDestructible_right", "BatcaveWallDestructible_left"] {
        let found = cars.iter().find(|&&i| name_of(w, i).as_deref() == Some(n)).copied();
        if let Some(slot) = found.and_then(|i| slot_of(w, i)) {
            car::car_reset(w, WallActor(slot));
        }
    }
}

fn create(w: &mut World) {
    let of = |w: &World, c: StageActorClass| -> Vec<i32> {
        w.stage_run
            .as_ref()
            .into_iter()
            .flat_map(|r| r.actors.iter())
            .filter(|a| a.cls == c)
            .map(|a| i32::try_from(a.index).expect("a placed index"))
            .collect()
    };
    let bats = of(w, StageActorClass::BatcaveBatsBP);
    let vents = vents::vents_create(w);
    let cars = of(w, StageActorClass::BatcaveWallDestructible);
    // (every swarm ticks from the start: PrimaryActorTick bCanEverTick, no TickInterval)
    let mut pf = IndexMap::new();
    for f in w.players.clone() {
        pf.insert(w.fighter(f).id, crate::fighter::capsule_half(w, f));
    }
    w.stage_run.as_mut().expect("a stage run").s =
        StageOwn::Batcave(BatcaveOwn { vents, cars, bats: bats.clone(), tick: bats.clone(), pf });
    // the swarms' BeginPlay, in level order.
    // Unverified: their place among the level's BeginPlays (the level script's own draws) is not read.
    if w.setup.allow_hazards {
        for i in bats {
            if let Some(slot) = slot_of(w, i) {
                bats::bats_begin(w, slot);
            }
        }
    }
    stage_delay(w, GAME_MODE_OWNER, 0, 0.2, EV_START_NEW_MATCH);
}

fn tickers(w: &World) -> &[i32] {
    &own(w).tick
}

fn pre_tick(w: &mut World, owner: i32) {
    if let Some(slot) = slot_of(w, owner)
        && class_at(w, slot) == StageActorClass::BatcaveBatsBP
    {
        bats::bats_tick(w, owner);
    }
}

/// The fogs' overlaps (`vents::vents_tick`), then the cars' montages and Box1 overlaps (`car::car_frame`), in the
/// leftover pass behind the frame's Delays.
// Unverified: the fogs' and the cars' overlap order against each other is not read.
fn tick(w: &mut World) {
    vents::vents_tick(w);
    for i in own(w).cars.clone() {
        if let Some(slot) = slot_of(w, i) {
            car::car_frame(w, WallActor(slot));
        }
    }
    let all = crate::world::pawns::all_list(w);
    for &f in &all {
        if crate::fighter::grounded(w, f) && w.fighter(f).life.dead <= 0.0 {
            let (id, half) = (w.fighter(f).id, crate::fighter::capsule_half(w, f));
            own_mut(w).pf.insert(id, half);
        }
    }
    crate::world::pawns::put_fighters(w, all);
}

/// The Lab reset: the old pawns leave the fogs' sets (`vents::vents_reset`).
fn reset(w: &mut World) {
    vents::vents_reset(w);
}

fn timer(w: &mut World, ev: i32, owner: i32) {
    if owner == GAME_MODE_OWNER {
        if ev == EV_START_NEW_MATCH {
            reset_level_objects(w);
        }
    } else if ev == crate::world::wall_bp::WALL_DELAY {
        if let Some(a) = crate::world::wall_bp::wall_of_owner(w, owner) {
            crate::world::wall_bp::wall_delayed(w, a);
        }
    } else if ev >= bats::BATS_EV {
        if let Some(slot) = slot_of(w, owner)
            && class_at(w, slot) == StageActorClass::BatcaveBatsBP
        {
            bats::bats_timer(w, slot, ev);
        }
    } else if ev >= car::CAR_EV {
        if let Some(slot) = slot_of(w, owner)
            && class_at(w, slot) == StageActorClass::BatcaveWallDestructible
        {
            car::car_timer(w, WallActor(slot), ev);
        }
    } else if ev >= vents::VENT_EV {
        vents::vents_timer(w, ev, owner);
    }
}

/// A placed actor's entry that only creates (a swarm, a fog): no hit path of its own.
fn creating(class: StageActorClass, create: fn(&mut World, u32)) -> StageActorEntry {
    StageActorEntry {
        class,
        hurt_type: None,
        friendly_fire: false,
        can_take_damage: true,
        hit_pause_multiplier: None,
        surface: None,
        create: Some(create),
        any_damage: None,
        hit_by_enemy: None,
        hit_by_ally: None,
        knocked_back: None,
        hurt: None,
        sounds: &[],
        damage_disabled: None,
        invulnerable: None,
        current_damage: None,
        victim_pause: None,
        mesh: None,
        reset: None,
        solid_hit: None,
    }
}

/// The level entries (`stages({ ... })`) and the classes' (`stageActors({ ... })`).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let (fog, car, bats) =
        (StageActorClass::FogOcclusionBP, StageActorClass::BatcaveWallDestructible, StageActorClass::BatcaveBatsBP);
    r.stage_actors.enter(fog.name(), creating(fog, vents::fog_create))?;
    r.stage_actors.enter(car.name(), car::car_entry(car))?;
    r.stage_actors.enter(bats.name(), creating(bats, bats::bats_create))?;
    r.stages.enter(
        "Map_Batcave",
        StageEntry {
            create: Some(create),
            tickers: Some(tickers),
            pre_tick: Some(pre_tick),
            tick: Some(tick),
            timer: Some(timer),
            reset: Some(reset),
            ..StageEntry::default()
        },
    )
}
