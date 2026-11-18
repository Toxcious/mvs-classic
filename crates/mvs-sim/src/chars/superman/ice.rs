//! Superman's ice breath: the pellets Superman_IceBreath_C and Superman_IceBreathEducated_C
//! ([`IcePellet`], a projectile class), the stream actor Superman_IceBreath_Stream_C ([`IceStream`]) and the spawner
//! hook that spends the meter ([`pawn::spend_ice`]).
//!
//! Read: Superman_IceBreath (the pellet; Superman_IceBreathEducated has no graph of its own),
//! Superman_IceBreath_Stream, Buff_SupermanAllyIceGauntlet, BuffParentActor (SetLifeTimer @4080, the owner's
//! HitEnemyPlayer bind), Components/Comp_State_Manager SpawnBuffActor @760.
//!
//! The move (Move List rows 9 and 13) is the montages' own: Superman_IceBreath_Montage and
//! Superman_Air_IceBreath_Montage spawn the pellets with AnimNotify_ConditionalSpawner (six in the first pass, three
//! in each further Loop), give Buff_SupermanSlow for the notify's length and leave to End on
//! Condition_NoIceBreathSuperman or a second press; all of that is shared code on his data. This file is what the
//! spawned classes do.

use super::pawn;
use crate::Real;
use crate::State;
use crate::actors::item::{ItemDef, item_attack_type};
use crate::actors::projectile::{Projectile, ProjectileExt, attach_parent, hit_collision, hurt_dist2};
use crate::actors::world_actor::{WorldActorState, actor_cls, actor_dead, actor_owner, actor_p, actor_place};
use crate::actors::{ActorClass, ActorId, ActorRef, ItemId, ProjectileClass, ProjectileId, WorldActorClass};
use crate::combat::BuffClass;
use crate::combat::boxes::{WorldSphere, obj as obj_type};
use crate::combat::buffs::spawn_buff;
use crate::combat::elem::{elem_of, elem_remove_stack, elem_sphere_overlap, instigator_charge, slow_projectile};
use crate::combat::phases;
use crate::combat::spawned_hits::hit_box_types;
use crate::consts::DELTA_SECONDS;
use crate::events::NotifyRef;
use crate::fighter::anim::montages::notify_data;
use crate::fighter::common::{actor_class_def, actor_def, actor_is_a, data_of, lookup_owner, owner_team, warn_text};
use crate::fighter::control::data_name;
use crate::fighter::match_side::SpawnRequest;
use crate::fighter::traces::sphere_overlap_actors;
use crate::fighter::{FighterId, no_hit};
use crate::math::{self};
use crate::name;
use crate::registry::entries::{ActorEntry, ActorHooks, ProjectileHooks, SizeHooks, WorldActorHooks};
use crate::registry::props::{num, obj, text};
use crate::registry::{CharHooks, Registry, RegistryError};
use crate::world::World;
use crate::world::spawning::{comp_spawn_item, destroy_projectile, destroy_spawned_actor, request_props};
use indexmap::IndexSet;

/// Superman_IceBreath_C.
pub const PELLET: &str = "Superman_IceBreath_C";
/// Superman_IceBreathEducated_C.
pub const PELLET_EDUCATED: &str = "Superman_IceBreathEducated_C";
/// Superman_IceBreath_Stream_C.
pub const STREAM: &str = "Superman_IceBreath_Stream_C";

