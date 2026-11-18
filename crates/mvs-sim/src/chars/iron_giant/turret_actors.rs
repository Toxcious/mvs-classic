//! The ally turret's actors: C017_Pilot_Reticle_C
//! ([`PilotReticle`]) and C017_Ally_Turret_Bomb_C ([`TurretBomb`], a DealerActor), world actors spawned by turret.rs.
//! Both are SpawnedActorParent children made by Comp_C017_Ally_Turret's own CompSpawnActor calls, not by a notify's
//! spawn request, so their kinds have no spawn: the turret calls [`spawn_pilot_reticle`] / [`spawn_turret_bomb`].
//! Sim space: x = -UE Y, y = UE Z.
//!
//! The shared code reaches their places (`actors::world_actor`) and the bomb's dealer fields (`combat::hits` dealer,
//! `combat::spawned_hits`).
#![allow(clippy::neg_cmp_op_on_partial_ord)] // NaN-keeping negated tests

use mvs_collision::Ground;

use super::state::{giant, giant_of, giant_w};
use crate::Real;
use crate::State;
use crate::actors::world_actor::{WorldActor, WorldActorState, actor_p};
use crate::actors::{ActorClass, ActorId, ActorRef, WorldActorClass};
use crate::combat::HitPacket;
use crate::combat::elem::{charge_pct, instigator_charge};
use crate::combat::hits::{actor_hit_enemy, apply_projectile_hit, dealer, projectile_blocks};
use crate::consts::DELTA_SECONDS;
use crate::fighter::FighterId;
use crate::fighter::common::{actor_def, sound_at_for, warn_text};
use crate::fighter::control::{attack_dodged, dodge_over};
use crate::fighter::moves::V2;
use crate::fighter::{actor_y, invulnerable, no_hit};
use crate::math::min;
use crate::math::ue::{SMALL_NUMBER, safe_normal, size2d};
use crate::name;
use crate::registry::entries::{ActorEntry, ActorHooks, SizeHooks, WorldActorHooks};
use crate::registry::props::{flag, num, obj};
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use crate::world::blast::{BlastBounds, blast_box_loop, check_blast_box_exit, new_blast_bounds, spawn_box};
use crate::world::spawning::{comp_spawn_actor, new_world_actor};
use crate::world::stage::{ComponentMove, SOLID_NOPLATFORM};

pub const PILOT_RETICLE: &str = "C017_Pilot_Reticle_C";
pub const TURRET_BOMB: &str = "C017_Ally_Turret_Bomb_C";

/// A timer's expiry against the ticks counted since it was set (`actors.rs` `elapsed`, Projectile's MaxLifetime).
fn elapsed(ticks: i32) -> crate::Clock {
    crate::consts::clock(crate::real(ticks))
}

// ---- C017_Pilot_Reticle

/// `PilotReticle` (C017_Pilot_Reticle_C: SpawnedActorParent; a Capsule without collision, 'HitBoxOn?' false,
/// NumberOfActorsAllowed 1, CanChangeFacingDirection false, DestroyOnExitedArena?, Die With Instigator).
/// ReceiveBeginPlay (@744, after the parent's): MainMesh = the owner's (@899), AimReticule.K2_SetRelativeRotation((0,
/// Yaw -90, 0)) for an owner facing left (@1077: the template's yaw 90 is kept otherwise; either way the component's
/// world yaw is 0), the owner's Comp_C017_Ally_Turret.SetReticleControl(self) (@1204) and Active Pilot = its
/// ActivePilot (@1312). ReceiveTick (@1386 -> @48) only draws three debug lines for a locally controlled pilot
/// (DrawDebugLine: nothing in the build). ArmAnim (@1410: Arm_Timeline into the arrow material's Scrol1) has no caller
/// in the decoded graphs.
/// The actor is moved by the component alone (K2_SetActorLocation(AimLocation) @2558 on the ticks with input, and
/// AimLocation starts as its location, @9571-9621), so its place is the turret's aim_x / aim_y while it lives, and it
/// lives while the component's PilotAimReticle does: its dead flag follows `AllyTurret.reticle` ([`sync_reticles`]),
/// which KickPilot's K2_DestroyActor (@5515) and the owner's reset clear inside their own calls.
/// For the renderer: x / y, the owner, `pilot`, `flip` (C017_Pilot_Reticle_C's components in the data: AimReticule =
/// PS_C017_UI_Reticule at relative (0, 0, -1200.69), scale 1.5, yaw 90 or with flip 0; ReticuleArm =
/// SM_C017_ReticuleArrow at (0, 0, 800), scale (200, 380, 150), a child of AimReticule).
/// The parent's BeginPlay blast-box loop at the spawn and its CheckBlastBoxExit (SpawnedActorParent @20219, the 0.1 s
/// looping timer @204, with DestroyOnExitedArena?), counted as Projectile.expiry counts it, against the bounds it
/// stored (`world::blast`: the jump-out area's corner and its mirror); the reticle starts 500 ahead of the Giant and
/// is only clamped into the box by the first aim input.
#[derive(State, Debug, Default)]
pub struct PilotReticle {
    /// The owner faced left at BeginPlay (@1017-1077).
    pub flip: bool,
    pub x: Real,
    pub y: Real,
    pub age: i32,
    /// CheckBlastBoxTimer's next expiry, in seconds of counted deltas.
    pub blast_at: crate::Clock,
    /// Its BlastBoxBoundsMin / Max (SpawnedActorParent @9720-10573).
    pub blast_bounds: BlastBounds,
    pub pilot: Option<FighterId>,
    /// The actor's dead flag is set by the turret's flag alone (`AllyTurret.reticle` off), not by a destroy of its
    /// own: the getter (`gone || !reticle`) reads it alive again while the flag is back on (`!gone`).
    pub off: bool,
}

