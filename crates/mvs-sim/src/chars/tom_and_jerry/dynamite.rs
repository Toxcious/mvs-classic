//! Dynamite_SpawnedActor: the dynamite ([`Dynamite`], a
//! SpawnedActorParent child held as a projectile: the bounces, the explosion, the reflected boost), its data child
//! CantSplitDynamite_SpawnedActor, and the DynamiteSplit perk's three splits (Buff_TomAndJerry_DynamiteSplit, a
//! marker). The item of the same name is `items::dynamite` (`ItemExt::Dynamite`).
//!
//! The graph (ubergraph offsets): ReceiveBeginPlay @292 (the fuse loop @302, BounceEvent bound @367, the owner with
//! the perk buff -> CanAimReflect = false @869, Delay(0.5) @931 -> @54: gravity scale 6, BounceGate); BounceEvent @986
//! (BounceCount + 1 >= MaxBounceCount -> Attempt to Detonate @1144, else the bounce sound @1159); Reflected @1225 (the
//! perk's three splits @2555 / @3303 / @3934, BounceCount 0, HasBeenReflected, gravity scale 2 @4199, Delay(0.5) @4244
//! -> @111 gravity scale 4); ReflectedMulticast @4756; Blocked @15; OnHitAnything @5708 (Attempt to Detonate);
//! ReceiveEndPlay @4732 -> @4688 (BoomVisuals @4377, then Attempt to Detonate unless StopDetonateOnDeath).
//
// The explosion meets the overlap's other actors too (projectiles, items and World.actors actors with a
// Comp_Actor_Destructible): each that passes IsVulnerableTo gets `hits::do_damage` with the ExplosionHit and no rows
// (pfgDoDamage @1799). The dynamite itself, which the overlap does not ignore, is the owner's team and fails the test.
// Unverified (dynamite b): the splits' directions go through `comp_spawn_projectile`, which mirrors the Y part by the
// owner's facing (the Unverified note there); the graph's Abs(Y) is read as that convention: forward of the owner.
// Unverified (dynamite c): the Delays count from the age the call finds (BeginPlay's counts the spawn frame), and a
// Delay called again while pending is ignored (UE's Delay node).

use std::sync::Arc;

use crate::Real;
use crate::State;
use crate::actors::projectile::{ProjectileExt, class_p, set_gravity_scale, update_max_lifetime};
use crate::actors::{ActorClass, ActorRef, Projectile, ProjectileClass, ProjectileId};
use crate::combat::elem::charge_pct;
use crate::combat::hits::{DamageCall, actor_hit_enemy, apply_projectile_hit, dealer, do_damage, projectile_blocks};
use crate::combat::spawned_hits::is_vulnerable_to;
use crate::combat::{BuffClass, DataVec, HitPacket};
use crate::events::{Event, SoundKey};
use crate::fighter::common::latent_frames;
use crate::fighter::control::{attack_dodged, dodge_over};
use crate::fighter::invulnerable;
use crate::fighter::moves::V2;
use crate::fighter::traces::{
    OBJ_DAMAGE_DESTRUCTIBLE, OBJ_ITEM_DESTRUCTIBLE, OBJ_PAWN, OBJ_PROJECTILE_DESTRUCTIBLE, sphere_overlap_actors,
};
use crate::math::ue::{normal, rotate_angle_axis, size};
use crate::name;
use crate::registry::entries::{ActorEntry, ActorHooks, BuffEntry, BuffHooks, ProjectileHooks, ReflectHooks};
use crate::registry::props::{flag, num_or, obj};
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use crate::world::spawning::{comp_spawn_projectile, destroy_projectile};
use mvs_data::value::Value;

/// The dynamite's class.
pub const DYNAMITE: &str = "Dynamite_SpawnedActor_C";
/// The splits' class.
pub const CANT_SPLIT_DYNAMITE: &str = "CantSplitDynamite_SpawnedActor_C";
/// The DynamiteSplit perk's marker.
pub const DYNAMITE_SPLIT_BUFF: &str = "Buff_TomAndJerry_DynamiteSplit_C";

