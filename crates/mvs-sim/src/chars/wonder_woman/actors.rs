//! Wonder Woman's actors (all but its two WW_ShieldActor entries: shield.rs):
//! WonderWoman_MeterBracerClash_C ([`BracerClash`], a projectile's `ext`, its growth by the meter), WW_BracerShield_C
//! ([`BracerShield`], a world actor), LassoProjectileFastV3_C ([`Lasso`], a projectile of the tether family with its
//! sphere and its pull), Buff_NullifyNextHit ([`NullifyNextHit`], a buff's `ext`) and the condition
//! LassoOutCondition.
//!
//! The shared code reaches the bracer shield's place (`actors::world_actor`: [`bracer_place`]).
//!
//! Deviation: the lasso's TetherParams come in through `ProjectileHooks::make_opts` (the spawn's constructor
//! arguments); its own state is made in `construct`.

use super::perks_lasso::{
    LassoPerkState, lasso_bounce, lasso_grapple_box, lasso_grapple_on, lasso_perk_begin, lasso_perk_tick,
    lasso_tipper_spares,
};
use super::state::{METER_PER_BUFF_BLOCK, add_meter, clear_meter, meter_of, wonder_woman_mut};
use crate::Real;
use crate::State;
use crate::actors::projectile::{
    CollisionResponse, MakeOpts, ProjectileExt, TetherParams, class_def, class_p, hit_collision, set_hit_buff_duration,
    set_hitbox,
};
use crate::actors::world_actor::{WorldActor, WorldActorState};
use crate::actors::{ActorClass, ActorId, ActorRef, Projectile, ProjectileClass, ProjectileId, WorldActorClass};
use crate::combat::boxes::WorldSphere;
use crate::combat::buffs::{buff, buff_mut, spawn_buff};
use crate::combat::elem::{charge_buff, charge_pct};
use crate::combat::hits::{actor_ally_hit, actor_hit_enemy, apply_projectile_hit, dealer, projectile_blocks};
use crate::combat::phases::{alive, give_vbuf, los_blocked, no_hit, victim_pose};
use crate::combat::{HitPacket, OnHitBuffRow};
use crate::consts::DELTA_SECONDS;
use crate::events::Event;
use crate::fighter::common::{actor_data, latent_frames};
use crate::fighter::control::{attack_dodged, dodge_over};
use crate::fighter::match_side::{SpawnRequest, TutorialEvent};
use crate::fighter::moves::V2;
use crate::fighter::traces::{TraceBody, body_info};
use crate::fighter::{FighterId, actor_y, facing, invulnerable};
use crate::math::ue::{deg_atan2, size2d};
use crate::math::{max, min, to_int32};
use crate::name;
use crate::names::Name;
use crate::registry::entries::{
    ActorEntry, ActorHooks, BuffEntry, BuffHooks, Condition, ProjectileHooks, WorldActorHooks,
};
use crate::registry::props::{num, obj};
use crate::registry::signals::{BuffSignal, BuffSignalArg};
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use crate::world::blast::{BlastBounds, blast_box_loop, new_blast_bounds, spawn_box};
use crate::world::flow::{reset_pass, tutorial_event};
use crate::world::pawns::{all_list, match_fighters, put_fighters};
use crate::world::spawning::destroy_projectile;
use crate::{combat::buff_class::BuffClass, combat::buffs::BuffExt};

const NULLIFY: Name = name!("Buff_NullifyNextHit_C");
const CLASH: &str = "WonderWoman_MeterBracerClash_C";
const LASSO: &str = "LassoProjectileFastV3_C";
const BRACER_SHIELD: &str = "WW_BracerShield_C";
/// The build's fml_un_L_wristSocket as the sim's tracks name it (facing right the L track), and the mirror.
const LASSO_SOCKET: Name = name!("fml_un_L_wristSocket");
const LASSO_SOCKET_MIRROR: Name = name!("fml_un_R_wristSocket");
const LASSO_SOCKET_SHAPE: Name = name!("socket:fml_un_L_wristSocket");

/// `lerp`: fround(a + fround(t x fround(b - a))).
fn lerp(a: Real, b: Real, t: Real) -> Real {
    a + (t * (b - a))
}

fn sound(w: &mut World, f: FighterId, ev: Name, x: Real, y: Real) {
    let fighter = w.fighter(f).id;
    w.events.push(Event::Sound { fighter, ev, x: Some(x), y: Some(y), hit: None, pooled: false, key: None });
}

fn emitter(w: &mut World, cls: Name, f: FighterId, vfx: Name, x: Real, y: Real, scale: Real) {
    let fighter = w.fighter(f).id;
    w.events.push(Event::ProjectileDeath {
        cls,
        fighter,
        vfx: Some(vfx),
        x,
        y,
        ox: 0.0,
        rot: [0.0; 3],
        scale,
        tint: None,
        to: None,
    });
}

fn pr_ref(w: &World, id: ProjectileId) -> &Projectile {
    w.actors.projectiles.get(id).expect("a live projectile")
}

fn pr_mut(w: &mut World, id: ProjectileId) -> &mut Projectile {
    w.actors.projectiles.get_mut(id).expect("a live projectile")
}

// ---- WonderWoman_MeterBracerClash (the down attack "Amazon Shout", WonderWoman_ChargeBracer_Montage)
// A ParentProjectilePhysics child (through WonderWoman_BracerClash) attached to the owner's Root socket, 150 up, at
// scale 5; EnableProjectileMovement is false. The world's Projectile is the parent (the attach, HitBoxOn? two
// ticks after ActivateObject: SpawnedActorParent's Delay(0.02)).

/// `BracerClash extends Projectile`: what the class adds.
#[derive(State, Debug)]
pub struct BracerClash {
    /// GrowthScale: the world scale of HitBox, StaticMesh and HitBox2D (uniform), for the view's mesh too. It starts
    /// at the root's scale (the notify's Scale3D 5: the Capsule has bAbsoluteScale).
    pub growth: Real,
    pub growth_value: Real,
    /// The hit circle's radius at scale 1.
    pub unit: Real,
    /// The ticks of the Delay(0.02) and of the Delay(HitDuration) behind it, -1 for none.
    pub on_age: i32,
    pub off_age: i32,
}

impl Default for BracerClash {
    /// The field initial values.
    fn default() -> Self {
        BracerClash { growth: 1.0, growth_value: 0.0, unit: 0.0, on_age: -1, off_age: -1 }
    }
}

fn clash_mut(w: &mut World, id: ProjectileId) -> &mut BracerClash {
    match &mut pr_mut(w, id).ext {
        ProjectileExt::BracerClash(s) => s,
        _ => panic!("a bracer clash"),
    }
}

fn clash(w: &World, id: ProjectileId) -> &BracerClash {
    match &pr_ref(w, id).ext {
        ProjectileExt::BracerClash(s) => s,
        _ => panic!("a bracer clash"),
    }
}

fn clash_construct(_w: &World, p: &mut Projectile) {
    p.ext = ProjectileExt::BracerClash(BracerClash::default());
}

