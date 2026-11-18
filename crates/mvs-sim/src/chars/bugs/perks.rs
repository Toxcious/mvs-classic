//! The signature perks of Bugs Bunny: Lingering Love (LingeringLoveProjectile_C, a
//! [`StillActor`]), Shockwave (TunnelShockwave_C, [`BugsShockwave`]), Escapist and Safety; the teleport's Timeline
//! Finished tunnel.rs calls.
//!
//! - Buff_BugsBunny_LingeringLove: a marker the kit reads (BugsHasLingeringKiss / BugsNoLingeringKiss, pie.rs), which
//!   opens BugsBunny_Attack_BlowLingeringKiss_Montage; its Spawner (t 0.711) makes LingeringLoveProjectile_C.
//! - Buff_BugsBunny_Shockwave and Buff_Bugs_Escapist: read by Buff_TunnelTeleport's Timeline Finished
//!   ([`tunnel_finished`]).
//! - Buff_Bugs_Safety: BugsBunnySafe caches IsSafetyPerk (the Instigator holds the buff) and no graph of the build
//!   reads the cached value: no entry.
//! None of the four buff classes has a graph of its own beyond the parent's: no buff entry.
//!
//! A buff hook here is passed the World.

#![allow(clippy::doc_lazy_continuation, reason = "comment blocks kept as written")]

use super::pawn::set_lingering_kiss;
use super::state::bugs_of;
use crate::Real;
use crate::State;
use crate::actors::projectile::{ProjectileExt, class_p, stop_simulating};
use crate::actors::{ActorClass, ActorRef, Projectile, ProjectileClass, ProjectileId};
use crate::combat::boxes::WorldSphere;
use crate::combat::buffs::buff_of;
use crate::events::Event;
use crate::fighter::FighterId;
use crate::fighter::common::{actor_components, buff_def, latent_frames};
use crate::fighter::moves::{Impulse, V2};
use crate::fighter::traces::{TraceBody, body_info};
use crate::math::{max, min};
use crate::name;
use crate::registry::entries::{ActorEntry, ActorHooks, ProjectileHooks};
use crate::registry::props::{num, obj};
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use crate::world::spawning::comp_spawn_projectile;

/// `KISS`.
pub const KISS: &str = "LingeringLoveProjectile_C";
/// `SHOCKWAVE`.
pub const SHOCKWAVE: &str = "TunnelShockwave_C";

/// `StillActor extends Projectile` (LingeringLoveProjectile_C): a projectile that never moves. SpawnedActorParent
/// BeginPlay @3777-3792: EnableProjectileMovement false (the CDO default; neither class sets it) ->
/// ProjectileMovement.Deactivate, whose StopMovementImmediately zeroes Velocity: the actor stays where it was spawned
/// (no gravity, no move); its hit box, timers and own tick run on. No field of its own.
#[derive(State, Debug, Default)]
pub struct StillActor {}

/// `Shockwave extends StillActor` (TunnelShockwave_C): `left`, the Delay(Duration_0)'s frames left.
#[derive(State, Debug, Default)]
pub struct BugsShockwave {
    pub left: Real,
}

/// The shockwave's Capsule: half height, radius (absolute scale, tagged HitBox) and the core segment's half length.
const HALF: Real = 350.0;
const RAD: Real = 100.0;
const CORE: Real = HALF - RAD;

fn pr(w: &World, id: ProjectileId) -> &Projectile {
    w.actors.projectiles.get(id).expect("a live projectile")
}

fn pm(w: &mut World, id: ProjectileId) -> &mut Projectile {
    w.actors.projectiles.get_mut(id).expect("a live projectile")
}

