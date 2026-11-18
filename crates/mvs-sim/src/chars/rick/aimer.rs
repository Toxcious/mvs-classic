//! Rick's C020PortalAimer_C ([`PortalAimer`], a world actor) and the condition his
//! portal montages check, Condition_C020PortalReadyForPartner.
//!
//! The aimer (a SpawnedActorParent child; offsets of its ubergraph unless a function is named) is spawned by the Spawner
//! notify of C020_Attack_PortalGun_Montage2 (ground) and C020_Attack_PortalGun_Air_Montage2 (air) at L_Hand + (0, 1000 /
//! 1500, 100), destroyed at that notify's end (DestroyAllActorsOnNotifyEnd), and activated by the montages' ActivateSpawned
//! (the air one with ActivationConditions [0] = true). Class data: AimerMoveSpeed 3200, MaxDistanceOfPortal 3500,
//! BlastBoxBuffer 700, InputActivateDelay 0.2, ActivationConditions [false], ForceSpawnPositionInsideBlastBox, Die With
//! Instigator, DestroyOnExitedArena?, Capsule 250 / 250 NoCollision; SpawnPortalNextToInstigator and Spawn Partner Portal
//! false (no class default, and no graph writes the first: xref writes). Sim space: x = -UE Y, y = UE Z.
//!
//! BeginPlay @6296 (inside the spawn, in the order the flow stack runs it):
//! - the parent's: BlastBoxBoundsMax / Min from the blast box's jump-out area (BPI_GetBlastBoxBounds(UseJumpBuffer true) =
//!   BlastBoxJumpOutArea origin + extent, SpawnedActorParent @9233-10573: per axis max / min with (0, 0, 0)); outside that
//!   area (BPI_CheckIfPositionOverlaps, edges in) with ForceSpawnPositionInsideBlastBox: clamped into it, unswept
//!   (@14195-14989).
//! - @6321 the gun mesh (SM_C020PortalGun, tag AimPortalGun) at FacingLeftTransform / FacingRightTransform by Rick's facing,
//!   on his Mesh at L_Hand; MeshManager.AddMesh, SetSubMeshVisibility: the view's (`gun_left` below).
//! - @7991 Spawn Partner Portal = the component's Get Allied Portals has length 1. False: SetBounds @8354, then @8608
//!   LocallyControlled (KeepOnCamera?, the SkillShotIndicator, KeepOnCamera.Register @1888), then @8369 the ally list for a
//!   locally controlled Rick in a match of more than 3 players (@6152 / @5246).
//! - @7522 (authority) MulticastPosition(BPI_C020.GetSafePortalLocation(250, 250, its location, Rick + (0, 0, 250)) @7849;
//!   no BPI_C020: (0, 0, 0)) -> @12609, an unswept set.
//! - @7452 InputActive = false; Delay(InputActivateDelay) @7463 -> @30 InputActive = true.
//!
//! ReceiveTick @12125, while !Spawn Partner Portal and InputActive (@8781): the flow stack runs authority and !Activated ->
//! Move @42 first (@8796 -> @5202), then the aim @9767, then the facing @8831 (pushed in that order @8781-8791).
// Unverified (aimer a): the actor's tick against Rick's in the frame (TG_PrePhysics both, order not read): the port's
// World.actors pass after the fighters, so the move reads the stick of this frame and the facing write is seen by Rick's
// next tick.
//! K2_SetActorLocation(sweep true) of the Move @1812 on a NoCollision root: the full move and no hit
//! (AActor::K2_SetActorLocation).
// Deviation (aimer c): IsLocallyControlled is true for every fighter of the port (as the other graphs' reads): the ally
// list is built in a match of more than 3 fighters, KeepOnCamera? and the indicator are on for every Rick.
// Unverified (aimer d): the ally list's player index i is World.fighters[i] (PlayerDataManager.GetByPlayerIndex, the
// TeamIndex of its player data against the aimer's Comp_Actor_Destructible, which is taken as Rick's team).
// Deviation (aimer e): a release by the Spawner notify's end or Rick's defeat (the world's `dead`) runs the EndPlay cleanup
// at the frame's list pass (World.step), after that frame's blast-box test: Rick stays on CantDieList for it.
// The spawn's place is the notify's (`spawn_place`): no caller of this class sets `SpawnRequest.origin` (the aimer is
// spawned by notifies only).