fn reticle_mut(w: &mut World, id: ActorId) -> Option<&mut PilotReticle> {
    match &mut w.actors.actors.get_mut(id)?.state {
        WorldActorState::PilotReticle(r) => Some(r),
        _ => None,
    }
}

/// The reticle's place (its `x`, `y`).
pub fn reticle_place(w: &World, a: &WorldActor, s: &PilotReticle) -> Option<(Real, Real)> {
    let _ = (w, a);
    Some((s.x, s.y))
}

/// The getter `dead` (`gone || !giant(owner).allyTurret?.reticle`) of every listed reticle of `f`'s, written
/// where the turret's flag changes (`turret.rs`) and the actor's own destroys set it.
pub fn sync_reticles(w: &mut World, f: FighterId) {
    let on = giant_of(w, f).and_then(|g| g.ally_turret.as_ref()).is_some_and(|t| t.reticle);
    let mut i = 0;
    while let Some(id) = w.actors.actors.nth(i) {
        i += 1;
        let Some(a) = w.actors.actors.get_mut(id) else { continue };
        if a.owner != f {
            continue;
        }
        let dead = a.dead;
        let WorldActorState::PilotReticle(r) = &mut a.state else { continue };
        if !on && !dead {
            r.off = true;
            a.dead = true;
        } else if on && dead && r.off {
            r.off = false;
            a.dead = false;
        }
    }
}

fn reticle_step(w: &mut World, id: ActorId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let f = a.owner;
    let Some(t) = giant(w, f).ally_turret.as_ref() else { return };
    let (ax, ay) = (t.aim_x, t.aim_y);
    let doe = flag(actor_p(w, ActorRef::Actor(id)), "DestroyOnExitedArena?");
    let Some(r) = reticle_mut(w, id) else { return };
    r.age += 1;
    r.x = ax;
    r.y = ay;
    if !doe || !(elapsed(r.age - 1) > r.blast_at) {
        return;
    }
    r.blast_at += f64::from(0.1_f32);
    // DestroySpawnedActor(true) (@20685). The component keeps aiming without it (its tick tests ActivePilot alone,
    // @9130; K2_SetActorLocation on the destroyed actor does nothing).
    if check_blast_box_exit(r.x, r.y, &r.blast_bounds) {
        if let Some(a) = w.actors.actors.get_mut(id) {
            a.dead = true;
        }
        if let Some(t) = giant_w(w, f).ally_turret.as_mut() {
            t.reticle = false;
        }
        sync_reticles(w, f);
    }
}

