#![allow(clippy::doc_lazy_continuation, reason = "the doc lists wrap by the notes' own layout")]
//! PolymorphGrenade ([`PolymorphGrenade`], a DealerActor): its hit gives
//! Buff_C020Polymorph, a child of `combat::polymorph` (entered there).
//!
//! PolymorphGrenade, a SpawnedActorParent child thrown by C020_Attack_PolymorphGun_Montage (Spawner_C_1 at L_Hand,
//! direction (0, 1, 0), 1201; the air throw (0, 1, -0.5)), the ability slot 1 (TriggerAbilityIndex: the world's
//! `actor_spawn_ability`).
//!
//! The body is Batman's SmokeGrenade's (`actors::smoke_bomb`, whose notes hold the reads): EnableProjectileMovement unset,
//! so the parent deactivates the ProjectileMovement and HandleActorPostSpawn's launch (the direction, normalised, x
//! InitialSpeed, Y along the facing, plus half the owner's velocity) is a velocity change of the body's first step; the
//! Capsule (35 / 35, bSimulatePhysics, AngularDamping 20, SatchelPhysicalMaterial, X locked, Platform unlisted: Block) with
//! MineMesh (SM_C020Satchel at scale 3, MassInKgOverride 46.94) and the Box (42 x 42 x 12 at (0.75, 0.75, 1) under
//! MineMesh: half 94.5 x 36 in the plane, 7.5 over the centre) welded into it (`rigid_sphere` compound_step with
//! `grenade_body`'s mass). The spawn place goes through FindTeleportSpot (Comp_Spawner's AdjustIfPossibleButAlwaysSpawn,
//! as the smoke grenade's).
//!
//! ReceiveBeginPlay @3495: the parent's; Box, MineMesh and Capsule SetCollisionEnabled(PhysicsOnly) @3505-3581 (the welded
//! body stays: the smoke grenade's note); the team-coloured PS_NinjaBomb on the Box @3936 (the drawing's); Delay(0.0667)
//! @4254 -> @2938: Box.SetCollisionEnabled(QueryAndPhysics) (no query of the port asks for its PhysicsBody type),
//! sfx_cha_C020_polymorph_gun_loop on the actor's Ak (Ak Fuse ID) @2976; Delay(0.45) @3067 -> @15 Delay(1.3) -> @70 the
//! blast. The chained Delays count one after the other (as HarleyBomb's arm).
//! The blast @70:
//! - SphereOverlapActors(the location, BlastRadius 450, [ItemDestructible, Pawn, DamageDestructible,
//!   ProjectileDestructible], BaseFighter_C, ignoring self and null) @141; per fighter @539
//!   Comp_Pawn_HitBoxHandler.pfgDoDamage(null, the fighter, Hit, null, [], On Hit Buffs Param): Hit is 3 / 10 / 1 / (1, 1),
//!   HitReaction 6, ArmorPierce, HitEachActorOnlyOnce; On Hit Buffs Param: Buff_C020Polymorph_C to a hit enemy for 6 s. For
//!   an enemy the port's gates of a spawned actor's pfgDoDamage (the projectile test ahead of @4608: `projectile_blocks`;
//!   an invulnerable victim's dodge: `attack_dodged`) and the hit (`apply_projectile_hit`); for a team mate
//!   NotifyAllyOfHit (`ally_hit`: nothing for the Instigator). Then, the Instigator's TeamIndex equal to the fighter's (the
//!   Instigator himself too) @728: the Instigator's SpawnBuffActor(Buff_Enlarge_C, the fighter, the Instigator, 1, 3.75,
//!   true, false) @949 and BPI_IncrementCoopActions @1172.
//! - SphereOverlapActors(the same, SpawnedActorParent_C, ignoring self) @1268; per actor: a ParentProjectilePhysics @1591
//!   with PolymorphScaleUp? and its Comp_Actor_Destructible's TeamIndex equal to the Instigator's @1969 ->
//!   BPI_IncrementCoopActions @2160, then SpawnBuffActor(IsProjectile ? Buff_Polymorph_ProjectileEnlarge_C :
//!   Buff_Polymorph_ProjectileEnlarge_WithHieghtChange_C, the actor, the Instigator, 1, 6.0, true, false) @2471
//!   (`spawn_actor_buff`); else a Steven_ShieldActor @2858 -> @2039, the same spawn with no test (an enemy's shield too:
//!   `chars/steven/barrier.rs`, its kind's size hooks).
//! - DetonateClient @2622 (PS_C020PolymorphImpact at the location, scale 0.82, Team; the fuse stopped over 200 ms;
//!   sfx_cha_C020_polymorph_gun_end at the location), then K2_DestroyActor @2636. ReceiveDestroyed @4319 stops the fuse the
//!   same way (ExecuteActionOnPlayingID(Stop, Ak Fuse ID, 200, 4: Linear)) for any other end (out of the arena, Die With
//!   Instigator); after the blast's stop it finds nothing playing.
//! DestroyOnExitedArena?: SpawnedActorParent's 0.1 s timer (as HarleyBomb's): out of the blast box the actor is destroyed
//! (no DeathVFX, no death sound in its data).
// Deviation: the overlap's order is the port's trace order (`fighter::traces`), not PhysX's; a pass's hit or buff that
// changes what a later one meets is met in that order.
//! The knockback's side (KnockBackDirectionVector (1, 1), neither VelocityInsteadOfFacing nor IgnoreFacing): pfgDoDamage
//! @10428-11168 takes ComingFromLeft = the handler owner's actor yaw < 0 (modifier -1 @21014). The grenade is spawned at yaw
//! +-90 along the thrower's facing (every Comp_Spawner spawn) and the welded body turns about UE X only (the plane's
//! normal: compound_step's angle), which at yaw +-90 is a pitch: past 90 degrees FRotator reads it as pitch 180 - a with the
//! yaw turned by 180. So the side is the spawn facing, reversed while cos(angle) < 0 ([`side`], passed as the hit's
//! direction).
// Deviation: the body's turns about the plane's own axes (wa, wb) are not kept by compound_step, so a yaw they would give
// is not read; at exactly 90 degrees the facing is kept.
// Deviation: the projectile pass meets the `World.projectiles` entries (their HurtBox, ProjectileDestructible) only; a
// ParentProjectilePhysics child among `World.actors` (none ported but Rick's Meeseeks) or an item is warned and gets
// nothing.
//! Not ported: BPI_IncrementCoopActions (the player state's coop count: no port); the PlayerAttachCollision EndOverlap path
//! @4432 and the BlastBoxPlayerExit loop @3122 (AttachedTo is never written: dormant, as the smoke grenade's);
//! ReceiveAnyDamage (no Comp_Actor_Destructible: nothing damages it); the presentation of the fuse and the throw.

