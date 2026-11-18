//! Black Adam's grab: C021GrabHit_C
//! ([`GrabHit`], a world actor) on his HitEnemyPlayer, its hold Buff_Held_By_C021 (Buff_HeldByEnemy's child with
//! nothing of its own: `combat::holds::hold_kind`'s parent kind), and its pawn hooks (`hit_enemy`, `knocked_back`,
//! `match_reset`, joined), which pawn.rs composes.
//!
//! Combo03's listener (C021GrabHit), a SpawnedActorParent child with no shape that the montage's Spawner attaches
//! to him (f10.018); the montage activates every one at f52.571 and its Spawner End (f68.698) destroys them.
//!   BeginPlay @3984 -> @3679: his Comp_HitBoxHandler_Parent.HitEnemyPlayer and DamageManager.KnockedBack bound
//!     (@3829-3916): CharHooks.hit_enemy / knocked_back.
//!   KnockedBack @10: not being destroyed -> K2_DestroyActor.
//!   HitEnemyPlayer @3674 -> @2151: the victim's CheckHasBuff(Buff_StevenBubbleEnemy) @2301; with it @2483: the
//!     listener unbound, BPI_SetIsHoldingEnemy(true) @2697. Else a BPI_Base_Fighter not NotPlayerCharacter @2819-2894
//!     -> HitPlayer = the victim @2987, SpawnBuffActor(Buff_Held_By_C021, the victim, him, 0, 1.0, BreakIfBlocked)
//!     @3006; a valid buff: SpawnedBuff, its OnDestroyed bound to OnGrappleBuffDestroyed @3174-3197, then @2483. An
//!     invalid buff: nothing more (the listener stays bound). Unlike SupermanFlightHit there is no attack-count test.
//!   ActivateObject @210: SpawnedBuff invalid or being destroyed @253-295 -> his MontageNavigation
//!     NotifyChangeSection(End, null) @360, K2_DestroyActor @410. Else OnDestroyed unbound, SpawnedBuff destroyed
//!     @493, its own handler's pfgDoDamage(null, HitPlayer, GRAB_PACKET) @1682, K2_DestroyActor.
//!   OnGrappleBuffDestroyed -> @4004 -> @309: NotifyChangeSection(End), K2_DestroyActor (no release hit).
//!   ReceiveEndPlay @3650: the parent's, a valid SpawnedBuff unbound and destroyed @3538-1810, both delegates unbound
//!     @1846, BPI_SetIsHoldingEnemy(false) @167.
//! Combo03 has no End section: NotifyChangeSection(End) finds none and the montage goes on.
//! The hold: Buff_Held_By_C021 is a data child of Buff_HeldByEnemy (AttachSocket None, AttachOffset (0, 200, 500)),
//! on the shared hold (`combat/holds.rs`), whose socket rule for None is the mesh component's location.
//! Model as SupermanFlightHit's (`chars/superman/flight.rs`): the packet through [`apply_projectile_hit`], the
//! actor's AttackID his attack count at the spawn.
// Deviation: OnDestroyed is found by the actor's step (after the fighters' ticks of the frame the buff ended in), not
// inside the buff's destroy; an actor ended by the Spawner's notify end or with him runs its EndPlay in the world's
// end pass of that frame.
// Unverified: Buff_StevenBubbleEnemy is on no fighter of the roster, and every fighter of the port is a player's
// (BPI_GetNotPlayerCharacter false): the bubble branch is written from the graph and reached by no match; the
// not-player branch @3243-3492 (a BP_FighterCharacter of character 14 goes on to the buff, others to @2483) is not.
// ForceKnockback of the packet is read by nothing in the port's spawned-actor hit path (HitReaction is: the victim's
// rumble, [`apply_projectile_hit`]).

use crate::State;
use crate::actors::world_actor::WorldActorState;
use crate::actors::{ActorClass, ActorId, ActorRef, WorldActorClass};
use crate::combat::buff_class::BuffClass;
use crate::combat::buffs::spawn_buff;
use crate::combat::hits::{actor_hit_enemy, apply_projectile_hit, dealer, projectile_blocks};
use crate::combat::holds::end_hold_buff;
use crate::combat::{DataEnum, DataVec, HitPacket, VacuumInfo};
use crate::fighter::FighterId;
use crate::fighter::anim::grouped::active_instance;
use crate::fighter::anim::slot_jump_to_section;
use crate::fighter::control::attack_dodged;
use crate::fighter::match_side::SpawnRequest;
use crate::fighter::moves::V2;
use crate::fighter::{actor_y, invulnerable};
use crate::name;
use crate::registry::entries::{ActorEntry, ActorHooks, SizeHooks, WorldActorHooks};
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use crate::world::flow::reset_pass;