/// Comp_C017_Ally_Turret @6364: CompSpawnActor(C017_Pilot_Reticle, not attached, socket None, relative transform (0,
/// 500, 500)) at (x, y) (the turret works the place out). The parent's BeginPlay (SpawnedActorParent @5864-6177):
/// with more live actors of the class in the owner's SpawnedActors than NumberOfActorsAllowed, the oldest is
/// destroyed (`comp_spawn_actor`). `None` when the class is not in the data.
pub fn spawn_pilot_reticle(w: &mut World, f: FighterId, x: Real, y: Real, pilot: Option<FighterId>) -> Option<ActorId> {
    let cls = name!("C017_Pilot_Reticle_C");
    let Some(d) = actor_def(w, ActorRef::Fighter(f), cls) else {
        warn_text(w, &format!("{PILOT_RETICLE} not exported"));
        return None;
    };
    let (doe, force) = (flag(&d.p, "DestroyOnExitedArena?"), flag(&d.p, "ForceSpawnPositionInsideBlastBox"));
    let mut r = PilotReticle {
        flip: w.fighter(f).body.facing_dir < 0.0,
        x,
        y,
        age: 0,
        blast_at: f64::from(0.1_f32),
        blast_bounds: new_blast_bounds(),
        pilot,
        off: false,
    };
    // The parent's BeginPlay loop.
    // Unverified: destroyed there, the turret's reticle flag is left as the caller sets it.
    let sb = spawn_box(w);
    let dead = blast_box_loop(&sb, &mut r.x, &mut r.y, None, force, &mut r.blast_bounds, doe);
    let actor = new_world_actor(w, cls, WorldActorClass::C017PilotReticle, f, dead, WorldActorState::PilotReticle(r));
    let id = w.actors.actors.insert(actor);
    let charge = instigator_charge(w, f);
    comp_spawn_actor(w, id, charge, f);
    // The getter reads the flag: a reticle spawned while it is off is dead to every reader.
    sync_reticles(w, f);
    Some(id)
}

// ---- C017_Ally_Turret_Bomb

/// LineTraceSingle on trace type 5 (ProjectileGroundNoPlatform: the stage block, World.blockers' note, and the stage
/// solids that block it, `static_grounds`: the first entry over them) from (x0, y0) to (x1, y1): the fraction of the
/// way at which the line enters the box, -1 for no hit.
/// A trace that starts inside or on the block is a hit at its start: the raycast answers distance 0 and
/// LineTraceSingleByChannel returns a blocking hit with Time 0 and ImpactPoint = Start (ConvertQueryImpactHit
/// 1428f5e60; the wall code's traceBlock rule). The graph takes the ImpactPoint on any true return (@3361-3538,
/// bInitialOverlap is not read), so the target is the bomb's own place, TravelTime 0, and the move ends at its first
/// update. A polygon solid answers by its own line trace (`PolyBlock::ray`'s time, 0 from inside it).
fn trace_ground(g: &Ground, x0: Real, y0: Real, x1: Real, y1: Real) -> Real {
    let (dx, dy) = (x1 - x0, y1 - y0);
    let (mut t0, mut t1) = (0.0, 1.0);
    for i in 0..4 {
        let (p, q) = match i {
            0 => (-dx, x0 - crate::math::narrow(g.left)),
            1 => (dx, crate::math::narrow(g.right) - x0),
            2 => (-dy, y0 - crate::math::narrow(g.bottom)),
            _ => (dy, crate::math::narrow(g.top) - y0),
        };
        if p == 0.0 {
            if q < 0.0 {
                return -1.0;
            }
            continue;
        }
        let r = q / p;
        if p < 0.0 {
            if r > t1 {
                return -1.0;
            }
            if r > t0 {
                t0 = r;
            }
        } else {
            if r < t0 {
                return -1.0;
            }
            if r < t1 {
                t1 = r;
            }
        }
    }
    t0
}

