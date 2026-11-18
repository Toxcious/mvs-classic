//! Batman's grapple hook GrappleHookProjectileV2_C, AnimNotify_GrappleMoveCommand_C and the pawn's grapple
//! rendezvous (BPI_AddGrappleMoveCommandInfo / Notify, BPI_EndGrappleMoveCommand).
//!
//! GrappleHookProjectileV2 (its ubergraph's offsets), a ParentProjectilePhysics child. Spawned by the
//! ConditionalSpawner of Batman_Special_F_Montage (socket R_Hand) and Batman_Special_Air_F_Montage (L_Hand) with
//! BatmanNoHeavyGrapple_C (the twin with BatmanHasHeavyGrapple_C spawns GrappleHookProjectile_Perk_C, the perk's
//! child: `perks_grapple`), AimOn (ProjectileAimer_C, AimType 3), InitialSpeed / MaxSpeed 8500. Capsule radius 44,
//! half height 60, object type WorldStaticDetection (ECC_GameTraceChannel15), which the stage block and the Platform,
//! Floor and BlockAll profiles block (BlockKind::Detect); MaxLifetime 0.275, Health 1, DestroyOnExitedArena? false,
//! a zero HitBoxDamage with HitEachActorOnlyOnce.
//! - ReceiveBeginPlay @857: sfx_cha_batman_grapple_shoot posted on the owner; the owner's DamageManager KnockedBack
//!   -> AfterOwnerKnockedBack @4170 (DestroySpawnedActor(false)); every BasePlatformBP with Moving has its Box
//!   ignored by the Capsule's moves (@31-454: IgnoreComponentWhenMoving(Box, true)); the owner's EvadeTriggered
//!   -> OnEvade @6045 (clear ChargeAnimationTimer, DestroySpawnedActor(false)) and OnIsJumping -> OnJump @6081.
//! - ReceiveTick @1537: the beam's source (GetGrappleOrigin: drawing); @755 on authority a valid
//!   QueuedAttachFighter -> OnGrappleHitServer(it, QueuedAttachAlly), every tick (the DoOnce makes the later calls
//!   nothing).
//! - HitEnemyPlayer @3972 (after the parent's hit) / HitAlly @4186: IsValidGrappleActor -> QueuedAttachFighter =
//!   the fighter, QueuedAttachAlly false / true.
//! - ReceiveHit @3243 (the Capsule's blocking hit): OnGrappleHitServer(null, false).
//! - OnGrappleHitServer @566: authority, a DoOnce; HitBoxOn? = false and UpdateHitbox,
//!   UpdateMaxLifetimeWithAuthority(0), DestroyOnBlocked = false, then OnGrappleHitMulticast @5062:
//!   ProjectileMovement.StopSimulating, sfx_cha_batman_grapple_attach, Attached = true; with a fighter
//!   sfx_cha_batman_grapple_attach_player, K2_AttachToActor(it, SnapToTarget) and AttachedFighter = it. Then @5492
//!   SpawnEmitterAttached(PS_BatmanGrappleGunImpact, the root, (10, 0, 0)); an ally reels at once
//!   (AttackAnimationMulticast), anything else after a timer of NonAllyReelDelay (0.25).
//! - AttackAnimationMulticast @3967 -> @3442: the owner's BPI_IncrementPlayerAttackCount(1),
//!   PlayAnimMontage(Batman_Special_F_Charge_Montage) @3345, IsReeling = true, then @459 a timer of 0.15 ->
//!   BeforeActivateGrapple @6030 -> ActivateObject.
//! - ActivateObject @1639: not Attached -> K2_DestroyActor @3228. Else the destination = the actor's location +
//!   (AttachedFighter valid ? (same team ? AllyTravelOffset : EnemyTravelOffset) : 0) x (actor Y > owner Y ? 1 : -1),
//!   Y and Z clamped to the blast box bounds less BlastBoxBuffer (1000); the time = FMin(GetDistanceTo(owner) /
//!   ReelTravelVelocity, 0.5); MulticastGrappleInfo @3816 -> the owner's BPI_AddGrappleMoveCommandInfo; then a timer
//!   of ChargeAnimationDuration (0.45) -> AfterGrappleCharge @4073: StopAnimation @3675 and a timer of 0.1 ->
//!   DelayedDestroy @15 (DestroySpawnedActor(false)).
//! - BPI_CanMarkFighter: !Attached.
//!
//! The owner's side (Pawn_BatmanV2):
//! - BPI_AddGrappleMoveCommandInfo @4735: GrappleDestination, GrappleTime, then @613 GrappleInfoRecieved = true and
//!   with a valid 'Grapple Move Notify' @663.
//! - BPI_AddGrappleMoveCommandNotify @1640 (the notify's NotifyBegin): the notify, then @1625 with
//!   GrappleInfoRecieved @663. Either order joins @663.
//! - @663: SkipImpulseValidation = true; turned to face the destination; BPI_AddMoveCommand(the notify,
//!   FMax(GrappleTime x GrappleMoveTimeMultiplier, 0.1), (|dY|, dZ) x GrappleDistanceMultiplier, XY and Z override,
//!   StopOnEnd); @389 K2_SetTimerDelegate(GrappleTime, once) -> GrappleMoveTimer.
//! - BPI_EndGrappleMoveCommand @497 (the notify's NotifyEnd and the timer): with a valid notify,
//!   BPI_CompleteMoveCommand(it), the timer cleared, GrappleInfoRecieved = false, the notify null,
//!   SkipImpulseValidation = false.
//!
//! Model. The hook is a projectile of the World's list. Its move is swept against the stage block and the
//! WorldStaticDetection blockers (`ProjectileHooks::receive_hit`), less the moving platforms BeginPlay found. On a
//! fighter it rides that fighter's actor location ([`ride`]). Its timers are counted on the actor's age and run in
//! its tick (the kind's `step`), the pawn's on the fighter's tick in the timer pass (`CharHooks::timer_tick`).
//! The queue of a fighter hit (HitEnemyPlayer / HitAlly) is read by the ReceiveTick behind the move it came in. A
//! fighter moving into the hook (his tick, `World::overlap_mid_frame`) and the test at the spawn place (age 0) come
//! ahead of the hook's move and its tick: queued, attached by the kind's step after that move. The hook's own move
//! overlaps inside its movement component's tick, ahead of ReceiveTick @755 of the same frame; the port tests that
//! overlap where the move ends, behind the kind's step, so that hit attaches at once (`attach`).
//! Deviation: inside one move the build's HitBox overlap runs before ReceiveHit, so an enemy overlapped in the move
//! that is blocked by a wall still takes the zero hit; the port runs ReceiveHit inside the projectile's step and
//! turns the hitbox off there, ahead of the end-of-move test: that hit is lost and the hook stays on the wall.
//! Deviation: the actor's timers (NonAllyReelDelay, 0.15, ChargeAnimationDuration, 0.1) fire in the actor's tick of
//! the frame they are due in, not in that frame's timer pass.
//! Deviation: the owner's KnockedBack, EvadeTriggered and OnIsJumping are taken at the hook's next tick, not inside
//! the broadcast: by the knocked-back count, the last evade time and the last jump time. EvadeFromAttack's broadcast
//! (BaseFighter @170285) writes no last evade time: an evade out of an attack does not destroy the hook here.
//! Unverified: StopAnimMontage(the charge montage) is ACharacter::StopAnimMontage of the stock 4.26 source (a
//! Montage_Stop with the montage's own BlendOut when it is the active one), not read in the exe.
//! Deviation: PlayAnimMontage is the port's start_montage, past the attack start: its attack count stands for
//! BPI_IncrementPlayerAttackCount.
//! Deviation: GetDistanceTo and the destination are taken in the play plane (the hook's depth is not kept).
//! Unverified: DestroySpawnedActor(false) is K2_DestroyActor: the port's projectiles take no hit pause and the
//! class's CanBePooled was not read.

