//! FartBomb_SpawnedActor ([`FartBomb`], a projectile): the "MouseTrapBait", a
//! SpawnedActorParent child with EnableProjectileMovement, spawned by C020_Attack_FartBomb_Montage (Spawner_C_0: offset
//! (0, 150, 50), direction (0, 1, -1), 900 / max 1300, no socket).
//!
//! The latent links are the Delays' LatentInfo Linkage in the bytecode (89 -> 144, 2810 -> 159, 2865 -> 174,
//! 2980 -> 30, 3035 -> 56, 3090 -> 15).
//!
//! What the parent and the data give with no code here: the movement (ProjectileMovement: bounce, Bounciness 0.1, gravity
//! scale 5, no stop speed; the root Capsule with no radius in its template: UCapsuleComponent's 22 / 44, profile
//! ProjectileNavPlatform: platforms block it), the hit box (HitBox2D 120, 2D: Supports2DHitboxes) with HitBoxOn? false at
//! the spawn, DestroyedOnHit false, DestroyOnExitedArena?, the packet 6 / 1500 / 22.5 / (0, 1) with HitReaction 4 and
//! sfx_cha_C020_fart_trap_hit, the ability slot 4 (TriggerAbilityIndex: the world's). The hit box sits 117.63 over the
//! root (HitBox and HitBox2D at RelativeLocation Z 117.629684 on the Capsule): [`hit_y`].
//!
//! ReceiveBeginPlay @1594: the parent's (its NumberOfActorsAllowed 1 @5864-6515: the oldest FartBomb of the owner's
//! SpawnedActors past the limit is destroyed, as BugsBunnySafe's begin does it), then
//! - @1619: the owner facing left -> SetActorScale3D(scale x (1, -1, 1)) @1852: the mirror is the drawing's (the hit box
//!   is a circle in the plane; the projectile's `facing` holds the side).
//! - @1872 BoxTraceSingle from Z - 50 to Z - 150, half size (100, 100, 0), ProjectileGroundTrace, ignoring self @2140,
//!   then K2_AttachToActor(HitActor, KeepWorld, weld) @2465 and, for a DestructibleObjectBP hit, its ObjectDestroyed bound
//!   to GroundDestroyed @2746 (`bind_ground_destroyed`). The stage's block, platforms and placed walls are static actors:
//!   the attach moves nothing (the movement component goes on moving the root).
// Deviation: a hit on a spawned actor (a moving or breakable one: no such body answers ProjectileGroundTrace in the port)
// is warned, not attached.
//! - GroundDestroyed @3188: a DoOnce (Has_Been_Initd_1 / IsClosed_1, its own) -> HasHit = true @3250 (read by ReceiveTick's
//!   loop alone, which does nothing: not kept), Mesh.PlayAnimation(null) @3149 (the view's), then Delay(TriggerTime) @3090
//!   -> Detonate @15.
// Unverified: whether Mesh.PlayAnimation(null) leaves the snap pose or the reference pose is not read; the view keeps what
// it draws.
//! - Delay(12) @2810 -> GroundDestroyed: never reached (the actor ends at about 1.4 s below).
//! - Delay(0.4) @2865 -> Detonate.
//! - @1008 authority with DestroyOnInstigatorHit: the Instigator's Comp_Pawn_DamageManager KnockedBack bound to
//!   OnInstigatorKnockedBack @1229; ReceiveEndPlay @3900 -> @1308 unbinds it.
//!
//! Detonate @2936: EffectMulticast @3465 (SpawnEmitterAttached(PS_C020_FartCloud, Mesh, (0, 0, 100)) with Team), HitBoxOn?
//! = true and UpdateHitbox @2950; Delay(0.15) @2980 -> @30 HitBoxOn? = false, UpdateHitbox; Delay(0.4) @3035 -> @56
//! Mesh.K2_DestroyComponent, Delay(0.6) @89 -> @144 K2_DestroyActor. The two first Delays start in the same tick. A second
//! Detonate runs EffectMulticast and the hit box's on again, and each Delay node that is no longer pending starts again (a
//! pending one ignores the retrigger). Its callers: the Delay(0.4) above, ReceiveAnyDamage (below; before or after that
//! Delay, so twice) and the Delay(TriggerTime) behind GroundDestroyed.
// Deviation: the fart cloud is sent as an emitter at a fixed place (the Mesh's place + (0, 0, 100) at the Detonate); the
// build attaches it to the Mesh @3465, which does not move once the bomb lies still, and is destroyed with it 0.4 s later
// (what the attached system does then is the view's).
//! OnInstigatorKnockedBack @3869: HitBoxOn? false -> DestroySpawnedActor(true) @3884 (before the detonation and in the 0.4 s
//! after the cloud's hit box went off); while the hit box is on nothing.
// Deviation: the KnockedBack broadcast is read in the bomb's tick (the fighter's knocked_back_count against the count at the
// bind, as `chars/creature/fastball.rs`), not inside the hit: a knockback dealt behind the projectiles' tick of the frame (a
// projectile's hit, detectProjectileHits) is met one frame late. A ReceiveAnyDamage reads it first ([`kb_seen`]): a count
// that moved since the last read is a broadcast that came ahead of the damage, so a bomb whose hit box was off is destroyed
// before the damage reaches it, as in the build (a projectile's hit in detectProjectileHits that knocks Rick back, then a
// DestroyProjectiles hit on the bomb in the same frame; a melee knockback lands in the fighter pass, ahead of the bomb's
// tick, which destroys it first). Deviation: the caller still takes it as a hit (the melee path's hitNonPlayer signal, the
// dealer's HitNonPlayer / HitAnything); in the build the bomb is gone inside the broadcast, before the dealer meets it.
//! ReceiveTick @3372: authority, !HasHit -> the loop over OverlappingInvulnerableFighters @214, which nothing of the class
//! fills: nothing (so ActivateHitMulticast @3367 and its DoOnce @3188 are never reached from it).
//! ReceiveAnyDamage @3266: MulticastDamageTaken @3295 (Mesh.PlayAnimation(MouseTrap_SnapFast): `snap_fast` for the view),
//! then Detonate @3280, with no gate: every damage, before the 0.4 s Detonate or after it. Its Delays start as the
//! Detonate's above: the bomb always has a pending latent node (the Delay(12) @2810), so a Delay made after its tick of the
//! frame is first counted in the next one, and one made ahead of it in that tick: both end at the age now + latent_frames.
//!
//! What reaches it: ApplyDamage of a DamageAndLaunch with the bomb as HitActor. A hit box meets the bomb at its
//! ProjectileCollision sphere (USphereComponent default radius 32 at the root, object type ProjectileDestructible, Overlap
//! to HitBox alone: the projectile's `hurt_radius`), and its Comp_Actor_Destructible has IsProjectile, so pfgDoDamage takes
//! the projectile branch @15084 (meleeProjectileTest, projectileHitsProjectile, spawnedHit): the bomb has no
//! CanProjectileClank (no clank), so only a Hitframe or a HitBoxDamage with DestroyProjectiles reaches DamageAndLaunch, and
//! CanBeHitBy @659 lets another team's alone through (a team mate's: HitByAlly, which SpawnedActorParent ignores). The
//! damage's size is not read. The sphere is a body of the traces too ([`bodies`]: no graph turns it off), so the object
//! queries for ProjectileDestructible meet it (the polymorph grenade's blast passes, which do nothing to this class).

