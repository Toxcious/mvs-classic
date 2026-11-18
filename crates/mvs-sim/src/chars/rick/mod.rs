//! Rick (content rick, Pawn_C020): his state, the pawn and C020_AnimBPV2's sim part, the
//! Meeseeks (summon and air flap), the perks, the blaster shots, the missile, the fart bomb, the polymorph grenade,
//! the teleport buffs, the portal component, the portals and the portal aimer. His attacks themselves are data played
//! by the shared code.
//!
//! `mod.rs` is this file: the state, matchReset's new state, and mod.rs's own entries (C020_ChargedWithPerk,
//! AnimNotify_C020ListenForAbilityHit, AnimNotify_ClearUnactivatedMeeseeks, C020_Buff_Silenced ([`SilencedExt`],
//! [`SilencedActorExt`]), Buff_Educated_C020 and the perk markers): the track's [`register_index`]. Its
//! `hookJoiner` chain is the joined hooks: each hook here has one file (pawn.rs, portal_comp.rs, portal.rs), which sets
//! it. Each file enters its own classes, in the character registry's import order.

pub mod aimer;
pub mod air_flap;
pub mod blaster;
pub mod fart_bomb;
pub mod grenade;
pub mod meeseeks;
pub mod missile;
pub mod pawn;
pub mod perks;
pub mod portal;
pub mod portal_comp;
pub mod state;
pub mod teleport;

pub use state::{RickState, rick, rick_mut, rick_of, rick_w};

use crate::Real;
use crate::State;
use crate::actors::ActorRef;
use crate::actors::world_actor::actor_place;
use crate::chars::CharState;
use crate::combat::buff_class::BuffClass;
use crate::combat::buffs::{
    BuffExt, BuffRef, EDUCATED, buff, buff_mut, buff_of, silenced_begin, silenced_end, silenced_pop,
};
use crate::combat::elem::{ElemExt, actor_inst_def, elem_buff, elem_buff_mut};
use crate::combat::pop_text::pop_text_on;
use crate::events::NotifyRef;
use crate::fighter::FighterId;
use crate::fighter::common::{latent_frames, lookup_owner};
use crate::fighter::match_side::play_vo;
use crate::name;
use crate::registry::entries::{ActorBuffHooks, BuffEntry, BuffHooks, Condition, NotifyHandler};
use crate::registry::props::name_of;
use crate::registry::{CharHooks, Registry, RegistryError};
use crate::world::World;

/// The roster key.
pub const KEY: &str = "rick";

/// C020_Buff_Silenced_C's extension on a fighter (`SilencedExt`).
#[derive(State, Debug, Default)]
pub struct SilencedExt {
    /// The Delay's ticks left, 0 once it resumed.
    pub left: Real,
    /// The Instigator.
    pub by: Option<FighterId>,
}

/// C020_Buff_Silenced_C's extension on a spawned actor (`SilencedActorExt`), an elemental buff's (`combat::elem`
/// ElemExt): numbers only.
#[derive(State, Copy, PartialEq, Debug, Default)]
pub struct SilencedActorExt {
    /// The Delay's ticks left.
    pub left: Real,
    /// The Instigator's id, -1 for none.
    pub inst: i32,
}

/// `state: () => new RickState()`.
fn new_state(_w: &World, _f: FighterId) -> CharState {
    CharState::Rick(RickState::default())
}

/// `matchReset`: a match reset spawns a new pawn, so a new state (its BeginPlay writes nothing the sim keeps beyond
/// the class defaults).
fn match_reset(w: &mut World, f: FighterId) {
    w.fighter_mut(f).cs = CharState::Rick(RickState::default());
}

/// C020_ChargedWithPerk Check (a Charge100Percent child): the parent's Check @0 (the charge full) AND the actor's
/// BPI_CheckIfHasBuff(Buff_C020_Whip_C) @108-251; no fighter interface: false (@313).
fn charged_with_perk(w: &mut World, f: FighterId) -> bool {
    w.fighter(f).charge.charge_pct >= 1.0 && buff_of(w, f, name!("Buff_C020_Whip_C")).is_some()
}