/// `TurretBomb` (C017_Ally_Turret_Bomb_C: SpawnedActorParent with its own graph; root Capsule 80 / 80 with
/// bAbsoluteScale, HitBox sphere 200 at relative scale 0.5 on it, PS_C017_Turret_Projectile01 (PS_C017_SPF_Projectile)
/// on the HitBox, both sphere meshes hidden; 'HitBoxOn?' false, DestroyedOnHit false, DestroyOnExitedArena? false,
/// no MaxLifetime, Die With Instigator; EnableProjectileMovement is off, so the parent's BeginPlay deactivates
/// ProjectileMovement (SpawnedActorParent @3777-3792). The spawner writes ProjectileMovement.Velocity
/// (Comp_Spawner BPI_SetInitialMovement @8407; @10931 gates only the physics impulse), but the component is inactive,
/// so the direction and the two speeds of the turret's CompSpawnActor (@974) move nothing: the bomb travels by
/// MoveComponentTo).
/// ReceiveBeginPlay (@2802, after the parent's), in this order:
/// - @2827-2970 'Target Prediction Location' = the location of the owner's Comp_C017_Ally_Turret.PilotAimReticle
/// - @3033-3134 Speed = the actor's scale X (the spawn transform's (ProjectileSpeed, 1, 1)); SetActorScale3D(1)
/// - @3157-3538 LineTraceSingle(the actor's location -> the target, trace type 5, the owner ignored): on a hit the
///   target is the ImpactPoint
/// - @3565-3685 TravelTime = Vector_Distance(target, location) / Speed
/// - @2998-2666 MoveComponentTo(Capsule, target, zero rotation, no ease out, no ease in, TravelTime, Move)
/// - @3713-4095 the Team parameter of the particle and the mesh (SelectFloat(0.6, ColorValue, IsLocalOnline): the
///   renderer's) and sfx_cha_C017_weaponmode_bomb_cast on the actor
///
/// The move's end (@5045 -> @2636, a DoOnce -> @1009, once): ProjectileMovement stopped, StartExpansion (@4312: the
/// Capsule's relative scale = HitStartScale, ShouldExpand, sfx_..._bomb_impact_expanding at the Capsule, and a
/// TickDamage at once @4556 -> @2585), then the DamageTimer (ExpansionTime, once -> EndExpantion @1200) and the looping
/// DamageTickTimer (RepeatedDamageTime -> TickDamage @1307).
/// ReceiveTick (@4561) with ShouldExpand: TimeExpanding += dt; the Capsule's relative scale =
/// FMin(Lerp(HitStartScale, MaxScale, TimeExpanding / ExpansionTime), MaxScale).
/// TickDamage (@2585-2569): every BaseFighter overlapping HitBox whose Comp_Actor_Destructible.TeamIndex is not the
/// bomb's takes pfgDoDamage(TickHitDamage) with KnockBackDirectionVector = (Y, Z) of GetDirectionUnitVector(the
/// fighter's location, the bomb's) (@2074-2290; IgnoreFacing): toward the bomb. EndExpantion (@4287): the tick timer
/// cleared, pfgDoDamage(LastHitDamage) on each such fighter (@159-915), K2_DestroyActor (@916), sfx_..._bomb_despawn
/// at the particle (@80). Both calls pass no buffs. The handler's HitEnemyPlayer delegate posts sfx_..._bomb_hit at
/// the actor (@4916).
/// LastHitDamage is not IgnoreFacing: the handler's side is the actor's yaw (pfgDoDamage @10428-11168: coming from the
/// left with a yaw below 0), and the bomb is spawned with a world transform of zero rotation (Comp_Spawner @3326
/// SelectTransform), so its (1, 1) points to UE +Y for either facing of the Giant: `facing` -1.
/// For the renderer: x / y (the Capsule), `scale` (the Capsule's relative scale: 1 in flight, HitStartScale to
/// MaxScale while exploding; the HitBox, the particle and the meshes are its children), `exploding`, tx / ty, the
/// owner.
/// The move. MoveComponentTo is the World's ComponentMove. The Capsule is the actor's root (SpawnedActorParent's SCS:
/// DefaultSceneRoot is not among its root nodes, the Capsule is the first scene component of them), so its relative
/// location is the world's and the world target is where it goes. The action's callback target is the actor (@2666,
/// self): AActor::Tick 14294e9e0 runs ReceiveTick, then the actor's latent actions, and a spawned actor ticks in its
/// spawn frame with a BeginPlay latent action counting that frame. So the first update is in the spawn frame's tick,
/// each update comes after the frame's ReceiveTick, and the arrival frame does not grow the scale (ShouldExpand is set
/// after its ReceiveTick).
/// The timers. Both are set inside the arrival's update (@1200, @1307) and become active in that frame's timer pass
/// with ExpireTime = InternalTime + Rate (FTimerManager::Tick FUN_142e45520), so they count from the arrival frame: the
/// DamageTimer fires at the first frame whose time since then is > ExpansionTime; the looping DamageTickTimer is
/// re-armed by ExpireTime += (float)(CallCount x Rate) in a double (item 43), with CallCount calls in a row when a
/// frame passes several periods, and the two come out in ExpireTime order.
// Deviation: three leftovers. (1) The timer pass is counted at the bomb's tick of the following frames (the pass comes
// after every tick of its frame, so the calls land one tick function later than here: after the frame's ReceiveTick
// grew the scale, as here, but after the fighters' ticks of that frame too). (2) A bomb whose lead-in Delay ends in
// the world's leftover latent pass (the component's tick is off after a kick out of weapon mode,
// Comp_C017_Ally_Turret @9680) is spawned after the tick groups: whether it still ticks in that frame is not read
// (TG_NewlySpawned inside the later groups); it does here. (3) Equal ExpireTimes of the two timers come out by the
// heap's layout (item 44); the class defaults (1.5, 0.2) give none. Read for (1) and (2): a per-tick read of the
// Capsule's location and scale and of the victims' damage on the live actor (cap.py).
/// HitBox.GetOverlappingActors(BaseFighter) is the HitBox sphere (radius x 0.5 x the Capsule's scale, at depth 0)
/// against the fighters' root capsules ([`bomb_damage`]); an invulnerable fighter dodges it (pfgDoDamage's own test).
/// HitBox is the bomb's own SphereComponent (SCS_Node_4) whose template serialises a radius and a scale only, so it
/// keeps UShapeComponent's constructor profile OverlapAllDynamic (FUN_142984280; type WorldDynamic, Pawn Overlap,
/// HitBox and HurtBox Ignore) and UPrimitiveComponent's bGenerateOverlapEvents true (FUN_142956f30): it overlaps a
/// fighter's root capsule (Pawn profile: WorldDynamic Block, min with the sphere's Overlap) and none of his hurt
/// boxes, and a fighter whose capsule has no collision is not found.
/// The actor's `sizeScale`, `dodge`, `elem`, `charge` and `lifetimePause` are the [`WorldActor`]'s.
#[derive(State, Debug)]
pub struct TurretBomb {
    /// HitBoxDamage: never dealt ('HitBoxOn?' false).
    pub hit: HitPacket,
    pub attack_id: i32,
    /// -1 (the zero rotation).
    pub facing: Real,
    pub x: Real,
    pub y: Real,
    /// 'Target Prediction Location'.
    pub tx: Real,
    pub ty: Real,
    /// TravelTime.
    pub travel: Real,
    /// The running MoveComponentTo (`move`), `None` from its end.
    pub mv: Option<ComponentMove>,
    pub age: i32,
    pub scale: Real,
    /// ShouldExpand.
    pub exploding: bool,
    /// The age at the move's end.
    pub t0: i32,
    /// TimeExpanding.
    pub expand_t: Real,
    /// The DamageTickTimer's ExpireTime, seconds after `t0` (a double, as the manager's).
    pub next_tick: crate::Clock,
    /// The HitBox sphere's radius at scale 1 (SphereRadius x its RelativeScale3D.X).
    pub sphere_r: Real,
    /// The HitBox sphere as [`bomb_sphere`] last placed it (`sph[0]`: x, y, r; its depth 0).
    pub sph_x: Real,
    pub sph_y: Real,
    pub sph_r: Real,
}