use super::pawn::set_aim_direction;
use super::portal_comp::{get_allied_portals, spawn_object};
use super::state::rick_of;
use super::teleport::{bpi_get_safe_portal_location, is_c020};
use crate::Real;
use crate::State;
use crate::actors::world_actor::{WorldActorState, actor_p};
use crate::actors::{ActorClass, ActorId, ActorRef, WorldActor, WorldActorClass};
use crate::consts::DELTA_SECONDS;
use crate::fighter::common::{actor_class_def, latent_frames, warn_text};
use crate::fighter::match_side::SpawnRequest;
use crate::fighter::{FighterId, actor_y};
use crate::input::{Dir, get8_way_no_facing};
use crate::math::{self, max, min, to_int32};
use crate::name;
use crate::names::Name;
use crate::registry::entries::{ActorEntry, ActorHooks, Condition, SizeHooks, WorldActorHooks};
use crate::registry::props::{flag, num as pnum, obj};
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use crate::world::blast::{BlastBounds, blast_box_loop, blast_timer_exit, new_blast_bounds, spawn_box};
use crate::world::spawn_spot::spawn_place;
use crate::world::spawning::request_props;

pub const AIMER: &str = "C020PortalAimer_C";
const EXIT_PORTAL: Name = name!("ExitPortalParent_C");
const AUTO_PORTAL: Name = name!("AutoTeleportPortal_C");

/// Normal of (0, +-1, +-1): InvSqrt(2).
fn diag() -> Real {
    math::ue::inv_sqrt(2.0)
}

/// FMath::Clamp(X, Min, Max): X < Min ? Min : X < Max ? X : Max.
fn clamp(v: Real, lo: Real, hi: Real) -> Real {
    if v < lo {
        lo
    } else if v < hi {
        v
    } else {
        hi
    }
}

/// `PortalAimer` (C020PortalAimer_C).
#[derive(State, Debug)]
pub struct PortalAimer {
    /// K2_GetActorLocation.
    pub x: Real,
    pub y: Real,
    /// Capsule.CapsuleRadius / HalfHeight (bAbsoluteScale, scale 1: the unscaled ones).
    pub radius: Real,
    pub half_height: Real,
    /// BlastBoxBoundsMin / Max (Y, Z) of the parent's BeginPlay, in sim space.
    pub blast_bounds: BlastBounds,
    /// MinSpawnBounds / MaxSpawnBounds (SetBounds), in sim space; X (+-500) has no sim axis.
    pub min_x: Real,
    pub max_x: Real,
    pub min_y: Real,
    pub max_y: Real,
    /// Spawn Partner Portal.
    pub partner: bool,
    /// InputActive.
    pub input_active: bool,
    /// The Delay(InputActivateDelay)'s frames left, 0: not running.
    pub input_left: i32,
    /// Activated.
    pub activated: bool,
    /// The Delay(1.0) @3453 -> ReleaseToPool: frames left, 0: not running.
    pub release_left: i32,
    /// ActivationConditions.
    pub conds: Vec<bool>,
    /// Allies.
    pub allies: Vec<FighterId>,
    /// AllyIndex.
    pub ally_index: usize,
    /// For the view: PortalGunMesh's relative transform is FacingLeftTransform (true) or FacingRightTransform, on Rick's
    /// Mesh at L_Hand.
    pub gun_left: bool,
    /// KeepOnCamera? and the SkillShotIndicator's visibility (BeginPlay @8674).
    pub locally_controlled: bool,
    /// The actor's ticks, for the parent's CheckBlastBoxTimer.
    pub age: i32,
    pub blast_at: Real,
}

impl Default for PortalAimer {
    /// The field initial values.
    fn default() -> Self {
        PortalAimer {
            x: 0.0,
            y: 0.0,
            radius: 0.0,
            half_height: 0.0,
            blast_bounds: new_blast_bounds(),
            min_x: 0.0,
            max_x: 0.0,
            min_y: 0.0,
            max_y: 0.0,
            partner: false,
            input_active: false,
            input_left: 0,
            activated: false,
            release_left: 0,
            conds: Vec::new(),
            allies: Vec::new(),
            ally_index: 0,
            gun_left: false,
            locally_controlled: false,
            age: 0,
            blast_at: 0.1,
        }
    }
}

/// The C020PortalAimer_C's place (its `x`, `y`).
pub fn place(w: &World, a: &WorldActor, s: &PortalAimer) -> Option<(Real, Real)> {
    let _ = (w, a);
    Some((s.x, s.y))
}