/// `IcePellet extends Projectile`: what the class adds (the ProjectileExt of both pellet classes).
///
/// Superman_IceBreath's super is SpawnedActorParent itself, not ParentProjectilePhysics. The world's Projectile holds
/// both parents' rules. ParentProjectilePhysics' own graph (HitEnemyPlayer @201 DestroyOnHitFighter, HitNonPlayer @263
/// DestroyOnHit(Heavy)Projectile, ReceiveAnyDamage @1069 TakesDamage, the Capsule hit @1002
/// DestroyOnCollideWithEnvironment, ReceiveTick @796 AimDownProjectileMovement, the reflect and clank events) does not
/// exist on this class; each of those reads a property the pellet's data does not have (off), it has no HurtBox
/// (never a target) and its hit has no DestroyProjectiles and no CanProjectileClank (the projectile-against-projectile
/// test stops at the victim's HitBy), so none of them runs here.
/// Class data: HitBoxOn? true, EnableProjectileMovement, no gravity, bRotationFollowsVelocity (Projectile.pitch),
/// InitialLifeSpan 0.4, Die With Instigator, DestroyOnExitedArena?, DestroyOnBlocked, DestroyedOnHit false; no
/// HitBox2D, no HurtBox, Comp_Pawn_DamageManager.CanTakeDamage false, no ProjectileAttackType (0, Normal).
/// HitBoxDamage: 0.2 damage, no knockback, HitStunMult 0, HitEachActorOnlyOnce, OnHitAudio
/// sfx_cha_superman_icebreath_impact; 'On Hit Buffs': Buff_Ice, 1 stack, duration 0, to the enemy hit. Those are the
/// projectile's (the shared hit).
/// The hit box is the HitBox sphere (radius 42, relative scale 1.55, tag HitBox) under the root Capsule
/// (NoCollision). It is on from the spawn: HitBoxOn? is the class default and BeginPlay @4865 sets
/// SetGenerateOverlapEvents(true) on it, so the spawn-place test applies. That test is the overlap update of
/// ActivateHitbox's SetCollisionEnabled @16817 (Comp_Spawner's BPI_UpdateHitbox @12653 -> HitBoxesOn), at the
/// template scale; its SetRelativeScale3D(HitBoxScale) @16855 then sets (1, 1, 1): a radius of 42 x the actor scale
/// from the first step on (`on_unit`).
// Deviation: the radius drops at the pellet's first step, not inside the spawner's call (a hit test of the world's
// between the two still sees the template's).
// Deviation: the sphere's relative location (-1, 0, 2) x the actor's scale is left out (the circle is tested at the
// actor's place: 10 units low at the largest scale).
#[derive(State, Debug, Default)]
pub struct IcePellet {
    /// The HitBox sphere's radius at actor scale 1, at its template scale (the spawn's overlap update).
    pub unit: Real,
    /// The same once HitBoxesOn set its scale to HitBoxScale (every pass after the spawn's).
    pub on_unit: Real,
    /// The age the InitialLifeSpan timer ends it at.
    pub end_age: i32,
    /// The team mates its hit box overlaps (the begin of an overlap gives the gauntlet), by fighter id.
    pub allies: Vec<i32>,
    /// The projectiles its hit box overlaps (by projectile id).
    pub inside: Vec<i32>,
    /// The other spawned actors its hit box overlaps ([`pellet_others`]).
    pub touching: IndexSet<ActorRef>,
}

/// `IceStream` (Superman_IceBreath_Stream_C; SpawnedActorParent, the air montage's AnimNotify_Spawner alone).
/// Attached to the owner at the notify's socket ('VB HeadSocket'), HitBoxOn? false, EnableProjectileMovement false,
/// Die With Instigator; the notify's end destroys it (DestroyAllActorsOnNotifyEnd, the world's despawn). It deals
/// nothing: its sim part is the particle's pitch, which a view needs. The ground montage has no such actor: an
/// AnimNotify_PlayVFX of PS_SupermanIceBreathStream on the head socket.
#[derive(State, Debug, Default)]
pub struct IceStream {
    /// Rotation (degrees): the IceStream particle's relative pitch.
    pub rotation: Real,
    /// The notify's Socket, for the view.
    pub socket: crate::names::Name,
}

// ---- Superman_IceBreath

fn pr_ref(w: &World, pr: ProjectileId) -> &Projectile {
    w.actors.projectiles.get(pr).expect("a live projectile")
}

