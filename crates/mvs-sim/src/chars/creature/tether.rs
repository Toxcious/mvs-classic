//! Reindog's tether: TetherProjectile_C and SuperSaveTetherProjectile_C
//! ([`TetherProjectile`], a projectile's `ext`): the throw, the attach to an ally, the pull and its end, the break
//! warning, the ribbon and the beam's damage.
//!
//! `TetherProjectile`'s overrides are called from the shared code: `hitboxActive`, `hitCollision`,
//! `canClank` (all false: `actors::projectile`), `writeHoming` ([`write_homing`]), `homeAccel` ([`home_accel`]),
//! `step` while on an ally ([`ride`]); its getters `isAttached` / `attachedTo` through `actors::world_actor`
//! (`ally`) and `attachedToFighter` through `state::get_tether_info`. The pull's end (`pullEnds`) is
//! [`pull_end`], called by `combat::holds`.
//!
//! Thrown by Creature_Tether_Throw / Air_Throw's AnimNotify_ConditionalSpawner (Condition_IsNotBuddyReindog, Socket
//! head, 13000) and activated by Tether_Pull / Air_Pull's AnimNotify_ActivateSpawned. Ability slot 0 (Tether,
//! cooldown 24) is the throws' AnimNotify_AbilityTrigger (the generic one): the ground throw's trigger (0.0679) comes
//! ahead of its spawner (0.0685), the air throw's (0.0697) behind it; nothing of the tether reads the slot.
//!
//! The class's graph, by entry:
//! - ReceiveBeginPlay @14039: BPI_SetTether(self) on the Instigator @14148 (the pawn's cache, `state::set_tether`),
//!   with GetAllies > 0 CheckForHomingTarget @6907 (every IsValid ally that is a BP_FighterCharacter becomes
//!   HomingTargetComponent = its root; the loop's break is never set, so the last ally wins), the EndPoint
//!   TargetPoint @11878, the owner's HitStunStart / HitStunEnded bound to HitStunEffectOn / Off @12148 / @12234 (the
//!   beam's tint: cosmetic, the render's), Delay(0.225) @12321 -> @4989, Delay(3.0) @12376 -> @4905, the Cylinder's
//!   world rotation kept as OriginalRotation @11423, AttachBeam(self) and PS_TetherPoints on the owner's mesh at Gem
//!   @9915, sfx_cha_reindog_tether_cast on the owner @10052, sfx_cha_reindog_tether_loop on self @10125 (AK RayLoop),
//!   the team colour.
//! - @4989 (0.225 s): a DoOnce, SetVelocityInLocalSpace(0) @5060 and, unless Attached, SpawnAttachPoint,
//!   AttachedToAlly = false, DestroyOnBlocked = false. Blocked (14543) enters at @5060, behind the DoOnce.
//! - @4905 (3 s): AttachedToAlly ? Delay(5.0) -> @1412 : K2_DestroyActor. @1412: TetherBreakWarning = true,
//!   Timeline_0 (the beam's alpha), Delay(1.0) -> @1483: Activated ? nothing : K2_DestroyActor.
//! - The Sphere's (PlayerAttachCollision, 50 x scale 8 = 400) begin overlap -> @16: !AttachedToAlly &&
//!   IsOverlappingActor(other), else Delay(0.05) -> @16 again with the event's OtherActor as it then stands (@1338: a
//!   loop while the test fails); then other a BaseFighter, the owner one, the same TeamIndex, other != owner,
//!   !other.NotPlayerCharacter (else the flow ends); unless Attached SpawnAttachPoint first (@1393);
//!   K2_AttachToActor(other, None, SnapToTarget x 3, weld) @653, DestroyOnBlocked = false, AttachedToAlly = true,
//!   MulticastAttachedToAlly (AttachedToFighter = true @15463), AttachedAlly = the attach parent, collision off on
//!   PlayerAttachCollision / Capsule / StaticMesh / DamageBox / HitBox, AttachFXMulticast @12922 (overlap events off,
//!   sfx_cha_reindog_tether_attach on its Ak component @13277, PS_CreatureTetherAlly on the root @13375,
//!   PS_TetherProjectile destroyed @13548), the ally's OnExitedArena and OnDestroyed bound to the actor's end.
//! - SpawnAttachPoint: StopSimulating, Attached = true, PS_TetherPoints on the root at (10, 0, 0),
//!   AttachBeam(EndPoint).
//! - ReceiveTick (PrimaryActorTick.TickInterval 0.033) @12534: the beam's source points at the owner's Gem; unless
//!   Attached nothing more @11863; the EndPoint at self; the Cylinder's world transform: (Gem + self) / 2, scale
//!   (0.5, 0.5, VSize(Gem - self) / 100), ComposeRotators(OriginalRotation, (Roll -SignOfFloat(yaw) x pitch)) with
//!   GetYawPitchFromVector(Gem - self) @8348; TetherSlope / TetherB (the line in the (Y, Z) plane) @8887; then each of
//!   Cylinder.GetOverlappingActors(BaseFighter) less the owner and the attach parent @9529 that IsVulnerableTo the
//!   owner's TeamIndex @5658: ApplyDamage(it, DeltaSeconds x DamagePerSecond, owner's controller, self) @5876,
//!   sfx_cha_reindog_tether_hit at it @6013, and with HitVFXEnabled PS_HitImpact_TEMPLATE at (its X, its Y, the
//!   line's Z at its Y) + (55, 0, 0), world scale 0.625, Color = BFGetDamageColor(it) @6413-6609. After the loop
//!   @6742: HitVFXEnabled && VFXTriggered -> both false, K2_SetTimerDelegate(ResetVFXEnabled, HitVFXWindow, not
//!   looping) @6839 (-> HitVFXEnabled = true @14907).
//! - ActivateObject @13945 (BPI_ActivateObject): only once (Activated); without AttachedToAlly K2_DestroyActor; else
//!   Activated = true @14008 -> @11551: the AttachedAlly and owner casts, the same TeamIndex @10826 (else destroy),
//!   RemoveBuff(None, 11 = Held) @10966 on the ally (every buff of the Held group destroyed; with this data
//!   BreakStates is Held alone and BreakBuffs empty, so BreakBuffStates below finds nothing and the Delay(0.033) is
//!   never taken), T = FMin(GetDistanceTo x 0.0004, 0.4) @11139, PullAudioMulticast(ally, T) @11181
//!   (sfx_cha_reindog_tether_grab at self @13624, PS_CreatureTetherRibbon on the ally's mesh @13710, Delay(T) @13886
//!   -> the ribbon destroyed), BreakBuffStates(BreakStates, BreakBuffs) on the ally @11213, broadcast ally.IsPickedUp
//!   @11298, DestroyedBuffs ? Delay(0.033) @11353 : on -> @1546: the owner's CheckHasBuff(BigBrother) -> @1759-2511
//!   (below); else @2512 the owner's CheckHasBuff(WreckingBall) -> the ball on the ally @2731 (the owner's
//!   Comp_Spawner, `perks_ball`) and T x WreckingBallPullTimeMultiplier (2) @3835 / @4556;
//!   SpawnBuffActor(Buff_StunnedMoveToPositionThroughGeo) @3050, StartMoveServer @4313, Delay(T) @4611 -> @4742:
//!   DestroySpawnedActors(Creature_WreckingBall_C) (with or without the perk), the ally's StopAnimMontage(null) @4812
//!   and LaunchCharacter((0, 0, 200), true, true) @4849, K2_DestroyActor @2497.
//!   BigBrother @1759: the same mover on the ally toward the instigator's plain location (no raise, T unmultiplied)
//!   @2153, Delay(T) @2350 -> @2409: the OWNER's StopAnimMontage(null) and LaunchCharacter((0, 0, 200)) @2446 (the
//!   ally is not launched), K2_DestroyActor @2497; no tutorial event, no ball, no DestroySpawnedActors.
//! - ReceiveDestroyed @14924: sfx_cha_reindog_tether_end at the location, ExecuteActionOnPlayingID(Stop, AK RayLoop,
//!   500 ms, curve 4 = Linear) @14852, the EndPoint, TetherSourceVFX and ConnectedVFX destroyed. ReceiveEndPlay
//!   @15589 (and ReleaseToPool; the port pools nothing): BPI_SetTether(null) on the Instigator, with no identity
//!   test: the end of any tether clears a newer one.
//! - BPI_ProcessTutorialEvent(UniqueCharacterMechanic0) @1298 (the attach) and (UniqueCharacterMechanic1) @3011 (the
//!   plain pull). Left out: BPI_IncrementCoopActions @11786 (the co-op statistics, read by nothing of a match).
//! - Dormant: the DamageBox's ComponentHit @14770 (DamageBox is NoCollision), ReceiveAnyDamage (@10730, a pop_flow),
//!   ExitBlastBoxWarningMulticast (no caller), SwapPosition / DisableProjectile (no caller), the line trace 'Tether'
//!   @7560 (its result is not read).
//!
//! Model. The tether is a projectile of the world's list; on an ally it rides that fighter's actor location
//! (SnapToTarget), as Batman's grapple does. Its HitBox (untagged) and HitBox2D / HurtBox (NoCollision) leave it no
//! hit box and no hurt shape. Its move is swept against the stage block (`receive_hit`; the Capsule ignores the
//! Platform channel), whose hit stops it (HandleImpact: StopSimulating). The spawn place is FindTeleportSpot's (the
//! Capsule's 22 / 44, the stage block alone). Its Delays and its timer count on the actor's age in its kind's step
//! (the projectile pass): one the graph creates in its BeginPlay, its ActivateObject or an overlap event counts the
//! frame it is made in, one made in a Delay's link counts from the next frame.
// Unverified: ReceiveTick's TickInterval 0.033 at the fixed step: every 2nd frame from the spawn frame, the first
// tick's DeltaSeconds the world's (0.0166668), every later one 2 x 0.0166668; the cooldown's carry-over of the
// overshoot (one 1-frame interval in about 50) is not read in the exe.
// Deviation: the Delays are taken in the projectile pass every frame; on a frame without the actor's tick the build
// takes them in the world's leftover latent pass, behind every tick group.
// Deviation: the PlayerAttachCollision's begin overlaps are tested once a frame behind the tether's move (and at its
// spawn, behind BeginPlay), against the fighters' root capsules alone, a fighter waiting for its respawn left out;
// the build dispatches them inside whichever move made them.
// Deviation: the Cylinder (the engine's Cylinder mesh, radius 50, height 100, scaled) is a rectangle in the play
// plane: half width 25, from the Gem to the tether, the Gem's depth taken as 0 and the mesh's convex collision as the
// exact cylinder; it overlaps a fighter's root capsule alone. The roll composition puts its axis on the Gem -> tether
// line for both facings (evaluated from FRotationMatrix; SignOfFloat(yaw) 0, a tether right under or over the Gem,
// leaves it level, as here).
// Deviation: homing toward the ally's root in the play plane (the port's actors all stand at depth 0); the build's
// ProjectileMovement homes in 3D.
// The beam's start is the owner's mesh socket Gem; a character whose data lacks it falls back to the mesh's own
// place, with a warning (Creature's data has it).
// Deviation: the ally's OnExitedArena / OnDestroyed are read at the tether's next step (the ally's ring-out: `dead`),
// as Arya's knife reads its target's.
// The pull's Delay(T) is the tether's own (`end_at`): it fires at T whatever became of the move. On the frame the
// move's timeline also ends (a Delay made in ActivateObject counts the same frames as the timeline) the end waits for
// the move's last step (the tether's PULL_ENDS entry, [`pull_end`], from `holds::pull_tick`), as the rescue's.
// Unverified: the order of the two in that frame is not read.
//
// SuperSaveTetherProjectile_C: refused. The throws spawn it under Condition_IsBuddyReindog alone, whose mutator
// (Mutator_ReindogBuddyAI) no package spawns: never spawned in this build. Its ReceiveTick @1561 calls
// TetherProjectile's, which does not reach ParentProjectilePhysics' (own tick); a spawn warns and is destroyed at its
// first step.
//
// Comp_Pawn_Tether (Pawn_Reindog's component): its spawn array is not used by the montage route (the legacy tether
// component); no entry (components have no registry), an `ignoredOk` line.
#![allow(clippy::neg_cmp_op_on_partial_ord, reason = "NaN-keeping negated tests")]