const GRAB_HIT: &str = "C021GrabHit_C";

/// `GrabHit`: C021GrabHit_C's state beside [`crate::actors::WorldActor`]'s. The actor is where its owner is
/// (AttachToActor), with no velocity.
#[derive(State, Debug, Default)]
pub struct GrabHit {
    /// The AttackID: his attack count at the spawn.
    pub attack_id: i32,
    /// Its handler is on his HitEnemyPlayer.
    pub bound: bool,
    /// HitPlayer.
    pub hit_player: Option<FighterId>,
    /// The fighter whose hold is SpawnedBuff, while its OnDestroyed is bound.
    pub held: Option<FighterId>,
    /// ReceiveEndPlay ran.
    pub ended: bool,
}

/// `GRAB_PACKET` (the actor's `hit`).
pub(crate) static GRAB_PACKET: std::sync::LazyLock<HitPacket> = std::sync::LazyLock::new(grab_packet);

fn grab_packet() -> HitPacket {
    HitPacket {
        damage: Some(6.0),
        base_knock_back: Some(1700.0),
        knock_back_scaling: Some(20.0),
        knock_back_direction_vector: Some(DataVec { x: Some(-1.0), y: Some(-0.25), z: None }),
        attack_tag: Some(name!("None")),
        hit_reaction: Some(DataEnum::Value(2)),
        hit_pause_time: Some(0.0),
        hit_stun_mult: Some(1.0),
        on_hit_audio: Some(name!("sfx_cha_superman_hit_punch")),
        directional_knockback: Some(false),
        velocity_instead_of_facing: Some(false),
        knock_back_mirror: Some(false),
        ignore_facing: Some(false),
        attack_id: Some(0),
        do_los_check: Some(false),
        force_knockback: Some(true),
        hit_only_one_fighter: Some(true),
        armor_pierce: Some(false),
        destroy_projectiles: Some(false),
        reflect_projectiles: Some(false),
        vacuum_knockback: Some(false),
        play_on_hit_effects: Some(true),
        apply_hit_pause_to_damage_dealer: Some(true),
        attack_decay_type: Some(DataEnum::Value(0)),
        attack_decay_name: Some(name!("None")),
        additive_kb_mode: Some(DataEnum::Value(0)),
        kb_scaling_starting_dmg: Some(0.0),
        hit_each_actor_only_once: Some(true),
        account_for_velocity_with_vacuum_knockback: Some(true),
        ignore_kbi: Some(true),
        kbi_multiplier: Some(1.0),
        vacuum_info: Some(VacuumInfo {
            use_vacuum_knock_back: Some(false),
            account_for_attacker_velocity: Some(true),
            ..VacuumInfo::default()
        }),
        ..HitPacket::default()
    }
}

fn state(w: &World, id: ActorId) -> Option<&GrabHit> {
    match &w.actors.actors.get(id)?.state {
        WorldActorState::GrabHit(s) => Some(s),
        _ => None,
    }
}

fn state_mut(w: &mut World, id: ActorId) -> Option<&mut GrabHit> {
    match &mut w.actors.actors.get_mut(id)?.state {
        WorldActorState::GrabHit(s) => Some(s),
        _ => None,
    }
}

/// SpawnedBuff is valid and not being destroyed: the victim's hold is the one this actor spawned.
fn buff_live(w: &World, id: ActorId) -> bool {
    let (Some(a), Some(s)) = (w.actors.actors.get(id), state(w, id)) else { return false };
    let Some(v) = s.held else { return false };
    w.fighter(v)
        .holds
        .hold
        .as_ref()
        .is_some_and(|h| h.cls == BuffClass::Data(name!("Buff_Held_By_C021_C")) && h.by == a.owner)
}