use crate::Real;
use crate::State;
use crate::actors::projectile::{Projectile, ProjectileExt, hit_collision, hit_near, set_hitbox};
use crate::actors::{ActorClass, ActorRef, ProjectileClass, ProjectileId};
use crate::events::Event;
use crate::fighter::common::{actor_class_def, latent_frames, warn_text};
use crate::fighter::traces::{TRACE_PROJECTILE_GROUND, TraceBody, body_info, box_trace_single};
use crate::math::to_int32;
use crate::name;
use crate::names::Name;
use crate::registry::entries::{ActorEntry, ActorHooks, HazardHooks, ProjectileHooks};
use crate::registry::props::{flag, num, obj};
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use crate::world::spawning::destroy_projectile;
use crate::world::wall_bp::{WallActor, bind_ground_destroyed, wall_delegate_fns};

const CLS: Name = name!("FartBomb_SpawnedActor_C");

/// `FartBomb extends Projectile` (FartBomb_SpawnedActor_C).
#[derive(State, Debug)]
pub struct FartBomb {
    /// HitBoxOn?.
    pub box_on: bool,
    /// The age the Detonate fires at (Delay(0.4) from the begin).
    pub det_at: i32,
    /// The hit box's off (Delay(0.15)).
    pub off_at: i32,
    /// Mesh.K2_DestroyComponent (Delay(0.4)).
    pub mesh_at: i32,
    /// K2_DestroyActor (Delay(0.6)).
    pub end_at: i32,
    /// The Mesh destroyed: the view stops drawing it.
    pub mesh_gone: bool,
    /// MulticastDamageTaken's Mesh.PlayAnimation(MouseTrap_SnapFast) ran (the view's).
    pub snap_fast: bool,
    /// The Instigator's knocked_back_count at the bind.
    pub kb_at: i32,
    /// GroundDestroyed's DoOnce closed: the age its Delay(TriggerTime) fires Detonate at, -1 still open.
    pub ground_at: i32,
    /// The HitBox2D's and the Mesh's Z over the root, per class data (unscaled).
    pub hit_off: Real,
    pub mesh_off: Real,
}