/// `Dynamite extends Projectile` (Dynamite_SpawnedActor_C, CantSplitDynamite_SpawnedActor_C).
#[derive(State, Debug)]
pub struct Dynamite {
    /// BounceCount.
    pub bounces: i32,
    /// BounceGate.
    pub bounce_gate: bool,
    /// HasSplit.
    pub has_split: bool,
    /// HasBeenReflected.
    pub reflected: bool,
    /// StopDetonateOnDeath.
    pub stop_detonate_on_death: bool,
    /// Detonated.
    pub blasted: bool,
    /// ShowDestroyEffects.
    pub show_fx: bool,
    /// AlwaysShowVisibles.
    pub always_show: bool,
    pub hidden: bool,
    /// BlockingActor (the explosion's ignored actor).
    pub blocking: Option<ActorRef>,
    /// BeginPlay's Delay(0.5): the age it fires at (-1: none).
    pub gate_at: Real,
    /// Reflected's Delay(0.5).
    pub grav_at: Real,
    /// The do-once of UpdateMaxLifetimeWithAuthority(PostSplitLifetime).
    pub life_once: bool,
    pub end_done: bool,
}

impl Default for Dynamite {
    /// The field initial values.
    fn default() -> Self {
        Dynamite {
            bounces: 0,
            bounce_gate: false,
            has_split: false,
            reflected: false,
            stop_detonate_on_death: false,
            blasted: false,
            show_fx: false,
            always_show: false,
            hidden: false,
            blocking: None,
            gate_at: -1.0,
            grav_at: -1.0,
            life_once: false,
            end_done: false,
        }
    }
}

fn pr(w: &World, id: ProjectileId) -> &Projectile {
    w.actors.projectiles.get(id).expect("a live projectile")
}

fn pm(w: &mut World, id: ProjectileId) -> &mut Projectile {
    w.actors.projectiles.get_mut(id).expect("a live projectile")
}

fn dy(w: &World, id: ProjectileId) -> &Dynamite {
    match &pr(w, id).ext {
        ProjectileExt::Dynamite(d) => d,
        e => panic!("a dynamite without its state: {e:?}"),
    }
}

fn dx(w: &mut World, id: ProjectileId) -> &mut Dynamite {
    match &mut pm(w, id).ext {
        ProjectileExt::Dynamite(d) => d,
        e => panic!("a dynamite without its state: {e:?}"),
    }
}

fn loop_key(p: &Projectile) -> SoundKey {
    SoundKey { tag: name!("proj"), n: p.seq }
}

/// The constructor's tail: the flags of the class data, and CanAimReflect false for an owner with the perk buff
/// (BeginPlay @869: BPI_CheckIfHasBuff at BeginPlay), read by Comp_ReflectProjectile from the actor.
fn construct(w: &World, p: &mut Projectile) {
    let cp = class_p(w, p);
    let d = Dynamite {
        bounce_gate: flag(cp, "BounceGate"),
        has_split: flag(cp, "HasSplit"),
        show_fx: flag(cp, "ShowDestroyEffects"),
        always_show: flag(cp, "AlwaysShowVisibles"),
        ..Dynamite::default()
    };
    if let Some(o) = p.owner
        && crate::combat::buffs::buff_of(w, o, name!("Buff_TomAndJerry_DynamiteSplit_C")).is_some()
    {
        let mut np = cp.clone();
        np.insert("CanAimReflect".to_owned(), Value::Bool(false));
        p.own_p = Some(Arc::new(np));
    }
    p.ext = ProjectileExt::Dynamite(d);
}