fn m_of(w: &World, id: ActorId) -> &PortalAimer {
    match &w.actors.actors.get(id).expect("a live aimer").state {
        WorldActorState::PortalAimer(m) => m,
        _ => unreachable!("a C020PortalAimer_C's state"),
    }
}

fn m_mut(w: &mut World, id: ActorId) -> &mut PortalAimer {
    match &mut w.actors.actors.get_mut(id).expect("a live aimer").state {
        WorldActorState::PortalAimer(m) => m,
        _ => unreachable!("a C020PortalAimer_C's state"),
    }
}

fn owner_of(w: &World, id: ActorId) -> FighterId {
    w.actors.actors.get(id).expect("a live aimer").owner
}

/// The class's number (a missing one warns and reads 0).
fn data_num(w: &World, id: ActorId, k: &str) -> Real {
    match pnum(actor_p(w, ActorRef::Actor(id)), k) {
        Some(v) => v,
        None => {
            warn_text(w, &format!("C020PortalAimer data missing: {k}"));
            0.0
        }
    }
}

/// SetBounds (function): from Rick's location and the blast bounds, once (the box does not follow him).
fn set_bounds(w: &mut World, id: ActorId) {
    let f = owner_of(w, id);
    let (d, buf) = (data_num(w, id, "MaxDistanceOfPortal"), data_num(w, id, "BlastBoxBuffer"));
    let (fx, fy) = (w.fighter(f).body.x, actor_y(w, f));
    let m = m_mut(w, id);
    // Max = (500, min(i.Y + d, BlastMax.Y - buf), min(i.Z + d, BlastMax.Z - buf)), Min the other way: sim x = -Y.
    m.min_x = max(fx - d, m.blast_bounds.left + buf);
    m.max_x = min(fx + d, m.blast_bounds.right - buf);
    m.max_y = min(fy + d, m.blast_bounds.top - buf);
    m.min_y = max(fy - d, m.blast_bounds.bottom + buf);
}

/// ClampPosition (function): per axis FClamp(p, Min + CapsuleRadius / 2, Max - CapsuleRadius / 2); Valid is
/// BooleanOR(true, ...): always true.
fn clamp_x(m: &PortalAimer, x: Real) -> Real {
    let h = m.radius / 2.0;
    clamp(x, m.min_x + h, m.max_x - h)
}

fn clamp_y(m: &PortalAimer, y: Real) -> Real {
    let h = m.radius / 2.0;
    clamp(y, m.min_y + h, m.max_y - h)
}

/// BPI_C020.GetSafePortalLocation(scaled capsule, the aimer's location, Rick + (0, 0, 250)); (0, 0) without BPI_C020.
fn safe_location(w: &mut World, id: ActorId) -> (Real, Real) {
    let f = owner_of(w, id);
    if !is_c020(w, Some(f)) {
        return (0.0, 0.0);
    }
    let m = m_of(w, id);
    let (r, hh, x, y) = (m.radius, m.half_height, m.x, m.y);
    let (fx, fy) = (w.fighter(f).body.x, (actor_y(w, f) + 250.0));
    bpi_get_safe_portal_location(w, f, r, hh, x, y, fx, fy)
}

/// The ally list @6152 / @5246: player index 0 .. PlayerCount inclusive; each pawn of the aimer's team other than the
/// instigator, AddUnique (Unverified aimer d).
fn build_allies(w: &mut World, id: ActorId) {
    let f = owner_of(w, id);
    let team = w.fighter(f).team.team;
    let n = w.players.len();
    for i in 0..=n {
        let Some(g) = w.players.get(i).copied() else { continue };
        if w.fighter(g).team.team != team || g == f {
            continue;
        }
        let m = m_mut(w, id);
        if !m.allies.contains(&g) {
            m.allies.push(g);
        }
    }
}

/// BeginPlay (see the head), after the parent's blast-box loop (in [`spawn`]).
fn begin_play(w: &mut World, id: ActorId) {
    let f = owner_of(w, id);
    m_mut(w, id).gun_left = crate::fighter::facing(w, f) < 0.0; // @6541
    // @7991: the whole block needs the component (@8091 pops it): without one no SetBounds (the bounds stay 0), no
    // LocallyControlled and no ally list.
    if rick_of(w, f).is_some() {
        let partner = get_allied_portals(w, f).len() == 1; // @8276
        m_mut(w, id).partner = partner;
        if !partner {
            set_bounds(w, id);
            m_mut(w, id).locally_controlled = true; // Deviation aimer c
            if w.players.len() > 3 {
                build_allies(w, id);
            }
        }
    }
    let s = safe_location(w, id); // @7849
    let delay = data_num(w, id, "InputActivateDelay");
    let m = m_mut(w, id);
    (m.x, m.y) = s; // MulticastPosition -> @12609
    m.input_active = false;
    m.input_left = to_int32(latent_frames(delay));
}