use super::state::{batman, batman_of, batman_w};
use crate::Real;
use crate::State;
use crate::actors::projectile::{
    Projectile, ProjectileExt, StepEnd, clear_max_lifetime, set_hitbox, stop_simulating, yaw_x,
};
use crate::actors::{ActorClass, ProjectileClass};
use crate::actors::{ActorRef, ProjectileId};
use crate::combat::boxes::WorldSphere;
use crate::combat::hit::timer_ticks;
use crate::combat::{hits, phases};
use crate::events::{Event, NotifyRef};
use crate::fighter::FighterId;
use crate::fighter::anim::montages::{notify_data, start_montage, stop_montage};
use crate::fighter::match_side::SpawnRequest;
use crate::fighter::moves::{MoveCtl, MoveEnd, V2};
use crate::fighter::moves::{add_move_cmd, complete_remove_move};
use crate::math::{self};
use crate::names::Name;
use crate::registry::entries::{ActorEntry, ActorHooks, NotifyHandler, ProjectileHooks, ProjectileReceiveHit};
use crate::registry::props::{num, num_in};
use crate::registry::{CharHooks, Registry, RegistryError};
use crate::world::World;
use crate::world::spawning::destroy_projectile;
use crate::{fighter, name};
use indexmap::IndexSet;
use mvs_anim::montage::{NotifyTick, PROXY_NONE};
use mvs_data::value::Props;

