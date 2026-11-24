//! Item_Dynamite_C, an ItemActorParent child, one of Bugs Bunny's ACME box pool.
//! Durability 2, CanUseItem false (ItemUse drops it), a HitBoxDamage of zeros, ItemIdleLife 15. Offsets are its
//! ubergraph's.
//! - ItemThrowHit @1688: the parent's, then with (!HitPlayer || Enemy) and not Detonated (an ally's hit does not set
//!   it off): Detonated, SphereOverlapActors(its location, 400, [PhysicsBody, Pawn, WorldDynamic,
//!   ProjectileDestructible], nothing ignored) @1919 and pfgDoDamage on each actor with the packet @196-1237;
//!   BoomMulticast @1517 (ProjectileMovement.Deactivate, the Geo hidden, PS_BugsBunnyRocketImpact at its location,
//!   sfx_shared_item_dynamite_explode there); Delay(0.033) @1531 -> ReleaseToPool @15.
//! - ReceiveAnyDamage @2271: the parent's, then ReleaseToPool @2317 with no test
//!   (`actors::item::item_receive_damage`).
//!
//! Unverified: the item's own bodies among the overlap's are skipped (its pfgDoDamage on itself not read). An item
//! met there (its PickupArea, WorldDynamic) takes the call as a spawned HitActor (`do_damage`), one no fighter has
//! picked up with team -1; a World.actors actor and a mortar aim take it the same way. Unverified: the release Delay
//! counts from the item's own step of the next frames (World.step's item pass), not read against the latent pass. The Geo hidden by BoomMulticast: the view hides it
//! at the blast. Ported for a spawn with no owner (the FFA area's): ItemThrowHit comes from a hit with the hit box
//! on, which an item has from a throw on (a fighter its owner), so the blast always has its owner.

use crate::Real;
use crate::State;
use crate::actors::item::{ItemExt, is_owned};
use crate::actors::{ActorClass, ActorRef, ItemClass, ItemId};
use crate::combat::hits::{self, DamageCall};
use crate::combat::packet::{DataEnum, DataVec};
use crate::combat::{HitPacket, VacuumInfo, elem};
use crate::events::Event;
use crate::fighter::common::{latent_frames, warn_once};
use crate::fighter::moves::V2;
use crate::fighter::traces::{OBJ_PAWN, OBJ_PHYSICS_BODY, OBJ_PROJECTILE_DESTRUCTIBLE, OBJ_WORLD_DYNAMIC};
use crate::fighter::{self, FighterId};
use crate::name;
use crate::registry::entries::ItemHooks;
use crate::registry::{ActorEntry, ActorHooks, Registry, RegistryError};
use crate::world::World;
use crate::world::flow::reset_pass;

/// `Dynamite extends Item` (Item_Dynamite_C).
#[derive(State, Debug, Default)]
pub struct Dynamite {
    /// Detonated.
    pub detonated: bool,
    /// Frames left of the Delay(0.033) before ReleaseToPool; 0: none running.
    pub boom: Real,
    /// The velocity the blast's hits read (VelocityInsteadOfFacing) while the blast runs.
    pub boom_v: Option<[Real; 2]>,
}

