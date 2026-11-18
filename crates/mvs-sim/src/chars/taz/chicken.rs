//! Taz's chicken: Buff_Chicken, the Buff_PolymorphParent child Buff_Tasty spawns at its cap
//! (`tasty`), and C015_HealPickup, the heal it drops when hit (offsets of each ubergraph unless named).

use crate::Real;
use crate::State;
use crate::actors::projectile::{ProjectileExt, class_def, class_p};
use crate::actors::{ActorClass, ActorRef, Projectile, ProjectileClass, ProjectileId};
use crate::combat::buffs::{BuffExt, BuffRef, apply_healing, buff, buff_mut, inst_view, spawn_buff};
use crate::combat::polymorph::{PolymorphChild, polymorph_child};
use crate::combat::{BuffClass, HitPacket};
use crate::consts::{DELTA_SECONDS, SOLID_NAV};
use crate::events::{Event, TazHealKind};
use crate::fighter::common::{latent_frames, warn_text};
use crate::fighter::control::emit;
use crate::fighter::traces::solids::sphere_static;
use crate::fighter::{FighterId, actor_y, cap_radius, capsule_half};
use crate::math::ue::{floor_to_int, fmod};
use crate::math::{max, min};
use crate::name;
use crate::names::Name;
use crate::registry::body_settings::sphere_settings;
use crate::registry::entries::{ActorEntry, ActorHooks, ProjectileHooks};
use crate::registry::props::{num, obj};
use crate::registry::signals::{BuffSignal, BuffSignalArg};
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use crate::world::pawns::match_fighters;
use crate::world::spawning::comp_spawn_projectile;
use crate::world::stage_query::{rigid_statics, with_static_shapes};
use mvs_collision::Ground;
use mvs_collision::rigid_sphere::{Body, DEFAULT_K, Solver, Static};

const BLOCK: Name = name!("Buff_TastyBlock_C");
const PICKUP: Name = name!("C015_HealPickup_C");

// ---- Buff_Chicken
// A Buff_PolymorphParent child (`combat::polymorph` does the parent: the mesh, the hit box, the polymorph's health)
// with MaxStacks / BlockDuration 8 / DamageForHealthItem 1.5 in its defaults, DestroyBuffWhenOwnerDefeated,
// BreakBuffs Buff_Tasty. A prop of Taz's kit (Chicken_SkelMesh, no character's skin): rule 2 of `polymorph_owner`
// finds Taz's data for it.
// - ReceiveBeginPlay @10: the parent's BeginPlay, then (authority, the owner a BP_Fighter_Character) its
//   OnBFDefeated -> OnOwnerDefeated @153-176; Spawner = the Instigator's Comp_Spawner @288.
// - OnOwnerDefeated @623: ShouldSpawnBlockBuff = false, K2_DestroyActor. (The parent's DestroyBuffWhenOwnerDefeated
//   ends the buff in the defeat ahead of the broadcast, so this does not run in the port's order; kept as the graph
//   has it.)
// - ReceiveEndPlay @312: ShouldSpawnBlockBuff and the owner's Comp_State_Manager valid -> SpawnBuffActor(
//   Buff_TastyBlock_C, owner, owner, 0, BlockDuration, true) @530.
// - HandleDamageRecieved (override, @0-777): dmg = the parent's HandleDamageRecieved(hit) @5; total = dmg +
//   LeftOverDamage; LeftOverDamage = Percent_FloatFloat(total, DamageForHealthItem) (fmod); for i in
//   1..FFloor(total / DamageForHealthItem): RandomFloatInRange(0, 0) @342 and RandomFloat @380 (two draws of the
//   stream, the first one unused), Spawner.CompSpawnActor(C015_HealPickup_C, Instigator, the owner's location,
//   direction (0, RandomFloat - 0.5, 1), speed 0) @650; returns total @777.
// Deviation: the two draws are the world's stream (`World::random_float`), the one every random node of the port
// takes.

/// Buff_Chicken_C's extension.
#[derive(State, Debug, Default)]
pub struct ChickenExt {
    /// LeftOverDamage.
    pub left: Real,
    /// ShouldSpawnBlockBuff.
    pub block: bool,
    /// The Instigator (none: none).
    pub inst: Option<FighterId>,
}

fn chicken(w: &World, b: BuffRef) -> Option<&ChickenExt> {
    match &buff(w, b)?.ext {
        BuffExt::Chicken(x) => Some(x),
        _ => None,
    }
}

fn chicken_mut(w: &mut World, b: BuffRef) -> Option<&mut ChickenExt> {
    match &mut buff_mut(w, b)?.ext {
        BuffExt::Chicken(x) => Some(x),
        _ => None,
    }
}

fn chicken_begin(w: &mut World, _f: FighterId, b: BuffRef, from: Option<FighterId>, _duration: Option<Real>) {
    if let Some(x) = buff_mut(w, b) {
        x.ext = BuffExt::Chicken(ChickenExt { left: 0.0, block: true, inst: from });
    }
}