/// GrappleHookProjectileV2_C.
pub const GRAPPLE: &str = "GrappleHookProjectileV2_C";
/// Batman_Special_F_Charge_Montage.
pub const CHARGE: Name = name!("Batman_Special_F_Charge_Montage");

/// `Grapple`: what the class adds to the projectile (also the base of GrappleHookProjectile_Perk_C's,
/// `perks_grapple::PerkGrapple`).
#[derive(State, Debug)]
pub struct Grapple {
    /// QueuedAttachFighter / QueuedAttachAlly.
    pub queued: Option<FighterId>,
    pub queued_ally: bool,
    /// OnGrappleHitServer's DoOnce.
    pub closed: bool,
    /// Attached.
    pub stuck: bool,
    /// AttachedFighter.
    pub target: Option<FighterId>,
    /// IsReeling (GetGrappleOrigin's Prop1 socket: the drawing).
    pub reeling: bool,
    /// The ages its timers fire at (-1: none): NonAllyReelDelay -> AttackAnimationMulticast, 0.15 ->
    /// ActivateObject, ChargeAnimationTimer -> AfterGrappleCharge, 0.1 -> DelayedDestroy.
    pub reel_at: Real,
    pub activate_at: Real,
    pub charge_end_at: Real,
    pub destroy_at: Real,
    /// What BeginPlay's bindings compare against (the delegates' stand-ins, see the module's Model).
    pub kb_at: i32,
    pub evade_at: Real,
    pub jump_at: Real,
    /// The stage platforms whose Box the Capsule ignores (BasePlatformBP with Moving), by stage index.
    pub ignored: Vec<i32>,
    /// The team mates its hit box is over, by fighter id (the begin of the overlap is the one pfgDoDamage call).
    pub ally_in: IndexSet<i32>,
}

impl Default for Grapple {
    /// The field initial values.
    fn default() -> Self {
        Grapple {
            queued: None,
            queued_ally: false,
            closed: false,
            stuck: false,
            target: None,
            reeling: false,
            reel_at: -1.0,
            activate_at: -1.0,
            charge_end_at: -1.0,
            destroy_at: -1.0,
            kb_at: 0,
            evade_at: 0.0,
            jump_at: 0.0,
            ignored: Vec::new(),
            ally_in: IndexSet::new(),
        }
    }
}

/// The hook's `Grapple` part, of either class.
pub fn grapple_of(p: &Projectile) -> Option<&Grapple> {
    match &p.ext {
        ProjectileExt::Grapple(g) => Some(g),
        ProjectileExt::PerkGrapple(g) => Some(&g.g),
        _ => None,
    }
}

fn grapple_of_mut(p: &mut Projectile) -> Option<&mut Grapple> {
    match &mut p.ext {
        ProjectileExt::Grapple(g) => Some(g),
        ProjectileExt::PerkGrapple(g) => Some(&mut g.g),
        _ => None,
    }
}

pub(super) fn pr_ref(w: &World, pr: ProjectileId) -> &Projectile {
    w.actors.projectiles.get(pr).expect("a live projectile")
}