/// ReceiveBeginPlay (WonderWoman_MeterBracerClash @963 -> @322), with p = the owner's BPI_GetMeterPercentage @421 (0
/// for an owner that is no Wonder Woman @935): GrowthValue = Lerp(GrowthValue, MaxGrowthRate, p) @467-522 (both
/// 1.05), ScaleClamp = Lerp(5, 15, p) @549-604 (nothing reads it), HitBoxDamage.KnockBackScaling = Lerp(0,
/// KnockBackScaling x 2, p) @631 / @733 / @884 and Damage = Lerp(Damage, Damage x 3.5, p) @682 / @784 / @848:
/// 5 damage and no scaling with an empty meter, 17.5 and 15 with a full one. Then the parent's BeginPlay @920.
fn clash_begin(w: &mut World, id: ProjectileId) {
    let p = pr_ref(w, id).owner.map_or(0.0, |f| meter_of(w, f));
    let (growth_value, scaling, damage) = {
        let pr = pr_ref(w, id);
        let cp = class_p(w, pr);
        let h = &pr.hit;
        (
            lerp(num(cp, "GrowthValue").unwrap_or(0.0), num(cp, "MaxGrowthRate").unwrap_or(0.0), p),
            lerp(0.0, h.knock_back_scaling.unwrap_or(0.0) * 2.0, p),
            lerp(h.damage.unwrap_or(0.0), h.damage.unwrap_or(0.0) * 3.5, p),
        )
    };
    let pr = pr_mut(w, id);
    pr.hit.knock_back_scaling = Some(scaling);
    pr.hit.damage = Some(damage);
    let (scale, radius) = (pr.scale, pr.hit_radius);
    let c = clash_mut(w, id);
    c.growth_value = growth_value;
    c.growth = scale;
    c.unit = if scale != 0.0 { radius / scale } else { 0.0 };
}

/// pfgDoDamage for a team mate inside the hitbox (`actor_ally_hit`: the Buff_ArmorPack row of 'On Hit Buffs',
/// ApplyBuffHit? and ApplyBuffAlly?, then NotifyAllyOfHit), behind HitEachActorOnlyOnce, DoLOSCheck and the
/// invulnerable test, as the enemy test of World.projectileTest. The enemies' hits are the world's, and so are the
/// actors that are no fighter: the projectiles' hit bodies meet them (`combat::spawned_hits`), a team mate's
/// WW_ShieldActor and VelmaEvidence among them (their bodies, `ActorEntry::spawned_meet_bodies`), which take the
/// Buff_ArmorPack row through the ally rows @5931-6835.
/// Unverified: every overlapped actor gets the call in the build (minions too: native); the port tests the fighters.
pub(crate) fn clash_allies(w: &mut World, id: ProjectileId) {
    let Some(f) = pr_ref(w, id).owner else { return };
    let all = all_list(w);
    for &o in &all {
        let oid = w.fighter(o).id;
        if o == f
            || w.fighter(o).team.team != w.fighter(f).team.team
            || !alive(w, o)
            || no_hit(w, o)
            || pr_ref(w, id).hit_set.contains(&oid)
        {
            continue;
        }
        let Some(vp) = victim_pose(w, o) else { continue };
        let (px, py, r, s3) = {
            let p = pr_ref(w, id);
            (p.x, p.y, p.hit_radius, p.sphere_3d)
        };
        let sphere = WorldSphere { x: px, y: py, z: if s3 { Some(0.0) } else { None }, r, ..WorldSphere::default() };
        let met = crate::combat::hit::hurt::hurt_hit(w, o, &[sphere], &vp).is_some();
        give_vbuf(w, vp);
        if !met {
            continue;
        }
        if pr_ref(w, id).hit.do_los_check.unwrap_or(false) {
            let (ox, oy) = (w.fighter(o).body.x, w.fighter(o).body.y);
            if los_blocked(w, px, py, ox, oy) {
                continue;
            }
        }
        if invulnerable(w, o) {
            continue;
        }
        if pr_ref(w, id).hit.hit_each_actor_only_once.unwrap_or(false) {
            pr_mut(w, id).hit_set.insert(oid);
        }
        let rows = {
            let pr = pr_ref(w, id);
            let rows = obj(class_p(w, pr), "On Hit Buffs").and_then(|o| o.get("OnHitBuffs"));
            OnHitBuffRow::rows(&w.names, rows)
        };
        let mult = charge_buff(w, ActorRef::Projectile(id));
        let (d, hit) = (dealer(w, ActorRef::Projectile(id)), pr_ref(w, id).hit.clone());
        actor_ally_hit(w, &d, o, &hit, &rows, mult);
    }
    put_fighters(w, all);
}

/// ReceiveTick (WonderWoman_BracerClash @1307), while not Activated: GrowthScale = HitBox.GetComponentScale x
/// (1 + DeltaSeconds x GrowthValue) @1365-1549, set as the world scale of HitBox, StaticMesh and HitBox2D
/// @1576-1658. The latent actions of ActivateObject follow (clash_activate).
// Unverified: the actor's delta is taken as the world's (it is attached to the owner but has its own time dilation).
fn clash_step(w: &mut World, id: ProjectileId) -> bool {
    if !pr_ref(w, id).activated {
        let c = clash_mut(w, id);
        c.growth *= 1.0 + (DELTA_SECONDS * c.growth_value);
        let (unit, growth) = (c.unit, c.growth);
        pr_mut(w, id).hit_radius = unit * growth;
    } else {
        // @15 (behind SpawnedActorParent's Delay(0.02) @843): every OnHitBuffs row of class Buff_ArmorPack gets
        // BuffDuration = GrowthScale.Z x 0.2 @338-426, then HitBoxOn? = true and UpdateHitbox @482-493 (the
        // parent's, Projectile.step); Delay(HitDuration 0.15) @507 -> HitBoxOn? = false @566. The timer of
        // HitboxDuration 0.2 (HitboxExpired @1021) finds it off already.
        // UpdateHitbox (neither WonderWoman_BracerClash nor WonderWoman_MeterBracerClash overrides it, nor sets
        // HitBoxScale) -> SpawnedActorParent @15092 -> @8122 -> HitBoxesOn(..., HitBoxScale (1, 1, 1)) @8531 ->
        // Comp_Pawn_HitBoxHandler ActivateHitbox @16855 SetRelativeScale3D(HitBoxScale): the growth's world scale on
        // the hit box gives way to the Capsule's (the spawn scale 5) x 1, so the clash deals at the spawn scale
        // whatever the meter; the growth drives the mesh, the armor duration, the blast sound and the emitter alone
        // (as Steven_BubbleCharge, Steven's bubble).
        let (on_age, off_age, growth) = {
            let c = clash(w, id);
            (c.on_age, c.off_age, c.growth)
        };
        let age = pr_ref(w, id).age;
        if age == on_age {
            set_hit_buff_duration(w, id, "Buff_ArmorPack_C", growth * 0.2);
            let unit = clash(w, id).unit;
            let p = pr_mut(w, id);
            p.hit_radius = unit * p.scale;
        }
        if age == off_age {
            set_hitbox(pr_mut(w, id), false);
        }
    }
    if hit_collision(pr_ref(w, id)) {
        clash_allies(w, id);
    }
    false
}

/// ActivateObject (WonderWoman_MeterBracerClash @53): the parent's @1700 (a do-once @1147 / @739: Activated,
/// SetVisuals(true), the Delay(0.02) above), then the owner's BPI_ClearMeter @162, HurtBox.SetCollisionEnabled
/// (QueryOnly) @199 and the HitboxDuration timer.
/// SetVisuals(true) -> FX_Blast @2334-2344: sfx_cha_wonderwoman_charge_bracer_blast_01 while VSizeXY(GrowthScale)
/// < 13.472, else _02, and PS_WonderWomanBracers_Activate on the Capsule at GrowthScale x 0.1.
fn clash_activate(w: &mut World, id: ProjectileId) {
    if clash(w, id).on_age >= 0 {
        return;
    }
    let age = pr_ref(w, id).age;
    let hit_duration = num(class_p(w, pr_ref(w, id)), "HitDuration").unwrap_or(0.0);
    let on = age + to_int32(latent_frames(0.02));
    let c = clash_mut(w, id);
    c.on_age = on;
    c.off_age = on + to_int32(latent_frames(hit_duration));
    let growth = c.growth;
    let Some(f) = pr_ref(w, id).owner else { return };
    let (cls, px, py) = {
        let p = pr_ref(w, id);
        (p.cls, p.x, p.y)
    };
    let ev = if size2d([growth, growth]) < 13.472 {
        name!("sfx_cha_wonderwoman_charge_bracer_blast_01")
    } else {
        name!("sfx_cha_wonderwoman_charge_bracer_blast_02")
    };
    sound(w, f, ev, px, py);
    emitter(w, cls, f, name!("PS_WonderWomanBracers_Activate"), px, py, growth * 0.1);
    clear_meter(w, f);
}