use crate::Real;
use crate::State;
use crate::actors::smoke_bomb::{GrenadeCache, grenade_body, grenade_statics};
use crate::actors::world_actor::{WorldActor, WorldActorState, actor_cls, actor_owner, actor_p};
use crate::actors::{ActorClass, ActorId, ActorRef, WorldActorClass};
use crate::combat::HitPacket;
use crate::combat::buffs::spawn_buff;
use crate::combat::elem::{charge_pct, spawn_actor_buff};
use crate::combat::hits::{actor_hit_enemy, ally_hit, apply_projectile_hit, dealer, projectile_blocks};
use crate::combat::packet::OnHitBuffRow;
use crate::combat::phases::alive;
use crate::consts::DELTA_SECONDS;
use crate::events::{Event, SoundKey};
use crate::fighter::common::{actor_class_def, actor_data, actor_is_a, latent_frames, warn_once, warn_text};
use crate::fighter::control::attack_dodged;
use crate::fighter::match_side::SpawnRequest;
use crate::fighter::moves::V2;
use crate::fighter::traces::solids::sphere_static;
use crate::fighter::traces::{
    OBJ_DAMAGE_DESTRUCTIBLE, OBJ_ITEM_DESTRUCTIBLE, OBJ_PAWN, OBJ_PROJECTILE_DESTRUCTIBLE, TraceBody, body_info,
    sphere_overlap_actors,
};
use crate::fighter::{FighterId, invulnerable};
use crate::math::ue::{SMALL_NUMBER, cosf, safe_normal, sinf};
use crate::math::{max, to_int32};
use crate::name;
use crate::names::Name;
use crate::registry::entries::{ActorEntry, ActorHooks, SizeHooks, WorldActorHooks};
use crate::registry::props::{flag, num, obj};
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use crate::world::blast::{BlastBounds, blast_box_loop, blast_timer_exit, new_blast_bounds, spawn_box};
use crate::world::spawn_spot::{find_teleport_spot, spawn_place};
use crate::world::spawning::request_props;
use mvs_collision::Ground;
use mvs_collision::rigid_sphere::{Body, Solver};