/// Move @42: the stick by BPI_Get8WayDirection(UseFacing false) (Get8WayDirectionNoFacing), Neutral and Default no move; the
/// direction normalized, x DeltaSeconds x AimerMoveSpeed, ClampPosition, K2_SetActorLocation (no hit).
fn move_(w: &mut World, id: ActorId) {
    let f = owner_of(w, id);
    let (ix, iy) = (w.fighter(f).controls.inp.x, w.fighter(f).controls.inp.y);
    let d = get8_way_no_facing(ix, iy);
    let g = diag();
    let (nx, ny) = match d {
        Dir::Neutral | Dir::Default => return,
        Dir::Up => (0.0, 1.0),
        Dir::Down => (0.0, -1.0),
        Dir::Fwd => (1.0, 0.0),   // (0, -1, 0)
        Dir::Back => (-1.0, 0.0), // (0, 1, 0)
        Dir::FwdUp => (g, g),
        Dir::FwdDown => (g, -g),
        Dir::BackUp => (-g, g),
        Dir::BackDown => (-g, -g),
    };
    let step = DELTA_SECONDS * data_num(w, id, "AimerMoveSpeed");
    let m = m_mut(w, id);
    m.x = clamp_x(m, m.x + (nx * step));
    m.y = clamp_y(m, m.y + (ny * step));
}

/// The aim @9767: the owner's AnimBP BPI_SetAimDirection(FClamp(DegAtan2(|d.Y|, d.Z) / 180, 0, 1)), d =
/// GetDirectionUnitVector(aimer, Rick): 1 with the aimer straight above him, 0.5 level, 0 straight below. A zero vector (the
/// same place) gives 0.
fn aim(w: &mut World, id: ActorId) {
    let f = owner_of(w, id);
    if rick_of(w, f).is_none() {
        return; // no C020_AnimBPV2 (BPI_AnimBP): the call is not made
    }
    let m = m_of(w, id);
    // |d.Y| alone is read: the sign of x does not matter
    let dy = w.fighter(f).body.x - m.x;
    let dz = actor_y(w, f) - m.y;
    let [_, uy, uz] = math::ue::safe_normal([0.0, dy, dz], math::ue::SMALL_NUMBER);
    let deg = math::ue::deg_atan2(uy.abs(), uz);
    set_aim_direction(w, f, clamp(deg / 180.0, 0.0, 1.0));
}

/// The facing @8831: BPI_IsFacingLeft != (aimer.Y > Rick.Y) -> BPI_SetFacingLogic(!facingLeft), then the gun re-attached
/// with the transform of the new facing.
fn face(w: &mut World, id: ActorId) {
    let f = owner_of(w, id);
    let left = crate::fighter::facing(w, f) < 0.0;
    if left == (m_of(w, id).x < w.fighter(f).body.x) {
        return;
    }
    crate::fighter::set_facing(w, f, if left { 1.0 } else { -1.0 });
    m_mut(w, id).gun_left = !left;
}

/// EndPlay @12226 (and ReleaseToPool @12442, after the parent's: the destroy, whose EndPlay runs this first): every blast
/// box UpdateCantDieList(Rick, false) @10853, the AnimBP's aim 0.5 @11213; locally controlled: the input widget's Exit,
/// MeshManager.RemoveMesh and the gun's K2_DestroyComponent (the view's).
fn cleanup(w: &mut World, id: ActorId) {
    let f = owner_of(w, id);
    w.fighter_mut(f).life.cant_die = false;
    if rick_of(w, f).is_some() {
        set_aim_direction(w, f, 0.5);
    }
}

/// ReleaseToPool (CanBePooled false: K2_DestroyActor, its EndPlay inside the call).
fn release(w: &mut World, id: ActorId) {
    let Some(a) = w.actors.actors.get_mut(id) else { return };
    if a.dead {
        return;
    }
    a.dead = true;
    cleanup(w, id);
}