fn pr_mut(w: &mut World, pr: ProjectileId) -> &mut Projectile {
    w.actors.projectiles.get_mut(pr).expect("a live projectile")
}

fn st(w: &World, pr: ProjectileId) -> &IcePellet {
    match &pr_ref(w, pr).ext {
        ProjectileExt::IcePellet(s) => s,
        _ => panic!("an ice pellet"),
    }
}

fn st_mut(w: &mut World, pr: ProjectileId) -> &mut IcePellet {
    match &mut pr_mut(w, pr).ext {
        ProjectileExt::IcePellet(s) => s,
        _ => panic!("an ice pellet"),
    }
}

fn owner(w: &World, pr: ProjectileId) -> FighterId {
    pr_ref(w, pr).owner.expect("an ice pellet has an owner")
}

/// `makePellet`: the class's state. USphereComponent::GetScaledSphereRadius: the radius x the smallest axis of the
/// world scale. AActor InitialLifeSpan: a timer set at the spawn whose expiry destroys the actor (EndPlay: the
/// parent's death effects, `destroy_projectile`). Counted as the parent's MaxLifetime timer is (the projectile's
/// `life_ticks`): the first pass whose time is above the span, and the pass after it.
// Unverified: the engine timer's exact pass is not measured for this actor (a queued notify's spawn inside the mesh
// tick); the pellet lives 25 ticks here.
fn make(w: &World, f: FighterId, r: &SpawnRequest) -> ProjectileExt {
    let d = actor_class_def(w, f, r.cls).unwrap_or_else(|| panic!("{}: no class data", w.names.str(r.cls)));
    let hb = attach_parent(&d.components, "HitBox");
    let hit_box = obj(&d.components, "HitBox");
    let own = hit_box.and_then(|c| obj(c, "RelativeScale3D"));
    let axis = |k: usize, a: &str| (hb.scale[k] * own.and_then(|o| num(o, a)).unwrap_or(1.0)).abs();
    let (sx, sy, sz) = (axis(0, "X"), axis(1, "Y"), axis(2, "Z"));
    let radius = hit_box.and_then(|c| num(c, "SphereRadius")).unwrap_or(0.0);
    let unit = radius * math::min(math::min(sx, sy), sz);
    // HitBoxScale (the class's, else SpawnedActorParent's (1, 1, 1)) replaces the box's relative scale.
    let hbs = obj(&d.p, "HitBoxScale");
    let on = |k: usize, a: &str| (hb.scale[k] * hbs.and_then(|o| num(o, a)).unwrap_or(1.0)).abs();
    let on_unit = radius * math::min(math::min(on(0, "X"), on(1, "Y")), on(2, "Z"));
    let life = num(&d.p, "InitialLifeSpan").unwrap_or(0.0);
    let mut k = 0.0;
    if life > 0.0 {
        // (a negated test: a NaN time never ends the count)
        #[allow(clippy::neg_cmp_op_on_partial_ord, reason = "`!(a > b)`")]
        while !(crate::consts::clock(k) > f64::from(life)) {
            k += 1.0;
        }
    }
    let end_age = if life > 0.0 { math::to_int32(k + 1.0) } else { 0 };
    ProjectileExt::IcePellet(IcePellet { unit, on_unit, end_age, ..IcePellet::default() })
}

/// ReceiveBeginPlay @4295: the parent's, SpawnTime, the owner's BPI_UpdateIceBreath @4460 ([`spawner`]: the spawn runs
/// inside the owner's mesh tick, and the world makes its Projectile at the end of the fighters' ticks), the team
/// colour, the hit box's overlap events. The passive rows of its Comp_Pawn_PassiveBuffManager: Buff_Elemental_Ice (0
/// stacks) and Buff_Elemental_Fireproof on itself (the World's spawn; the Iceproof row applies to nobody). The
/// constructor's hit radius is the HitBox sphere at the actor scale, and it has no HurtBox: nothing of it answers
/// another actor's hit box.
fn begin(w: &mut World, pr: ProjectileId) {
    let unit = st(w, pr).unit;
    let p = pr_mut(w, pr);
    p.hit_radius = unit * p.scale;
    p.hurt_radius = 0.0;
}