use crate::Real;
use crate::State;
use crate::actors::projectile::{ProjectileExt, class_def, class_p, stop_simulating};
use crate::actors::tether_shape::{attach_radius, gem_of, rect_hits, sphere_hits};
use crate::actors::{ActorClass, ActorRef, Projectile, ProjectileClass, ProjectileId};
use crate::chars::creature::perks_ball::{BIG_BROTHER_BUFF, WRECKING_BALL_BUFF, destroy_balls, spawn_ball};
use crate::chars::creature::state::set_tether;
use crate::combat::buffs::{break_buff_states, buff_of, remove_buff};
use crate::combat::hit::apply_direct_damage;
use crate::combat::hit::knockback::timer_ticks;
use crate::combat::holds::{picked_up, pull_launch, pull_time, pull_toward};
use crate::consts::DELTA_SECONDS;
use crate::events::{Event, SoundKey};
use crate::fighter::common::{latent_frames, peers, warn_text};
use crate::fighter::control::data_name;
use crate::fighter::match_side::TutorialEvent;
use crate::fighter::moves::V2;
use crate::fighter::{FighterId, actor_y, invulnerable};
use crate::math::ue::{SMALL_NUMBER, safe_normal, size2d};
use crate::name;
use crate::names::Name;
use crate::registry::entries::{ActorEntry, ActorHooks, ProjectileHooks, ProjectileReceiveHit};
use crate::registry::props::{arr, num, obj};
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use crate::world::flow::{reset_pass, tutorial_event};
use crate::world::spawning::destroy_projectile;
use indexmap::IndexSet;