// ---- WW_BracerShield (neutral special "Defense of the Gods": Special_ShieldBuff, Special__Air_ShieldBuff)
// A SpawnedActorParent child attached to the owner (no socket: the actor itself), 30 above its location.

/// `BracerShield` (WW_BracerShield_C).
#[derive(State, Debug, Default)]
pub struct BracerShield {
    pub age: i32,
    pub x: Real,
    pub y: Real,
    /// SpawnedActorParent's stored bounds; its CheckBlastBoxTimer (0.1 s) never comes before the Delay(0.01) ends it.
    pub blast_bounds: BlastBounds,
    /// The Transforms' Translation Z.
    pub up: Real,
    /// Sphere1's at the spawn's Scale3D.
    pub radius: Real,
}

fn shield_mut(w: &mut World, id: ActorId) -> Option<(&mut WorldActor, bool)> {
    let a = w.actors.actors.get_mut(id)?;
    let ok = matches!(a.state, WorldActorState::BracerShield(_));
    Some((a, ok))
}

/// Sphere1 now: SetSizeScale's SetActorScale3D sets the root's world scale whole, in place of the spawn's (absolute).
fn scaled_radius(w: &World, a: &WorldActor, s: &BracerShield) -> Real {
    match a.size_scale {
        None => s.radius,
        Some(k) => {
            let r = actor_data(w, &w.data, ActorRef::Fighter(a.owner), a.cls)
                .and_then(|d| obj(&d.components, "Sphere1"))
                .and_then(|c| num(c, "SphereRadius"))
                .unwrap_or(0.0);
            r * k
        }
    }
}

/// The bracer shield's place (its `x`, `y`), `None` for none.
pub fn bracer_place(_w: &World, _a: &WorldActor, s: &BracerShield) -> Option<(Real, Real)> {
    Some((s.x, s.y))
}

/// ReceiveBeginPlay @3715 -> @3496: the instigator's Comp_Pawn_AbilityManager.TriggerAbilityActual(1) @1818 and
/// SpawnBuffActor(Buff_NullifyNextHit, the owner, the owner as Instigator, 0 stacks, ShieldBuffDuration 3.5)
/// @3331, then Delay(0.01) @3441. The parent's BeginPlay @3715 comes first: its blast-box loop destroys the actor
/// once BeginPlay returns, the rest run whole.
fn shield_spawn(w: &mut World, f: FighterId, r: &SpawnRequest) -> (WorldActorState, bool) {
    let data = w.data.clone();
    let a = actor_data(w, &data, ActorRef::Fighter(f), r.cls);
    let np = crate::world::spawning::request_props(w, f, r);
    let t = obj(&np, "Transforms");
    let up = t.and_then(|t| obj(t, "Translation")).and_then(|t| num(t, "Z")).unwrap_or(0.0);
    let scale = t.and_then(|t| obj(t, "Scale3D")).and_then(|t| num(t, "X")).unwrap_or(1.0);
    let sphere = a.and_then(|a| obj(&a.components, "Sphere1")).and_then(|c| num(c, "SphereRadius")).unwrap_or(0.0);
    let (bx, ay) = (w.fighter(f).body.x, actor_y(w, f));
    let mut s =
        BracerShield { age: 0, x: bx, y: ay + up, blast_bounds: new_blast_bounds(), up, radius: sphere * scale };
    let p = a.map(|a| &a.p);
    let force = p.is_some_and(|p| crate::registry::props::flag(p, "ForceSpawnPositionInsideBlastBox"));
    let exits = p.is_some_and(|p| crate::registry::props::flag(p, "DestroyOnExitedArena?"));
    let sb = spawn_box(w);
    let dead = blast_box_loop(&sb, &mut s.x, &mut s.y, None, force, &mut s.blast_bounds, exits);
    crate::fighter::control::trigger_ability(w, f, 1);
    let dur = p.and_then(|p| num(p, "ShieldBuffDuration")).unwrap_or(0.0);
    spawn_buff(w, f, NULLIFY, dur, Some(0), Some(f), true, None);
    (WorldActorState::BracerShield(s), dead)
}

/// @15, behind the Delay(0.01): every actor that overlaps Sphere1 (radius 1200) and is a BaseFighter, a player
/// character and of the owner's team @434-989, the owner among them, gets SpawnBuffActor(Buff_NullifyNextHit,
/// 3.5 s, the owner as Instigator) @999 and SpawnBuffActor(Buff_Cleanse, duration 0) @1109; then K2_DestroyActor
/// @1578. The owner's second Buff_NullifyNextHit is refused (one is there: no CanOverride, stack or refresh).
/// The ally's sound and beam need GuardedAlly, which nothing writes. ReceiveEndPlay @3691
/// removes the armor of ActorsGivinArmor, an empty list.
// Deviation: the overlap is the sphere against each fighter's root capsule in the play plane (World.sphereTraceMulti),
// in the sim's fighter order; the Delay counts the spawn frame (the actor ticks as newly spawned).
fn shield_step(w: &mut World, id: ActorId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let f = a.owner;
    let (fx, fy) = (w.fighter(f).body.x, actor_y(w, f));
    let (ok, age) = {
        let Some((a, ok)) = shield_mut(w, id) else { return };
        let WorldActorState::BracerShield(s) = &mut a.state else { return };
        s.age += 1;
        s.x = fx;
        s.y = fy + s.up;
        (ok, s.age)
    };
    if !ok || f64::from(age) != f64::from(latent_frames(0.01)) {
        return;
    }
    let (x, y, r, dur) = {
        let a = w.actors.actors.get(id).expect("the shield");
        let WorldActorState::BracerShield(s) = &a.state else { return };
        let dur = actor_data(w, &w.data, ActorRef::Fighter(f), a.cls)
            .and_then(|d| num(&d.p, "ShieldBuffDuration"))
            .unwrap_or(0.0);
        (s.x, s.y, scaled_radius(w, a, s), dur)
    };
    let all = all_list(w);
    let hit = crate::world::capsules_in_sphere(w, &all, x, y, r);
    put_fighters(w, all);
    for o in hit {
        if w.fighter(o).life.not_player_character || w.fighter(o).team.team != w.fighter(f).team.team {
            continue; // !BPI_GetNotPlayerCharacter AND same TeamIndex @602-989
        }
        spawn_buff(w, o, NULLIFY, dur, Some(0), Some(f), true, None);
        spawn_buff(w, o, name!("Buff_Cleanse_C"), 0.0, Some(0), Some(f), true, None);
        if o != f {
            tutorial_event(w, TutorialEvent::UniqueCharacterMechanic2); // not the Instigator (@1253): @1538
        }
    }
    if let Some(a) = w.actors.actors.get_mut(id) {
        a.dead = true;
    }
}

