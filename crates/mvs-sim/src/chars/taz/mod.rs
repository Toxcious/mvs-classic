//! Taz (C015; content taz, Pawn_C015): his state, the eat (Buff_EatenByC015) and its spit,
//! the burp, the projectile eat (Buff_C015HitProjectile), the tornado, the dogpile, his projectiles, the Tasty buffs,
//! the chicken (a polymorph child) and his perks (EatProjectiles and its anvil, TornadoReflect, TeamFightCloud), and
//! C015_AnimBP's slot mix. His attacks themselves are data played by the shared code.
//!
//! `mod.rs` is this file (the state, the slot graph, the death, matchReset, the eat's world and timer ticks, the
//! knock-out, the evade); the joined hooks are composed here: matchReset (the new state, then dogpile.rs's part),
//! timerTick (eat.rs's spit offset, tornado.rs's, dogpile.rs's, perks_cloud.rs's) and worldTick (eat.rs's spit,
//! perks_cloud.rs's), in the character registry's order. Each file enters its own classes.

pub mod burp;
pub mod chicken;
pub mod dogpile;
pub mod eat;
pub mod pawn;
pub mod perks;
pub mod perks_cloud;
pub mod perks_reflect;
pub mod pose;
pub mod projectile_eat;
pub mod projectiles;
pub mod state;
pub mod tasty;
pub mod tornado;
pub mod ui;

pub use state::{TazState, taz, taz_mut, taz_of, taz_w};

use crate::Real;
use crate::chars::CharState;
use crate::fighter::FighterId;
use crate::fighter::anim::SlotMix;
use crate::name;
use crate::names::Name;
use crate::registry::{CharHooks, Registry, RegistryError};
use crate::world::World;

/// The roster key.
pub const KEY: &str = "taz";

/// `state: () => new TazState()`.
fn new_state(_w: &World, _f: FighterId) -> CharState {
    CharState::Taz(TazState::default())
}

/// The joined matchReset: a match reset spawns a new pawn, so a new state, then dogpile.rs's part.
fn match_reset(w: &mut World, f: FighterId) {
    w.fighter_mut(f).cs = CharState::Taz(TazState::default());
    dogpile::match_reset(w, f);
}

/// The joined timerTick: OffsetSpitFighter's Delay(0.0), then tornado.rs's, dogpile.rs's and
/// perks_cloud.rs's parts.
fn timer_tick(w: &mut World, f: FighterId) {
    eat::spit_offset_tick(w, f);
    tornado::timer_tick(w, f);
    dogpile::timer_tick(w, f);
    perks_cloud::timer_tick(w, f);
}

/// The joined worldTick: this frame's BPI_SpitFighters after the mesh ticks, then perks_cloud.rs's part.
fn world_tick(w: &mut World, f: FighterId) {
    eat::spit_tick(w, f);
    perks_cloud::world_tick(w, f);
}

/// His EvadeButtonPressed override (Pawn_C015 function EvadeButtonPressed -> ubergraph @16421): the parent called
/// with the constant 5 (Default), whatever Direction it was given.
fn evade_direction(_w: &World, _f: FighterId, _direction: i32) -> i32 {
    5
}

/// C015_AnimBP's slot graph inputs (`CharHooks.slot_input`; `graph_slots` walks the graph from the data), by the data's
/// input name: the variables pose.rs's `anim_update` writes, or a node's own bound alpha (the exposed-input functions
/// @1431, @1557, @1725). AnimGraphNode_LayeredBoneBlend (node 10) is 0: `graph_slots` takes it as its base pose alone
/// (the Deviation in pose.rs). Here, not in pose.rs: it is a hook that returns a value (`OWNED` in mod.rs).
fn slot_input(w: &World, f: FighterId, name: Name) -> Real {
    let s = taz(w, f);
    let n = |lit: Name| name == lit;
    if n(name!("RunValue")) {
        s.run_value
    } else if n(name!("TurningBlend")) {
        s.turning_blend
    } else if n(name!("LandingBlend")) {
        s.landing_blend
    } else if n(name!("MovingInAirBlend")) {
        s.moving_in_air_blend
    } else if n(name!("AnimGraphNode_TwoWayBlend_5")) {
        pose::clamp01(s.aim_direction * 2.0)
    } else if n(name!("AnimGraphNode_TwoWayBlend_4")) {
        pose::clamp01((s.aim_direction - 0.5) * 2.0)
    } else if n(name!("AnimGraphNode_TwoWayBlend_3")) {
        if s.percent_charged >= 0.5 { 1.0 } else { 0.0 }
    } else if n(name!("AnimGraphNode_LayeredBoneBlend")) {
        0.0
    } else {
        Real::NAN
    }
}

/// `slotMix: (f) => taz(f)`.
fn slot_mix(w: &World, f: FighterId) -> Option<&SlotMix> {
    Some(&taz(w, f).slot_mix)
}

/// `slotMix`, written.
fn slot_mix_mut(w: &mut World, f: FighterId) -> Option<&mut SlotMix> {
    taz_mut(w, f).map(|s| &mut s.slot_mix)
}

/// `death`: the port does not update a dead pawn's AnimBP.
fn death(w: &mut World, f: FighterId) {
    taz_w(w, f).slot_mix.slot_mix.clear();
}

/// His own OnExitedArena -> OnKnockedOutServer (pawn.rs `on_knocked_out`). BlastBoxPlayerExit broadcasts it @493,
/// ahead of the game mode's PlayerKnockedOut, which is this hook's call, in the same ring-out and with nothing between
/// that reads the meter.
fn player_knocked_out(w: &mut World, f: FighterId, loser: FighterId) {
    if loser == f {
        pawn::on_knocked_out(w, f);
    }
}

/// Taz's entries: his hooks (mod.rs's, the joined ones, then burp.rs's, tornado.rs's, pose.rs's and tasty.rs's),
/// then each file's classes in registration order (`mod.rs` imports eat.rs, pawn.rs, perks.rs and
/// perks_reflect.rs; the registry imports projectile-eat, burp, tornado, ui, dogpile, perks-cloud,
/// projectiles, pose, tasty and chicken after it).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let mut h = CharHooks {
        state: Some(new_state),
        match_reset: Some(match_reset),
        timer_tick: Some(timer_tick),
        world_tick: Some(world_tick),
        evade_direction: Some(evade_direction),
        slot_input: Some(slot_input),
        slot_mix: Some(slot_mix),
        slot_mix_mut: Some(slot_mix_mut),
        death: Some(death),
        player_knocked_out: Some(player_knocked_out),
        ..CharHooks::default()
    };
    burp::hooks(&mut h);
    tornado::hooks(&mut h);
    pose::hooks(&mut h);
    tasty::hooks(&mut h);
    r.chars.enter(KEY, h)?;
    eat::register(r)?;
    perks::register(r)?;
    perks_reflect::register(r)?;
    projectile_eat::register(r)?;
    burp::register(r)?;
    tornado::register(r)?;
    ui::register(r)?;
    dogpile::register(r)?;
    perks_cloud::register(r)?;
    projectiles::register(r)?;
    tasty::register(r)?;
    chicken::register(r)?;
    Ok(())
}