/// `TETHER`: the class, and the pull's end key (`PULL_ENDS`).
pub const TETHER: Name = name!("TetherProjectile_C");
/// `SUPER_SAVE`.
pub const SUPER_SAVE: Name = name!("SuperSaveTetherProjectile_C");
const LOOP: Name = name!("sfx_cha_reindog_tether_loop");
/// The loop sound's key (`'tether' + id`).
const LOOP_TAG: Name = name!("tether");
/// TickInterval 0.033 at the fixed step (see the header's Unverified note).
const TICK_FRAMES: i32 = 2;
/// The Cylinder mesh's radius 50 x scale 0.5.
const CYL_HALF_WIDTH: Real = 25.0;

/// `TetherProjectile extends Projectile`: what the class adds.
#[derive(State, Debug)]
pub struct TetherProjectile {
    /// HomingTargetComponent's actor (its root).
    pub home_to: Option<FighterId>,
    /// ProjectileMovement.HomingAccelerationMagnitude.
    pub home_mag: Real,
    /// Attached (SpawnAttachPoint: stopped, the tether points shown, the beam to the EndPoint).
    pub pinned: bool,
    /// AttachedAlly; AttachedToAlly and AttachedToFighter with it (set with the attach, never cleared).
    pub ally: Option<FighterId>,
    /// Activated.
    pub pulled: bool,
    /// TetherBreakWarning (the beam's Timeline_0 from then on).
    pub break_warning: bool,
    /// The 0.225 link's DoOnce.
    pub stop_once: bool,
    /// The ages its Delays fire at (-1: none): 0.225 -> @4989, 3 -> @4905, 5 -> @1412, 1 -> @1483, the overlap retry
    /// 0.05 -> @16 (with the event's OtherActor), 0.033 -> @1546, the pull's T -> @4742, PullAudio's T (the ribbon).
    pub at0225: Real,
    pub at3: Real,
    pub at5: Real,
    pub at1: Real,
    pub retry_at: Real,
    pub retry_who: Option<FighterId>,
    pub break_at: Real,
    pub end_at: Real,
    /// The pull's Delay(T) fired on the move's last frame: its end waits for that step.
    pub end_due: bool,
    /// PS_CreatureTetherRibbon on the ally while >= 0.
    pub ribbon_at: Real,
    /// The fighters (ids) the PlayerAttachCollision overlaps.
    pub over: IndexSet<i32>,
    /// HitVFXEnabled.
    pub vfx_on: bool,
    /// ResetVFXEnabled's timer, as the age it is seen at.
    pub vfx_at: Real,
    /// The pull took the BigBrother branch (the owner's perk, `perks_ball`): its end launches the owner. Unset (not
    /// false) on a tether without the perk.
    pub big_brother: Option<bool>,
}