pub(super) fn pr_mut(w: &mut World, pr: ProjectileId) -> &mut Projectile {
    w.actors.projectiles.get_mut(pr).expect("a live projectile")
}

fn g(w: &World, pr: ProjectileId) -> &Grapple {
    grapple_of(pr_ref(w, pr)).expect("a grapple hook")
}

pub(super) fn g_mut(w: &mut World, pr: ProjectileId) -> &mut Grapple {
    grapple_of_mut(pr_mut(w, pr)).expect("a grapple hook")
}

pub(super) fn owner(w: &World, pr: ProjectileId) -> FighterId {
    pr_ref(w, pr).owner.expect("a grapple hook has an owner")
}

pub(super) fn class_p(w: &World, pr: ProjectileId) -> &Props {
    crate::actors::projectile::class_p(w, pr_ref(w, pr))
}

/// `Grapple.step`'s override, ahead of the projectile's own step: on a fighter the movement is stopped and the actor
/// rides its parent's location (SnapToTarget); nothing of the parent's tick is left but the age (no MaxLifetime from
/// the hit on, DestroyOnExitedArena? false). `Some`: the step is done (`ProjectileHooks::step_override`).
fn ride(w: &mut World, pr: ProjectileId) -> Option<Option<StepEnd>> {
    let p = pr_ref(w, pr);
    let t = grapple_of(p).and_then(|g| g.target)?;
    if p.dead {
        return None;
    }
    let (tx, ty) = (w.fighter(t).body.x, fighter::actor_y(w, t));
    let p = pr_mut(w, pr);
    p.prev_x = p.x;
    p.prev_y = p.y;
    p.age += 1;
    p.x = tx;
    p.y = ty;
    Some(None)
}

fn sound(w: &mut World, pr: ProjectileId, ev: Name) {
    let p = pr_ref(w, pr);
    let e = Event::Sound { fighter: p.owner_id, ev, x: Some(p.x), y: Some(p.y), hit: None, pooled: false, key: None };
    w.events.push(e);
}

/// IsValidGrappleActor: a BPI_BaseFighter that is not dead and not the owner, with the hook on the side the owner
/// faces (FacingLeft: the hook's UE Y >= his; else <=). UE Y = -x; FacingLeft is facing < 0.
pub(super) fn valid_target(w: &World, pr: ProjectileId, vic: FighterId) -> bool {
    let f = owner(w, pr);
    if vic == f || w.fighter(vic).life.dead > 0.0 {
        return false;
    }
    let (x, fx) = (pr_ref(w, pr).x, w.fighter(f).body.x);
    if fighter::facing(w, f) < 0.0 { x <= fx } else { x >= fx }
}

/// QueuedAttachFighter / QueuedAttachAlly written by a fighter hit, and ReceiveTick @755 reading them (see the
/// module's Model): a hit of the hook's own move (the end-of-move test, after the kind's step) is attached at once.
fn attach(w: &mut World, pr: ProjectileId, vic: FighterId, ally: bool) {
    let g = g_mut(w, pr);
    g.queued = Some(vic);
    g.queued_ally = ally;
    if !w.overlap_mid_frame && pr_ref(w, pr).age > 0 {
        hit_server(w, pr, Some(vic), ally);
    }
}