const CLS: &str = "PolymorphGrenade_C";
const PPP: Name = name!("ParentProjectilePhysics_C");
const SPAWNED_PARENT: Name = name!("SpawnedActorParent_C");
const STEVEN_SHIELD: Name = name!("Steven_ShieldActor_C");
const ENLARGE: Name = name!("Buff_Enlarge_C");
const PROJ_ENLARGE: Name = name!("Buff_Polymorph_ProjectileEnlarge_C");
const PROJ_ENLARGE_HEIGHT: Name = name!("Buff_Polymorph_ProjectileEnlarge_WithHieghtChange_C");
const FUSE_TAG: Name = name!("rickpoly");

/// The blast's tick: the three chained Delays.
fn arm() -> i32 {
    to_int32(latent_frames(0.0667))
}

fn blast_at_age() -> i32 {
    arm() + to_int32(latent_frames(0.45)) + to_int32(latent_frames(1.3))
}

/// `PolymorphGrenade` (PolymorphGrenade_C).
#[derive(State, Debug)]
pub struct PolymorphGrenade {
    /// Hit, the packet of the blast's pfgDoDamage.
    pub hit: HitPacket,
    /// The Capsule's, at the root's scale (the kind's size hook).
    pub radius: Real,
    /// The owner's facing at the spawn (the spawn yaw).
    pub facing: Real,
    pub attack_id: i32,
    /// The launch, pending until the first step.
    pub pvx: Real,
    pub pvy: Real,
    pub vx: Real,
    pub vy: Real,
    pub spin: Real,
    /// The angular velocity about the plane's x and y.
    pub wa: Real,
    pub wb: Real,
    /// About the normal (spin = wc x radius).
    pub wc: Real,
    /// The welded Box's turn.
    pub angle: Real,
    /// Ticks lived, the spawn frame's included.
    pub age: i32,
    /// Ak Fuse ID playing.
    pub fuse: bool,
    /// The arena check's next expiry.
    pub blast_at: crate::Clock,
    pub blast_bounds: BlastBounds,
    pub x: Real,
    pub y: Real,
    /// The welded body and the step's buffers. Not state.
    #[state(skip)]
    pub cache: GrenadeCache,
}

impl Default for PolymorphGrenade {
    /// The field initial values.
    fn default() -> Self {
        PolymorphGrenade {
            hit: HitPacket::default(),
            radius: 35.0,
            facing: 0.0,
            attack_id: 0,
            pvx: 0.0,
            pvy: 0.0,
            vx: 0.0,
            vy: 0.0,
            spin: 0.0,
            wa: 0.0,
            wb: 0.0,
            wc: 0.0,
            angle: 0.0,
            age: 0,
            fuse: false,
            blast_at: f64::from(0.1_f32),
            blast_bounds: new_blast_bounds(),
            x: 0.0,
            y: 0.0,
            cache: GrenadeCache::default(),
        }
    }
}