impl Default for TetherProjectile {
    /// The field initial values.
    fn default() -> Self {
        TetherProjectile {
            home_to: None,
            home_mag: 0.0,
            pinned: false,
            ally: None,
            pulled: false,
            break_warning: false,
            stop_once: false,
            at0225: -1.0,
            at3: -1.0,
            at5: -1.0,
            at1: -1.0,
            retry_at: -1.0,
            retry_who: None,
            break_at: -1.0,
            end_at: -1.0,
            end_due: false,
            ribbon_at: -1.0,
            over: IndexSet::new(),
            vfx_on: true,
            vfx_at: -1.0,
            big_brother: None,
        }
    }
}

fn pr(w: &World, id: ProjectileId) -> &Projectile {
    w.actors.projectiles.get(id).expect("a live projectile")
}

fn pm(w: &mut World, id: ProjectileId) -> &mut Projectile {
    w.actors.projectiles.get_mut(id).expect("a live projectile")
}

fn t_of(p: &Projectile) -> &TetherProjectile {
    match &p.ext {
        ProjectileExt::TetherProjectile(t) => t,
        e => panic!("TetherProjectile_C without its state: {e:?}"),
    }
}

fn t(w: &World, id: ProjectileId) -> &TetherProjectile {
    t_of(pr(w, id))
}

fn t_mut(w: &mut World, id: ProjectileId) -> &mut TetherProjectile {
    match &mut pm(w, id).ext {
        ProjectileExt::TetherProjectile(t) => t,
        e => panic!("TetherProjectile_C without its state: {e:?}"),
    }
}

fn owner(w: &World, id: ProjectileId) -> FighterId {
    pr(w, id).owner.expect("a tether has an owner")
}

fn team(w: &World, f: FighterId) -> i32 {
    w.fighter(f).team.team
}

fn sound(w: &mut World, id: ProjectileId, ev: Name, at: Option<(Real, Real)>) {
    let e = Event::Sound {
        fighter: pr(w, id).owner_id,
        ev,
        x: at.map(|a| a.0),
        y: at.map(|a| a.1),
        hit: None,
        pooled: false,
        key: None,
    };
    w.events.push(e);
}

fn loop_key(w: &World, id: ProjectileId) -> SoundKey {
    SoundKey { tag: LOOP_TAG, n: pr(w, id).seq }
}

/// `TetherProjectile.writeHoming`, after the parent's: bIsHomingProjectile written false from outside
/// (`combat::elem` clear_homing: MulticastReflect @23793-23862, which nulls HomingTargetComponent too): this homing
/// stops; nothing of the class turns it on again.
pub fn write_homing(p: &mut Projectile, on: bool) {
    if let ProjectileExt::TetherProjectile(t) = &mut p.ext
        && !on
    {
        t.home_to = None;
    }
}

/// `TetherProjectile.homeAccel`: bIsHomingProjectile toward HomingTargetComponent (the ally's root) at
/// HomingAccelerationMagnitude, `None` without a target, a magnitude or a distance. Called by
/// `projectile::home_accel` in place of the parent's.
pub fn home_accel(w: &World, p: &Projectile) -> Option<V2> {
    let s = t_of(p);
    let to = s.home_to?;
    if !(s.home_mag > 0.0) {
        return None;
    }
    let hx = w.fighter(to).body.x - p.x;
    let hy = actor_y(w, to) - p.y;
    // ComputeHomingAcceleration: (target - location).GetSafeNormal() x HomingAccelerationMagnitude; the zero vector at
    // no distance (None: no acceleration to add).
    let n = safe_normal([0.0, hx, hy], SMALL_NUMBER);
    if n[1] == 0.0 && n[2] == 0.0 {
        return None;
    }
    Some(V2 { x: n[1] * s.home_mag, y: n[2] * s.home_mag })
}