fn chicken_damage(
    w: &mut World,
    f: FighterId,
    b: BuffRef,
    _hit: Option<&HitPacket>,
    parent: &mut dyn FnMut(&mut World) -> Real,
) -> Real {
    // (read ahead of the parent's call, which may end the buff: the build's function goes on with its variables)
    let per = inst_view(w, b).num("DamageForHealthItem").unwrap_or(1.5);
    let (left, inst) = chicken(w, b).map_or((0.0, None), |x| (x.left, x.inst));
    let dmg = parent(w);
    let total = dmg + left;
    if let Some(x) = chicken_mut(w, b) {
        x.left = fmod(total, per); // Percent_FloatFloat: FMath::Fmod
    }
    // FFloor(Divide_FloatFloat(total, per)) (0 for a zero divisor)
    let n = crate::math::real(floor_to_int(if per == 0.0 { 0.0 } else { total / per }));
    let mut i = 1.0;
    while i <= n {
        i += 1.0;
        w.random_float();
        let r = w.random_float();
        // Spawner is the Instigator's Comp_Spawner (@288): none without an Instigator (Accessed None, no spawn).
        if let Some(inst) = inst {
            let (x, y) = (w.fighter(f).body.x, actor_y(w, f));
            comp_spawn_projectile(w, inst, PICKUP, x, y, [Some(0.0), Some(r - 0.5), Some(1.0)], 0.0, 0.0, None);
        }
    }
    total
}

fn chicken_on(w: &mut World, _f: FighterId, b: BuffRef, s: BuffSignal, _a: &BuffSignalArg) -> bool {
    if s != BuffSignal::BfDefeated {
        return false;
    }
    if let Some(x) = chicken_mut(w, b) {
        x.block = false;
    }
    true
}

fn chicken_end(w: &mut World, f: FighterId, b: BuffRef) {
    if !chicken(w, b).is_some_and(|x| x.block) || w.fighter(f).life.dead != 0.0 {
        return;
    }
    let dur = inst_view(w, b).num("BlockDuration").unwrap_or(8.0);
    spawn_buff(w, f, BLOCK, dur, Some(0), Some(f), true, None);
}

// ---- C015_HealPickup
// A SpawnedActorParent child (MaxLifetime 10, Die With Instigator, DestroyOnExitedArena?,
// ForceSpawnPositionInsideBlastBox; EnableProjectileMovement true: no spawn impulse, HandleActorPostSpawn) whose root
// Capsule (half height 80, radius 60, X translation locked, AngularDamping 200, bOverrideMass) simulates with gravity:
// the ProjectileMovement does not move it (`stopped`; its velocity and LockOn's homing @866 write a component that
// does not drive a simulating root). The Capsule ignores Pawn and PhysicsBody and blocks the stage and the platforms
// (Platform left at its Block default).
// - ReceiveBeginPlay (authority) -> Delay(CollisionDelay 0.4) @792 -> OverlapFighter (sphere r100) and LockOnFighter
//   (r500) collision on @15 / @53.
// - OverlapFighter's begin overlap @246 (authority): CheckIsAlly (a BP_Fighter_Character whose
//   Comp_Actor_Destructible TeamIndex is the pickup's: Taz's team, Taz too) and a valid Comp_Actor_Damage -> a DoOnce
//   -> ApplyDamage(-FMin(HealAmount 1.5, CurrentDamage)) @615 (apply_healing: none at 0 damage),
//   HealVisualMulticast @694 (PS_Heal on the target's root @149: the TazHealFx event) and ReleaseToPool @92 (the
//   actor ends).
// - ReceiveAnyDamage @747 (its HurtBox, r150, ProjectileDestructible): ReleaseToPool.
// Deviation: the capsule is stepped as a sphere of radius 80 (its half height: the floor contact; the rigid sphere
//   has no capsule body); its mass and inertia are the sphere step's defaults (bOverrideMass's value not read).
// Deviation: the overlap begin is taken as a test of the r100 sphere at the pickup against each ally's root capsule
//   each frame from the Delay's end on (a fighter inside when the collision turns on begins it then); a defeated ally
//   waiting for its respawn is left out.

/// `HealPickup extends Projectile` (C015_HealPickup_C).
#[derive(State, Debug, Default)]
pub struct HealPickup {
    /// The body's linear velocity.
    pub bvx: Real,
    pub bvy: Real,
    pub bspin: Real,
    /// `collideAt`: the age the Delay(CollisionDelay) fires at.
    pub collide_at: Real,
}

/// The body's radius (the capsule's half height).
const RADIUS: Real = 80.0;

fn pickup(p: &Projectile) -> &HealPickup {
    match &p.ext {
        ProjectileExt::HealPickup(x) => x,
        e => panic!("C015_HealPickup_C without its state: {e:?}"),
    }
}