/// Attempt to Detonate, its explosion (the caller destroys the actor: K2_DestroyActor @1923). False: it had detonated.
fn detonate(w: &mut World, id: ProjectileId) -> bool {
    if dy(w, id).blasted {
        return false;
    }
    dx(w, id).blasted = true;
    let me = ActorRef::Projectile(id);
    let (x, y, vx, attack_id, owner) = {
        let p = pr(w, id);
        (p.x, p.y, p.vx, p.attack_id, p.owner)
    };
    let Some(f) = owner else { return true };
    let (reflected, has_split, blocking) = {
        let d = dy(w, id);
        (d.reflected, d.has_split, d.blocking)
    };
    let r = if reflected { 300.0 } else { 200.0 };
    let types = [
        crate::real(OBJ_DAMAGE_DESTRUCTIBLE),
        crate::real(OBJ_PAWN),
        crate::real(OBJ_ITEM_DESTRUCTIBLE),
        crate::real(OBJ_PROJECTILE_DESTRUCTIBLE),
    ];
    let found = sphere_overlap_actors(w, x, y, r, &types, &[blocking], -1.0);
    let boosted = !has_split && reflected;
    // The velocity's normal's Y (UE) is the sim's -vx; GreaterEqual(Y, 0).
    let (kx, ky) = if !reflected {
        (0.0, 1.0)
    } else if -vx >= 0.0 {
        (0.7, 0.3)
    } else {
        (-0.7, 0.3)
    };
    let empty = mvs_data::value::Props::new();
    let mut hit = HitPacket::from_props(&w.names, obj(class_p(w, pr(w, id)), "ExplosionHit").unwrap_or(&empty), None);
    hit.damage = Some(if boosted { 7.0 } else { 4.0 });
    hit.base_knock_back = Some(if boosted { 1350.0 } else { 650.0 });
    hit.knock_back_scaling = Some(if boosted { 15.0 } else { 5.0 });
    hit.knock_back_direction_vector = Some(DataVec { x: Some(kx), y: Some(ky), z: None });
    let dir = V2 { x: -kx, y: ky }; // IgnoreFacing: the vector's X is UE's Y, sim -x
    let team = w.fighter(f).team.team;
    for o in found {
        let ActorRef::Fighter(vic) = o else {
            // pfgDoDamage(null, actor, ExplosionHit, null, {[]}, {[]}) @1799 on every other actor the IsVulnerableTo
            // test (@906 / 982) passes.
            if is_vulnerable_to(w, o, team) {
                let ctx = DamageCall { hit_box: false, comp: None, clank: false, rows: Some(&[]) };
                do_damage(w, me, o, &hit, &ctx);
            }
            continue;
        };
        // IsVulnerableTo(the owner's TeamIndex): another team.
        if w.fighter(vic).team.team == team
            || w.fighter(vic).life.dead > 0.0
            || crate::combat::phases::no_hit_by_team(w, team, vic)
        {
            continue;
        }
        if projectile_blocks(w, &dealer(w, me), vic) {
            continue;
        }
        if invulnerable(w, vic) {
            let tick = w.flow.tick;
            attack_dodged(w, me, vic, attack_id, hit.base_knock_back.unwrap_or(0.0), tick, None, false);
            continue;
        }
        dodge_over(w, me, vic);
        let d = dealer(w, me);
        if apply_projectile_hit(w, &d, vic, &hit, &[], V2 { x, y }, Some(dir), &[], charge_pct(w, me)) {
            actor_hit_enemy(w, &d, vic);
        }
    }
    true
}

/// ReceiveEndPlay @4732 -> @4688.
fn end_play(w: &mut World, id: ProjectileId) {
    if dy(w, id).end_done {
        return;
    }
    dx(w, id).end_done = true;
    let (cls, fighter, x, y, key, reflected) = {
        let p = pr(w, id);
        (p.cls, p.owner_id, p.x, p.y, loop_key(p), dy(w, id).reflected)
    };
    if dy(w, id).show_fx || dy(w, id).always_show {
        // BoomVisuals @4377.
        w.events.push(Event::ProjectileDeath {
            cls,
            fighter,
            vfx: Some(name!("PS_TomJerryDynamiteImpact")),
            x,
            y,
            ox: 0.0,
            rot: [0.0, 0.0, 0.0],
            scale: if reflected { 0.5 } else { 1.0 },
            tint: None,
            to: None,
        });
        w.events.push(Event::Sound {
            fighter,
            ev: name!("sfx_shared_item_dynamite_explode"),
            x: Some(x),
            y: Some(y),
            hit: None,
            pooled: false,
            key: None,
        });
    }
    if !dy(w, id).stop_detonate_on_death {
        detonate(w, id);
    }
    w.events.push(Event::SoundStop { fighter, key, fade: Some(0.0), ev: None, curve: None, cut: false });
}