/// The partner block @2374: next to Rick (300 ahead, 100 up), SetBounds, an AutoTeleportPortal at the safe place of that
/// spot; then the Delay(1.0) @3453.
fn partner_portal(w: &mut World, id: ActorId, left: bool) {
    let f = owner_of(w, id);
    let (fx, fy) = (w.fighter(f).body.x, actor_y(w, f));
    let m = m_mut(w, id);
    m.x = fx + if left { -300.0 } else { 300.0 };
    m.y = fy + 100.0;
    set_bounds(w, id);
    let s = safe_location(w, id); // @2935
    spawn_object(w, f, s.0, s.1, AUTO_PORTAL); // @3346 (Facing is not read)
    delay_release(w, id);
}

/// Delay(1.0) @3453 -> @15 ReleaseToPool; a Delay that is pending ignores the call.
fn delay_release(w: &mut World, id: ActorId) {
    let m = m_mut(w, id);
    if m.release_left == 0 {
        m.release_left = to_int32(latent_frames(1.0));
    }
}

/// ActivateObject @1925 (authority).
fn activate(w: &mut World, id: ActorId) {
    let f = owner_of(w, id);
    m_mut(w, id).activated = true;
    if rick_of(w, f).is_none() {
        return; // @2076
    }
    if !w.actors.actors.get(id).is_some_and(|a| a.dead) {
        w.fighter_mut(f).life.cant_die = true; // !IsActorBeingDestroyed: every blast box UpdateCantDieList(Rick, true) @3997
    }
    let left = crate::fighter::facing(w, f) < 0.0; // @2314
    if m_of(w, id).partner {
        partner_portal(w, id, left);
        return;
    }
    let s = safe_location(w, id); // @4404
    spawn_object(w, f, s.0, s.1, EXIT_PORTAL); // @4837: (0, safe.Y, safe.Z)
    if flag(actor_p(w, ActorRef::Actor(id)), "SpawnPortalNextToInstigator") || m_of(w, id).conds.first() == Some(&true)
    {
        partner_portal(w, id, left); // @4999
        return;
    }
    delay_release(w, id);
}

/// OnInputSnapToAlly(InputType) @12457: !Activated, !Spawn Partner Portal, InputType 1 -> SetPositionServer(the AllyIndex
/// ally's location) (ClampPosition, unswept, @12043), AllyIndex + 1 wrapping to 0. No graph of the build calls it (the input
/// widget's binding is native or UMG, not ported): nothing in the port calls it either.
// Not on the trace: ported for the call surface, called by nothing.
pub fn snap_to_ally(w: &mut World, id: ActorId, input_type: i32) {
    let m = m_of(w, id);
    if m.activated || m.partner || input_type != 1 {
        return;
    }
    let Some(g) = m.allies.get(m.ally_index).copied() else {
        warn_text(w, "C020PortalAimer: Allies[AllyIndex] out of range");
        return;
    };
    let (gx, gy) = (w.fighter(g).body.x, actor_y(w, g));
    let m = m_mut(w, id);
    m.x = clamp_x(m, gx);
    m.y = clamp_y(m, gy);
    m.ally_index = if m.ally_index + 1 >= m.allies.len() { 0 } else { m.ally_index + 1 };
}

/// BPI_SetActivationConditions (SpawnedActorParent @4520 Array_Set): the air montage's ActivateSpawned writes [0].
fn set_condition(w: &mut World, id: ActorId, n: i32, value: bool) {
    let m = m_mut(w, id);
    if let Ok(i) = usize::try_from(n)
        && i < m.conds.len()
    {
        m.conds[i] = value;
    }
}

fn step(w: &mut World, id: ActorId) {
    let doe = flag(actor_p(w, ActorRef::Actor(id)), "DestroyOnExitedArena?");
    let m = m_mut(w, id);
    m.age += 1;
    let (partner, active) = (m.partner, m.input_active);
    // ReceiveTick @12125.
    if !partner && active {
        if !m_of(w, id).activated {
            move_(w, id);
        }
        aim(w, id);
        face(w, id);
    }
    // Its latent actions, after the tick (the BeginPlay Delay counts the spawn frame).
    let m = m_mut(w, id);
    if m.input_left > 0 {
        m.input_left -= 1;
        if m.input_left == 0 {
            m.input_active = true; // @30
        }
    }
    if m.release_left > 0 {
        m.release_left -= 1;
        if m.release_left == 0 {
            release(w, id); // @15
            return;
        }
    }
    // The parent's CheckBlastBoxTimer (0.1 s, looping): CheckBlastBoxExit @20219-20685, the location InRange (edges in) of the
    // stored BlastBoxBoundsMin / Max on each axis, else with DestroyOnExitedArena? DestroySpawnedActor(true).
    let m = m_mut(w, id);
    // The next expiry is rounded (`F(blastAt + F(0.1))`): the shared timer's double sum, rounded.
    let mut at = f64::from(m.blast_at);
    let gone = blast_timer_exit(doe, &mut at, &m.blast_bounds, crate::real(m.age - 1), m.x, m.y);
    m.blast_at = crate::math::narrow(at);
    if gone {
        release(w, id);
    }
}