/// AnimNotify_C020ListenForAbilityHit: Received_NotifyBegin / End @121, the mesh owner's BPI_ListenForAbilityHit on a
/// BPI_C020 owner (a Rick; on another fighter's mesh, a Disguise's copy of his montage, nothing).
fn listen_begin(w: &mut World, f: FighterId, _n: NotifyRef, _key: i32) {
    if rick_of(w, f).is_some() {
        pawn::listen_for_ability_hit(w, f, true);
    }
}

fn listen_end(w: &mut World, f: FighterId, _n: NotifyRef, _key: i32) {
    if rick_of(w, f).is_some() {
        pawn::listen_for_ability_hit(w, f, false);
    }
}

/// AnimNotify_ClearUnactivatedMeeseeks: Received_NotifyEnd alone, the mesh owner's BPI_C020.ClearUnactivatedProjectiles.
fn clear_end(w: &mut World, f: FighterId, _n: NotifyRef, _key: i32) {
    if rick_of(w, f).is_some() {
        pawn::clear_unactivated_projectiles(w, f);
    }
}

/// The Delay(0.2) resume @70: the Instigator's ability_seed VO, whoever owns the buff. \`left\` is the ticks left.
fn silenced_resume(w: &mut World, left: &mut Real, by: Option<FighterId>) {
    if *left <= 0.0 {
        return;
    }
    *left -= 1.0;
    if *left > 0.0 {
        return;
    }
    // Unverified: the instigator's own stream draws the RandomFloat (the sound code's postVO rule).
    if let Some(f) = by
        && w.fighter_mut(f).rng.next() <= 0.25
    {
        play_vo(w, f, name!("ability_seed"), &[], true);
    }
}

fn silenced_begin_hook(w: &mut World, f: FighterId, b: BuffRef, from: Option<FighterId>, dur: Option<Real>) {
    silenced_begin(w, f, b, dur);
    if let Some(x) = buff_mut(w, b) {
        x.ext = BuffExt::C020Silenced(SilencedExt { left: latent_frames(0.2), by: from });
    }
}

fn silenced_tick(w: &mut World, _f: FighterId, b: BuffRef) -> bool {
    let Some(BuffExt::C020Silenced(e)) = buff(w, b).map(|x| &x.ext) else { return false };
    let (mut left, by) = (e.left, e.by);
    silenced_resume(w, &mut left, by);
    if let Some(BuffExt::C020Silenced(e)) = buff_mut(w, b).map(|x| &mut x.ext) {
        e.left = left;
    }
    false
}

fn silenced_end_hook(w: &mut World, f: FighterId, _b: BuffRef) {
    silenced_end(w, f);
}

/// On a spawned actor (the megaseed's enemy row, pfgDoDamage's HitActor rows @18293-19121): the parent's ReceiveBeginPlay
/// @705 finds no Comp_Pawn_AbilityManager on the owner, its MulticastUI @1300 and EndPlay @976 stop at the BPI_BaseFighter
/// cast: no HUD state and no cooldown block. The PopText @69-645 has no owner-class test: "Silenced!" at the actor's
/// location, the event naming the actor's owner. The child's Delay(0.2) still resumes into the Instigator's VO @70.
fn silenced_actor_begin(w: &mut World, a: ActorRef, i: u32, from: Option<FighterId>) -> bool {
    let inst = from.map_or(-1, |f| w.fighter(f).id);
    if let Some(e) = elem_buff_mut(w, a, i) {
        e.ext = ElemExt::C020Silenced(SilencedActorExt { left: latent_frames(0.2), inst });
    }
    let (x, y, _) = actor_place(w, a).unwrap_or((0.0, 0.0, false));
    let owner = lookup_owner(w, a);
    let audio = actor_inst_def(w, a, i).and_then(|d| name_of(w, &d, "Params Audio"));
    let g = silenced_pop(audio, x, y);
    pop_text_on(w, owner, &g);
    false
}