/// The PolymorphGrenade_C's place (its `x`, `y`).
pub fn place(w: &World, a: &WorldActor, s: &PolymorphGrenade) -> Option<(Real, Real)> {
    let _ = (w, a);
    Some((s.x, s.y))
}

/// The PolymorphGrenade_C as a dealer (`DealerActor`): its place, facing and AttackID.
pub fn dealer_parts(w: &World, a: &WorldActor, s: &PolymorphGrenade) -> (Real, Real, Real, i32) {
    let _ = (w, a);
    (s.x, s.y, s.facing, s.attack_id)
}

/// The PolymorphGrenade_C's HitBoxDamage packet (`hit`).
pub fn hit(s: &PolymorphGrenade) -> &HitPacket {
    &s.hit
}

fn g_of(w: &World, id: ActorId) -> Option<&PolymorphGrenade> {
    match &w.actors.actors.get(id)?.state {
        WorldActorState::PolymorphGrenade(g) => Some(g),
        _ => None,
    }
}

fn g_mut(w: &mut World, id: ActorId) -> Option<&mut PolymorphGrenade> {
    match &mut w.actors.actors.get_mut(id)?.state {
        WorldActorState::PolymorphGrenade(g) => Some(g),
        _ => None,
    }
}

fn set_dead(w: &mut World, id: ActorId) {
    if let Some(a) = w.actors.actors.get_mut(id) {
        a.dead = true;
    }
}

fn fuse_key(w: &World, id: ActorId) -> SoundKey {
    SoundKey { tag: FUSE_TAG, n: w.actors.actors.get(id).map_or(0, |a| a.seq) }
}

fn stage_platform_grounds(w: &World) -> Vec<Ground> {
    w.stage
        .stage_platforms
        .iter()
        .map(|p| Ground {
            left: f64::from(p.left),
            right: f64::from(p.right),
            bottom: f64::from(p.bottom),
            top: f64::from(p.top),
        })
        .collect()
}

/// One physics step: the pending launch as a velocity change, then the body against the stage block and its platforms and
/// the spawned actors' solid boxes (`grenadeStatics`). What the body would meet besides them warns.
fn grenade_move(w: &mut World, id: ActorId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let (owner, cls, k) = (a.owner, a.cls, a.size_scale.unwrap_or(1.0));
    let data = w.data.clone();
    let Some(def) = actor_data(w, &data, ActorRef::Fighter(owner), cls) else { return };
    let Some(g) = g_mut(w, id) else { return };
    let mut c = std::mem::take(&mut g.cache);
    // `bodyOf`: the body at scale 1 read at the spawn, else at the root's scale.
    if c.body.is_none_or(|(at, _)| at != k) {
        c.body = Some((k, grenade_body(w, def, CLS, k)));
    }
    let Some(body) = c.body.and_then(|(_, b)| b) else {
        // its material was refused (warned)
        if let Some(g) = g_mut(w, id) {
            g.cache = c;
        }
        return;
    };
    if w.data.stage.name.starts_with("Map_M011") {
        warn_once(
            w,
            name!("PolymorphGrenade: Sky Arena's arm donut collider is not ported (profile and place unread)"),
        );
    }
    grenade_statics(w, owner, cls, &body.bx, name!("PolymorphGrenade_C body"), &mut c.grounds, &mut c.statics);
    let floor = sphere_static(w, w.stage.ground);
    let Some(g) = g_mut(w, id) else { return };
    g.vx += g.pvx;
    g.vy += g.pvy;
    g.pvx = 0.0;
    g.pvy = 0.0;
    let mut b = Body {
        x: f64::from(g.x),
        y: f64::from(g.y),
        vx: f64::from(g.vx),
        vy: f64::from(g.vy),
        spin: f64::from(g.spin),
        radius: f64::from(g.radius),
        angle: f64::from(g.angle),
        wa: f64::from(g.wa),
        wb: f64::from(g.wb),
        wc: f64::from(g.wc),
        ..Body::default()
    };
    Solver::default().compound_step(&mut b, &floor, Some(&c.statics), DELTA_SECONDS, &body.set, &body.bx, &body.mass);
    g.cache = c;
    (g.x, g.y, g.vx, g.vy, g.spin, g.angle, g.wa, g.wb, g.wc) = (
        crate::math::narrow(b.x),
        crate::math::narrow(b.y),
        crate::math::narrow(b.vx),
        crate::math::narrow(b.vy),
        crate::math::narrow(b.spin),
        crate::math::narrow(b.angle),
        crate::math::narrow(b.wa),
        crate::math::narrow(b.wb),
        crate::math::narrow(b.wc),
    );
}