/// `TetherProjectile.step` while on an ally and not dead: the movement is stopped and the actor rides its parent's
/// location (SnapToTarget). True: it rode (the shared step returns); false: the parent's step runs.
pub fn ride(w: &mut World, id: ProjectileId) -> bool {
    let p = pr(w, id);
    let Some(a) = t_of(p).ally else { return false };
    if p.dead {
        return false;
    }
    let (ax, ay) = (w.fighter(a).body.x, actor_y(w, a));
    let p = pm(w, id);
    p.prev_x = p.x;
    p.prev_y = p.y;
    p.age += 1;
    p.x = ax;
    p.y = ay;
    true
}

/// SpawnAttachPoint.
fn spawn_attach_point(w: &mut World, id: ProjectileId) {
    stop_simulating(pm(w, id));
    t_mut(w, id).pinned = true;
}

/// The begin overlap's flow from @16 with `o` the event's OtherActor; `retry`: the age a Delay(0.05) made here fires
/// at.
fn attach_test(w: &mut World, id: ProjectileId, o: FighterId, retry: Real) {
    t_mut(w, id).retry_who = Some(o);
    let (x, y) = (pr(w, id).x, pr(w, id).y);
    let r = attach_radius(w, pr(w, id));
    if t(w, id).ally.is_some() || w.fighter(o).life.dead > 0.0 || !sphere_hits(w, x, y, r, o) {
        let s = t_mut(w, id);
        if s.retry_at < 0.0 {
            s.retry_at = retry; // a Delay node already counting ignores the call
        }
        return;
    }
    let f = owner(w, id);
    if o == f || team(w, o) != team(w, f) || w.fighter(o).life.not_player_character {
        return; // @318-629
    }
    if !t(w, id).pinned {
        spawn_attach_point(w, id);
    }
    // K2_AttachToActor(SnapToTarget): the actor at the ally's root; collision and overlap events off.
    let (ox, oy) = (w.fighter(o).body.x, actor_y(w, o));
    let p = pm(w, id);
    p.x = ox;
    p.y = oy;
    let s = t_mut(w, id);
    s.ally = Some(o);
    s.over.clear();
    // AttachFXMulticast: the attach sound on the tether's Ak component; PS_CreatureTetherAlly on its root and
    // PS_TetherProjectile gone (the render's, from `ally`).
    sound(w, id, name!("sfx_cha_reindog_tether_attach"), Some((ox, oy)));
    tutorial_event(w, TutorialEvent::UniqueCharacterMechanic0); // @1298
}

/// The overlap's begin edges since the last test. `base`: the age a Delay made here counts from (an event ahead of
/// this frame's latent pass counts this frame).
fn attach_overlaps(w: &mut World, id: ProjectileId, base: Real) {
    if t(w, id).ally.is_some() {
        return;
    }
    let r = attach_radius(w, pr(w, id));
    // the begin overlap sees a minion too
    for i in 0..peers(w).len() {
        let v = peers(w)[i];
        let (x, y) = (pr(w, id).x, pr(w, id).y);
        let on = w.fighter(v).life.dead <= 0.0 && sphere_hits(w, x, y, r, v);
        let vid = w.fighter(v).id;
        if !on {
            t_mut(w, id).over.shift_remove(&vid);
            continue;
        }
        if !t_mut(w, id).over.insert(vid) {
            continue;
        }
        attach_test(w, id, v, base + latent_frames(0.05));
        if t(w, id).ally.is_some() {
            return;
        }
    }
}

/// @1546: the mover toward the owner and the Delay(T); the owner's perks pick the branch (CheckHasBuff on Fighter_1:
/// BigBrother @1546 -> @1759-2350, else WreckingBall @2512 -> the ball @2731 and T x the multiplier, else
/// @3050-4611). `base`: the age the Delay counts from.
fn start_pull(w: &mut World, id: ProjectileId, base: Real) {
    let Some(a) = t(w, id).ally else { return };
    let f = owner(w, id);
    let by = pr(w, id).seq;
    let time = if buff_of(w, f, BIG_BROTHER_BUFF).is_some() {
        t_mut(w, id).big_brother = Some(true);
        pull_toward(w, a, f, 0.4, false, 1.0, TETHER, by)
    } else if buff_of(w, f, WRECKING_BALL_BUFF).is_some() {
        spawn_ball(w, f, a);
        let mult = num(class_p(w, pr(w, id)), "WreckingBallPullTimeMultiplier").unwrap_or(1.0);
        pull_toward(w, a, f, 0.4, true, mult, TETHER, by)
    } else {
        tutorial_event(w, TutorialEvent::UniqueCharacterMechanic1); // @3011, ahead of StunnedMoveToPositionThroughGeo
        pull_toward(w, a, f, 0.4, true, 1.0, TETHER, by)
    };
    t_mut(w, id).end_at = base + latent_frames(time);
}