/// Comp_Pawn_DamageManager.CanTakeDamage false in a class's components (true in the component's class defaults and
/// false only on the templates that clear it: in the six characters' data, the two pellets).
fn cannot_take_damage(components: &mvs_data::value::Props) -> bool {
    obj(components, "Comp_Pawn_DamageManager").and_then(|c| c.get("CanTakeDamage")).and_then(|v| v.as_bool())
        == Some(false)
}

/// The actor's own overlap graph for a spawned actor of another team (HitBox begin overlap @1148-2475; the cast to
/// SpawnedActorParent @1148, the teams @1227):
/// - a FireWallHazard loses a stack of Buff_Elemental_Fire @1424 (RemoveBuffStack(1) on its state manager), and
///   nothing else @1575;
/// - one with Buff_Elemental_Iceproof: nothing @1580-1775;
/// - ProjectileAttackType 1 (Heavy) @1825, or Comp_Pawn_DamageManager.CanTakeDamage false @1973: SpawnBuffActor(
///   Buff_IceSlowProjectile, the actor, self, 0, 0.0) @1870 (`slow_projectile`; the 0.0 is written to the buff's
///   BuffDuration by SpawnBuffActor @760 and SetLifeTimer(0) sets no timer @4130, so the class's 5 s never runs: the
///   slow lasts until the projectile hits something or dies);
/// - any other: the instigator's Comp_Spawner.CompSpawnActor(IceBlockItem, at the actor's location, no rotation,
///   direction (0, 0, 0), speed 0) @2349, then DestroySpawnedActor(true) on the actor @2475.
// Unverified: the pairs are the world's projectile-against-projectile pairs (this hit sphere against the other's
// HurtBox: its sphere, FireWallHazard's capsule). The sphere overlaps the object types WorldDynamic, Pawn,
// DamageDestructible, ItemDestructible, ProjectileDestructible and HurtBox; which component of each class answers is
// not decoded and a class without a HurtBox is not met. The spawned actors that are no projectiles (thrown items, the
// typed lists) are the trace gather's of the types the HitBox template answers, where the body answers HitBox
// ([`pellet_others`]).
// Deviation: tested where the pellet's move of the tick ends, not at the spawn place.
fn pellet_actors(w: &mut World, pr: ProjectileId) {
    let f = owner(w, pr);
    let (px, py, hit_radius) = {
        let p = pr_ref(w, pr);
        (p.x, p.y, p.hit_radius)
    };
    let mut i = 0;
    while let Some(o) = w.actors.projectiles.nth(i) {
        i += 1;
        if o == pr {
            continue;
        }
        let (oid, dead, hurt_radius, d2, class, attack_type, cls, ox, oy) = {
            let op = pr_ref(w, o);
            (op.seq, op.dead, op.hurt_radius, hurt_dist2(op, px, py), op.class, op.attack_type, op.cls, op.x, op.y)
        };
        let k = st(w, pr).inside.iter().position(|&s| s == oid);
        let r = hit_radius + hurt_radius;
        if dead || hurt_radius <= 0.0 || d2 > r * r {
            if let Some(k) = k {
                st_mut(w, pr).inside.remove(k);
            }
            continue;
        }
        if k.is_some() {
            continue;
        }
        st_mut(w, pr).inside.push(oid);
        let a = ActorRef::Projectile(o);
        'class: {
            if owner_team(w, a) == w.fighter(f).team.team {
                break 'class;
            }
            // the cast @1345 (FireStormHazard_C, its child, too)
            if matches!(class, ProjectileClass::FireWallHazard | ProjectileClass::FireStormHazard) {
                elem_remove_stack(w, a, BuffClass::BuffElementalFire, 1);
                break 'class;
            }
            if elem_of(w, a, BuffClass::BuffElementalIceproof).is_some() {
                break 'class;
            }
            let lo = lookup_owner(w, a);
            let damageable = !data_of(w, lo)
                .fighter
                .actors
                .as_ref()
                .and_then(|m| m.get(w.names.str(cls)))
                .is_some_and(|d| cannot_take_damage(&d.components));
            if attack_type == 1 || !damageable {
                slow_projectile(w, a, BuffClass::BuffIceSlowProjectile);
            } else {
                let charge = instigator_charge(w, f);
                comp_spawn_item(w, name!("IceBlockItem_C"), f, ox, oy, 0.0, 0.0, false, charge);
                destroy_projectile(w, o, false);
            }
        }
        elemental_overlap(w, pr, a);
    }
    pellet_others(w, pr);
}