/// The root Capsule as a body of the traces (`ActorKind.bodies`): Custom, object type WorldStatic, query on (the
/// shape's OverlapAllDynamic enable: no CollisionEnabled saved), Overlap to the channels it lists; its size the
/// SpawnedActorParent template's, which saves none: UCapsuleComponent's 22 / 44, at the root's scale (bAbsoluteScale:
/// the spawn's Scale3D, or SetSizeScale's). Upright at the actor. What meets it: the object queries for WorldStatic
/// (SupermanGroundLazer LineTraceThroughPlatforms ends there @3804).
// Unverified: the size from UCapsuleComponent's constructor defaults, not read in the exe for this build.
fn shield_bodies(w: &World, id: ActorId, out: &mut Vec<TraceBody>) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let WorldActorState::BracerShield(s) = &a.state else { return };
    if a.dead {
        return;
    }
    let comps = actor_data(w, &w.data, ActorRef::Fighter(a.owner), a.cls).map(|d| &d.components);
    let cap = comps.and_then(|c| obj(c, "Capsule"));
    let Some(info) = body_info(cap, None) else { return };
    if !info.on {
        return;
    }
    let r1 = comps.and_then(|c| obj(c, "Sphere1")).and_then(|c| num(c, "SphereRadius")).unwrap_or(0.0);
    let k = if r1 > 0.0 { scaled_radius(w, a, s) / r1 } else { 1.0 }; // the root's scale, as Sphere1's radius takes it
    let r = cap.and_then(|c| num(c, "CapsuleRadius")).unwrap_or(22.0) * k;
    let hh = cap.and_then(|c| num(c, "CapsuleHalfHeight")).unwrap_or(44.0) * k;
    let seg = max(hh - r, 0.0);
    out.push(TraceBody {
        ax: s.x,
        ay: s.y - seg,
        bx: s.x,
        by: s.y + seg,
        hx: 0.0,
        hy: 0.0,
        r,
        z: Real::NAN,
        object_type: info.kind,
        profile: info.profile,
        component: name!("Capsule"),
        actor: Some(ActorRef::Actor(id)),
        platform: -1,
        blocks: info.blocks,
        poly: None,
        floor: None,
    });
}

// ---- LassoProjectileFastV3 (neutral special "Lasso of Truth": WonderWoman_LassoThrowFast / _AirFast)
// A SpawnedActorParent child of the tether family (actors/projectile.rs TetherParams, whip_tick): thrown at the
// notify's 4250, the stick bends it while CanControl, BeginPlay's Delay(LassoTravelDuration 0.275) @4601 -> @15
// StartReturn, whose Delay(0.033) @803 -> ReturnMovementServer @12886: Velocity = unit(the actor -> the instigator
// mesh's socket fml_un_L_wristSocket, always the left one) x 4900 @12971-13190, Bounciness 0.1 @13239, homing at
// InitialSpeed x 10 on the instigator's root @13284-13492, and ReturnMovementMulticast @12627:
// HitSphere.SetCollisionEnabled(QueryOnly) @12787, bSweepCollision = LassoGrappleActive (false) @12825; then
// Delay(0.36) @1216 -> K2_DestroyActor @1286 while not Attached.
// MaxSpeed is not written (it stays the spawner's 4250), so the first ComputeVelocity cuts the 4900: one move of
// (4900 + 4250) / 2 / 60 = 76.25, then 70.83 a tick (ComputeMoveDelta 1429a0e00).
// Morty's C019_Attack_Air_Whip_Montage spawns this class too (the build's data): his mesh has no
// fml_un_L_wristSocket (the return aims at the mesh's origin, what GetSocketLocation gives for a missing socket)
// and his AnimBP is no WonderWoman_AnimBPV2 (the aim writes' cast fails @11160 / @21809: wonder_woman_mut).
// The parent hitbox is off for good (HitBoxOn? false): everything the lasso does to a fighter is HitSphere's
// begin overlap (lasso_overlap), from the return's tick on.
// Unverified: the Delays count from the tick that starts them; the blast box's sides (Tick @8560-9121 ->
// StartReturn) are the whip's GetActorBounds model.
// The perk branches (Buff_Perk_WonderWomanLassoGrapple: the Platform response, the bounce's grapple,
// LassoGrappleServer, ActivateGrappleDamageBox; Buff_Perk_WonderWoman_LassoTipper: ActivateLassoTipper,
// LassoTipperBuffer, TipperSet) are in perks_lasso.rs; the Capsule overlap @22931 changes nothing there (see that
// file). Not ported: the Box component's ComponentHit @10301 (StartReturn while CanControl; a child shape under
// PS_TetherProjectile, which a root sweep does not test: no hit event is taken to reach it), OnProjectileBounce
// @10316 (perk only behind its Delay), BPI_IncrementCoopActions @14503 and the HitAlly broadcast @14540 (no reader
// in the port), Buff_ChargeOnDamage.ReBindHitTarget @7759.
// The wrist sockets under the mesh mirror. The build turns a fighter by WAnimationMirrorNode (bEnabled = FacingLeft
// in WonderWoman_AnimBPV2), whose table (WonderWoman_MirrorTable) swaps fml_un_L_wrist and fml_un_R_wrist: facing
// left, the bone the build calls L is where the right-facing R bone's mirror image is, and the other way round.
// The sim's box tracks are the right-facing pose, placed by x * facing (a reflection): a track keeps the hand
// its name has facing right. So, in the sim's names:
//   the build's fml_un_L_wristSocket = facing right the L track, facing left the R track (the return's target,
//     @13148, the socket not chosen by the facing: the hand at her back when she faces left);
//   the build's facing-chosen socket (Tick @7920-8439: FacingLeft ? R : L) = the L track at either facing (the
//     rope's source: the throwing hand).
// The pose read is the AnimBP's LassoAim mix of the tick (pawn.rs slot_compose, through the fighter's sampled
// boxes): the arm raised or lowered by LassoAimU / LassoAimD as they stood at her anim update, a tick behind the
// writes below.

/// `Lasso extends Projectile`: what the class adds (its perk variables, perks_lasso.rs's, among them).
#[derive(State, Debug)]
pub struct Lasso {
    /// EnemyLassoed.
    pub enemy_lassoed: bool,
    /// AllyLassoed.
    pub ally_lassoed: bool,
    /// HitSphere's collision (ReturnMovementMulticast).
    pub sphere_on: bool,
    /// HitSphere's world radius.
    pub radius: Real,
    /// The ids of the fighters HitSphere overlaps (begin-overlap bookkeeping).
    pub inside: Vec<i32>,
    /// GateVO: the age PlayVO's Delay(1.0) reopens it at.
    pub vo_gate: i32,
    /// For the view's rope, written each tick the actor lives (the far end is the actor: x, y; alive: !dead): the
    /// beam's source, the instigator mesh's wrist socket by its facing in that tick. Nothing in the sim reads them.
    pub wrist_x: Real,
    pub wrist_y: Real,
    /// The perk variables, made at BeginPlay with a lasso perk only (perks_lasso.rs).
    pub perk: Option<LassoPerkState>,
}

impl Default for Lasso {
    /// The field initial values.
    fn default() -> Self {
        Lasso {
            enemy_lassoed: false,
            ally_lassoed: false,
            sphere_on: false,
            radius: 0.0,
            inside: Vec::new(),
            vo_gate: -1,
            wrist_x: 0.0,
            wrist_y: 0.0,
            perk: None,
        }
    }
}

pub(super) fn lasso(w: &World, id: ProjectileId) -> &Lasso {
    match &pr_ref(w, id).ext {
        ProjectileExt::Lasso(s) => s,
        _ => panic!("a lasso"),
    }
}

pub(super) fn lasso_mut(w: &mut World, id: ProjectileId) -> &mut Lasso {
    match &mut pr_mut(w, id).ext {
        ProjectileExt::Lasso(s) => s,
        _ => panic!("a lasso"),
    }
}

/// The return's target, the build's fml_un_L_wristSocket (see above).
// Deviation: the track is chosen by her facing at the throw (the spawn resolves TetherParams.socket once); the build
// reads the socket at the return's tick, 19 ticks later. The two throw montages do not turn her; a turn inside
// those ticks by another cause would aim the one return write at the other wrist.
fn lasso_opts(w: &World, f: FighterId, r: &SpawnRequest) -> MakeOpts {
    let travel = crate::fighter::common::actor_def(w, ActorRef::Fighter(f), r.cls)
        .and_then(|a| num(&a.p, "LassoTravelDuration"))
        .unwrap_or(0.0);
    let socket = if facing(w, f) < 0.0 { LASSO_SOCKET_MIRROR } else { LASSO_SOCKET };
    MakeOpts {
        tether: Some(TetherParams {
            travel,
            return_speed: 4900.0,
            return_max_speed: None,
            socket: Some(socket),
            hit_sphere: None,
            snap: false,
        }),
        ..MakeOpts::default()
    }
}