/// OnGrappleHitServer @566 and OnGrappleHitMulticast @5062.
fn hit_server(w: &mut World, pr: ProjectileId, o: Option<FighterId>, ally: bool) {
    if g(w, pr).closed {
        return;
    }
    g_mut(w, pr).closed = true;
    {
        let p = pr_mut(w, pr);
        set_hitbox(p, false);
        clear_max_lifetime(p);
        // DestroyOnBlocked = false: the kind's `blocked` reads `closed`.
        stop_simulating(p);
    }
    sound(w, pr, name!("sfx_cha_batman_grapple_attach"));
    g_mut(w, pr).stuck = true;
    if let Some(o) = o {
        sound(w, pr, name!("sfx_cha_batman_grapple_attach_player"));
        g_mut(w, pr).target = Some(o);
        let (ox, oy) = (w.fighter(o).body.x, fighter::actor_y(w, o));
        let p = pr_mut(w, pr);
        p.x = ox;
        p.y = oy;
    }
    // @5492: SpawnEmitterAttached(PS_BatmanGrappleGunImpact) at (10, 0, 0) in the root's frame (its X: the rotation
    // the hook flew with). Sent as a projectileDeath event; the render attaches it to the hook.
    let p = pr_ref(w, pr);
    let yx = yaw_x(p);
    let [fx, fy] = math::ue::turn_by_pitch(p.pitch, [1.0, 0.0]);
    let fx = yx * fx;
    let e = Event::ProjectileDeath {
        cls: p.cls,
        fighter: p.owner_id,
        vfx: Some(name!("PS_BatmanGrappleGunImpact")),
        x: p.x + 10.0 * fx,
        y: p.y + 10.0 * fy,
        ox: 0.0,
        rot: [p.pitch, if yx > 0.0 { -90.0 } else { 90.0 }, 0.0],
        scale: 1.0,
        tint: None,
        to: None,
    };
    w.events.push(e);
    if ally {
        attack_animation(w, pr);
    } else {
        let t = timer_ticks(Some(num(class_p(w, pr), "NonAllyReelDelay").unwrap_or(0.25)));
        let at = crate::real(pr_ref(w, pr).age) + t;
        g_mut(w, pr).reel_at = at;
    }
}

/// AttackAnimationMulticast @3442.
fn attack_animation(w: &mut World, pr: ProjectileId) {
    let f = owner(w, pr);
    start_montage(w, f, CHARGE, &PROXY_NONE, None);
    w.fighter_mut(f).charge.begin_attack = false;
    let at = crate::real(pr_ref(w, pr).age) + timer_ticks(Some(0.15));
    let g = g_mut(w, pr);
    g.reeling = true;
    g.activate_at = at;
}

/// ActivateObject @1639. True: K2_DestroyActor.
fn activate_object(w: &mut World, pr: ProjectileId) -> bool {
    if !g(w, pr).stuck {
        return true;
    }
    let f = owner(w, pr);
    let t = g(w, pr).target;
    let cp = class_p(w, pr);
    let p = pr_ref(w, pr);
    let b = p.blast_bounds;
    let buf = num(cp, "BlastBoxBuffer").unwrap_or(1000.0);
    let off = match t {
        Some(t) => {
            let k =
                if w.fighter(t).team.team == w.fighter(f).team.team { "AllyTravelOffset" } else { "EnemyTravelOffset" };
            num_in(cp, k, "Y").unwrap_or(0.0)
        }
        None => 0.0,
    };
    let (px, py) = (p.x, p.y);
    let (fx, fy) = (w.fighter(f).body.x, fighter::actor_y(w, f));
    let sign = if -px > -fx { 1.0 } else { -1.0 }; // the actor's UE Y > the owner's
    // The bounds its BeginPlay stored (@1929 / @2013 BlastBoxBoundsMax / Min). UE Y = -x: Y + off x sign clamped to
    // [min Y + buf, max Y - buf] is x - off x sign in [left + buf, right - buf].
    let clamp = |v: Real, lo: Real, hi: Real| {
        if v < lo {
            lo
        } else if v < hi {
            v
        } else {
            hi
        }
    };
    let x = clamp(px - off * sign, b.left + buf, b.right - buf);
    let y = clamp(py, b.bottom + buf, b.top - buf);
    let dist = math::ue::size2d([px - fx, py - fy]);
    let vel = num(cp, "ReelTravelVelocity").unwrap_or(5000.0);
    let time = math::min(dist / vel, 0.5);
    let charge = timer_ticks(Some(num(cp, "ChargeAnimationDuration").unwrap_or(0.45)));
    // GrappleDestination is an FVector: float32. MulticastGrappleInfo @3816: the BPI_Batman cast.
    if batman_of(w, f).is_some() {
        grapple_info(w, f, x, y, time);
    }
    let at = crate::real(pr_ref(w, pr).age) + charge;
    g_mut(w, pr).charge_end_at = at;
    false
}

fn make(_w: &World, _f: FighterId, _r: &SpawnRequest) -> ProjectileExt {
    ProjectileExt::Grapple(Grapple::default())
}