// ---- LingeringLoveProjectile
// The parent's lifetime (MaxLifetime 2), clank (CanProjectileClank), death sound (Ak Death Location
// sfx_cha_bugs_blowkiss_kiss) and OnHitBuffs (Buff_Charm_C 1.6 s on an enemy, Buff_ScoobySnack_C x3 8 s on an ally) are
// the world's. Its hit box (HitBoxOn? true, HitOnlyOneFighter) is the component tagged HitBox: `Hitbox`, a sphere of
// radius 100 (`Hitbox2D`, tagged Hitbox2D, a capsule of radius 100, in 2D): the shared constructor's read.
// Its MeleeDestructible sphere (ItemDestructible, 120) and ProjectileDestructible sphere (80), both at the root and
// overlapping HitBox, are its bodies for the queries (`kiss_bodies`). Not met in the port: a hit box (a Hitframe, a
// projectile's) does not test them; without DestroyProjectiles / ReflectProjectiles such a hit is only the actor's
// HitByEnemy (pfgDoDamage @19122), which SpawnedActorParent ignores.
// - ReceiveBeginPlay @186: the parent's, then @10 on authority the Instigator cast to BPI_BugsBunny @64 and, on
//   success, its BPI_SetBugsLingeringKiss(self) @143 (pawn.rs `set_lingering_kiss`: the kiss before it destroyed,
//   this one kept).
// - ReceiveAnyDamage @201 (authority): K2_DestroyActor, whatever the damage or the causer (no parent call, so
//   ParentProjectilePhysics' Heavy / Light rule, which it is no child of, does not apply).
// - The parent's OnHitAnything (bound at BeginPlay @8344) -> @20701 -> @18379 DestroyedOnHit (true here) ->
//   Delay(0.033) -> resume@15 DestroySpawnedActor(false) (the death effects): the world's (`hit_anything`).
// - SetTeamColor @220: the parent's, then PS_TetherProjectile's Team (the view's).
// foreignOwner: an owner that is no Bugs Bunny (a Disguise's copy of the montage, Taz burping the kiss) fails the cast
// @129 and nothing is called; nothing else of its own reads the owner.

/// The StillActor constructor's tail: the zero Velocity (BPI_SetInitialMovement's write on the deactivated component
/// is not kept).
fn still_construct(_w: &World, p: &mut Projectile) {
    stop_simulating(p);
    p.ext = ProjectileExt::StillActor(StillActor::default());
}

/// The kiss's two destructible spheres, at the actor.
fn kiss_bodies(w: &World, a: ActorRef, out: &mut Vec<TraceBody>) {
    let ActorRef::Projectile(id) = a else { return };
    let p = pr(w, id);
    if p.no_collision {
        return;
    }
    let Some(owner) = p.owner else { return };
    let cs = actor_components(w, ActorRef::Fighter(owner), p.cls);
    for (key, component) in
        [("MeleeDestructible", name!("MeleeDestructible")), ("ProjectileDestructible", name!("ProjectileDestructible"))]
    {
        let c = cs.and_then(|cs| obj(cs, key));
        let Some(info) = body_info(c, None).filter(|i| i.on) else { continue };
        let r = c.and_then(|c| num(c, "SphereRadius")).unwrap_or(0.0);
        if r.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) {
            continue;
        }
        out.push(TraceBody {
            ax: p.x,
            ay: p.y,
            bx: p.x,
            by: p.y,
            hx: 0.0,
            hy: 0.0,
            r,
            z: Real::NAN,
            object_type: info.kind,
            profile: info.profile,
            component,
            actor: Some(a),
            platform: -1,
            blocks: info.blocks,
            poly: None,
            floor: None,
        });
    }
}

/// ReceiveBeginPlay: the owner's BPI_SetBugsLingeringKiss when it is a Bugs Bunny.
fn kiss_begin(w: &mut World, id: ProjectileId) {
    if let Some(o) = pr(w, id).owner
        && bugs_of(w, o).is_some()
    {
        set_lingering_kiss(w, o, id);
    }
}

/// ReceiveAnyDamage @201: destroyed, whatever the damage or the causer.
fn kiss_any_damage(_w: &mut World, _id: ProjectileId, _damage: Real, _by: ActorRef) -> bool {
    true
}