/// The pull's Delay(T) link: @4742 DestroySpawnedActors(Creature_WreckingBall_C) on the owner, the ally's
/// StopAnimMontage(null), LaunchCharacter((0, 0, 200), true, true); the BigBrother branch's @2409 the owner's two
/// calls alone. The caller then destroys the actor.
fn end_pull(w: &mut World, id: ProjectileId) {
    let s = t_mut(w, id);
    s.end_due = false;
    s.end_at = -1.0;
    let (big, ally) = (s.big_brother == Some(true), s.ally);
    let f = owner(w, id);
    if big {
        pull_launch(w, f);
        return;
    }
    destroy_balls(w, f);
    if let Some(a) = ally {
        pull_launch(w, a);
    }
}

/// ReceiveTick from @11863 on (the actor is Attached): the cylinder's damage.
fn cylinder_tick(w: &mut World, id: ProjectileId, dt: Real) {
    let f = owner(w, id);
    let (gx, gy) = gem_of(w, f, TETHER);
    let (px, py) = (pr(w, id).x, pr(w, id).y);
    let (vx, vy) = (px - gx, py - gy);
    let len = size2d([vx, vy]); // VSize @8464
    // The axis: Gem -> tether; level when the two share a UE Y (SignOfFloat(yaw) 0, no roll).
    let level = vx == 0.0 || len == 0.0;
    let (ux, uy) = if level { (1.0, 0.0) } else { (vx / len, vy / len) };
    let (sx, sy) = if level { ((gx + px) / 2.0 - len / 2.0, (gy + py) / 2.0) } else { (gx, gy) };
    let dps = num(class_p(w, pr(w, id)), "DamagePerSecond").unwrap_or(0.0);
    let dmg = dt * dps;
    // TetherSlope / TetherB @8887 (Divide_FloatFloat by 0 is 0): the hit effect's height on the line.
    let slope = if vx == 0.0 { 0.0 } else { vy / vx };
    let ally = t(w, id).ally;
    let fid = w.fighter(f).id;
    let mut triggered = false;
    // GetOverlappingActors(BaseFighter) @9529: a minion too
    for i in 0..peers(w).len() {
        let v = peers(w)[i];
        if v == f || Some(v) == ally || w.fighter(v).life.dead > 0.0 {
            continue;
        }
        if !rect_hits(w, sx, sy, ux, uy, len, CYL_HALF_WIDTH, v) {
            continue;
        }
        // IsVulnerableTo(the owner's TeamIndex): !IsInvulnerable?, !DamageDisabled, another team (FriendlyFire
        // false).
        if invulnerable(w, v) || w.fighter(v).invuln.damage_disabled || team(w, v) == team(w, f) {
            continue;
        }
        // Deviation: the port's Dodgeball rule spares an opponent the cylinder's damage.
        if crate::fighter::spared_by(w, f, v) {
            continue;
        }
        // ApplyDamage -> Comp_Actor_Damage OnTakeAnyDamage @546-910: the temp health's share first, CurrentDamage +=
        // (float), RecordDamage to the causer's instigator.
        apply_direct_damage(w, v, dmg, Some(f));
        let (vx_, vy_) = (w.fighter(v).body.x, actor_y(w, v));
        sound(w, id, name!("sfx_cha_reindog_tether_hit"), Some((vx_, vy_)));
        if t(w, id).vfx_on {
            triggered = true;
            let e = Event::ProjectileDeath {
                cls: pr(w, id).cls,
                fighter: fid,
                vfx: Some(name!("PS_HitImpact_TEMPLATE")),
                x: vx_,
                y: gy + slope * (vx_ - gx),
                ox: 55.0,
                rot: [0.0, 0.0, 0.0],
                scale: 0.625,
                tint: Some(crate::real(w.fighter(v).id)),
                to: None,
            };
            w.events.push(e);
        }
    }
    if t(w, id).vfx_on && triggered {
        let window = num(class_p(w, pr(w, id)), "HitVFXWindow");
        let at = crate::real(pr(w, id).age) + timer_ticks(window);
        let s = t_mut(w, id);
        s.vfx_on = false;
        s.vfx_at = at;
    }
}

/// The constructor's tail (`new TetherProjectile`): no hurt shape (HurtBox NoCollision), the homing magnitude.
fn construct(w: &World, p: &mut Projectile) {
    p.hurt_radius = 0.0;
    let home_mag = class_def(w, p)
        .and_then(|d| obj(&d.components, "ProjectileMovement"))
        .and_then(|m| num(m, "HomingAccelerationMagnitude"))
        .unwrap_or(0.0);
    p.ext = ProjectileExt::TetherProjectile(TetherProjectile { home_mag, ..TetherProjectile::default() });
}