/// ReceiveBeginPlay @857 (see the module's header).
pub(super) fn begin(w: &mut World, pr: ProjectileId) {
    let f = owner(w, pr);
    let fi = w.fighter(f);
    let (id, kb, ev, jp) = (fi.id, fi.attackers.knocked_back_count, fi.evade.last_evade_time, fi.jump.last_jump_time);
    // posted on the owner
    let e = Event::Sound {
        fighter: id,
        ev: name!("sfx_cha_batman_grapple_shoot"),
        x: None,
        y: None,
        hit: None,
        pooled: false,
        key: None,
    };
    w.events.push(e);
    let ignored = w.stage.stage_platforms.iter().filter(|p| p.moving == Some(true)).map(|p| p.stage_index).collect();
    let g = g_mut(w, pr);
    g.kb_at = kb;
    g.evade_at = ev;
    g.jump_at = jp;
    g.ignored = ignored;
}

/// `receiveHit.boxes`: none once stopped or on a fighter; else World.blockers('detect') less the ignored stage
/// platforms, with the polygon solids that block it.
fn boxes(w: &mut World, pr: ProjectileId, out: &mut crate::world::stage_query::StageShapes) {
    out.boxes.clear();
    out.polys.clear();
    let p = pr_ref(w, pr);
    let g = grapple_of(p).expect("a grapple hook");
    if p.stopped || g.target.is_some() {
        return;
    }
    let ignored = g.ignored.clone();
    w.blocker_shapes_into(crate::world::spawned_platform::BlockKind::Detect, out);
    if !ignored.is_empty() {
        // The list begins with the stage's platforms, in their order.
        let drop: Vec<bool> = w.stage.stage_platforms.iter().map(|p| ignored.contains(&p.stage_index)).collect();
        let mut i = 0;
        out.boxes.retain(|_| {
            let keep = !drop.get(i).copied().unwrap_or(false);
            i += 1;
            keep
        });
    }
}

/// ReceiveHit @3243. OnGrappleHitServer stops the movement, so HandleImpact does not run after it.
fn receive_hit(w: &mut World, pr: ProjectileId, _nx: Real, _ny: Real, _t: Real, _pen: bool) -> bool {
    hit_server(w, pr, None, false);
    false
}

/// `step`: the kit's ReceiveTick part and its timers. True: destroyed.
pub(super) fn step(w: &mut World, pr: ProjectileId) -> bool {
    let f = owner(w, pr);
    // AfterOwnerKnockedBack @4170, OnEvade @6045, OnJump @6081 (see the module's Deviation).
    if owner_left(w, pr) {
        return true;
    }
    // ReceiveTick @755.
    let (queued, ally) = (g(w, pr).queued, g(w, pr).queued_ally);
    if let Some(q) = queued {
        hit_server(w, pr, Some(q), ally);
    }
    let age = crate::real(pr_ref(w, pr).age);
    if age == g(w, pr).reel_at {
        attack_animation(w, pr);
    }
    if age == g(w, pr).activate_at && activate_object(w, pr) {
        return true;
    }
    if age == g(w, pr).charge_end_at {
        // AfterGrappleCharge @4073: StopAnimation @3675, then the 0.1 timer.
        if w.fighter(f).montages.montage.as_ref().is_some_and(|m| m.montage.name == CHARGE) {
            stop_montage(w, f, None, None);
        }
        g_mut(w, pr).destroy_at = age + timer_ticks(Some(0.1));
    }
    age == g(w, pr).destroy_at
}

/// The kit's step's tests of the owner's KnockedBack, EvadeTriggered and OnIsJumping, against the values its
/// BeginPlay kept: any since the spawn destroys the hook at its next tick.
pub(super) fn owner_left(w: &World, pr: ProjectileId) -> bool {
    let f = w.fighter(owner(w, pr));
    let g = g(w, pr);
    f.attackers.knocked_back_count != g.kb_at
        || f.evade.last_evade_time > g.evade_at
        || f.jump.last_jump_time > g.jump_at
}