// ---- TunnelShockwave
// Spawned by `tunnel_finished` below (the traveller's Comp_Spawner, owner the traveller). The parent's hit box (its
// Capsule, half height 350, radius 100, absolute scale, tagged HitBox; HitBoxOn? true, HitEachActorOnlyOnce) deals 6 /
// 1600 / 0 / (0, 1) SpikeUp4 to each fighter of another team once. The spawn's rotation rot(0, 0, 90) (@1645, roll 90)
// lays the capsule's axis along UE Y: a bar 700 wide on the play plane (sim x), 200 tall, at Destination - 100.
// Deviation: against a fighter's hurt capsules the bar is met as nine spheres of radius 100 along its core segment
// (x +-250), as BugsBunnyRocketUp's capsule dealer (rockets.rs): the hurt test has no capsule dealer shape.
// - ReceiveBeginPlay @96: the parent's, then (not a dedicated server) SpawnEmitterAtLocation(PS_SupermanSPU_Impact,
//   the actor's location, rot 0, scale 1) @198 and (the render's, from the event's fighter) its Team from the
//   Instigator's Comp_Pawn_SkinManager.GetColorIndex (@287-590; a fighter always has the component, so @367 never
//   pops out ahead of the Delay); then on authority Delay(Duration_0 0.25) @678 -> resume@15 K2_DestroyActor. The
//   Delay counts the spawn frame (BeginPlay inside the spawn, the actor's tick in its spawn frame).
// - HitEnemyPlayer (its HitBoxHandler's) @30: PF_Post_Event(sfx_cha_bugs_generic_hit) on itself.
// foreignOwner: the owner is read for the team only (any fighter taking Bugs's tunnel, BP_FighterCharacter).

/// The Shockwave constructor's tail: StillActor's, then the Capsule's radius at its absolute scale (no spawn or
/// HitBoxScale scale).
fn shockwave_construct(_w: &World, p: &mut Projectile) {
    stop_simulating(p);
    p.hit_radius = RAD;
    p.ext = ProjectileExt::BugsShockwave(BugsShockwave::default());
}

/// `Shockwave.hitShapes`: the nine spheres along the core segment (x +-250).
pub fn shockwave_shapes(p: &Projectile, out: &mut Vec<WorldSphere>) {
    for i in 0..9 {
        out.push(WorldSphere { x: p.x + CORE * (crate::real(i) / 4.0 - 1.0), y: p.y, r: RAD, ..Default::default() });
    }
}

/// `Shockwave.hitNear`: the point of the core segment nearest (x, y), for the tests against another actor's sphere or
/// box.
pub fn shockwave_near(p: &Projectile, x: Real, _y: Real) -> V2 {
    V2 { x: max(p.x - CORE, min(p.x + CORE, x)), y: p.y }
}

/// ReceiveBeginPlay: the impact effect, and the Delay(Duration_0).
fn shockwave_begin(w: &mut World, id: ProjectileId) {
    let p = pr(w, id);
    let (fighter, x, y) = (p.owner_id, p.x, p.y);
    let dur = num(class_p(w, p), "Duration_0").unwrap_or(0.0);
    w.events.push(Event::ProjectileDeath {
        cls: name!("TunnelShockwave_C"),
        fighter,
        vfx: Some(name!("PS_SupermanSPU_Impact")),
        x,
        y,
        ox: 0.0,
        rot: [0.0; 3],
        scale: 1.0,
        tint: None,
        to: None,
    });
    if let ProjectileExt::BugsShockwave(s) = &mut pm(w, id).ext {
        s.left = latent_frames(dur);
    }
}

/// The Delay's countdown: true (K2_DestroyActor) when it runs out.
fn shockwave_step(w: &mut World, id: ProjectileId) -> bool {
    match &mut pm(w, id).ext {
        ProjectileExt::BugsShockwave(s) if s.left > 0.0 => {
            s.left -= 1.0;
            s.left == 0.0
        }
        _ => false,
    }
}

/// HitEnemyPlayer @30: the hit sound on itself.
fn shockwave_hit_enemy(w: &mut World, id: ProjectileId, _victim: FighterId) {
    let p = pr(w, id);
    let e = Event::Sound {
        fighter: p.owner_id,
        ev: name!("sfx_cha_bugs_generic_hit"),
        x: Some(p.x),
        y: Some(p.y),
        hit: None,
        pooled: false,
        key: None,
    };
    w.events.push(e);
}