/// The sim side of a facing hit's X: the spawn facing, reversed while the body's turn is past 90 degrees.
fn side(g: &PolymorphGrenade) -> Real {
    // the port's body angle (radians, the PhysX stand-in's): no FMath routine, the CRT's float cosine
    if cosf(g.angle) < 0.0 { -g.facing } else { g.facing }
}

/// A spawned actor's dead flag (`s.dead`): a fighter, a stage actor and one gone from its arena read as dead.
fn dead_of(w: &World, a: ActorRef) -> bool {
    let w = &w.actors;
    match a {
        ActorRef::Projectile(id) => w.projectiles.get(id).is_none_or(|p| p.dead),
        ActorRef::Item(id) => w.items.get(id).is_none_or(|p| p.dead),
        ActorRef::Actor(id) => w.actors.get(id).is_none_or(|p| p.dead),
        ActorRef::Marker(id) => w.markers.get(id).is_none_or(|p| p.dead),
        ActorRef::Ship(id) => w.ships.get(id).is_none_or(|p| p.dead),
        ActorRef::Mortar(id) => w.mortars.get(id).is_none_or(|p| p.dead),
        ActorRef::AimPunch(id) => w.aim_punches.get(id).is_none_or(|p| p.dead),
        ActorRef::Accelerator(id) => w.accelerators.get(id).is_none_or(|p| p.dead),
        ActorRef::Fighter(_) | ActorRef::Stage(_) | ActorRef::Buff(_) => true,
    }
}

/// `inChain`: the class or one of its parents in the data.
fn in_chain(w: &World, f: FighterId, cls: Name, sup: Name) -> bool {
    actor_is_a(w, ActorRef::Fighter(f), cls, sup).unwrap_or(false)
}

/// The class's Comp_Actor_Destructible IsProjectile (@2307).
fn is_projectile(w: &World, f: FighterId, cls: Name) -> bool {
    actor_class_def(w, f, cls)
        .and_then(|d| obj(&d.components, "Comp_Actor_Destructible"))
        .is_some_and(|c| flag(c, "IsProjectile"))
}

fn types() -> [Real; 4] {
    [
        crate::real(OBJ_ITEM_DESTRUCTIBLE),
        crate::real(OBJ_PAWN),
        crate::real(OBJ_DAMAGE_DESTRUCTIBLE),
        crate::real(OBJ_PROJECTILE_DESTRUCTIBLE),
    ]
}