impl Default for FartBomb {
    /// The field initial values.
    fn default() -> Self {
        FartBomb {
            box_on: false,
            det_at: -1,
            off_at: -1,
            mesh_at: -1,
            end_at: -1,
            mesh_gone: false,
            snap_fast: false,
            kb_at: 0,
            ground_at: -1,
            hit_off: 0.0,
            mesh_off: 0.0,
        }
    }
}

fn pr(w: &World, id: ProjectileId) -> &Projectile {
    w.actors.projectiles.get(id).expect("a live projectile")
}

fn pm(w: &mut World, id: ProjectileId) -> &mut Projectile {
    w.actors.projectiles.get_mut(id).expect("a live projectile")
}

fn bomb(w: &World, id: ProjectileId) -> &FartBomb {
    match &pr(w, id).ext {
        ProjectileExt::FartBomb(b) => b,
        _ => unreachable!("a FartBomb_SpawnedActor_C's state"),
    }
}

fn bomb_mut(w: &mut World, id: ProjectileId) -> &mut FartBomb {
    match &mut pm(w, id).ext {
        ProjectileExt::FartBomb(b) => b,
        _ => unreachable!("a FartBomb_SpawnedActor_C's state"),
    }
}

/// `hitY`: the HitBox2D's centre, over the root by its Z offset at the root's scale (`actors::projectile::hit_xy`).
pub fn hit_y(p: &Projectile, b: &FartBomb) -> Real {
    p.y + b.hit_off * p.scale
}

/// `meshY`.
fn mesh_y(p: &Projectile, b: &FartBomb) -> Real {
    p.y + b.mesh_off * p.scale
}

/// ProjectileCollision's radius at the root's scale (`pc.SphereRadius ?? 32` x its relative X scale x the root's), and its
/// body info, `None` for a class with no component.
fn collision_radius(w: &World, p: &Projectile) -> Option<(Real, crate::fighter::traces::BodyInfo)> {
    let owner = p.owner?;
    let def = actor_class_def(w, owner, p.cls)?;
    let pc = obj(&def.components, "ProjectileCollision")?;
    let info = body_info(Some(pc), None)?;
    let sx = obj(pc, "RelativeScale3D").and_then(|s| num(s, "X")).unwrap_or(1.0);
    Some((num(pc, "SphereRadius").unwrap_or(32.0) * sx * p.scale, info))
}

/// The constructor's tail: the offsets, and ProjectileCollision as the hurt sphere (GetScaledSphereRadius: the root's scale;
/// the parent's HurtBox is none).
fn construct(w: &World, p: &mut Projectile) {
    let comps = p.owner.and_then(|o| actor_class_def(w, o, p.cls)).map(|d| &d.components);
    let z = |c: &str| {
        comps.and_then(|m| obj(m, c)).and_then(|c| obj(c, "RelativeLocation")).and_then(|l| num(l, "Z")).unwrap_or(0.0)
    };
    p.ext = ProjectileExt::FartBomb(FartBomb { hit_off: z("HitBox2D"), mesh_off: z("Mesh"), ..FartBomb::default() });
    p.hurt_radius = match collision_radius(w, p) {
        Some((r, info)) if info.on => r,
        _ => 0.0,
    };
}