// ---- Buff_TunnelTeleport's Timeline Finished
// TimelineFinished @15: the Instigator valid and a BPI_BaseFighter (@15-140), else nothing. Then, by the pushed flows
// (@150 push 175, @155 push 718, the Has-Been-Initd gates popping to 718 first), the Shockwave DoOnce @718 and then
// the Escapist DoOnce @195. Each DoOnce is the buff actor's own (a new actor per teleport): once per teleport.
// Shockwave @718: the Instigator's BPI_CheckIfHasBuff(Buff_BugsBunny_Shockwave_C) @843; the owner (the traveller) a
// BP_FighterCharacter @1116; his PlayerState's BPI_IncrementPlayerAttackCount(1) @1227 (the count a montage start
// bumps); PF_Post_Event_At_Location(sfx_cha_bugs_tunnel_shockwave_exit, Destination) @1269; on authority the count
// read @1478 and his Comp_Spawner.CompSpawnActor(TunnelShockwave_C, no attach, owner the traveller, world transform
// at Destination + (0, 0, -100), rot (0, 0, 90), direction 0, speeds 0, attack id the count) @1708.
// Escapist @195: the traveller a BP_FighterCharacter @306, authority @336, the Instigator's
// BPI_CheckIfHasBuff(Buff_Bugs_Escapist_C) @445 -> the traveller's LaunchCharacter(EscapistLaunchVector (0, 0, 2250),
// true, true) @620. In the same frame the teleport's Delay(FighterTeleportDuration) completes after it (both come due
// on the 18th counted frame from Activate; the Timeline's Finished runs in its component tick, the Delay in the
// leftover latent pass after the tick groups) and its LaunchCharacter((0, 0, 1), true, true) @2029 takes the one
// pending-launch slot: the launch the next movement tick applies is (0, 0, 1), not Escapist's. tunnel.rs makes the two
// calls in that order in the buff's tick (the impulse: last wins).
// Unverified (Escapist): whether the 2250 is applied for one frame before the Delay's write depends on the
// timeline's tick against the fighter's movement tick inside TG_PrePhysics, which code does not fix; the port applies
// neither in between (one slot, read by the next movement).

/// `tunnelFinished`: the perks' part of the tunnel teleport's end. `dx`, `dy`: Destination (sim space, y the actor's
/// height).
pub fn tunnel_finished(w: &mut World, f: FighterId, from: Option<FighterId>, dx: Real, dy: Real) {
    let Some(from) = from else { return }; // no Instigator (never: Comp_Pawn_Tunnel spawns it with the tunnel's owner)
    if buff_of(w, from, name!("Buff_BugsBunny_Shockwave_C")).is_some() {
        w.fighter_mut(f).decay.attack_count += 1;
        let (fighter, count) = (w.fighter(f).id, w.fighter(f).decay.attack_count);
        w.fighter_mut(f).events.push(Event::Sound {
            fighter,
            ev: name!("sfx_cha_bugs_tunnel_shockwave_exit"),
            x: Some(dx),
            y: Some(dy),
            hit: None,
            pooled: false,
            key: None,
        });
        comp_spawn_projectile(w, f, name!("TunnelShockwave_C"), dx, dy - 100.0, [Some(0.0); 3], 0.0, 0.0, Some(count));
    }
    if buff_of(w, from, name!("Buff_Bugs_Escapist_C")).is_some() {
        let v = buff_def(w, ActorRef::Fighter(f), name!("Buff_TunnelTeleport_C"))
            .and_then(|d| obj(d, "EscapistLaunchVector"));
        let (vy, vz) = (v.and_then(|v| num(v, "Y")).unwrap_or(0.0), v.and_then(|v| num(v, "Z")).unwrap_or(0.0));
        // LaunchCharacter(v, true, true)
        w.fighter_mut(f).moves.impulse = Some(Impulse { vx: -vy, vy: vz, xy: true, kind: 0, z: true, d: None });
    }
}

/// This file's registry entries (LingeringLoveProjectile_C and TunnelShockwave_C; the perk buffs have none).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let kiss = ProjectileHooks {
        construct: Some(still_construct),
        bodies: Some(kiss_bodies),
        begin: Some(kiss_begin),
        any_damage: Some(kiss_any_damage),
        ..ProjectileHooks::default()
    };
    let mut e =
        ActorEntry::new(ActorClass::Projectile(ProjectileClass::LingeringLoveProjectile), ActorHooks::Projectile(kiss));
    e.foreign_owner = true;
    r.actors.enter(KISS, e)?;
    let wave = ProjectileHooks {
        construct: Some(shockwave_construct),
        begin: Some(shockwave_begin),
        step: Some(shockwave_step),
        hit_enemy: Some(shockwave_hit_enemy),
        ..ProjectileHooks::default()
    };
    let mut e = ActorEntry::new(ActorClass::Projectile(ProjectileClass::TunnelShockwave), ActorHooks::Projectile(wave));
    e.foreign_owner = true;
    r.actors.enter(SHOCKWAVE, e)
}