impl Default for TurretBomb {
    /// The field initial values.
    fn default() -> Self {
        TurretBomb {
            hit: HitPacket::default(),
            attack_id: 0,
            facing: -1.0,
            x: 0.0,
            y: 0.0,
            tx: 0.0,
            ty: 0.0,
            travel: 0.0,
            mv: None,
            age: 0,
            scale: 1.0,
            exploding: false,
            t0: 0,
            expand_t: 0.0,
            next_tick: 0.0,
            sphere_r: 0.0,
            sph_x: 0.0,
            sph_y: 0.0,
            sph_r: 0.0,
        }
    }
}

fn bomb_of(w: &World, id: ActorId) -> Option<&TurretBomb> {
    match &w.actors.actors.get(id)?.state {
        WorldActorState::TurretBomb(b) => Some(b),
        _ => None,
    }
}

fn bomb_mut(w: &mut World, id: ActorId) -> Option<&mut TurretBomb> {
    match &mut w.actors.actors.get_mut(id)?.state {
        WorldActorState::TurretBomb(b) => Some(b),
        _ => None,
    }
}

/// The bomb's place (its `x`, `y`).
pub fn bomb_place(w: &World, a: &WorldActor, s: &TurretBomb) -> Option<(Real, Real)> {
    let _ = (w, a);
    Some((s.x, s.y))
}