fn begin(w: &mut World, id: ProjectileId) {
    let f = owner(w, id);
    // AdjustIfPossibleButAlwaysSpawn (Comp_Pawn_Tether's spawn): FindTeleportSpot for the Capsule came ahead of
    // BeginPlay (the stage block alone). BPI_SetTether(self) on the Instigator @14148.
    set_tether(w, f, Some(id));
    // CheckForHomingTarget: the last ally (GetAllies: the owner's team, a fighter waiting for its respawn left out;
    // the order unverified).
    for i in 0..peers(w).len() {
        let o = peers(w)[i];
        if o != f && team(w, o) == team(w, f) && w.fighter(o).life.dead <= 0.0 {
            t_mut(w, id).home_to = Some(o);
        }
    }
    let s = t_mut(w, id);
    s.at0225 = latent_frames(0.225);
    s.at3 = latent_frames(3.0);
    // PS_TetherPoints on the owner's Gem and the beam: the render's, while the tether lives.
    sound(w, id, name!("sfx_cha_reindog_tether_cast"), None); // posted on the owner
    let e = Event::SoundStart { fighter: pr(w, id).owner_id, ev: LOOP, key: loop_key(w, id) };
    w.events.push(e);
    // The overlaps at the spawn place.
    attach_overlaps(w, id, 0.0);
}

/// `receiveHit.boxes`: none besides the stage block.
fn hit_boxes(_w: &mut World, _id: ProjectileId, out: &mut crate::world::stage_query::StageShapes) {
    out.boxes.clear();
    out.polys.clear();
}

/// The Capsule's blocking hit: no ReceiveHit of its own; HandleImpact stops the movement.
fn receive_hit(_w: &mut World, _id: ProjectileId, _nx: Real, _ny: Real, _t: Real, _pen: bool) -> bool {
    false
}

/// `step`: the class's tick part and its latent actions. True: K2_DestroyActor.
fn step(w: &mut World, id: ProjectileId) -> bool {
    let age = pr(w, id).age;
    let a = crate::real(age);
    // AttachedAllyExitedArena / AttachedTargetDestroyed.
    if let Some(al) = t(w, id).ally
        && w.fighter(al).life.dead > 0.0
    {
        return true;
    }
    // ResetVFXEnabled (the timer pass of an earlier frame).
    let s = t_mut(w, id);
    if s.vfx_at >= 0.0 && a >= s.vfx_at {
        s.vfx_on = true;
        s.vfx_at = -1.0;
    }
    // The overlaps behind the move (an event ahead of this frame's latent pass).
    attach_overlaps(w, id, a - 1.0);
    // ReceiveTick.
    if (age - 1) % TICK_FRAMES == 0 && t(w, id).pinned && !pr(w, id).dead {
        let dt = if age == 1 { DELTA_SECONDS } else { crate::real(TICK_FRAMES) * DELTA_SECONDS };
        cylinder_tick(w, id, dt);
    }
    // The latent actions.
    if a == t(w, id).at0225 {
        let s = t_mut(w, id);
        s.at0225 = -1.0;
        if !s.stop_once {
            s.stop_once = true;
            let p = pm(w, id);
            p.vx = 0.0; // SetVelocityInLocalSpace(0)
            p.vy = 0.0;
            if !t(w, id).pinned {
                spawn_attach_point(w, id); // AttachedToAlly false (it is), DestroyOnBlocked false (`blocked`)
            }
        }
    }
    if a == t(w, id).at3 {
        t_mut(w, id).at3 = -1.0;
        if t(w, id).ally.is_none() {
            return true;
        }
        t_mut(w, id).at5 = a + latent_frames(5.0);
    }
    if a == t(w, id).at5 {
        let s = t_mut(w, id);
        s.at5 = -1.0;
        s.break_warning = true;
        s.at1 = a + latent_frames(1.0);
    }
    if a == t(w, id).at1 {
        t_mut(w, id).at1 = -1.0;
        if !t(w, id).pulled {
            return true;
        }
    }
    if a == t(w, id).retry_at {
        t_mut(w, id).retry_at = -1.0;
        if let Some(o) = t(w, id).retry_who {
            attach_test(w, id, o, a + latent_frames(0.05));
        }
    }
    if a == t(w, id).ribbon_at {
        t_mut(w, id).ribbon_at = -1.0;
    }
    if a == t(w, id).break_at {
        t_mut(w, id).break_at = -1.0;
        start_pull(w, id, a);
    }
    // A pull end that waited for the move's last step, and the move did not take it.
    if t(w, id).end_due {
        end_pull(w, id);
        return true;
    }
    if a == t(w, id).end_at {
        let seq = pr(w, id).seq;
        let waits = t(w, id)
            .ally
            .and_then(|al| w.fighter(al).moves.pull.as_ref())
            .is_some_and(|p| p.end == TETHER && p.by == seq && p.k + 1.0 >= p.n);
        if waits {
            let s = t_mut(w, id);
            s.end_at = -1.0;
            s.end_due = true;
        } else {
            end_pull(w, id);
            return true;
        }
    }
    false
}

/// Blocked -> @5060 (behind the 0.225 link's DoOnce): SetVelocityInLocalSpace(0) and, unless Attached,
/// SpawnAttachPoint. The class's own handler: the parent's DestroyOnBlocked is never read.
fn blocked(w: &mut World, id: ProjectileId, _by: ActorRef) {
    let p = pm(w, id);
    p.vx = 0.0;
    p.vy = 0.0;
    if !t(w, id).pinned {
        spawn_attach_point(w, id);
    }
}