fn lasso_construct(w: &World, p: &mut Projectile) {
    let r = class_def(w, p)
        .and_then(|d| obj(&d.components, "HitSphere"))
        .and_then(|c| num(c, "SphereRadius"))
        .unwrap_or(0.0);
    p.ext = ProjectileExt::Lasso(Lasso { radius: r * p.scale, ..Lasso::default() });
}

/// The throwing hand's place: the instigator's mesh plus the facing-chosen socket's track (see above).
fn lasso_wrist(w: &mut World, id: ProjectileId) {
    let Some(f) = pr_ref(w, id).owner else { return };
    let si = w.fixed.samplers.get(&w.fighter(f).ch).and_then(|b| b.by_name.get(&LASSO_SOCKET_SHAPE).copied()).and_then(
        |si| {
            let b = w.fixed.samplers.get(&w.fighter(f).ch)?;
            b.offsets.get(usize::try_from(si).ok()?).copied()
        },
    );
    let pose = si.and_then(|_| crate::fighter::anim::sampled_boxes(w, f));
    let (mut x, mut y) = (crate::fighter::mesh_wx(w, f), crate::fighter::mesh_wy(w, f));
    if let (Some(pose), Some(o)) = (pose, si) {
        let o = usize::try_from(o).expect("a box offset");
        x += crate::real(pose[o]) * facing(w, f);
        y += crate::real(pose[o + 1]);
        crate::fighter::anim::put_pose(w, pose);
    }
    let l = lasso_mut(w, id);
    l.wrist_x = x;
    l.wrist_y = y;
}

/// ResetLassoAimServer @21884-21929: the AnimBP's LassoAimD and LassoAimU = 0.
fn lasso_aim_reset(w: &mut World, id: ProjectileId) {
    let Some(f) = pr_ref(w, id).owner else { return };
    if let Some(s) = wonder_woman_mut(w, f) {
        s.lasso_aim_u = 0.0;
        s.lasso_aim_d = 0.0;
    }
}

/// The instigator's IsPickedUp delegate (broadcast on the lassoed ally @14591): `combat::holds::picked_up`.
/// PlayVO(Target) @22055: behind GateVO, PF_Post_VO_Event(the owner, 'ability_lasso', Target, 0.5) @22090, then
/// GateVO for Delay(1.0) @22178.
pub(super) fn lasso_vo(w: &mut World, id: ProjectileId, target: FighterId) {
    let age = pr_ref(w, id).age;
    if age < lasso(w, id).vo_gate {
        return;
    }
    let Some(f) = pr_ref(w, id).owner else { return };
    crate::fighter::match_side::post_vo(w, f, name!("ability_lasso"), Some(target), 0.5);
    lasso_mut(w, id).vo_gate = age + to_int32(latent_frames(1.0));
}