/// The bomb as a dealer (`DealerActor`): its place, facing and AttackID.
pub fn bomb_dealer_parts(w: &World, a: &WorldActor, s: &TurretBomb) -> (Real, Real, Real, i32) {
    let _ = (w, a);
    (s.x, s.y, s.facing, s.attack_id)
}

/// The bomb's HitBoxDamage packet (`hit`).
pub fn bomb_hit(s: &TurretBomb) -> &HitPacket {
    &s.hit
}

/// `hitRadius`: the HitBox sphere's radius at the Capsule's scale.
fn hit_radius(b: &TurretBomb) -> Real {
    b.sphere_r * b.scale
}

/// `sphere()`: the HitBox sphere where the bomb is now (x, y, r).
fn bomb_sphere(b: &mut TurretBomb) -> (Real, Real, Real) {
    b.sph_x = b.x;
    b.sph_y = b.y;
    b.sph_r = hit_radius(b);
    (b.sph_x, b.sph_y, b.sph_r)
}

/// The bomb's damage packets of its class data (TickHitDamage, LastHitDamage), empty without one.
fn bomb_packet(w: &World, id: ActorId, last: bool) -> HitPacket {
    let p = actor_p(w, ActorRef::Actor(id));
    let k = if last { "LastHitDamage" } else { "TickHitDamage" };
    let none = mvs_data::value::Props::new();
    HitPacket::from_props(&w.names, obj(p, k).unwrap_or(&none), None)
}

/// TickDamage (`last` false) or EndExpantion's loop (`last` true) over the fighters of another team in the HitBox:
/// HitBox.GetOverlappingActors(BaseFighter_C), then the TeamIndex test. The HitBox is a SphereComponent with the
/// class defaults (OverlapAllDynamic, WorldDynamic): it ignores the HurtBox channel and overlaps the Pawn root
/// capsule, so the list is the fighters whose root capsule touches the sphere.
fn bomb_damage(w: &mut World, id: ActorId, last: bool) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let f = a.owner;
    let me = ActorRef::Actor(id);
    let Some((sx, sy, sr)) = bomb_mut(w, id).map(bomb_sphere) else { return };
    // a minion too (a BaseFighter_C)
    let all = w.pawns.as_deref().unwrap_or(&w.players);
    let list = crate::world::capsules_in_sphere(w, all, sx, sy, sr);
    let team = w.fighter(f).team.team;
    let packet = bomb_packet(w, id, last);
    for vic in list {
        if w.fighter(vic).team.team == team || no_hit(w, vic) {
            continue;
        }
        if projectile_blocks(w, &dealer(w, me), vic) {
            continue;
        }
        let Some(b) = bomb_of(w, id) else { return };
        let (bx, by, attack_id) = (b.x, b.y, b.attack_id);
        if invulnerable(w, vic) {
            let tick = w.flow.tick;
            attack_dodged(w, me, vic, attack_id, packet.base_knock_back.unwrap_or(0.0), tick, None, false);
            continue;
        }
        dodge_over(w, me, vic);
        let d = dealer(w, me);
        let cp = charge_pct(w, me);
        let at = V2 { x: bx, y: by };
        let ok = if last {
            apply_projectile_hit(w, &d, vic, &packet, &[], at, None, &[], cp)
        } else {
            let (dx, dy) = (bx - w.fighter(vic).body.x, by - actor_y(w, vic));
            let n = safe_normal([0.0, dx, dy], SMALL_NUMBER); // GetDirectionUnitVector
            let dir = V2 { x: n[1], y: n[2] };
            apply_projectile_hit(w, &d, vic, &packet, &[], at, Some(dir), &[], cp)
        };
        if ok {
            let d = dealer(w, me);
            actor_hit_enemy(w, &d, vic);
            // the HitEnemyPlayer delegate's
            sound_at_for(w, Some(f), name!("sfx_cha_C017_weaponmode_bomb_hit"), bx, by);
        }
    }
}