/// HitAlly @4186 behind NotifyAllyOfHit (the begin of the hit box's overlap with a team mate).
fn overlap(w: &mut World, pr: ProjectileId, vic: FighterId) {
    let f = owner(w, pr);
    if vic == f || w.fighter(vic).team.team != w.fighter(f).team.team {
        return;
    }
    let vid = w.fighter(vic).id;
    let p = pr_ref(w, pr);
    let sp = [WorldSphere {
        x: p.x,
        y: p.y,
        z: if p.sphere_3d { Some(0.0) } else { None },
        r: p.hit_radius,
        ..WorldSphere::default()
    }];
    let mut on = phases::hitbox_active(w, pr) && !p.dead && w.fighter(vic).life.dead <= 0.0 && !phases::no_hit(w, vic);
    if on {
        on = match phases::victim_pose(w, vic) {
            Some(vp) => {
                let met = crate::combat::hit::hurt::hurt_hit(w, vic, &sp, &vp).is_some();
                phases::give_vbuf(w, vp);
                met
            }
            None => false,
        };
    }
    if !on {
        g_mut(w, pr).ally_in.shift_remove(&vid);
        return;
    }
    if !g_mut(w, pr).ally_in.insert(vid) {
        return;
    }
    let d = hits::dealer(w, ActorRef::Projectile(pr));
    if pr_ref(w, pr).hit_set.contains(&vid) || hits::projectile_blocks(w, &d, vic) || fighter::invulnerable(w, vic) {
        return;
    }
    if pr_ref(w, pr).hit.hit_each_actor_only_once.unwrap_or(false) {
        pr_mut(w, pr).hit_set.insert(vid);
    }
    let hp = pr_ref(w, pr).hit.clone();
    hits::ally_hit(w, &d, vic, &hp);
    if valid_target(w, pr, vic) {
        attach(w, pr, vic, true);
    }
}

/// HitEnemyPlayer @3972, after the parent's.
fn hit_enemy(w: &mut World, pr: ProjectileId, vic: FighterId) {
    if valid_target(w, pr, vic) {
        attach(w, pr, vic, false);
    }
}

/// BPI_CanMarkFighter: !Attached.
fn can_mark(w: &World, pr: ProjectileId) -> bool {
    !g(w, pr).stuck
}

/// SpawnedActorParent Blocked with the instance's DestroyOnBlocked (OnGrappleHitServer writes it false).
fn blocked(w: &mut World, pr: ProjectileId, _by: ActorRef) {
    if !g(w, pr).closed {
        pr_mut(w, pr).blocked_destroy = true;
    }
}

fn activate(w: &mut World, pr: ProjectileId) {
    if activate_object(w, pr) {
        destroy_projectile(w, pr, false);
    }
}

/// The kind of GrappleHookProjectileV2_C, also the base of GrappleHookProjectile_Perk_C's (a child with no override
/// of these).
pub(super) fn kind() -> ProjectileHooks {
    ProjectileHooks {
        make: Some(make),
        begin: Some(begin),
        receive_hit: Some(ProjectileReceiveHit { boxes, hit: receive_hit }),
        step: Some(step),
        step_override: Some(ride),
        overlap: Some(overlap),
        hit_enemy: Some(hit_enemy),
        can_mark: Some(can_mark),
        blocked: Some(blocked),
        activate: Some(activate),
        ..ProjectileHooks::default()
    }
}

// ---- the pawn's rendezvous

/// BPI_AddGrappleMoveCommandInfo @4735 -> @613.
pub fn grapple_info(w: &mut World, f: FighterId, x: Real, y: Real, time: Real) {
    let s = batman_w(w, f);
    s.grapple_dest_x = x;
    s.grapple_dest_y = y;
    s.grapple_time = time;
    s.grapple_info = true;
    if s.grapple_notify >= 0.0 {
        grapple_go(w, f);
    }
}

/// BPI_AddGrappleMoveCommandNotify @1640 -> @1625.
fn grapple_notify(w: &mut World, f: FighterId, idx: i32) {
    let s = batman_w(w, f);
    s.grapple_notify = crate::real(idx);
    if s.grapple_info {
        grapple_go(w, f);
    }
}