/// HitSphere's begin overlap (@20838 -> @13698, authority; the other actor a BaseFighter @13728).
/// HitSphere (radius 59.719 at the root, object type HitBox) overlaps HurtBox capsules, not root capsules
/// (BaseFighter's ExperimentalHurtBoxes), for the owner, an ally and an enemy alike.
/// Native: SetCollisionEnabled dispatches the begin overlaps of what the sphere already overlaps at once (the
/// return's tick), and afterwards the sphere is tested where a move ends, never swept: the lasso's own move (the
/// world's projectile pass) and a fighter's updates (World.overlaps).
/// - the owner @13842: K2_DestroyActor @19089;
/// - a fighter of the instigator's team @14222: nothing while it is invulnerable @14341 or after the first ally
///   (AllyLassoed @14356-14371); IsPickedUp @14591; SpawnBuffActor(Buff_ArmorPack, the ally, the owner, 1 stack,
///   2.25 s) @14652; an ally with Buff_C017_Passive stops here @14758-14941 (an Iron Giant with his bolts out: the
///   armor alone); LaunchCharacter((0, 0, 400), XY and Z override) @14942; SpawnBuffActor(Buff_MoveCompTo_WithStun,
///   the ally, the owner, 0 stacks, 5.0 s) @15013 and, when that gave a buff, its move (combat::buffs
///   start_buff_move): TargetRelativeLoc = the owner's location + (0, 275, 50) facing right, - (0, 275, -50) facing
///   left @15362-15553 (275 behind her, 50 up, as she stands at the overlap), MoveDuration =
///   Vector_Distance2D(the ally, the target) x 0.00025 @16071-16203 (the distance in the level's XY plane: the
///   play axis alone, the height does not count), StartMove @16252; PlayVO @16288; the pfgDoDamage @16450 has an
///   all-zero packet (HitEachActorOnlyOnce false) and an ally target: CanBeHitBy fails, nothing comes of it;
/// - an enemy @16805: with the grapple perk's buff on the instigator ActivateGrappleDamageBox and nothing else
///   @16904-17120 (perks_lasso.rs lasso_grapple_box); nothing after the first enemy hit (LassoOnly1Enemy &&
///   EnemyLassoed @17121), none while LassoTipperBuffer @17198 or on a fighter in TipperSet @17213
///   (perks_lasso.rs); HitStunMult = FClamp(HitBoxDamage.HitStunMult + the target's CurrentDamage x 0.005, 0, 1.5)
///   @17288-17429 in a packet that copies HitBoxDamage's damage, knockback, direction, tag, reaction, hit pause,
///   audio and facing flags and sets the rest (@17476-18738: ArmorPierce false whatever the class default says,
///   HitEachActorOnlyOnce true, HitOnlyOneFighter false, no buffs); pfgDoDamage(null, the target, the packet, null,
///   no buffs) @18835; EnemyLassoed |= its HitSuccessful @18958-18996; PlayVO @19015.
///
/// The hit's X side is the actor's yaw, the throw's facing for the whole flight (the movement has no
/// bRotationFollowsVelocity: Projectile.turn), so (-0.5, 0.35) pulls a fighter in front of her towards her.
// Deviation: the engine fires one begin overlap per (HitSphere, hurt capsule) pair; here a fighter is handled once
// per stay inside the sphere (the graph's own gates hide the difference except for an enemy whose hit failed:
// native). pfgDoDamage's failures are taken as the invulnerable target (a dodge) and the actor already hit under
// this tag (HitEachActorOnlyOnce); a hit dealt from a fighter's own update is dealt at once, not after that update's
// other overlaps. The ally's Buff_ArmorPack keeps the class's ArmorStruct.
fn lasso_overlap(w: &mut World, id: ProjectileId, o: FighterId) {
    let oid = w.fighter(o).id;
    let at = lasso(w, id).inside.iter().position(|&i| i == oid);
    let (px, py, radius) = (pr_ref(w, id).x, pr_ref(w, id).y, lasso(w, id).radius);
    let pose = if w.fighter(o).life.dead > 0.0 || crate::combat::phases::no_hit_from(w, ActorRef::Projectile(id), o) {
        None
    } else {
        victim_pose(w, o)
    };
    let met = match pose {
        Some(vp) => {
            let sphere = WorldSphere { x: px, y: py, z: Some(0.0), r: radius, ..WorldSphere::default() };
            let m = crate::combat::hit::hurt::hurt_hit(w, o, &[sphere], &vp).is_some();
            give_vbuf(w, vp);
            m
        }
        None => false,
    };
    if !met {
        if let Some(i) = at {
            lasso_mut(w, id).inside.remove(i);
        }
        return;
    }
    if at.is_some() {
        return;
    }
    lasso_mut(w, id).inside.push(oid);
    let Some(f) = pr_ref(w, id).owner else { return };
    if o == f {
        destroy_projectile(w, id, false);
        return;
    }
    if w.fighter(o).team.team == w.fighter(f).team.team {
        if invulnerable(w, o) || lasso(w, id).ally_lassoed {
            return;
        }
        lasso_mut(w, id).ally_lassoed = true;
        crate::combat::holds::picked_up(w, o);
        spawn_buff(w, o, name!("Buff_ArmorPack_C"), 2.25, Some(1), Some(f), true, None);
        if crate::combat::buffs::buff_of(w, o, name!("Buff_C017_Passive_C")).is_some() {
            return;
        }
        // Unverified: the ally's movement mode and velocity under the pull are not read (native: whether the CMC's
        // falling step still integrates under a latent MoveComponentTo on its capsule, and which runs first): the
        // launch acts at his next update and gravity runs through the move (vy 314 falling to about -972 over a
        // 17-update pull), and he keeps that velocity at the move's end.
        let m = &mut w.fighter_mut(o).moves;
        m.impulse = Some(crate::fighter::moves::Impulse { vx: 0.0, vy: 400.0, xy: true, z: true, kind: 0, d: None });
        m.hit_launch = true; // a direct LaunchCharacter: the CMC takes it under the stun too
        if let Some(b) = spawn_buff(w, o, name!("Buff_MoveCompTo_WithStun_C"), 5.0, Some(0), Some(f), true, None) {
            let tx = w.fighter(f).body.x - 275.0 * facing(w, f);
            let ty = actor_y(w, f) + 50.0;
            let ox = w.fighter(o).body.x;
            let secs = ((ox - tx).abs()) * 0.00025;
            crate::combat::buffs::start_buff_move(w, o, b, tx, ty, secs);
        }
        lasso_vo(w, id, o);
        tutorial_event(w, TutorialEvent::UniqueCharacterMechanic1); // @16411, after PlayVO @16288
        return;
    }
    if lasso_grapple_on(w, id) {
        lasso_grapple_box(w, id);
        return;
    }
    if crate::actors::projectile::truthy(class_p(w, pr_ref(w, id)).get("LassoOnly1Enemy")) && lasso(w, id).enemy_lassoed
    {
        return;
    }
    if lasso_tipper_spares(w, id, o) {
        return; // !LassoTipperBuffer @17198, TipperSet @17213
    }
    let h = pr_ref(w, id).hit.clone();
    let damage = w.fighter(o).stats.damage;
    let mult = min(max(h.hit_stun_mult.unwrap_or(0.0) + (damage * 0.005), 0.0), 1.5);
    let me = ActorRef::Projectile(id);
    let mut ok = false;
    let d = dealer(w, me);
    let attack_id = pr_ref(w, id).attack_id;
    if projectile_blocks(w, &d, o) {
        ok = false; // (pfgDoDamage pops: no dodge, no hit; the lasso's graph goes on)
    } else if invulnerable(w, o) {
        let tick = w.flow.tick;
        attack_dodged(w, me, o, attack_id, h.base_knock_back.unwrap_or(0.0), tick, None, false);
    } else {
        dodge_over(w, me, o); // the call's @782, ahead of its hit record
        if !pr_ref(w, id).hit_set.contains(&oid) {
            pr_mut(w, id).hit_set.insert(oid);
            if w.overlap_mid_frame {
                w.fighter_mut(o).frame.hit_this_frame = true;
            }
            let pk = HitPacket {
                damage: h.damage,
                base_knock_back: h.base_knock_back,
                knock_back_scaling: h.knock_back_scaling,
                knock_back_direction_vector: h.knock_back_direction_vector,
                attack_tag: h.attack_tag,
                hit_reaction: h.hit_reaction,
                hit_pause_time: h.hit_pause_time,
                hit_stun_mult: Some(mult),
                on_hit_audio: h.on_hit_audio,
                on_hit_effect_override: h.on_hit_effect_override,
                directional_knockback: h.directional_knockback,
                velocity_instead_of_facing: h.velocity_instead_of_facing,
                knock_back_mirror: h.knock_back_mirror,
                ignore_facing: h.ignore_facing,
                attack_animation: h.attack_animation,
                attack_id: h.attack_id,
                do_los_check: h.do_los_check,
                force_knockback: h.force_knockback,
                queue_hit_pause_until_poses: Some(false),
                hit_only_one_fighter: Some(false),
                armor_pierce: Some(false),
                destroy_projectiles: Some(false),
                reflect_projectiles: Some(false),
                vacuum_knockback: Some(false),
                direct_knockback_towards_socket: Some(false),
                knockback_target_socket: Some(name!("None")),
                controller_rumble: None,
                play_on_hit_effects: Some(true),
                apply_hit_pause_to_damage_dealer: Some(true),
                attack_decay_type: Some(crate::combat::DataEnum::Value(0)),
                attack_decay_name: Some(name!("None")),
                kb_scaling_starting_dmg: Some(0.0),
                hit_each_actor_only_once: Some(true),
                account_for_velocity_with_vacuum_knockback: Some(true),
                ignore_kbi: Some(false),
                kbi_multiplier: Some(1.0),
                ..HitPacket::default()
            };
            let at = V2 { x: w.fighter(o).body.x, y: actor_y(w, o) };
            let pct = charge_pct(w, me);
            let d = dealer(w, me);
            ok = apply_projectile_hit(w, &d, o, &pk, &[], at, None, &[], pct); // HitSuccessful: false on Light Projectile Armor
        }
    }
    if ok {
        let d = dealer(w, me);
        actor_hit_enemy(w, &d, o);
    }
    let l = lasso_mut(w, id);
    l.enemy_lassoed = ok || l.enemy_lassoed;
    lasso_vo(w, id, o);
}

/// ReceiveBeginPlay @3906, after the parent's (the ability trigger: the spawn): ResetLassoAimServer @4586.
/// The owner's KnockedBack is bound to OwnerKnockedBack @5985-6103 (Projectile.kb_seq, read in `moved`).
/// The perk branches of BeginPlay (@6167-6907): perks_lasso.rs lasso_perk_begin.
/// Last (push_flow 6977 @4071): BPI_ProcessTutorialEvent(UniqueCharacterMechanic0) @7073.
fn lasso_begin(w: &mut World, id: ProjectileId) {
    lasso_aim_reset(w, id);
    lasso_wrist(w, id);
    lasso_perk_begin(w, id);
    tutorial_event(w, TutorialEvent::UniqueCharacterMechanic0);
}

/// OnProjectileBounce @10316 (perks_lasso.rs). Never ends the actor.
fn lasso_bounce_impact(w: &mut World, id: ProjectileId, _nx: Real, _ny: Real, _ivx: Real, _ivy: Real) -> bool {
    lasso_bounce(w, id);
    false
}