fn blast(w: &mut World, id: ActorId) {
    let me = ActorRef::Actor(id);
    let Some(a) = w.actors.actors.get(id) else { return };
    let f = a.owner;
    let Some(g) = g_of(w, id) else { return };
    let (gx, gy, attack_id, hit, sd) = (g.x, g.y, g.attack_id, g.hit.clone(), side(g));
    let p = actor_p(w, me);
    let r = num(p, "BlastRadius").unwrap_or(0.0);
    let rows = OnHitBuffRow::rows(&w.names, p.get("On Hit Buffs Param"));
    let team = w.fighter(f).team.team;
    // the fighters' pass
    let found = sphere_overlap_actors(w, gx, gy, r, &types(), &[Some(me), None], -1.0);
    for a in found {
        let ActorRef::Fighter(o) = a else { continue };
        if !alive(w, o) {
            continue;
        }
        if w.fighter(o).team.team != team {
            if crate::combat::phases::no_hit_by(w, f, o) || projectile_blocks(w, &dealer(w, me), o) {
                continue;
            }
            if invulnerable(w, o) {
                let tick = w.flow.tick;
                attack_dodged(w, me, o, attack_id, hit.base_knock_back.unwrap_or(0.0), tick, None, false);
            } else {
                if w.overlap_mid_frame {
                    w.fighter_mut(o).frame.hit_this_frame = true;
                }
                let d = hit
                    .knock_back_direction_vector
                    .map_or((1.0, 0.0), |d| (d.x.unwrap_or(Real::NAN), d.y.unwrap_or(Real::NAN)));
                let dir = V2 { x: d.0 * sd, y: d.1 };
                let dl = dealer(w, me);
                let charge = charge_pct(w, me);
                if apply_projectile_hit(w, &dl, o, &hit, &rows, V2 { x: gx, y: gy }, Some(dir), &[], charge) {
                    actor_hit_enemy(w, &dl, o);
                }
            }
            continue;
        }
        ally_hit(w, &dealer(w, me), o, &hit);
        spawn_buff(w, o, ENLARGE, 3.75, Some(1), Some(f), true, None);
    }
    // the spawned actors' pass
    let found = sphere_overlap_actors(w, gx, gy, r, &types(), &[Some(me)], -1.0);
    for a in found {
        if matches!(a, ActorRef::Fighter(_)) {
            continue;
        }
        let (Some(cls), Some(owner)) = (actor_cls(w, a), actor_owner(w, a)) else { continue };
        if dead_of(w, a) || !in_chain(w, f, cls, SPAWNED_PARENT) {
            continue;
        }
        if !in_chain(w, f, cls, PPP) {
            // @2858: a Steven_ShieldActor (the class or _Up) goes to @2039 with no PolymorphScaleUp? or team test: an
            // enemy's shield is enlarged too (its kind's size hooks, `chars/steven/barrier.rs`).
            if in_chain(w, f, cls, STEVEN_SHIELD) {
                let c = if is_projectile(w, f, cls) { PROJ_ENLARGE } else { PROJ_ENLARGE_HEIGHT };
                spawn_actor_buff(w, a, c, 1, 6.0, true, Some(f));
            }
            continue;
        }
        if !flag(actor_p(w, a), "PolymorphScaleUp?") || w.fighter(owner).team.team != team {
            continue;
        }
        if !matches!(a, ActorRef::Projectile(_)) {
            warn_text(
                w,
                &format!(
                    "{CLS}: {} is not a Projectile of the port: its polymorph enlarge is not ported",
                    w.names.str(cls)
                ),
            );
            continue;
        }
        let c = if is_projectile(w, f, cls) { PROJ_ENLARGE } else { PROJ_ENLARGE_HEIGHT };
        spawn_actor_buff(w, a, c, 1, 6.0, true, Some(f));
    }
    // DetonateClient, K2_DestroyActor
    let fid = w.fighter(f).id;
    w.events.push(Event::ProjectileDeath {
        cls: name!("PolymorphGrenade_C"),
        fighter: fid,
        vfx: Some(name!("PS_C020PolymorphImpact")),
        x: gx,
        y: gy,
        ox: 0.0,
        rot: [0.0, 0.0, 0.0],
        scale: 0.82,
        tint: None,
        to: None,
    });
    stop_fuse(w, id);
    w.events.push(Event::Sound {
        fighter: fid,
        ev: name!("sfx_cha_C020_polymorph_gun_end"),
        x: Some(gx),
        y: Some(gy),
        hit: None,
        pooled: false,
        key: None,
    });
    set_dead(w, id);
}