/// @663-1620, then @389.
fn grapple_go(w: &mut World, f: FighterId) {
    batman_w(w, f).skip_impulse_validation = true;
    let s = batman(w, f);
    let (dest_x, dest_y, time, notify) = (s.grapple_dest_x, s.grapple_dest_y, s.grapple_time, s.grapple_notify);
    let fd = fighter::facing(w, f);
    let left = fd < 0.0;
    let less = dest_x > w.fighter(f).body.x; // dest UE Y < his
    if (!left && !less) || (left && less) {
        fighter::set_facing(w, f, -fd); // SetRotationLogic(!FacingLeft)
    }
    let k = fighter::control::pawn_prop(w, f, "GrappleDistanceMultiplier").unwrap_or(1.0);
    let dy = (w.fighter(f).body.x - dest_x).abs();
    let dz = dest_y - fighter::actor_y(w, f);
    let tm = fighter::control::pawn_prop(w, f, "GrappleMoveTimeMultiplier").unwrap_or(1.0);
    let dur = math::max(time * tm, 0.1);
    let end = MoveEnd { stop: true, clamp: false, min: V2::default(), max: V2::default() };
    let mv = V2 { x: (dy * k), y: (dz * k) };
    add_move_cmd(w, f, math::to_int32(notify), dur, mv, true, true, &end, false, false, &MoveCtl::default());
    // K2_SetTimerDelegate(GrappleTime, once): a running timer restarts; a time of 0 sets none (and clears it).
    let n = timer_ticks(Some(time));
    let tick = crate::real(w.fighter(f).clock.tick);
    batman_w(w, f).grapple_timer_at = if n == Real::INFINITY { -1.0 } else { tick + n };
}

/// BPI_EndGrappleMoveCommand @497.
pub fn grapple_end(w: &mut World, f: FighterId) {
    let notify = batman(w, f).grapple_notify;
    if notify < 0.0 {
        return;
    }
    complete_remove_move(w, f, math::to_int32(notify));
    let s = batman_w(w, f);
    s.grapple_timer_at = -1.0;
    s.grapple_info = false;
    s.grapple_notify = -1.0;
    s.skip_impulse_validation = false;
}

/// AnimNotify_GrappleMoveCommand: the owner's BPI_Batman cast, then BPI_AddGrappleMoveCommandNotify(self).
fn notify_begin(w: &mut World, f: FighterId, _n: NotifyRef, idx: i32) {
    if batman_of(w, f).is_some() {
        grapple_notify(w, f, idx);
    }
}

/// AnimNotify_GrappleMoveCommand's end: BPI_EndGrappleMoveCommand. The state is queued in the charge montage: after
/// a stop made behind the frame's dispatch its end waits for the next dispatch (as AnimNotify_MoveCommand's end).
fn notify_end(w: &mut World, f: FighterId, n: NotifyRef, idx: i32) {
    if batman_of(w, f).is_none() {
        return;
    }
    if w.fighter(f).branching.defer_queued_ends {
        let data = w.data.clone();
        if notify_data(w, &data, f, n).tick != NotifyTick::BranchingPoint {
            w.fighter_mut(f).branching.pending_ends.push((n, idx));
            return;
        }
    }
    grapple_end(w, f);
}

/// GrappleMoveTimer -> BPI_EndGrappleMoveCommand, in the frame's timer pass (set in frame N, it fires in the pass
/// of frame N + timer_ticks(GrappleTime)).
fn timer_tick(w: &mut World, f: FighterId) {
    let at = batman(w, f).grapple_timer_at;
    if at >= 0.0 && crate::real(w.fighter(f).clock.tick) >= at {
        grapple_end(w, f);
    }
}

/// The hooks of this file in Batman's CharHooks.
pub fn hooks(h: &mut CharHooks) {
    h.timer_tick = Some(timer_tick);
}

/// This file's registry entries: the hook and the notify.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.actors.enter(
        GRAPPLE,
        ActorEntry::new(
            ActorClass::Projectile(ProjectileClass::GrappleHookProjectileV2),
            ActorHooks::Projectile(kind()),
        ),
    )?;
    r.notifies.enter(
        "AnimNotify_GrappleMoveCommand_C",
        NotifyHandler { begin: Some(notify_begin), end: Some(notify_end), ..NotifyHandler::default() },
    )?;
    Ok(())
}