fn lasso_moved(w: &mut World, id: ProjectileId) {
    if pr_ref(w, id).dead {
        return;
    }
    lasso_wrist(w, id);
    let f = pr_ref(w, id).owner;
    // OwnerKnockedBack @20843: K2_DestroyActor. Deviation: taken at the actor's next tick, not inside the hit.
    if let Some(f) = f
        && w.fighter(f).attackers.knocked_back_count != pr_ref(w, id).kb_seq
    {
        destroy_projectile(w, id, false);
        return;
    }
    // ControlLassoMulticast @10952, with the velocity the control branch wrote (@11235-12457): a = the velocity's
    // angle above the throw's direction in degrees (FacingLeft ? atan2(vZ, vY) : -atan2(-vZ, -vY)); LassoAimD =
    // a > 0 ? 0 : clamp(|a| / 30, 0, 1) and LassoAimU = a > 0 ? clamp(a / 30, 0, 1) : 0.
    if let Some(f) = f
        && pr_ref(w, id).controlled
    {
        let fc = facing(w, f);
        let p = pr_ref(w, id);
        // (DegAtan2 is odd in its first argument: -DegAtan2(-Z, -Y) is DegAtan2(Z, -Y) bit for bit)
        let a = deg_atan2(p.vy, p.vx * fc);
        if let Some(s) = wonder_woman_mut(w, f) {
            s.lasso_aim_d = if a > 0.0 { 0.0 } else { min(max(a.abs() / 30.0, 0.0), 1.0) };
            s.lasso_aim_u = if a > 0.0 { min(max(a / 30.0, 0.0), 1.0) } else { 0.0 };
        }
    }
    // The perk's latent actions, LassoGrappleServer and ActivateLassoTipper (ahead of ReturnMovementServer @858).
    lasso_perk_tick(w, id);
    if pr_ref(w, id).dead {
        return;
    }
    // ReturnMovementMulticast in this tick: HitSphere on, and what it overlaps already begins at once.
    if pr_ref(w, id).tipper_due {
        lasso_mut(w, id).sphere_on = true;
        for o in match_fighters(w) {
            if !pr_ref(w, id).dead {
                lasso_overlap(w, id, o);
            }
        }
    }
}

fn lasso_overlap_hook(w: &mut World, id: ProjectileId, vic: FighterId) {
    if lasso(w, id).sphere_on && !pr_ref(w, id).dead {
        lasso_overlap(w, id, vic);
    }
}

/// ReceiveDestroyed @7113: ResetLassoAimServer (the EndPoint and the effects it destroys are the view's).
/// (not in the Lab reset's pass: an old pawn there, the reset pass)
fn lasso_destroyed(w: &mut World, id: ProjectileId) {
    if !reset_pass(w) {
        lasso_aim_reset(w, id);
    }
}

// ---- LassoOutCondition
// LassoOutCondition Check @0-845: FilterArray(the owner's Comp_Spawner.SpawnedActors, the class LassoProjectile) @156,
// and for its first actor ActivationConditions[0] && !ActivationConditions[1] @572-749; without one false @773.
// LassoProjectile is the legacy lasso: no montage of the build's kit spawns it and LassoProjectileFastV3 is no child of
// it (its parent is SpawnedActorParent), so the filter is always empty and the answer false, with a lasso out too.
// (Its one user is the row LassoPull1 of a branch group no live montage names.)
fn lasso_out(_w: &mut World, _f: FighterId) -> bool {
    false
}

// ---- Buff_NullifyNextHit (WW_BracerShield, and WonderWoman_Air_DashAlly's ApplyBuff on herself)
// The class defaults do most of it through the buff's data: TakenDamageMods (damage, knockback scaling, hitstun
// and hit pause set to 0) and ArmorStruct (BuffWWNulifyNext: 10 armor, 2 hits, threshold 15000), added in
// ReceiveBeginPlay @2188 and removed in ReceiveEndPlay @3474 (combat::buffs buff_armor).
// ReceiveBeginPlay @1591: ProcBuff bound to the owner's Comp_Pawn_DamageManager.HitByEnemy @1848-1871 and
// CreateBuffVisuals @1934 (PS_ShieldBuff_P0 on the capsule's VFX_Root; the view's, from the live buff).
// ProcBuff @4256 -> @3885: TakenDamageMods = an all-zero struct (every type 0, DoNothing: the buff changes no
// later hit), then a do-once @3818: Timeline_0.PlayFromStart @3530 (0.15 s); finished @15: RemoveBuffVisuals(false)
// (PS_WonderWomanShieldBreak on the owner's root, no sound), the Instigator's BPI_BuffBlocked @129 (the meter,
// when it is a Wonder Woman) and K2_DestroyActor @166.
// ReceiveEndPlay @3200: RemoveBuffVisuals(true) (@181-1382: for an emitter that is still there,
// PS_WonderWomanBracers_Cancel on the owner's root and sfx_cha_wonderwoman_shieldbuff_end at the owner: not after
// ProcBuff's own removal, which destroyed the emitter).
// Unverified: the timeline is taken to finish 9 buff ticks after the hit (0.15 s of the fixed step, the hit's frame
// not counted); where HitByEnemy falls against the buff actor's tick of that frame is not read.
// The zeroed struct is the instance's `mods_off` (combat::buffs): a second hit inside those 0.15 s takes no mod of this
// buff.

/// Buff_NullifyNextHit's extension (`{ by, proc, broke }`).
#[derive(State, Debug)]
pub struct NullifyNextHit {
    /// The Instigator (`from ?? f`).
    pub by: Option<FighterId>,
    /// The buff ticks left of the timeline after the hit, -1 before it.
    pub proc: i32,
    pub broke: bool,
}

impl Default for NullifyNextHit {
    fn default() -> Self {
        NullifyNextHit { by: None, proc: -1, broke: false }
    }
}

fn nullify_begin(
    w: &mut World,
    f: FighterId,
    b: crate::combat::buffs::BuffRef,
    from: Option<FighterId>,
    _d: Option<Real>,
) {
    if let Some(x) = buff_mut(w, b) {
        x.ext = BuffExt::NullifyNextHit(NullifyNextHit { by: Some(from.unwrap_or(f)), proc: -1, broke: false });
    }
}

fn nullify_on(
    w: &mut World,
    _f: FighterId,
    b: crate::combat::buffs::BuffRef,
    s: BuffSignal,
    _a: &BuffSignalArg,
) -> bool {
    if s != BuffSignal::HitByEnemy {
        return false;
    }
    let Some(x) = buff_mut(w, b) else { return false };
    let BuffExt::NullifyNextHit(e) = &mut x.ext else { return false };
    x.mods_off = Some(true);
    if e.proc < 0 {
        e.proc = 9;
    }
    false
}

fn nullify_tick(w: &mut World, f: FighterId, b: crate::combat::buffs::BuffRef) -> bool {
    let Some(x) = buff_mut(w, b) else { return false };
    let BuffExt::NullifyNextHit(e) = &mut x.ext else { return false };
    if e.proc < 0 {
        return false;
    }
    e.proc -= 1;
    if e.proc > 0 {
        return false;
    }
    e.broke = true;
    let by = e.by;
    let (fx, fy) = (w.fighter(f).body.x, actor_y(w, f));
    let fighter = w.fighter(f).id;
    w.fighter_mut(f).events.push(Event::ProjectileDeath {
        cls: NULLIFY,
        fighter,
        vfx: Some(name!("PS_WonderWomanShieldBreak")),
        x: fx,
        y: fy,
        ox: 0.0,
        rot: [0.0; 3],
        scale: 1.0,
        tint: None,
        to: None,
    });
    if let Some(by) = by {
        add_meter(w, by, METER_PER_BUFF_BLOCK);
    }
    true
}

fn nullify_end(w: &mut World, f: FighterId, b: crate::combat::buffs::BuffRef) {
    if matches!(buff(w, b).map(|x| &x.ext), Some(BuffExt::NullifyNextHit(e)) if e.broke) {
        return;
    }
    let (fx, fy) = (w.fighter(f).body.x, actor_y(w, f));
    let fighter = w.fighter(f).id;
    let fi = w.fighter_mut(f);
    fi.events.push(Event::ProjectileDeath {
        cls: NULLIFY,
        fighter,
        vfx: Some(name!("PS_WonderWomanBracers_Cancel")),
        x: fx,
        y: fy,
        ox: 0.0,
        rot: [0.0; 3],
        scale: 1.0,
        tint: None,
        to: None,
    });
    fi.events.push(Event::Sound {
        fighter,
        ev: name!("sfx_cha_wonderwoman_shieldbuff_end"),
        x: Some(fx),
        y: Some(fy),
        hit: None,
        pooled: false,
        key: None,
    });
}