fn bomb_step(w: &mut World, id: ActorId) {
    let p = actor_p(w, ActorRef::Actor(id));
    let max = num(p, "MaxScale").unwrap_or(1.0);
    let expansion = num(p, "ExpansionTime").unwrap_or(0.0);
    let from = num(p, "HitStartScale").unwrap_or(1.0);
    let repeated = num(p, "RepeatedDamageTime").unwrap_or(0.0);
    let Some(a) = w.actors.actors.get(id) else { return };
    let f = a.owner;
    let Some(b) = bomb_mut(w, id) else { return };
    b.age += 1;
    // ReceiveTick (@4561).
    if b.exploding {
        b.expand_t += DELTA_SECONDS;
        let k = b.expand_t / expansion;
        b.scale = min(from + k * (max - from), max);
    }
    if let Some(mut m) = b.mv {
        let done = m.update(DELTA_SECONDS);
        b.x = m.x;
        b.y = m.y;
        b.mv = Some(m);
        if !done {
            return;
        }
        // @1009.
        b.mv = None;
        b.exploding = true;
        b.scale = from;
        b.t0 = b.age;
        b.next_tick = f64::from(repeated);
        let (bx, by) = (b.x, b.y);
        sound_at_for(w, Some(f), name!("sfx_cha_C017_weaponmode_bomb_impact_expanding"), bx, by);
        bomb_damage(w, id, false);
        return;
    }
    // The two timers, in ExpireTime order: the DamageTickTimer's calls whose expiry is before the DamageTimer's (or
    // every due one while that is not due), then EndExpantion, which clears the tick timer (@4287).
    let t = elapsed(b.age - b.t0);
    let (end, rate) = (expansion, repeated);
    let ended = t > f64::from(end);
    if rate > 0.0 && t > b.next_tick && !(ended && f64::from(end) < b.next_tick) {
        let calls = crate::math::dbl::to_int32(((t - b.next_tick) / f64::from(rate)).trunc()) + 1;
        for _ in 0..calls {
            bomb_damage(w, id, false);
        }
        let Some(b) = bomb_mut(w, id) else { return };
        b.next_tick += f64::from(crate::real(calls) * rate);
    }
    if ended {
        bomb_damage(w, id, true);
        let Some(a) = w.actors.actors.get_mut(id) else { return };
        a.dead = true;
        let Some(b) = bomb_of(w, id) else { return };
        let (bx, by) = (b.x, b.y);
        sound_at_for(w, Some(f), name!("sfx_cha_C017_weaponmode_bomb_despawn"), bx, by);
    }
}