/// The kind's `spawn`: CompSpawnActor at the notify's socket and translation (`spawn_place`), the parent's BeginPlay
/// blast-box loop, BeginPlay inside the spawn. Returns the state; BeginPlay itself runs from `spawned` once the actor is
/// listed.
fn spawn(w: &mut World, f: FighterId, r: &SpawnRequest) -> (WorldActorState, bool) {
    let np = request_props(w, f, r);
    let at = spawn_place(w, f, &np);
    let comps = actor_class_def(w, f, r.cls).map(|d| &d.components);
    let cap = comps.and_then(|c| obj(c, "Capsule"));
    let rad = cap.and_then(|c| pnum(c, "CapsuleRadius"));
    if rad.is_none() {
        warn_text(w, "C020PortalAimer: Capsule size missing");
    }
    let (conds, force, doe) = {
        let p = actor_class_def(w, f, r.cls).map(|d| &d.p);
        let conds = p
            .and_then(|p| p.get("ActivationConditions"))
            .and_then(mvs_data::value::Value::as_arr)
            .map_or_else(Vec::new, |a| a.iter().map(crate::registry::props::truthy).collect());
        (
            conds,
            p.is_some_and(|p| flag(p, "ForceSpawnPositionInsideBlastBox")),
            p.is_some_and(|p| flag(p, "DestroyOnExitedArena?")),
        )
    };
    let mut m = PortalAimer {
        x: at.x,
        y: at.y,
        radius: rad.unwrap_or(0.0),
        half_height: cap.and_then(|c| pnum(c, "CapsuleHalfHeight")).unwrap_or(0.0),
        conds,
        gun_left: crate::fighter::facing(w, f) < 0.0,
        ..PortalAimer::default()
    };
    let sb = spawn_box(w);
    // The parent's blast-box loop: the bounds stored, BPI_CheckIfPositionOverlaps(the jump-out area, edges in); outside,
    // ForceSpawnPositionInsideBlastBox clamps between the same corner and its mirror (@14585-14989). The class sets
    // ForceSpawnPositionInsideBlastBox: never the destroy.
    let dead = blast_box_loop(&sb, &mut m.x, &mut m.y, None, force, &mut m.blast_bounds, doe);
    (WorldActorState::PortalAimer(m), dead)
}

/// The rest of the kind's `spawn` that needs the actor's id: BeginPlay.
fn spawned(w: &mut World, id: ActorId) {
    begin_play(w, id);
}

/// Condition_C020PortalReadyForPartner.Check(actor): the actor's Comp_Pawn_Portal valid -> Get Allied Portals (cleaned first)
/// has length 1 @249; else false @307. Read by AnimNotify_ConditionSectionJumpSingleCheck on both portal montages (Release at
/// once with one portal standing).
fn ready_for_partner(w: &mut World, f: FighterId) -> bool {
    rick_of(w, f).is_some() && get_allied_portals(w, f).len() == 1
}

/// This file's registry entries (C020PortalAimer_C, Condition_C020PortalReadyForPartner_C).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let hooks = ActorHooks::World(WorldActorHooks {
        // nothing reads it (the Capsule's unscaled sizes; PortalGunMesh rides Rick's hand)
        size: Some(SizeHooks::default()),
        spawn: Some(spawn),
        spawned: Some(spawned),
        set_condition: Some(set_condition),
        activate: Some(activate),
        step: Some(step),
        end: Some(cleanup),
        // Its Capsule is NoCollision and it has no other body: no trace or overlap meets it.
        bodies: Some(|_, _, _| {}),
        ..WorldActorHooks::default()
    });
    r.actors.enter(AIMER, ActorEntry::new(ActorClass::World(WorldActorClass::C020PortalAimer), hooks))?;
    r.conditions.enter("Condition_C020PortalReadyForPartner_C", Condition::Fn(ready_for_partner))
}