/// This file's registry entries (the lasso's and the clash's kinds, the bracer shield's, LassoOutCondition,
/// Buff_NullifyNextHit).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let lasso_kind = ProjectileHooks {
        make_opts: Some(lasso_opts),
        construct: Some(lasso_construct),
        begin: Some(lasso_begin),
        bounce_impact: Some(lasso_bounce_impact),
        moved: Some(lasso_moved),
        overlap: Some(lasso_overlap_hook),
        destroyed: Some(lasso_destroyed),
        ..ProjectileHooks::default()
    };
    r.actors.enter(
        LASSO,
        ActorEntry::new(
            ActorClass::Projectile(ProjectileClass::LassoProjectileFastV3),
            ActorHooks::Projectile(lasso_kind),
        ),
    )?;
    let clash_kind = ProjectileHooks {
        construct: Some(clash_construct),
        begin: Some(clash_begin),
        step: Some(clash_step),
        activate: Some(clash_activate),
        ..ProjectileHooks::default()
    };
    r.actors.enter(
        CLASH,
        ActorEntry {
            parents: &["WonderWoman_BracerClash_C"],
            ..ActorEntry::new(
                ActorClass::Projectile(ProjectileClass::WonderWomanMeterBracerClash),
                ActorHooks::Projectile(clash_kind),
            )
        },
    )?;
    let shield_kind = WorldActorHooks {
        spawn: Some(shield_spawn),
        step: Some(shield_step),
        bodies: Some(shield_bodies),
        size: Some(crate::registry::entries::SizeHooks::default()), // its sphere reads scaledRadius; nothing of it is drawn
        ..WorldActorHooks::default()
    };
    r.actors.enter(
        BRACER_SHIELD,
        ActorEntry::new(ActorClass::World(WorldActorClass::WWBracerShield), ActorHooks::World(shield_kind)),
    )?;
    r.conditions.enter("LassoOutCondition_C", Condition::Fn(lasso_out))?;
    let hooks = BuffHooks {
        begin: Some(nullify_begin),
        on: Some(nullify_on),
        tick: Some(nullify_tick),
        end: Some(nullify_end),
        ..BuffHooks::default()
    };
    let e = BuffEntry::new(BuffClass::BuffNullifyNextHit, hooks);
    let n = e.class.registered_name().expect("a registered class");
    r.buffs.enter(n, e)?;
    let _ = (CollisionResponse::Block, DELTA_SECONDS);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::combat::buffs::buff_of;
    use crate::setup::{MatchPlayer, MatchSetup};
    use std::sync::Arc;

    fn world() -> Option<(World, FighterId, FighterId)> {
        let dir = mvs_store::dev_store_dir();
        let Ok(store) = mvs_store::Store::open(&dir) else {
            eprintln!("no store at {}: skipped", dir.display());
            return None;
        };
        let data = Arc::new(
            mvs_data::GameData::load(&store, &["wonderwoman", "shaggy"], "trainingroomsmall")
                .expect("the gamedata loads"),
        );
        let p = |c: &str| MatchPlayer { character: Some(c.into()), ..MatchPlayer::default() };
        let setup = MatchSetup { players: vec![p("wonderwoman"), p("shaggy")], ..MatchSetup::default() };
        let w = World::new(data, 21, &setup).expect("binds");
        let (f, vic) = (w.players[0], w.players[1]);
        Some((w, f, vic))
    }

    fn lasso_at(w: &mut World, f: FighterId, x: Real, y: Real) -> ProjectileId {
        let mut p = Projectile::new(LASSO_CLS, ProjectileClass::LassoProjectileFastV3, Some(f), w.fighter(f).id, 5);
        p.ext = ProjectileExt::Lasso(Lasso { sphere_on: true, radius: 80.0, ..Lasso::default() });
        (p.x, p.y) = (x, y);
        p.hit.damage = Some(6.0);
        p.hit.base_knock_back = Some(300.0);
        p.hit.knock_back_scaling = Some(1.0);
        let id = w.actors.projectiles.insert(p);
        w.fighter_mut(f).items.spawned_actors.push(ActorRef::Projectile(id));
        id
    }

    const LASSO_CLS: Name = name!("LassoProjectileFastV3_C");

    /// HitSphere's begin overlap: an enemy takes the packet once per stay, the owner ends the lasso.
    #[test]
    fn the_sphere_hits_an_enemy_once_and_the_owner_ends_it() {
        let Some((mut w, f, vic)) = world() else { return };
        let (vx, vy) = (w.fighter(vic).body.x, actor_y(&w, vic));
        let id = lasso_at(&mut w, f, vx, vy);
        let before = w.fighter(vic).stats.damage;
        lasso_overlap(&mut w, id, vic);
        let after = w.fighter(vic).stats.damage;
        assert!(after > before, "the packet dealt: {before} -> {after}");
        assert!(lasso(&w, id).enemy_lassoed && lasso(&w, id).inside == [w.fighter(vic).id]);
        lasso_overlap(&mut w, id, vic);
        assert_eq!(w.fighter(vic).stats.damage, after, "one hit per stay");
        let (fx, fy) = (w.fighter(f).body.x, actor_y(&w, f));
        {
            let p = w.actors.projectiles.get_mut(id).unwrap();
            (p.x, p.y) = (fx, fy);
        }
        lasso_overlap(&mut w, id, f);
        assert!(w.actors.projectiles.get(id).is_none_or(|p| p.dead), "the owner's touch destroys it");
    }

    /// Buff_NullifyNextHit: a hit starts the 0.15 s timeline, which ends in the buff's removal and the meter.
    #[test]
    fn the_nullify_buff_breaks_nine_ticks_after_the_hit() {
        let Some((mut w, f, _)) = world() else { return };
        let b = spawn_buff(&mut w, f, NULLIFY, 100.0, Some(0), Some(f), true, None).expect("the buff");
        assert!(!nullify_tick(&mut w, f, b), "no hit yet");
        nullify_on(&mut w, f, b, BuffSignal::HitByEnemy, &BuffSignalArg::default());
        let before = meter_of(&w, f);
        for i in 0..8 {
            assert!(!nullify_tick(&mut w, f, b), "tick {i}");
        }
        assert!(nullify_tick(&mut w, f, b), "the ninth tick breaks it");
        assert!(meter_of(&w, f) > before, "the meter takes the block");
    }

    /// WW_BracerShield's Delay(0.01) step: the owner (and her team) get the buffs, the actor ends.
    #[test]
    fn the_bracer_shield_gives_the_buff_and_ends() {
        let Some((mut w, f, _)) = world() else { return };
        let s = BracerShield {
            x: w.fighter(f).body.x,
            y: actor_y(&w, f) + 30.0,
            up: 30.0,
            radius: 1200.0,
            blast_bounds: new_blast_bounds(),
            ..BracerShield::default()
        };
        let x = crate::world::spawning::new_world_actor(
            &w,
            name!("WW_BracerShield_C"),
            WorldActorClass::WWBracerShield,
            f,
            false,
            WorldActorState::BracerShield(s),
        );
        let id = w.actors.actors.insert(x);
        for _ in 0..4 {
            if w.actors.actors.get(id).is_none_or(|a| a.dead) {
                break;
            }
            shield_step(&mut w, id);
        }
        assert!(buff_of(&w, f, NULLIFY).is_some(), "the owner has the armor");
        assert!(w.actors.actors.get(id).is_none_or(|a| a.dead), "the actor ended");
        assert_eq!(
            bracer_place(
                &w,
                w.actors.actors.get(id).unwrap(),
                match &w.actors.actors.get(id).unwrap().state {
                    WorldActorState::BracerShield(s) => s,
                    _ => unreachable!(),
                }
            ),
            Some((w.fighter(f).body.x, actor_y(&w, f) + 30.0))
        );
    }
}