/// The passive Buff_Elemental_Ice's handler of the same begin of an overlap (ElementalOnSphereBeginOverlap: the
/// class's BPI_GetSphereCollision is its HitBox, l.163 of the pseudo), bound in the buff's BeginPlay after the class's
/// own ComponentDelegateBinding: it runs after the class's handler ([`elem_sphere_overlap`]). The 0 stacks of the
/// passive are not read there: a same-team actor gets Elemental_Ice 1, an enemy one with Elemental_Fire loses 2 more.
/// A FireWallHazard has a passive Buff_Elemental_Iceproof: it loses the class's 1 alone.
// Unverified: the bind order (the component's delegate binding before the buff's AddDynamic) is taken from the order
// the two are made, not read natively; and an actor the class's handler destroyed is taken to fail the elemental's
// IsValid tests of its components @135 (DestroyActor marks them pending kill), so nothing is done to it.
fn elemental_overlap(w: &mut World, pr: ProjectileId, o: ActorRef) {
    if actor_dead(w, o) {
        return;
    }
    elem_sphere_overlap(w, ActorRef::Projectile(pr), o);
}

/// The same graph for the spawned actors that are no projectile: an owned item (ItemActorParent is a
/// SpawnedActorParent child), a world actor or a typed list's actor whose class is one. Its Iceproof test reads the
/// actor's passive elementals (none made: none has it); ProjectileAttackType and CanTakeDamage from its class data
/// (an item's IsProjectile plays no part). The ice block, then DestroySpawnedActor(true) (at once).
// Unverified: an item nobody owns (no instigator) is taken to fail the team test @1227. The passive elemental's
// handler ([`elemental_overlap`]) runs after the class's for every actor that passes the casts above; one with no
// owner (TeamIndex -1) is left out of both.
// Not ported: Buff_IceSlowProjectile on a World.actors actor (`slow_projectile` slows a projectile's or an item's
// movement component; no heavy or no-damage actor with a ProjectileMovement is in the ported roster): warned.
fn pellet_others(w: &mut World, pr: ProjectileId) {
    let f = owner(w, pr);
    let (px, py, hit_radius, pcls) = {
        let p = pr_ref(w, pr);
        (p.x, p.y, p.hit_radius, p.cls)
    };
    let types = hit_box_types(
        data_of(w, f)
            .fighter
            .actors
            .as_ref()
            .and_then(|m| m.get(w.names.str(pcls)))
            .and_then(|d| obj(&d.components, "HitBox")),
    );
    let found = if types.is_empty() {
        Vec::new()
    } else {
        sphere_overlap_actors(
            w,
            px,
            py,
            hit_radius,
            &types,
            &[Some(ActorRef::Projectile(pr))],
            crate::real(obj_type::HIT_BOX),
        )
    };
    st_mut(w, pr).touching.retain(|a| found.contains(a));
    for o in found {
        if matches!(o, ActorRef::Fighter(_) | ActorRef::Projectile(_)) || st(w, pr).touching.contains(&o) {
            continue;
        }
        st_mut(w, pr).touching.insert(o);
        let (Some(cls), Some(own)) = (actor_cls(w, o), actor_owner(w, o)) else { continue };
        if actor_dead(w, o) {
            continue;
        }
        let item = if let ActorRef::Item(it) = o { Some(it) } else { None };
        if item.is_none() {
            // the cast @1148
            match actor_is_a(w, ActorRef::Fighter(own), cls, name!("SpawnedActorParent_C")) {
                None => {
                    warn_text(w, &format!("{PELLET}: the class of {} is not in the data (left out)", w.names.str(cls)));
                    continue;
                }
                Some(false) => continue,
                Some(true) => {}
            }
        }
        'class: {
            if w.fighter(own).team.team == w.fighter(f).team.team {
                break 'class;
            }
            if elem_of(w, o, BuffClass::BuffElementalIceproof).is_some() {
                break 'class;
            }
            let heavy = match item {
                Some(it) => item_attack_type(w, it) == 1,
                None => num(actor_p(w, o), "ProjectileAttackType") == Some(1.0),
            };
            let cannot = match item {
                Some(it) => item_components_cannot_take_damage(w, it),
                None => actor_def(w, ActorRef::Fighter(own), cls).is_some_and(|d| cannot_take_damage(&d.components)),
            };
            if (heavy || cannot) && item.is_some() {
                // Buff_IceSlowProjectile on a thrown item (Buff_SlowProjectile @800-879); a held item's movement is
                // not active and the buff only waits for its end.
                slow_projectile(w, o, BuffClass::BuffIceSlowProjectile);
                break 'class;
            }
            if heavy || cannot {
                warn_text(w, &format!("Buff_IceSlowProjectile_C on {} (no projectile): not ported", w.names.str(cls)));
                break 'class;
            }
            let (x, y) = actor_place(w, o).map_or((0.0, 0.0), |p| (p.0, p.1));
            let charge = instigator_charge(w, f);
            comp_spawn_item(w, name!("IceBlockItem_C"), f, x, y, 0.0, 0.0, false, charge);
            destroy_spawned_actor(w, o, 0.0);
        }
        elemental_overlap(w, pr, o);
    }
}