/// The blast's packet (the Struct_HitBoxDamage the graph builds @196-1237).
fn explosion() -> HitPacket {
    HitPacket {
        sparse: true,
        damage: Some(10.0),
        base_knock_back: Some(1400.0),
        knock_back_scaling: Some(15.0),
        knock_back_direction_vector: Some(DataVec { x: Some(0.5), y: Some(0.5), z: None }),
        attack_tag: Some(name!("None")),
        hit_reaction: Some(DataEnum::Value(2)),
        hit_pause_time: Some(0.2),
        hit_stun_mult: Some(0.9),
        on_hit_audio: Some(name!("sfx_shared_hit_lfe")),
        on_hit_effect_override: None,
        directional_knockback: Some(false),
        velocity_instead_of_facing: Some(true),
        knock_back_mirror: Some(false),
        ignore_facing: Some(false),
        attack_animation: None,
        attack_id: Some(0),
        do_los_check: Some(false),
        force_knockback: Some(false),
        queue_hit_pause_until_poses: Some(false),
        hit_only_one_fighter: Some(false),
        armor_pierce: Some(false),
        ignore_actor: None,
        destroy_projectiles: Some(true),
        reflect_projectiles: Some(false),
        vacuum_knockback: Some(false),
        direct_knockback_towards_socket: Some(false),
        knockback_target_socket: Some(name!("None")),
        knockback_socket_offset: Some(DataVec { x: Some(0.0), y: Some(0.0), z: None }),
        controller_rumble: Some(name!("MediumForceFeedbackRumbleEffect")),
        play_on_hit_effects: Some(true),
        apply_hit_pause_to_damage_dealer: Some(true),
        attack_decay_type: Some(DataEnum::Value(0)),
        attack_decay_name: Some(name!("None")),
        additive_kb_mode: Some(DataEnum::Value(0)),
        kb_scaling_starting_dmg: Some(0.0),
        hit_each_actor_only_once: Some(true),
        account_for_velocity_with_vacuum_knockback: Some(true),
        ignore_kbi: Some(false),
        kbi_multiplier: Some(1.0),
        vacuum_info: Some(VacuumInfo {
            use_vacuum_knock_back: Some(false),
            account_for_attacker_velocity: Some(true),
            ..VacuumInfo::default()
        }),
        ..HitPacket::default()
    }
}

fn state(w: &mut World, it: ItemId) -> &mut Dynamite {
    match &mut w.actors.items.get_mut(it).expect("a live item").ext {
        ItemExt::Dynamite(d) => d,
        _ => unreachable!("Item_Dynamite_C holds a Dynamite"),
    }
}

/// `explode`: ItemThrowHit's blast.
fn explode(w: &mut World, it: ItemId, vx: Real, vy: Real) {
    state(w, it).detonated = true;
    if reset_pass(w) {
        return; // the Lab reset's end pass acts on the old pawns
    }
    // Every ItemThrowHit comes from a hit of its hit box, on from a throw (the thrower its owner).
    if !is_owned(w, it) {
        return warn_once(w, name!("Item_Dynamite_C: ItemThrowHit with no owner (not set off)"));
    }
    let (owner, x, y, attack_id) = {
        let i = w.actors.items.get(it).expect("a live item");
        (i.owner.expect("owned"), i.x, i.y, i.attack_id)
    };
    let (team, id) = (w.fighter(owner).team.team, w.fighter(owner).id);
    let types = [
        crate::real(OBJ_PHYSICS_BODY),
        crate::real(OBJ_PAWN),
        crate::real(OBJ_WORLD_DYNAMIC),
        crate::real(OBJ_PROJECTILE_DESTRUCTIBLE),
    ];
    let found = fighter::traces::sphere_overlap_actors(w, x, y, 400.0, &types, &[], -1.0);
    state(w, it).boom_v = Some([vx, vy]);
    let me = ActorRef::Item(it);
    let hit = explosion();
    for o in found {
        if o == me {
            continue;
        }
        match o {
            ActorRef::Fighter(vic) => hit_fighter(w, it, vic, team, attack_id, &hit),
            ActorRef::Projectile(p) => {
                if w.actors.projectiles.get(p).is_some_and(|p| !p.dead) {
                    crate::combat::spawned_hits::damage_spawned_actor(w, me, p, &hit);
                }
            }
            ActorRef::Item(o) => {
                if w.actors.items.get(o).is_some_and(|i| !i.dead) {
                    let call = DamageCall { hit_box: false, ..DamageCall::default() };
                    hits::do_damage(w, me, ActorRef::Item(o), &hit, &call);
                }
            }
            // A World.actors actor, a mortar aim (the routes test it is there); another family is warned there.
            // The build's pfgDoDamage on every actor met.
            other => {
                let call = DamageCall { hit_box: false, ..DamageCall::default() };
                hits::do_damage(w, me, other, &hit, &call);
            }
        }
    }
    state(w, it).boom_v = None;
    // BoomMulticast: PS_BugsBunnyRocketImpact at its location (scale 1), then the sound there.
    w.events.push(Event::ProjectileDeath {
        cls: name!("Item_Dynamite_C"),
        fighter: id,
        vfx: Some(name!("PS_BugsBunnyRocketImpact")),
        x,
        y,
        ox: 0.0,
        rot: [0.0, 0.0, 0.0],
        scale: 1.0,
        tint: None,
        to: None,
    });
    w.events.push(Event::Sound {
        fighter: id,
        ev: name!("sfx_shared_item_dynamite_explode"),
        x: Some(x),
        y: Some(y),
        hit: None,
        pooled: false,
        key: None,
    });
    // ProjectileMovement.Deactivate: in the build ahead of the parent's hit tail, whose SetSimulatePhysics(true) and
    // ItemHitGroundImpulse (ItemActorParent @5148-5253) set it flying on; that tail has run here already (the bounce).
    state(w, it).boom = latent_frames(0.033);
}