/// One CompSpawnActor(CantSplitDynamite_SpawnedActor_C) of Reflected: (0, |Y|, Z) of Normal(Velocity, 1e-4), turned
/// by RotateAngleAxis(it, `deg`, (`axis_x`, 0, 0)) when `turn` is `Some((deg, axis_x))`; the speed VSize(Velocity).
fn spawn_split(w: &mut World, id: ProjectileId, turn: Option<(Real, Real)>, up: Real) {
    let (vx, vy, x, y, max, owner) = {
        let p = pr(w, id);
        (p.vx, p.vy, p.x, p.y, p.max_speed, p.owner)
    };
    let Some(f) = owner else { return };
    let v = [0.0, -vx, vy];
    let len = size(v);
    let [nx, ny, nz] = normal(v, 1e-4);
    let mut d = [nx, ny.abs(), nz];
    if let Some((deg, axis_x)) = turn {
        d = rotate_angle_axis(d, deg, [axis_x, 0.0, 0.0]);
    }
    let dir = [Some(d[0]), Some(d[1]), Some(d[2])];
    comp_spawn_projectile(w, f, name!("CantSplitDynamite_SpawnedActor_C"), x, y + up, dir, len, max, None);
}

fn begin(w: &mut World, id: ProjectileId) {
    // (The parent's NumberOfActorsAllowed, IsA the class, came ahead: the World's parent BeginPlay.)
    let (fighter, key) = (pr(w, id).owner_id, loop_key(pr(w, id)));
    w.events.push(Event::SoundStart { fighter, ev: name!("sfx_shared_class_assassin_satchel_fuse_loop"), key });
    let age = crate::real(pr(w, id).age);
    if dy(w, id).gate_at < 0.0 {
        dx(w, id).gate_at = age + latent_frames(0.5); // @931
    }
}

fn step(w: &mut World, id: ProjectileId) -> bool {
    let age = crate::real(pr(w, id).age);
    let d = dy(w, id);
    if d.gate_at >= 0.0 && age >= d.gate_at {
        dx(w, id).gate_at = -1.0;
        set_gravity_scale(pm(w, id), 6.0); // @54
        dx(w, id).bounce_gate = true;
    }
    let d = dy(w, id);
    if d.grav_at >= 0.0 && age >= d.grav_at {
        dx(w, id).grav_at = -1.0;
        set_gravity_scale(pm(w, id), 4.0); // @111
    }
    false
}

/// BounceEvent @986.
fn bounce_impact(w: &mut World, id: ProjectileId, nx: Real, ny: Real, ivx: Real, ivy: Real) -> bool {
    let _ = (nx, ny, ivx, ivy);
    if !dy(w, id).bounce_gate {
        return false;
    }
    dx(w, id).bounces += 1;
    let max = num_or(class_p(w, pr(w, id)), "MaxBounceCount", 0.0);
    if crate::real(dy(w, id).bounces) >= max {
        return detonate(w, id);
    }
    let (fighter, x, y, key) = {
        let p = pr(w, id);
        (p.owner_id, p.x, p.y, loop_key(p))
    };
    w.events.push(Event::Sound {
        fighter,
        ev: name!("sfx_cha_tomandjerry_dynamite_bounce"),
        x: Some(x),
        y: Some(y),
        hit: None,
        pooled: false,
        key: Some(key),
    });
    false
}

fn blocked(w: &mut World, id: ProjectileId, by: ActorRef) {
    dx(w, id).blocking = Some(by); // @15
    if flag(class_p(w, pr(w, id)), "DestroyOnBlocked") {
        pm(w, id).blocked_destroy = true; // the parent's Blocked
    }
}

fn destroyed(w: &mut World, id: ProjectileId) {
    end_play(w, id);
}