/// `w.itemDef(it)?.components?.Comp_Pawn_DamageManager?.CanTakeDamage === false`.
fn item_components_cannot_take_damage(w: &World, it: ItemId) -> bool {
    crate::world::items::item_def_src(w, it).is_some_and(|src| cannot_take_damage(ItemDef::at(&w.data, src).components))
}

/// The pellet's sphere over a fighter: a hurt shape at the current pose. The sphere is the pellet's only collider
/// (object type HitBox, GameTraceChannel3) and BaseFighter's construction script @498-1639 sets the root capsule of a
/// fighter with HurtBox-tagged components to Ignore that channel: it overlaps hurt boxes only.
fn over(w: &mut World, pr: ProjectileId, o: FighterId) -> bool {
    if no_hit(w, o) {
        return false;
    }
    let Some(vp) = phases::victim_pose(w, o) else { return false };
    let p = pr_ref(w, pr);
    let sp = [WorldSphere { x: p.x, y: p.y, z: Some(0.0), r: p.hit_radius, ..WorldSphere::default() }];
    let met = crate::combat::hit::hurt::hurt_hit(w, o, &sp, &vp).is_some();
    phases::give_vbuf(w, vp);
    met
}

/// ReceiveTick @3920: the parent's, then SetActorScale3D(GetActorScale3D + 10 x DeltaSeconds) @4069 (the particle's
/// Scale parameter is the new scale + 10 x DeltaSeconds @4232: the projectile's `scale` is what a view needs). The
/// HitBox sphere is a child of the root and grows with it. The movement component ticks before its owner
/// (bTickBeforeOwner), so the order here is the build's: move, grow, test where the move ended.
// Unverified: the delta is the world's (the actor's own time dilation is not read).
fn step(w: &mut World, pr: ProjectileId) -> bool {
    let (end_age, unit) = {
        let s = st(w, pr);
        (s.end_age, s.on_unit)
    };
    let p = pr_mut(w, pr);
    p.scale += 10.0 * DELTA_SECONDS;
    p.hit_radius = unit * p.scale;
    if end_age > 0 && p.age >= end_age {
        return true;
    }
    if hit_collision(p) {
        pellet_actors(w, pr);
    }
    false
}