/// Comp_C017_Ally_Turret @974: CompSpawnActor(C017_Ally_Turret_Bomb, not attached, socket None, world transform ((0,
/// the emblem's Y, Z), zero rotation, scale (ProjectileSpeed, 1, 1)), AttackID 0: the owner's attack count,
/// HandleActorPostSpawn @2444) at (x, y). The reticle's place is the turret's AimLocation while the reticle lives.
/// Without a live reticle the graphs leave PilotAimReticle (read @2898) in one of two states. After a kick it is null:
/// KickPilot destroys a valid reticle and calls SetReticleControl(null) (Comp_C017_Ally_Turret @5419-5551), which
/// stores the null with HasPilot still set (@9240, the kick in weapon mode before its Delay ends) or without it
/// (@9695). After the reticle's own arena exit it still names the destroyed actor: nothing clears it there, and a
/// later kick stops at IsValid (@5419-5448) before SetReticleControl.
// Unverified: K2_GetActorLocation through either reference is taken as the Blueprint VM's cleared return, so the target
// is the world origin (and the trace runs from the emblem to it). Not in the graphs: what EX_Context does with a null
// and with a PendingKill object (stock: both skip the call and clear the result). Read that decides: the exe's
// EX_Context handler (UObject::ProcessContextOpcode), or the bomb's 'Target Prediction Location' on the live actor
// after a kick inside the 0.3 s and after the reticle left the arena.
pub fn spawn_turret_bomb(w: &mut World, f: FighterId, x: Real, y: Real, speed: Real) -> Option<ActorId> {
    let cls = name!("C017_Ally_Turret_Bomb_C");
    let Some(d) = actor_def(w, ActorRef::Fighter(f), cls) else {
        warn_text(w, &format!("{TURRET_BOMB} not exported"));
        return None;
    };
    let none = mvs_data::value::Props::new();
    let hit = HitPacket::from_props(&w.names, obj(&d.p, "HitBoxDamage").unwrap_or(&none), None);
    let hb = obj(&d.components, "HitBox");
    let sphere_r = hb.and_then(|h| num(h, "SphereRadius")).unwrap_or(0.0)
        * hb.and_then(|h| obj(h, "RelativeScale3D")).and_then(|s| num(s, "X")).unwrap_or(1.0);
    let (tx, ty) = match giant(w, f).ally_turret.as_ref() {
        Some(t) if t.reticle => (t.aim_x, t.aim_y),
        _ => (0.0, 0.0),
    };
    let attack_id = w.fighter(f).decay.attack_count;
    let grounds = crate::world::stage_query::static_shapes(w, SOLID_NOPLATFORM, name!("C017_Ally_Turret_Bomb trace"));
    // The constructor.
    let mut t = -1.0;
    for g in &grounds.boxes {
        let k = trace_ground(g, x, y, tx, ty);
        if k >= 0.0 && (t < 0.0 || k < t) {
            t = k;
        }
    }
    for q in &grounds.polys {
        if let Some(h) = q.ray(x, y, tx, ty) {
            let k = crate::math::narrow(h.time);
            if t < 0.0 || k < t {
                t = k;
            }
        }
    }
    let (tx, ty) = if t < 0.0 { (tx, ty) } else { (x + (tx - x) * t, y + (ty - y) * t) };
    // Divide_FloatFloat: a zero divisor gives 0 (KismetMathLibrary; the class default speed is 4000).
    let travel = if speed != 0.0 { size2d([tx - x, ty - y]) / speed } else { 0.0 };
    let b = TurretBomb {
        hit,
        attack_id,
        x,
        y,
        tx,
        ty,
        travel,
        mv: ComponentMove::call(None, x, y, tx, ty, false, false, travel, 0),
        sphere_r,
        sph_x: x,
        sph_y: y,
        sph_r: sphere_r,
        ..TurretBomb::default()
    };
    let actor = new_world_actor(w, cls, WorldActorClass::C017AllyTurretBomb, f, false, WorldActorState::TurretBomb(b));
    let id = w.actors.actors.insert(actor);
    let charge = instigator_charge(w, f);
    comp_spawn_actor(w, id, charge, f);
    let (bx, by) = bomb_of(w, id).map_or((x, y), |b| (b.x, b.y));
    sound_at_for(w, Some(f), name!("sfx_cha_C017_weaponmode_bomb_cast"), bx, by);
    Some(id)
}

/// SetSizeScale's SetActorScale3D writes the Capsule (the root) whole: `scale`, which the arrival's and the tick's
/// Capsule.SetRelativeScale3D (@4349, @4874) write over again (the root has no parent: relative is world).
fn bomb_scale(w: &mut World, id: ActorId, k: Real, _old: Real) {
    if let Some(b) = bomb_mut(w, id) {
        b.scale = k;
    }
}

/// This file's registry entries (the two actor kinds).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let reticle =
        WorldActorHooks { step: Some(reticle_step), size: Some(SizeHooks::default()), ..WorldActorHooks::default() };
    r.actors.enter(
        PILOT_RETICLE,
        ActorEntry::new(ActorClass::World(WorldActorClass::C017PilotReticle), ActorHooks::World(reticle)),
    )?;
    let bomb = WorldActorHooks {
        step: Some(bomb_step),
        size: Some(SizeHooks { scale: Some(bomb_scale), ..SizeHooks::default() }),
        ..WorldActorHooks::default()
    };
    r.actors.enter(
        TURRET_BOMB,
        ActorEntry::new(ActorClass::World(WorldActorClass::C017AllyTurretBomb), ActorHooks::World(bomb)),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::names::Name;

    #[test]
    fn a_trace_from_inside_the_block_hits_at_its_start() {
        let g = Ground { left: -100.0, right: 100.0, bottom: -50.0, top: 0.0 };
        assert_eq!(trace_ground(&g, 0.0, -10.0, 0.0, 500.0), 0.0);
        assert_eq!(trace_ground(&g, 0.0, 100.0, 0.0, -100.0), 0.5);
        assert_eq!(trace_ground(&g, 200.0, 100.0, 200.0, -100.0), -1.0);
    }

    #[test]
    fn the_names_match_their_literals() {
        let _: Name = name!("C017_Pilot_Reticle_C");
        assert_eq!(PILOT_RETICLE, "C017_Pilot_Reticle_C");
        assert_eq!(TURRET_BOMB, "C017_Ally_Turret_Bomb_C");
    }
}