/// The fuse stopped over 200 ms (Linear), when it plays.
fn stop_fuse(w: &mut World, id: ActorId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let fid = a.owner_id;
    let key = fuse_key(w, id);
    let Some(g) = g_mut(w, id) else { return };
    if g.fuse {
        g.fuse = false;
        w.events.push(Event::SoundStop {
            fighter: fid,
            key,
            fade: Some(0.2),
            ev: None,
            curve: Some(name!("Linear")),
            cut: false,
        });
    }
}

/// The welded Box as a body of the traces (`ActorKind.bodies`): Custom, ECC_PhysicsBody, PhysicsOnly from @3505 until the
/// Delay(0.0667)'s Box.SetCollisionEnabled(QueryAndPhysics) @2938, then on for the actor's life (the blast's K2_DestroyActor
/// ends it). So the object queries for PhysicsBody meet it from the arming tick (Bugs's rocket burst, the dynamite's blast,
/// the Meeseeks' overlaps). Its place is the body's (`grenade_body`'s WeldedBox: half 94.5 x 36 in the plane, its centre 7.5
/// over the actor's origin, turned by the body's angle). The other bodies: the Capsule (Custom, WorldDynamic, PhysicsOnly
/// from @3505) and MineMesh (PhysicsOnly) answer no query; PlayerAttachCollision is WorldDynamic, which no query of the port
/// gathers.
// Deviation: the box is met as the rectangle with its short ends rounded (a segment of half length 94.5 - 36 along its
// turned X, radius 36: a body of `fighter::traces` turns only as a segment), 14.9 short of each corner. The Box answers
// WorldDynamic with Overlap, so the Meeseeks' component overlaps would see it too; their handler takes Projectiles alone,
// which the grenade is not.
fn grenade_bodies(w: &World, id: ActorId, out: &mut Vec<TraceBody>) {
    let (Some(a), Some(g)) = (w.actors.actors.get(id), g_of(w, id)) else { return };
    if a.dead || g.age < arm() {
        return;
    }
    let Some(def) = actor_class_def(w, a.owner, a.cls) else { return };
    let k = a.size_scale.unwrap_or(1.0);
    let body = match g.cache.body {
        Some((at, b)) if at == k => b,
        _ => grenade_body(w, def, CLS, k),
    };
    let info = body_info(obj(&def.components, "Box"), None);
    let (Some(body), Some(info)) = (body, info) else { return };
    let bx = body.bx;
    let (c, s) = (cosf(g.angle), sinf(g.angle));
    let (cx, cy, h) = (
        g.x - s * crate::math::narrow(bx.oy),
        g.y + c * crate::math::narrow(bx.oy),
        max(crate::math::narrow(bx.hx - bx.hy), 0.0),
    );
    out.push(TraceBody {
        ax: cx - c * h,
        ay: cy - s * h,
        bx: cx + c * h,
        by: cy + s * h,
        hx: 0.0,
        hy: 0.0,
        r: crate::math::narrow(bx.hy),
        z: Real::NAN,
        object_type: info.kind,
        profile: info.profile,
        component: name!("Box"),
        actor: Some(ActorRef::Actor(id)),
        platform: -1,
        blocks: info.blocks,
        poly: None,
        floor: None,
    });
}