/// ReceiveEndPlay, once. Not in the Lab reset's pass (an old pawn there).
fn end_play(w: &mut World, id: ActorId) {
    let Some(s) = state(w, id) else { return };
    if s.ended {
        return;
    }
    let (owner, held) = (w.actors.actors.get(id).expect("a live actor").owner, s.held);
    state_mut(w, id).expect("a grab hit").ended = true;
    if !reset_pass(w) {
        if buff_live(w, id)
            && let Some(v) = held
        {
            end_hold_buff(w, v);
        }
        w.fighter_mut(owner).holds.holding_enemy = false;
    }
    let s = state_mut(w, id).expect("a grab hit");
    s.held = None;
    s.bound = false;
}

/// K2_DestroyActor on the actor.
fn destroy(w: &mut World, id: ActorId) {
    if let Some(a) = w.actors.actors.get_mut(id) {
        a.dead = true;
    }
    end_play(w, id);
}

/// His MontageNavigation.NotifyChangeSection(End, null) on his active montage instance.
fn jump_to_end(w: &mut World, owner: FighterId) {
    if let Some(m) = active_instance(w, owner) {
        slot_jump_to_section(w, owner, m, name!("End"));
    }
}

fn on_hit_enemy(w: &mut World, id: ActorId, vic: FighterId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let f = a.owner;
    let bubble =
        w.fighter(vic).buffs.buffs.iter().any(|b| b.class == BuffClass::BuffStevenBubbleEnemy && b.frames > 0.0);
    if bubble {
        state_mut(w, id).expect("a grab hit").bound = false;
        w.fighter_mut(f).holds.holding_enemy = true;
        return;
    }
    state_mut(w, id).expect("a grab hit").hit_player = Some(vic);
    // SpawnBuffActor: no new actor (blocked, or a hold already on the victim) is an invalid buff.
    let b = spawn_buff(w, vic, name!("Buff_Held_By_C021_C"), 1.0, Some(0), Some(f), true, None);
    let held = w
        .fighter(vic)
        .holds
        .hold
        .as_ref()
        .is_some_and(|h| h.by == f && h.cls == BuffClass::Data(name!("Buff_Held_By_C021_C")));
    if b.is_none() || !held {
        return;
    }
    let s = state_mut(w, id).expect("a grab hit");
    s.held = Some(vic);
    s.bound = false;
    w.fighter_mut(f).holds.holding_enemy = true;
}

/// The actor's own pfgDoDamage(null, HitPlayer).
fn deal(w: &mut World, id: ActorId) {
    let me = ActorRef::Actor(id);
    let Some(s) = state(w, id) else { return };
    let (Some(v), attack_id) = (s.hit_player, s.attack_id) else { return };
    if w.fighter(v).life.dead > 0.0 {
        return;
    }
    if projectile_blocks(w, &dealer(w, me), v) {
        return;
    }
    let packet: &HitPacket = &GRAB_PACKET;
    if invulnerable(w, v) {
        let tick = w.flow.tick;
        attack_dodged(w, me, v, attack_id, packet.base_knock_back.unwrap_or(0.0), tick, None, false);
        return;
    }
    let at = V2 { x: w.fighter(v).body.x, y: actor_y(w, v) };
    let d = dealer(w, me);
    if apply_projectile_hit(w, &d, v, packet, &[], at, None, &[], crate::combat::elem::charge_pct(w, me)) {
        actor_hit_enemy(w, &d, v);
    }
}

/// ActivateObject.
fn activate(w: &mut World, id: ActorId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    if a.dead {
        return;
    }
    let owner = a.owner;
    if !buff_live(w, id) {
        jump_to_end(w, owner);
        destroy(w, id);
        return;
    }
    let v = state_mut(w, id).expect("a grab hit").held.take().expect("a live buff has its fighter"); // OnDestroyed unbound
    end_hold_buff(w, v);
    deal(w, id);
    destroy(w, id);
}

/// OnGrappleBuffDestroyed, found by the step: the buff ended on its own.
fn step(w: &mut World, id: ActorId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let owner = a.owner;
    if a.dead || state(w, id).is_none_or(|s| s.held.is_none()) || buff_live(w, id) {
        return;
    }
    state_mut(w, id).expect("a grab hit").held = None;
    jump_to_end(w, owner);
    destroy(w, id);
}

/// The kind's `spawn` (the AttackID his attack count at the spawn).
fn spawn(_w: &mut World, _f: FighterId, r: &SpawnRequest) -> (WorldActorState, bool) {
    (WorldActorState::GrabHit(GrabHit { attack_id: r.attack_id, bound: true, ..GrabHit::default() }), false)
}