fn end(w: &mut World, id: ProjectileId) {
    end_play(w, id);
}

/// Reflected @1225 and ReflectedMulticast @4756.
fn reflected(w: &mut World, a: ActorRef) {
    let ActorRef::Projectile(id) = a else { return };
    let perk = |w: &World| {
        pr(w, id)
            .owner
            .is_some_and(|o| crate::combat::buffs::buff_of(w, o, name!("Buff_TomAndJerry_DynamiteSplit_C")).is_some())
    };
    if !dy(w, id).has_split && perk(w) {
        dx(w, id).has_split = true;
        let ang = num_or(class_p(w, pr(w, id)), "DynamiteSplitAngle", 0.0);
        spawn_split(w, id, Some((ang, 1.0)), 10.0); // @2555
        spawn_split(w, id, Some((ang, -1.0)), 10.0); // @3303
        spawn_split(w, id, None, 0.0); // @3934
        dx(w, id).stop_detonate_on_death = true; // @4068
        if !pr(w, id).dead {
            destroy_projectile(w, id, false); // @4079
        }
        let p = pm(w, id);
        p.vx = 0.0; // @4093
        p.vy = 0.0;
    }
    let d = dx(w, id);
    d.bounces = 0; // @4146
    d.reflected = true;
    // ReflectedMulticast @4756.
    if perk(w) {
        dx(w, id).show_fx = false;
        if !dy(w, id).always_show && !pr(w, id).dead {
            dx(w, id).hidden = true;
            pm(w, id).no_collision = true; // SetActorEnableCollision(false)
        }
    }
    // @157
    if dy(w, id).has_split && !dy(w, id).life_once {
        dx(w, id).life_once = true;
        if !pr(w, id).dead {
            let life = num_or(class_p(w, pr(w, id)), "PostSplitLifetime", 0.0);
            let ahead = i32::from(w.overlap_mid_frame || pr(w, id).age == 0);
            update_max_lifetime(pm(w, id), life, ahead);
        }
    }
    // @4199
    set_gravity_scale(pm(w, id), 2.0);
    let age = crate::real(pr(w, id).age);
    if dy(w, id).grav_at < 0.0 {
        dx(w, id).grav_at = age + latent_frames(0.5);
    }
}

/// OnHitAnything: its override, with no parent call (the detonation).
fn hit_anything(w: &mut World, id: ProjectileId, victim: ActorRef) {
    let _ = victim;
    if detonate(w, id) && !pr(w, id).dead {
        destroy_projectile(w, id, false);
    }
}

/// `kind`.
fn kind() -> ProjectileHooks {
    ProjectileHooks {
        construct: Some(construct),
        begin: Some(begin),
        step: Some(step),
        bounce_impact: Some(bounce_impact),
        blocked: Some(blocked),
        destroyed: Some(destroyed),
        end: Some(end),
        reflect: ReflectHooks { reflected: Some(reflected), ..ReflectHooks::default() },
        hit_anything: Some(hit_anything),
        ..ProjectileHooks::default()
    }
}

/// This file's registry entries (the two dynamite classes, Buff_TomAndJerry_DynamiteSplit_C).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.actors.enter(
        DYNAMITE,
        ActorEntry::new(ActorClass::Projectile(ProjectileClass::DynamiteSpawnedActor), ActorHooks::Projectile(kind())),
    )?;
    let child = ActorEntry {
        parents: &[DYNAMITE],
        ..ActorEntry::new(
            ActorClass::Projectile(ProjectileClass::CantSplitDynamiteSpawnedActor),
            ActorHooks::Projectile(kind()),
        )
    };
    r.actors.enter(CANT_SPLIT_DYNAMITE, child)?;
    // The perk's marker: read by the dynamite's BPI_CheckIfHasBuff; its own graph does nothing.
    r.buffs
        .enter(DYNAMITE_SPLIT_BUFF, BuffEntry::new(BuffClass::BuffTomAndJerryDynamiteSplit, BuffHooks::default()))?;
    Ok(())
}