/// The constructor's tail (`new HealPickup`): the movement stopped, the Delay's end.
fn construct(w: &World, p: &mut Projectile) {
    p.stopped = true;
    let delay = class_def(w, p).and_then(|d| num(&d.p, "CollisionDelay")).unwrap_or(0.4);
    p.ext = ProjectileExt::HealPickup(HealPickup { collide_at: latent_frames(delay), ..HealPickup::default() });
}

/// A sphere of radius `r` at (x, y) against a fighter's root capsule (`overlapsRoot`; a touch is no overlap).
fn overlaps_root(w: &World, o: FighterId, x: Real, y: Real, r: Real) -> bool {
    let seg = max(capsule_half(w, o) - cap_radius(w, o), 0.0);
    let ay = actor_y(w, o);
    let cy = max(ay - seg, min(y, ay + seg));
    let (dx, dy, rr) = (x - w.fighter(o).body.x, y - cy, r + cap_radius(w, o));
    dx * dx + dy * dy < rr * rr
}

fn step(w: &mut World, id: ProjectileId) -> bool {
    let p = w.actors.projectiles.get(id).expect("a live projectile");
    if crate::real(p.age) < pickup(p).collide_at {
        return false;
    }
    let heal = num(class_p(w, p), "HealAmount").unwrap_or(1.5);
    let (x, y, team) = (p.x, p.y, p.owner.map(|o| w.fighter(o).team.team));
    for o in match_fighters(w) {
        let fo = w.fighter(o);
        if fo.life.dead != 0.0 || Some(fo.team.team) != team || !overlaps_root(w, o, x, y, 100.0) {
            continue;
        }
        let damage = fo.stats.damage;
        if damage > 0.0 {
            apply_healing(w, o, min(heal, damage));
        }
        emit(w, o, |fighter| Event::TazHealFx { fighter, kind: TazHealKind::Pickup }); // @694
        return true;
    }
    false
}

fn physics(w: &mut World, id: ProjectileId) {
    let p = w.actors.projectiles.get(id).expect("a live projectile");
    let bi = class_def(w, p).and_then(|d| obj(&d.components, "Capsule")).and_then(|c| obj(c, "BodyInstance"));
    let Some(settings) = sphere_settings(w, bi, "C015_HealPickup_C") else {
        warn_text(w, "C015_HealPickup_C: no body settings: not moved");
        return;
    };
    let platforms: Vec<Ground> = w
        .stage
        .stage_platforms
        .iter()
        .map(|p| Ground {
            left: f64::from(p.left),
            right: f64::from(p.right),
            bottom: f64::from(p.bottom),
            top: f64::from(p.top),
        })
        .collect();
    // (and the stage solids that block PhysicsBody, its object type)
    let shapes = with_static_shapes(w, SOLID_NAV, name!("C015_HealPickup_C body"), &platforms);
    let list: Vec<Static> = rigid_statics(w, &shapes);
    let floor = sphere_static(w, w.stage.ground);
    let p = w.actors.projectiles.get_mut(id).expect("a live projectile");
    let ProjectileExt::HealPickup(h) = &mut p.ext else { return };
    let mut body = Body {
        x: f64::from(p.x),
        y: f64::from(p.y),
        vx: f64::from(h.bvx),
        vy: f64::from(h.bvy),
        spin: f64::from(h.bspin),
        radius: f64::from(RADIUS),
        ..Body::default()
    };
    Solver::default().sphere_step(&mut body, &floor, Some(&list), DELTA_SECONDS, &settings, DEFAULT_K);
    (p.x, p.y, h.bvx, h.bvy, h.bspin) = (
        crate::math::narrow(body.x),
        crate::math::narrow(body.y),
        crate::math::narrow(body.vx),
        crate::math::narrow(body.vy),
        crate::math::narrow(body.spin),
    );
}

fn any_damage(_w: &mut World, _id: ProjectileId, _damage: Real, _by: ActorRef) -> bool {
    true
}

/// This file's registry entries: Buff_Chicken_C's polymorph child and C015_HealPickup_C.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    polymorph_child(
        r,
        "Buff_Chicken_C",
        BuffClass::BuffChicken,
        PolymorphChild {
            prop: true,
            begin: Some(chicken_begin),
            damage: Some(chicken_damage),
            on: Some(chicken_on),
            end: Some(chicken_end),
            ..PolymorphChild::new(name!("Buff_Chicken_C:"))
        },
    )?;
    r.actors.enter(
        "C015_HealPickup_C",
        ActorEntry::new(
            ActorClass::Projectile(ProjectileClass::C015HealPickup),
            ActorHooks::Projectile(ProjectileHooks {
                construct: Some(construct),
                step: Some(step),
                physics: Some(physics),
                any_damage: Some(any_damage),
                ..ProjectileHooks::default()
            }),
        ),
    )
}