/// The HitBox's begin overlap with a BaseFighter (@10; its owner a BaseFighter @377).
/// Another team, the first time (PushedActors @635): the fighter joins PushedActors @924 unless it is immune to CC
/// group 8 or invulnerable @910; the branches behind it (the fighter's MoveCommands, its last launch's time,
/// HasPendingLaunch @3454-3915) all end in a return: nothing pushes.
// Unverified: the Move List says the breath pushes; the graph of this build does not, and nothing is done here.
/// The same team (@2517: a state manager, no NPC, not the owner @3190): SpawnBuffActor(
/// Buff_SupermanAllyIceGauntlet, the fighter, the owner, 1, 0.0) @3349, at each begin of an overlap.
// Deviation: one begin per pellet and fighter while they stay in touch (the build: one per component pair, which
// gives a second gauntlet to an ally who spent the first inside the same pellet).
fn overlap(w: &mut World, pr: ProjectileId, vic: FighterId) {
    let f = owner(w, pr);
    // !NotPlayerCharacter @2755-2801
    if vic == f || w.fighter(vic).team.team != w.fighter(f).team.team || w.fighter(vic).life.not_player_character {
        return;
    }
    let vid = w.fighter(vic).id;
    let k = st(w, pr).allies.iter().position(|&a| a == vid);
    let in_touch = {
        let p = pr_ref(w, pr);
        !(p.dead || w.fighter(vic).life.dead > 0.0 || !hit_collision(p))
    } && over(w, pr, vic);
    if !in_touch {
        if let Some(k) = k {
            st_mut(w, pr).allies.remove(k);
        }
        return;
    }
    if k.is_some() {
        return;
    }
    st_mut(w, pr).allies.push(vid);
    // Buff_SupermanAllyIceGauntlet's graph (its spend on the ally's next enemy hit) is `combat/gauntlet.rs`.
    spawn_buff(w, vic, name!("Buff_SupermanAllyIceGauntlet_C"), 0.0, Some(1), Some(f), true, None);
}

// HitEnemyPlayer: the passive Buff_Elemental_Ice's ElementalHitEnemy (SpawnBuffActor(Buff_Ice, 1, 20.0) on the
// victim, behind the hit's own 'On Hit Buffs' row: the 0.1 s NewStackApplyBuffer refuses that second stack), and the
// elemental of an actor that is no fighter is destroyed: the World's projectile hit runs it for every actor with an
// elemental; the class has no HitEnemyPlayer of its own.

/// The kind of Superman_IceBreath_C and Superman_IceBreathEducated_C (a data-only child of the pellet: its own HitBox
/// sphere, radius 70 x 4, and the notify's speed 1800; its spawner notifies stand behind HasEducatedBuff_C (Velma's
/// Buff_Educated), which no ported fighter is ever given, so nothing spawns it yet).
fn pellet_kind() -> ProjectileHooks {
    ProjectileHooks {
        make: Some(make),
        begin: Some(begin),
        step: Some(step),
        overlap: Some(overlap),
        ..ProjectileHooks::default()
    }
}

// ---- Superman_IceBreath_Stream