fn silenced_actor_tick(w: &mut World, a: ActorRef, i: u32) -> bool {
    let Some(ElemExt::C020Silenced(e)) = elem_buff(w, a, i).map(|b| b.ext) else { return false };
    let mut left = e.left;
    let by = if e.inst < 0 { None } else { w.by_id(e.inst) };
    silenced_resume(w, &mut left, by);
    if let Some(b) = elem_buff_mut(w, a, i) {
        b.ext = ElemExt::C020Silenced(SilencedActorExt { left, inst: e.inst });
    }
    false
}

/// mod.rs's own entries: C020_ChargedWithPerk_C, the two notifies, C020_Buff_Silenced_C (fighter and actor),
/// Buff_Educated_C020_C, the perk markers.
fn register_index(r: &mut Registry) -> Result<(), RegistryError> {
    r.conditions.enter("C020_ChargedWithPerk_C", Condition::Fn(charged_with_perk))?;
    r.notifies.enter(
        "AnimNotify_C020ListenForAbilityHit_C",
        NotifyHandler { begin: Some(listen_begin), end: Some(listen_end), ..NotifyHandler::default() },
    )?;
    r.notifies.enter(
        "AnimNotify_ClearUnactivatedMeeseeks_C",
        NotifyHandler { end: Some(clear_end), ..NotifyHandler::default() },
    )?;
    let actor = ActorBuffHooks {
        begin: Some(silenced_actor_begin),
        tick: Some(silenced_actor_tick),
        ..ActorBuffHooks::default()
    };
    r.buffs.enter(
        "C020_Buff_Silenced_C",
        BuffEntry {
            actor: Some(actor),
            ..BuffEntry::new(
                BuffClass::C020BuffSilenced,
                BuffHooks {
                    begin: Some(silenced_begin_hook),
                    tick: Some(silenced_tick),
                    end: Some(silenced_end_hook),
                    ..BuffHooks::default()
                },
            )
        },
    )?;
    // Buff_Educated_C020: Buff_Educated's child with no graph of its own: the parent's hooks whole.
    r.buffs.enter("Buff_Educated_C020_C", BuffEntry { class: BuffClass::BuffEducatedC020, ..EDUCATED })?;
    // The perk markers: BuffParentActor children with no graph (presence alone).
    for (name, class) in [
        ("Buff_C020_Whip_C", BuffClass::BuffC020Whip),
        ("Buff_C020PortalCatch_C", BuffClass::BuffC020PortalCatch),
        ("Buff_C020PortalIgniteProjectiles_C", BuffClass::BuffC020PortalIgniteProjectiles),
        ("Buff_C020PortalMeleeBuff_C", BuffClass::BuffC020PortalMeleeBuff),
    ] {
        r.buffs.enter(name, BuffEntry::new(class, BuffHooks::default()))?;
    }
    Ok(())
}

/// Rick's entries in the character registry's order: meeseeks.rs's and perks.rs's classes (mod.rs imports them
/// first), mod.rs's own and his hooks (pawn.rs's, then the joined files': portal_comp, portal), then blaster,
/// missile, fart bomb, grenade, teleport, portal, aimer.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    meeseeks::register(r)?;
    perks::register(r)?;
    register_index(r)?;
    let mut h = CharHooks { state: Some(new_state), match_reset: Some(match_reset), ..CharHooks::default() };
    pawn::hooks(&mut h);
    portal_comp::hooks(&mut h);
    portal::hooks(&mut h);
    r.chars.enter(KEY, h)?;
    blaster::register(r)?;
    missile::register(r)?;
    fart_bomb::register(r)?;
    grenade::register(r)?;
    teleport::register(r)?;
    portal::register(r)?;
    aimer::register(r)?;
    Ok(())
}