/// One fighter of the blast: pfgDoDamage's order (as `World.itemHits`): the reflect / IsInvulnerableToProjectiles
/// gate (@4032-4598) and the dodge pop (@4608) ahead of NotifyAllyOfHit (an invulnerable ally: nothing).
fn hit_fighter(w: &mut World, it: ItemId, vic: FighterId, team: i32, attack_id: i32, hit: &HitPacket) {
    if w.fighter(vic).life.dead > 0.0 || fighter::no_hit_from(w, ActorRef::Item(it), vic) {
        return;
    }
    let me = ActorRef::Item(it);
    let d = hits::dealer(w, me);
    if hits::projectile_blocks(w, &d, vic) {
        return;
    }
    if w.fighter(vic).team.team == team {
        if !fighter::invulnerable(w, vic) {
            hits::ally_hit(w, &d, vic, hit);
        }
        return;
    }
    if fighter::invulnerable(w, vic) {
        let tick = w.flow.tick;
        fighter::control::attack_dodged(w, me, vic, attack_id, 1400.0, tick, None, false);
        return;
    }
    fighter::control::dodge_over(w, me, vic);
    let pct = elem::charge_pct(w, me);
    if hits::apply_projectile_hit(w, &d, vic, hit, &[], V2 { x: d.x, y: d.y }, None, &[], pct) {
        hits::actor_hit_enemy(w, &d, vic);
    }
}

/// `throwHit`: ItemThrowHit @1688 on a hit of the thrown box.
fn throw_hit(w: &mut World, it: ItemId, hit_player: bool, enemy: bool, vx: Real, vy: Real) {
    if (!hit_player || enemy) && !state(w, it).detonated {
        explode(w, it, vx, vy);
    }
}

/// `step`: the Delay(0.033)'s end (ReleaseToPool @15); true destroys the item.
fn step(w: &mut World, it: ItemId) -> bool {
    let d = state(w, it);
    if d.boom > 0.0 {
        d.boom -= 1.0;
        return d.boom == 0.0;
    }
    false
}

fn sub() -> ItemExt {
    ItemExt::Dynamite(Dynamite::default())
}

/// This file's registry entries (Item_Dynamite_C).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let hooks = ItemHooks { sub: Some(sub), throw_hit: Some(throw_hit), step: Some(step), ..ItemHooks::default() };
    r.actors.enter(
        ItemClass::ItemDynamite.name(),
        ActorEntry {
            ownerless: true,
            ..ActorEntry::new(ActorClass::Item(ItemClass::ItemDynamite), ActorHooks::Item(hooks))
        },
    )
}