/// The joined `hitEnemy`: each live bound grab hit of his takes the victim (onHitEnemy).
pub fn hit_enemy(w: &mut World, f: FighterId, vic: FighterId) {
    let list = w.fighter(f).items.spawned_actors.clone();
    for a in list {
        let ActorRef::Actor(id) = a else { continue };
        if w.actors.actors.get(id).is_some_and(|x| !x.dead) && state(w, id).is_some_and(|s| s.bound) {
            on_hit_enemy(w, id, vic);
        }
    }
}

/// The joined `knockedBack`: each live grab hit of his is destroyed.
pub fn knocked_back(w: &mut World, f: FighterId) {
    let list = w.fighter(f).items.spawned_actors.clone();
    for a in list {
        let ActorRef::Actor(id) = a else { continue };
        if w.actors.actors.get(id).is_some_and(|x| !x.dead) && state(w, id).is_some() {
            destroy(w, id);
        }
    }
}

/// The joined `matchReset`: a new pawn holds nobody (the world's reset runs the actors' EndPlay without its fighter
/// part).
pub fn match_reset(w: &mut World, f: FighterId) {
    w.fighter_mut(f).holds.holding_enemy = false;
}

/// This file's registry entries (the grab hit's class). The hold class Buff_Held_By_C021 is entered on the shared
/// hold (`combat/holds.rs`).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let hooks = WorldActorHooks {
        spawn: Some(spawn),
        // a listener: no shape of its own and nothing drawn
        size: Some(SizeHooks::default()),
        activate: Some(activate),
        step: Some(step),
        end: Some(end_play),
        ..WorldActorHooks::default()
    };
    r.actors.enter(GRAB_HIT, ActorEntry::new(ActorClass::World(WorldActorClass::C021GrabHit), ActorHooks::World(hooks)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::setup::{MatchPlayer, MatchSetup};
    use std::sync::Arc;

    /// Combo03's grab to its damage, which no trace match reaches: the listener takes the victim (the hold bound),
    /// the activation ends the hold and deals the packet through the grab hit as a dealer (its owner's place). None
    /// without a dev store (the test says so and passes).
    #[test]
    fn grab_deals_its_packet() {
        let dir = mvs_store::dev_store_dir();
        let Ok(store) = mvs_store::Store::open(&dir) else {
            eprintln!("no store at {}: skipped", dir.display());
            return;
        };
        let data = Arc::new(
            mvs_data::GameData::load(&store, &["blackadam", "shaggy"], "trainingroomsmall")
                .expect("the gamedata loads"),
        );
        let p = |c: &str| MatchPlayer { character: Some(c.into()), ..MatchPlayer::default() };
        let setup = MatchSetup { players: vec![p("blackadam"), p("shaggy")], ..MatchSetup::default() };
        let mut w = World::new(data, 21, &setup).expect("binds");
        let (f, vic) = (w.players[0], w.players[1]);
        let st = WorldActorState::GrabHit(GrabHit { attack_id: 7, bound: true, ..GrabHit::default() });
        let x = crate::world::spawning::new_world_actor(
            &w,
            name!("C021GrabHit_C"),
            WorldActorClass::C021GrabHit,
            f,
            false,
            st,
        );
        let id = w.actors.actors.insert(x);
        w.fighter_mut(f).items.spawned_actors.push(ActorRef::Actor(id));
        assert_eq!(
            crate::actors::world_actor::world_actor_place_of(&w, id).map(|(x, ..)| x),
            Some(w.fighter(f).body.x)
        );
        assert_eq!(dealer(&w, ActorRef::Actor(id)).attack_id, 7);

        hit_enemy(&mut w, f, vic);
        assert_eq!(state(&w, id).and_then(|s| s.held), Some(vic), "the hold is bound");
        let before = w.fighter(vic).stats.damage;
        activate(&mut w, id);
        assert!(!buff_live(&w, id), "the hold ended");
        assert!(w.fighter(vic).stats.damage > before, "the packet dealt: {before} -> {}", w.fighter(vic).stats.damage);
        assert!(w.actors.actors.get(id).is_none_or(|a| a.dead), "the grab hit destroyed");
    }
}