fn activate(w: &mut World, id: ProjectileId) {
    if t(w, id).pulled {
        return;
    }
    let f = owner(w, id);
    let Some(a) = t(w, id).ally else {
        destroy_projectile(w, id, false);
        return;
    };
    t_mut(w, id).pulled = true;
    // (TetherLocation, BPI_IncrementCoopActions: statistics.)
    if team(w, a) != team(w, f) {
        destroy_projectile(w, id, false);
        return;
    }
    remove_buff(w, a, None, Some(name!("StateGroups::NewEnumerator12"))); // RemoveBuff(None, 11 = HELD_GROUP) @10966
    // PullAudioMulticast(ally, T): the grab sound at the tether, the ribbon on the ally for T.
    let d = size2d([w.fighter(f).body.x - w.fighter(a).body.x, actor_y(w, f) - actor_y(w, a)]);
    let time = pull_time(d, 0.4, 1.0);
    let (px, py) = (pr(w, id).x, pr(w, id).y);
    sound(w, id, name!("sfx_cha_reindog_tether_grab"), Some((px, py)));
    let age = crate::real(pr(w, id).age);
    t_mut(w, id).ribbon_at = age + latent_frames(time);
    // BreakBuffStates @11213 (Comp_State_Manager). With this data (BreakStates Held, BreakBuffs empty) RemoveBuff has
    // already taken every buff it could find: false, the pull starts at once.
    let p = class_p(w, pr(w, id));
    let states: Vec<String> = arr(p, "BreakStates").iter().filter_map(|v| v.as_str().map(str::to_owned)).collect();
    let buffs: Vec<Name> = arr(p, "BreakBuffs").iter().filter_map(|v| v.as_str()).map(|s| data_name(w, s)).collect();
    let states: Vec<&str> = states.iter().map(String::as_str).collect();
    let broke = break_buff_states(w, a, &states, &buffs);
    picked_up(w, a); // broadcast IsPickedUp
    if broke {
        t_mut(w, id).break_at = age + latent_frames(0.033);
    } else {
        start_pull(w, id, age);
    }
}

fn destroyed(w: &mut World, id: ProjectileId) {
    let (x, y) = (pr(w, id).x, pr(w, id).y);
    sound(w, id, name!("sfx_cha_reindog_tether_end"), Some((x, y)));
    let e = Event::SoundStop {
        fighter: pr(w, id).owner_id,
        key: loop_key(w, id),
        fade: Some(0.5),
        ev: None,
        curve: Some(name!("Linear")),
        cut: false,
    };
    w.events.push(e);
}

/// ReceiveEndPlay @15589: BPI_SetTether(null) on the Instigator, whichever tether the pawn holds (no identity test).
/// Not in the Lab reset's pass (an old pawn there).
fn end(w: &mut World, id: ProjectileId) {
    if !reset_pass(w) {
        let f = owner(w, id);
        set_tether(w, f, None);
    }
}

/// SuperSaveTetherProjectile_C's BeginPlay: refused (see the header).
fn super_save_begin(w: &mut World, _id: ProjectileId) {
    warn_text(
        w,
        "SuperSaveTetherProjectile_C not ported (never spawned: Condition_IsBuddyReindog is false, 05_plan finding 2)",
    );
}

fn super_save_step(_w: &mut World, _id: ProjectileId) -> bool {
    true
}

/// The tether's `PULL_ENDS` entry (`pullEnds({[TETHER]: ...})`): its Delay(T) fired on the move's last frame and
/// waited for that step. `by`: the tether's id the pull was started with.
pub fn pull_end(w: &mut World, _f: FighterId, by: i32) {
    let mut i = 0;
    while let Some(id) = w.actors.projectiles.nth(i) {
        i += 1;
        let p = pr(w, id);
        if p.seq != by || !matches!(p.ext, ProjectileExt::TetherProjectile(_)) {
            continue;
        }
        if p.dead || !t_of(p).end_due {
            return;
        }
        end_pull(w, id);
        destroy_projectile(w, id, true);
        return;
    }
}

/// This file's registry entries (the tether's and the super save's kinds).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let kind = ProjectileHooks {
        // ReceiveTick @14919 -> @12534 does not call the parent's: no AimDownProjectileMovement look-at.
        own_tick: true,
        construct: Some(construct),
        begin: Some(begin),
        receive_hit: Some(ProjectileReceiveHit { boxes: hit_boxes, hit: receive_hit }),
        step: Some(step),
        blocked: Some(blocked),
        activate: Some(activate),
        destroyed: Some(destroyed),
        end: Some(end),
        ..ProjectileHooks::default()
    };
    r.actors.enter(
        "TetherProjectile_C",
        ActorEntry::new(ActorClass::Projectile(ProjectileClass::TetherProjectile), ActorHooks::Projectile(kind)),
    )?;
    let super_save = ProjectileHooks {
        own_tick: true,
        begin: Some(super_save_begin),
        step: Some(super_save_step),
        ..ProjectileHooks::default()
    };
    r.actors.enter(
        "SuperSaveTetherProjectile_C",
        ActorEntry::new(
            ActorClass::Projectile(ProjectileClass::SuperSaveTetherProjectile),
            ActorHooks::Projectile(super_save),
        ),
    )
}