fn stream_state(w: &mut World, id: ActorId) -> &mut IceStream {
    match &mut w.actors.actors.get_mut(id).expect("a live actor").state {
        WorldActorState::IceStream(s) => s,
        _ => panic!("an ice stream"),
    }
}

/// BeginPlay @10: the team colour parameter (a view's). `socket`: the notify's Socket.
fn stream_spawn(w: &mut World, f: FighterId, r: &SpawnRequest) -> (WorldActorState, bool) {
    let np = request_props(w, f, r);
    let socket = text(&np, "Socket").map_or(name!("None"), |s| data_name(w, s));
    (WorldActorState::IceStream(IceStream { rotation: 0.0, socket }), false)
}

/// ReceiveTick @276: Rotation = FInterpTo(Rotation, Lerp(65, -65, NormalizeToRange(FClamp(the owner's
/// GetVelocity().Z, -1200, 1200), -1200, 1200)), DeltaSeconds, 5) @556, set as the IceStream component's relative
/// rotation (Roll 0, Pitch Rotation, Yaw 0) @690.
// Unverified: the owner's velocity is read where the world ticks its actors (behind the fighters' movement of the
// frame); the actor's tick against its owner's is not measured.
fn stream_step(w: &mut World, id: ActorId) {
    let owner = w.actors.actors.get(id).expect("a live actor").owner;
    let z = math::min(math::max(w.fighter(owner).body.vy, -1200.0), 1200.0);
    let n = (z + 1200.0) / 2400.0;
    let target = 65.0 + (n * -130.0);
    let s = stream_state(w, id);
    // FMath::FInterpTo: the target when the distance squared is below SMALL_NUMBER, else the current value plus the
    // distance x Clamp(DeltaTime x InterpSpeed, 0, 1).
    let dist = target - s.rotation;
    if dist * dist < 1e-8 {
        s.rotation = target;
    } else {
        s.rotation += dist * math::min(math::max(DELTA_SECONDS * 5.0, 0.0), 1.0);
    }
}

// ---- the hooks and the entries

/// AnimNotify_ConditionalSpawner's begin for a pellet class: the pellet's BeginPlay runs inside this CompSpawnActor
/// call, and its BPI_UpdateIceBreath takes one ice from the owner ([`pawn::spend_ice`]) inside the mesh tick, ahead of
/// the montage's Condition_NoIceBreathSuperman test of the next tick.
fn spawner(w: &mut World, f: FighterId, n: NotifyRef) {
    let data = w.data.clone();
    let class = text(&notify_data(w, &data, f, n).p, "ActorClass");
    if class == Some(PELLET) || class == Some(PELLET_EDUCATED) {
        pawn::spend_ice(w, f);
    }
}

/// The hooks of this file in Superman's CharHooks (`ice.rs` HOOKS: spawner).
pub fn hooks(h: &mut CharHooks) {
    h.spawner = Some(spawner);
}

/// This file's registry entries: the two pellet classes and the stream. Buff_SupermanAllyIceGauntlet, which a pellet
/// gives a team mate, is a shared class of `combat/gauntlet.rs`.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.actors.enter(
        PELLET,
        ActorEntry::new(
            ActorClass::Projectile(ProjectileClass::SupermanIceBreath),
            ActorHooks::Projectile(pellet_kind()),
        ),
    )?;
    r.actors.enter(
        PELLET_EDUCATED,
        ActorEntry::new(
            ActorClass::Projectile(ProjectileClass::SupermanIceBreathEducated),
            ActorHooks::Projectile(pellet_kind()),
        ),
    )?;
    // no shape: the view's IceStream particle at the root's scale
    let stream = WorldActorHooks {
        spawn: Some(stream_spawn),
        step: Some(stream_step),
        size: Some(SizeHooks::default()),
        ..WorldActorHooks::default()
    };
    r.actors.enter(
        STREAM,
        ActorEntry::new(ActorClass::World(WorldActorClass::SupermanIceBreathStream), ActorHooks::World(stream)),
    )
}