fn detonate(w: &mut World, id: ProjectileId) {
    let p = pr(w, id);
    let b = bomb(w, id);
    let ev = Event::ProjectileDeath {
        cls: p.cls,
        fighter: p.owner_id,
        vfx: Some(name!("PS_C020_FartCloud")),
        x: p.x,
        y: mesh_y(p, b) + 100.0 * p.scale,
        ox: 0.0,
        rot: [0.0, 0.0, 0.0],
        scale: 1.0,
        tint: None,
        to: None,
    };
    w.events.push(ev);
    let age = pr(w, id).age;
    set_hitbox(pm(w, id), true);
    let b = bomb_mut(w, id);
    b.box_on = true;
    // a pending Delay ignores the retrigger
    if b.off_at < age {
        b.off_at = age + to_int32(latent_frames(0.15));
    }
    if b.mesh_at < age {
        b.mesh_at = age + to_int32(latent_frames(0.4));
    }
}

/// The Instigator's KnockedBack broadcasts since the last read (OnInstigatorKnockedBack @3869): true when one came with the
/// hit box off (HitBoxOn? @3869 -> DestroySpawnedActor(true) @3884).
fn kb_seen(w: &mut World, id: ProjectileId) -> bool {
    let p = pr(w, id);
    let Some(f) = p.owner else { return false };
    let destroy_on_hit = flag(crate::actors::projectile::class_p(w, p), "DestroyOnInstigatorHit");
    let kb = w.fighter(f).attackers.knocked_back_count;
    let b = bomb_mut(w, id);
    if !destroy_on_hit || kb == b.kb_at {
        return false;
    }
    b.kb_at = kb;
    !b.box_on
}

/// GroundDestroyed's bind name (`wall_delegate_fns`).
const GROUND_DESTROYED: &str = "FartBomb_SpawnedActor_C.GroundDestroyed";

/// GroundDestroyed (the header): an ObjectDestroyed bound to it by a DestructibleObjectBP the ground trace hit. Its
/// Delay starts as ReceiveAnyDamage's Detonate Delays: it ends at the age now + latentFrames.
fn ground_destroyed(w: &mut World, a: ActorRef, _wall: WallActor) {
    let ActorRef::Projectile(id) = a else { return };
    if bomb(w, id).ground_at >= 0 {
        return;
    }
    let t = num(crate::actors::projectile::class_p(w, pr(w, id)), "TriggerTime").unwrap_or(0.0);
    let at = pr(w, id).age + to_int32(latent_frames(t));
    bomb_mut(w, id).ground_at = at;
}

fn begin(w: &mut World, id: ProjectileId) {
    let p = pr(w, id);
    let (x, y) = (p.x, p.y);
    let hit = box_trace_single(
        w,
        x,
        y - 50.0,
        x,
        y - 150.0,
        100.0,
        0.0,
        100.0,
        TRACE_PROJECTILE_GROUND,
        &[],
        Some(ActorRef::Projectile(id)),
    );
    if let Some(h) = &hit
        && h.actor.is_some()
    {
        warn_text(
            w,
            &format!(
                "{}: the ground trace met an actor ({}): the attach to it is not ported",
                w.names.str(CLS),
                w.names.str(h.component)
            ),
        );
    }
    bind_ground_destroyed(w, hit.as_ref(), ActorRef::Projectile(id), GROUND_DESTROYED); // @2667-2769
    let age = pr(w, id).age;
    let owner = pr(w, id).owner;
    let on = flag(crate::actors::projectile::class_p(w, pr(w, id)), "DestroyOnInstigatorHit");
    let kb = owner.map(|f| w.fighter(f).attackers.knocked_back_count);
    let b = bomb_mut(w, id);
    b.det_at = age + to_int32(latent_frames(0.4));
    if on && let Some(kb) = kb {
        b.kb_at = kb;
    }
}