/// The kind's `spawn`: the place through FindTeleportSpot, the launch pending, the parent's BeginPlay blast-box loop.
fn grenade_spawn(w: &mut World, f: FighterId, r: &SpawnRequest) -> (WorldActorState, bool) {
    let np = request_props(w, f, r);
    let s = spawn_place(w, f, &np);
    let data = w.data.clone();
    let def = actor_data(w, &data, ActorRef::Fighter(f), r.cls)
        .unwrap_or_else(|| panic!("{CLS}: no actor data for its spawn (the importer gives every spawned class)"));
    let cap_r = obj(&def.components, "Capsule").and_then(|c| num(c, "CapsuleRadius")).unwrap_or(35.0);
    let mut boxes =
        crate::world::stage_query::static_shapes(w, crate::consts::SOLID_NAV, name!("PolymorphGrenade_C spawn"));
    boxes.boxes.extend(stage_platform_grounds(w));
    let at = find_teleport_spot(boxes.view(), s.x, s.y, cap_r);
    let d = obj(&np, "InitialMovementDirection");
    let (dy, dz) = (d.and_then(|d| num(d, "Y")).unwrap_or(0.0), d.and_then(|d| num(d, "Z")).unwrap_or(0.0));
    let sp = num(&np, "InitialSpeed").unwrap_or(0.0);
    // the direction's unit vector (GetSafeNormal)
    let [_, ny, nz] = safe_normal([0.0, dy, dz], SMALL_NUMBER);
    let facing = crate::fighter::facing(w, f);
    let (fvx, fvy) = (w.fighter(f).body.vx, w.fighter(f).body.vy);
    let mut g = PolymorphGrenade {
        hit: HitPacket::from_props(&w.names, obj(&def.p, "Hit").unwrap_or(&mvs_data::value::Props::new()), None),
        radius: cap_r,
        facing,
        attack_id: r.attack_id,
        x: at.x,
        y: at.y,
        pvx: (((ny * sp) * facing) + fvx / 2.0),
        pvy: ((nz * sp) + fvy / 2.0),
        ..PolymorphGrenade::default()
    };
    // the body read once at the spawn (its warnings), kept for the steps at scale 1
    g.cache.body = Some((1.0, grenade_body(w, def, CLS, 1.0)));
    let sb = spawn_box(w);
    let dead = blast_box_loop(
        &sb,
        &mut g.x,
        &mut g.y,
        None,
        flag(&def.p, "ForceSpawnPositionInsideBlastBox"),
        &mut g.blast_bounds,
        flag(&def.p, "DestroyOnExitedArena?"),
    );
    (WorldActorState::PolymorphGrenade(g), dead)
}

/// The actor's tick (TG_PrePhysics): its latent actions, then the frame's physics step, then the timer.
fn grenade_step(w: &mut World, id: ActorId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let (fid, doe) = (a.owner_id, flag(actor_p(w, ActorRef::Actor(id)), "DestroyOnExitedArena?"));
    let key = fuse_key(w, id);
    let Some(g) = g_mut(w, id) else { return };
    g.age += 1;
    let age = g.age;
    if age == arm() {
        g.fuse = true;
        w.events.push(Event::SoundStart { fighter: fid, ev: name!("sfx_cha_C020_polymorph_gun_loop"), key });
    }
    if age >= blast_at_age() {
        blast(w, id);
        return;
    }
    grenade_move(w, id);
    let Some(g) = g_mut(w, id) else { return };
    let gone = blast_timer_exit(doe, &mut g.blast_at, &g.blast_bounds, crate::real(g.age - 1), g.x, g.y);
    if gone {
        set_dead(w, id);
    }
}

fn grenade_end(w: &mut World, id: ActorId) {
    stop_fuse(w, id);
}

/// The welded body and the Box's trace body at the root's scale (the cache's); BlastRadius is the blast's own; the view too.
fn grenade_scale(w: &mut World, id: ActorId, s: Real, old: Real) {
    if let Some(g) = g_mut(w, id) {
        g.radius *= s / old;
    }
}

/// This file's registry entry (PolymorphGrenade_C). `foreign_owner`: owner reads are the Instigator's place, facing,
/// velocity and team (a disguised wearer's own: the build's Instigator too).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let hooks = ActorHooks::World(WorldActorHooks {
        spawn: Some(grenade_spawn),
        step: Some(grenade_step),
        end: Some(grenade_end),
        bodies: Some(grenade_bodies),
        size: Some(SizeHooks { scale: Some(grenade_scale), ..SizeHooks::default() }),
        ..WorldActorHooks::default()
    });
    r.actors.enter(
        CLS,
        ActorEntry {
            foreign_owner: true,
            ..ActorEntry::new(ActorClass::World(WorldActorClass::PolymorphGrenade), hooks)
        },
    )
}