fn step(w: &mut World, id: ProjectileId) -> bool {
    // OnInstigatorKnockedBack @3869, ahead of the tick's latent actions: HitBoxOn? as the last tick left it.
    if kb_seen(w, id) {
        destroy_projectile(w, id, false);
        return false;
    }
    let age = pr(w, id).age;
    let b = bomb(w, id);
    let (det, ground, off, mesh) = (b.det_at, b.ground_at, b.off_at, b.mesh_at);
    if age == det {
        detonate(w, id);
    }
    if age == ground {
        detonate(w, id);
    }
    if age == off {
        bomb_mut(w, id).box_on = false;
        set_hitbox(pm(w, id), false);
    }
    if age == mesh {
        let b = bomb_mut(w, id);
        b.mesh_gone = true;
        if b.end_at < age {
            b.end_at = age + to_int32(latent_frames(0.6));
        }
    }
    age == bomb(w, id).end_at
}

/// ReceiveAnyDamage @3266: MulticastDamageTaken @3295, Detonate @3280; the actor stays.
fn any_damage(w: &mut World, id: ProjectileId, _amount: Real, _by: ActorRef) -> bool {
    if kb_seen(w, id) {
        return true; // (a KnockedBack ahead of the damage: DestroySpawnedActor(true) @3884)
    }
    bomb_mut(w, id).snap_fast = true;
    detonate(w, id);
    false
}

/// ProjectileCollision: a SphereComponent with no SphereRadius in its template (the native default 32) on the root Capsule
/// at its origin, profile ProjectileDestructible, at the root's scale.
fn bodies(w: &World, a: ActorRef, out: &mut Vec<TraceBody>) {
    let ActorRef::Projectile(id) = a else { return };
    let p = pr(w, id);
    let Some((r, info)) = collision_radius(w, p) else { return };
    if !info.on {
        return;
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
        component: name!("ProjectileCollision"),
        actor: Some(a),
        platform: -1,
        blocks: info.blocks,
        poly: None,
        floor: None,
    });
}

/// BPI_EnvironmentalHazard (C019ScrubShield). What its Sphere overlaps: HitBox and HitBox2D (object type HitBox, Overlap to
/// ProjectileDestructible) while HitBoxOn? has them on (the hit circle, as `actors::barrier` reads a projectile's);
/// ProjectileCollision is of the Sphere's own type (Ignore).
fn clean_overlaps(w: &World, a: ActorRef, x: Real, y: Real, r: Real) -> bool {
    let ActorRef::Projectile(id) = a else { return false };
    let p = pr(w, id);
    if p.dead || !hit_collision(p) {
        return false;
    }
    let near = hit_near(w, p, x, y);
    let (dx, dy, s) = (near.x - x, near.y - y, r + p.hit_radius);
    dx * dx + dy * dy <= s * s
}

/// BPI Clean @0-15: Blocked(null), the parent's (no override: DestroyOnBlocked -> Delay(0.0) -> DestroySpawnedActor,
/// `blocked_destroy`), Success = true.
fn clean(w: &mut World, a: ActorRef) -> bool {
    let ActorRef::Projectile(id) = a else { return false };
    if flag(crate::actors::projectile::class_p(w, pr(w, id)), "DestroyOnBlocked") {
        pm(w, id).blocked_destroy = true;
    }
    true
}

/// This file's registry entry (FartBomb_SpawnedActor_C). `foreign_owner`: owner reads are the Instigator's facing at the
/// begin, his knockback count and spawned actors (a disguised wearer's own: the build's Instigator too).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let hooks = ProjectileHooks {
        construct: Some(construct),
        bodies: Some(bodies),
        begin: Some(begin),
        step: Some(step),
        any_damage: Some(any_damage),
        ..ProjectileHooks::default()
    };
    r.actors.enter(
        "FartBomb_SpawnedActor_C",
        ActorEntry {
            foreign_owner: true,
            hazard: Some(HazardHooks { overlaps: clean_overlaps, clean }),
            ..ActorEntry::new(
                ActorClass::Projectile(ProjectileClass::FartBombSpawnedActor),
                ActorHooks::Projectile(hooks),
            )
        },
    )?;
    wall_delegate_fns(r, &[(GROUND_DESTROYED, ground_destroyed)])
}
