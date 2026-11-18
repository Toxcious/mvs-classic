//! TomAndJerry_SlingShot_Aim_02: the slingshot with Jerry
//! ([`SlingAim`], a SpawnedActorParent child with no ProjectileMovement): the aim with its trace alpha, aimed through
//! BPI_SetAimDirection (pawn.rs), Activate spawning Jerry at 3500 (jerry.rs spawn_jerry).
//!
//! Spawned by AnimNotify_Spawner of Tom_and_Jerry_SlingShot_Montage / _Air_Montage: Tom's location - (0, 0, 100), where it
//! stays; AnimNotify_ActivateSpawned (ActivateLastObject) at 0.718 / 0.721 s. Its AimInfo is the CDO's (no StoreAimInfo):
//! AimType 0, StartAimDirection (0, 1, 0), angles -45..45, AimSpeed 200.
//! - ReceiveBeginPlay @1582: the AimInfo copied with StartAimVector = (0, FacingLeft ? Y : -Y, Z), ArrowAimVector = it,
//!   then Delay(0.1) -> @15: not being destroyed and the owner's current montage neither SlingShot nor SlingShot_Air ->
//!   K2_DestroyActor.
//! - ReceiveTick @1984 (AimType 0): the ProjectileAimer turn rule (the stick's octant angle, the two limits, held at a
//!   difference >= 179, within 1 of the limit or no stick), then LineTraceSingle(location + arrow x 150, location + arrow
//!   x 800, TraceTypeQuery3 (WallJump), ignore self) @4692, the alpha @5203-5371, SetArrowRotationMulticast @5533:
//!   BPI_SetAimDirection(NormalizeToRange(angle(arrow), angle(start) + Min, angle(start) + Max) x 2 - 1) @6117-6972.
//! - ActivateObject @7048: the owner in hitstun -> nothing; else Jerry at Tom's location + (0, 0, 100) along ArrowAimVector
//!   at 3500, then K2_DestroyActor @1531.
// Unverified: CheckHitstunActive is the fighter's in_hitstun.
// Unverified: the parent's CheckBlastBoxTimer (0.1 s, looping, DestroyOnExitedArena?) is BMO's model (finn/bmo.rs): the
// actor never moves, so it only ends one spawned outside the blast box.

use super::jerry::{JERRY, spawn_jerry};
use super::pawn::set_aim_direction;
use super::state::{tj_mut, tj_of};
use crate::Real;
use crate::State;
use crate::actors::world_actor::{WorldActorState, actor_p};
use crate::actors::{ActorClass, ActorId, ActorRef, WorldActor, WorldActorClass};
use crate::consts::DELTA_SECONDS;
use crate::fighter::anim::grouped::active_instance;
use crate::fighter::anim::montages::slot_name;
use crate::fighter::common::{actor_class_def, latent_frames, warn_text};
use crate::fighter::match_side::SpawnRequest;
use crate::fighter::traces::{TRACE_WALL_JUMP, TraceBody, sphere_trace_single};
use crate::fighter::{FighterId, actor_y, facing, in_hitstun};
use crate::math::ue::{deg_atan2, rotate_angle_axis, round_to_int, size};
use crate::math::{max, min};
use crate::name;
use crate::names::Name;
use crate::registry::entries::{ActorEntry, ActorHooks, WorldActorHooks};
use crate::registry::props::{flag, num as pnum, obj};
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use crate::world::blast::{BlastBounds, blast_box_loop, blast_timer_exit_rounded, new_blast_bounds, spawn_box};
use crate::world::spawn_spot::spawn_place;
use crate::world::spawning::request_props;

/// The aimer's class.
pub const SLING_AIM: &str = "TomAndJerry_SlingShot_Aim_02_C";
const JERRY_N: Name = name!("Jerry_SpawnedActor_C");
const SLING: Name = name!("Tom_and_Jerry_SlingShot_Montage");
const SLING_AIR: Name = name!("Tom_and_Jerry_SlingShot_Air_Montage");
/// CompSpawnActor's InitialSpeed and MaxSpeed @1387.
const JERRY_SPEED: Real = 3500.0;

/// `SlingAim` (TomAndJerry_SlingShot_Aim_02_C): what the view reads of it (its place, the aim, the trace alpha) and its
/// aim state.
#[derive(State, Debug)]
pub struct SlingAim {
    pub x: Real,
    pub y: Real,
    /// What it last gave BPI_SetAimDirection.
    pub aim_direction: Real,
    /// The indicator's length alpha (its trace).
    pub trace_alpha: Real,
    /// ArrowAimVector (UE Y, Z).
    pub ay: Real,
    pub az: Real,
    /// StartAimVector (UE Y, Z).
    pub sy: Real,
    pub sz: Real,
    /// AimAngleMin, AimAngleMax, AimSpeed, AimType.
    pub aim_min: Real,
    pub aim_max: Real,
    pub aim_speed: Real,
    pub aim_type: Real,
    /// Delay(0.1) @1929 -> @15: frames left, 0 done.
    pub check_left: Real,
    /// Ticks, the spawn frame counted (the parent's CheckBlastBoxTimer).
    pub age: i32,
    pub blast_at: Real,
    pub blast_bounds: BlastBounds,
}

impl Default for SlingAim {
    /// The field initial values.
    fn default() -> Self {
        SlingAim {
            x: 0.0,
            y: 0.0,
            aim_direction: 0.0,
            trace_alpha: 0.0,
            ay: 0.0,
            az: 0.0,
            sy: 0.0,
            sz: 0.0,
            aim_min: 0.0,
            aim_max: 0.0,
            aim_speed: 0.0,
            aim_type: 0.0,
            check_left: 0.0,
            age: 0,
            blast_at: 0.1,
            blast_bounds: new_blast_bounds(),
        }
    }
}

/// The TomAndJerry_SlingShot_Aim_02_C's place (its `x`, `y`).
pub fn place(w: &World, a: &WorldActor, s: &SlingAim) -> Option<(Real, Real)> {
    let _ = (w, a);
    Some((s.x, s.y))
}

fn m_of(w: &World, id: ActorId) -> Option<&SlingAim> {
    match &w.actors.actors.get(id)?.state {
        WorldActorState::SlingAim(s) => Some(s),
        _ => None,
    }
}

fn m_mut(w: &mut World, id: ActorId) -> Option<&mut SlingAim> {
    match &mut w.actors.actors.get_mut(id)?.state {
        WorldActorState::SlingAim(s) => Some(s),
        _ => None,
    }
}

/// DegAtan2(Y, Z).
fn deg(y: Real, z: Real) -> Real {
    deg_atan2(y, z)
}

/// RotateAngleAxis((0, y, z), d, (1, 0, 0)).
fn rot(y: Real, z: Real, d: Real) -> (Real, Real) {
    let [_, y2, z2] = rotate_angle_axis([0.0, y, z], d, [1.0, 0.0, 0.0]);
    (y2, z2)
}

/// FindNearAngleDirection(Angle1, Angle2): the difference wrapped to the nearer turn (ReturnMult, Positive, the
/// difference rounded).
struct Near {
    mult: Real,
    pos: bool,
    diff: Real,
}

fn near(a1: Real, a2: Real) -> Near {
    let d0 = a1 + 180.0 - (a2 + 180.0);
    let s = if d0.abs() < (d0 - 360.0).abs() { d0 } else { d0 - 360.0 };
    let r = if s.abs() < (d0 + 360.0).abs() { s } else { d0 + 360.0 };
    Near { mult: if r < 0.0 { 1.0 } else { -1.0 }, pos: r < 0.0, diff: crate::math::real(round_to_int(r)) }
}

/// UKismetMathLibrary NormalizeToRange (4.26): equal ends give 0 below, 1 at or above; reversed ends are swapped.
fn normalize_to_range(v: Real, a: Real, b: Real) -> Real {
    if a == b {
        return if v < a { 0.0 } else { 1.0 };
    }
    let (lo, hi) = (min(a, b), max(a, b));
    (v - lo) / (hi - lo)
}

/// The stick's sign: 1, 0 or -1.
fn sign(v: Real) -> Real {
    if v == 0.0 {
        0.0
    } else if v > 0.0 {
        1.0
    } else {
        -1.0
    }
}

/// ReceiveTick's turn @2148-4461 (AimType 0).
fn turn(w: &mut World, id: ActorId) {
    let Some(f) = w.actors.actors.get(id).map(|a| a.owner) else { return };
    let fl = facing(w, f) < 0.0;
    let (mx, my) = (w.fighter(f).controls.inp.x, w.fighter(f).controls.inp.y);
    let Some(m) = m_mut(w, id) else { return };
    // `my > 0 ? -1 : 1` with a zero stick gives 0 (a positive zero).
    let target = deg(sign(mx), if my == 0.0 { 0.0 } else { -sign(my) });
    let la = rot(m.sy, m.sz, if fl { m.aim_max } else { -m.aim_min }); // @2775
    let lb = rot(m.sy, m.sz, if fl { m.aim_min } else { -m.aim_max }); // @2957
    let cur = deg(m.ay, m.az);
    let d_b = near(cur, deg(lb.0, lb.1)); // AngleDifrence
    let d_a = near(cur, deg(la.0, la.1)); // _1
    let d_t = near(cur, target); // _2
    let sel = (if d_t.pos { d_a.diff } else { d_b.diff }).abs();
    let hold = d_t.diff.abs() >= 179.0 || (m.aim_max != m.aim_min && sel <= 1.0) || (mx == 0.0 && my == 0.0);
    if hold {
        return;
    }
    let step = m.aim_speed * DELTA_SECONDS;
    let by = if m.aim_max == m.aim_min { step } else { min(max(step, 0.0), sel) };
    (m.ay, m.az) = rot(m.ay, m.az, by * d_t.mult);
}

/// The kind's `spawn`: CompSpawnActor at the notify's socket and translation (`spawn_place`), the parent's BeginPlay
/// blast-box loop, BeginPlay inside the spawn (the rest, once listed, is `spawned`).
fn spawn(w: &mut World, f: FighterId, r: &SpawnRequest) -> (WorldActorState, bool) {
    let np = request_props(w, f, r);
    let at = spawn_place(w, f, &np);
    let def = actor_class_def(w, f, r.cls).map(|d| &d.p);
    let info = def.and_then(|p| obj(p, "AimInfo"));
    if info.is_none() {
        warn_text(w, "TomAndJerry_SlingShot_Aim_02_C data missing: AimInfo");
    }
    let g = |k: &str| info.and_then(|i| pnum(i, k)).unwrap_or(0.0);
    let sd = info.and_then(|i| obj(i, "StartAimDirection"));
    let sdy = sd.and_then(|s| pnum(s, "Y")).unwrap_or(0.0);
    let sdz = sd.and_then(|s| pnum(s, "Z")).unwrap_or(0.0);
    let (force, doe) = (
        def.is_some_and(|p| flag(p, "ForceSpawnPositionInsideBlastBox")),
        def.is_some_and(|p| flag(p, "DestroyOnExitedArena?")),
    );
    // @801-1026: StartAimVector = (0, FacingLeft ? Y : -Y, Z); @1772: ArrowAimVector = StartAimVector.
    let sy = if facing(w, f) < 0.0 { sdy } else { -sdy };
    let mut m = SlingAim {
        x: at.x,
        y: at.y,
        aim_min: g("AimAngleMin"),
        aim_max: g("AimAngleMax"),
        aim_speed: g("AimSpeed"),
        aim_type: g("AimType"),
        sy,
        sz: sdz,
        check_left: latent_frames(0.1),
        ..SlingAim::default()
    };
    if m.aim_type == 0.0 {
        m.ay = m.sy;
        m.az = m.sz;
    }
    let sb = spawn_box(w);
    let dead = blast_box_loop(&sb, &mut m.x, &mut m.y, None, force, &mut m.blast_bounds, doe);
    (WorldActorState::SlingAim(m), dead)
}

/// The rest of the kind's `spawn` that needs the actor's id: the pawn's record of it.
fn spawned(w: &mut World, id: ActorId) {
    let Some(f) = w.actors.actors.get(id).map(|a| a.owner) else { return };
    if let Some(s) = tj_mut(w, f) {
        s.sling_aimer = Some(ActorRef::Actor(id));
    }
}

fn step(w: &mut World, id: ActorId) {
    let Some(f) = w.actors.actors.get(id).map(|a| a.owner) else { return };
    let doe = flag(actor_p(w, ActorRef::Actor(id)), "DestroyOnExitedArena?");
    let Some(m) = m_mut(w, id) else { return };
    m.age += 1;
    if m.aim_type == 0.0 {
        turn(w, id);
        // LineTraceSingle @4692 (TraceTypeQuery3, ignore self) and the alpha @5203-5371.
        let Some(m) = m_of(w, id) else { return };
        let (mx, my, ay, az) = (m.x, m.y, m.ay, m.az);
        let hit = sphere_trace_single(
            w,
            mx - ay * 150.0,
            my + az * 150.0,
            mx - ay * 800.0,
            my + az * 800.0,
            0.0,
            TRACE_WALL_JUMP,
            &[],
            Some(ActorRef::Actor(id)),
        );
        let alpha = match hit {
            Some(h) if h.blocking => 1.0 - normalize_to_range(size([0.0, mx - h.ix, my - h.iy]), 240.0, 850.0),
            _ => 0.0,
        };
        // SetArrowRotationMulticast @5533-6972: BPI_SetAimDirection on Tom's AnimBP (a Tom and Jerry's: another pawn's
        // anim instance is no Tom_and_Jerry_AnimBP).
        let fl = facing(w, f) < 0.0;
        let el = |y: Real, z: Real| deg_atan2(z, if fl { y } else { -y });
        let Some(m) = m_mut(w, id) else { return };
        m.trace_alpha = alpha;
        let s = el(m.sy, m.sz);
        m.aim_direction = normalize_to_range(el(m.ay, m.az), s + m.aim_min, s + m.aim_max) * 2.0 - 1.0;
        let dir = m.aim_direction;
        if tj_of(w, f).is_some() {
            set_aim_direction(w, f, dir);
        }
    }
    // Delay(0.1) @1929 -> @15 (the BeginPlay Delay counts the spawn frame).
    let Some(m) = m_mut(w, id) else { return };
    if m.check_left > 0.0 {
        m.check_left -= 1.0;
        if m.check_left == 0.0 {
            let cur = active_instance(w, f).map(|slot| slot_name(w, f, slot));
            if cur != Some(SLING) && cur != Some(SLING_AIR) {
                if let Some(a) = w.actors.actors.get_mut(id) {
                    a.dead = true;
                }
                return;
            }
        }
    }
    // The parent's CheckBlastBoxTimer (Unverified: BMO's model).
    let Some(m) = m_mut(w, id) else { return };
    let (x, y, age) = (m.x, m.y, m.age);
    let out = blast_timer_exit_rounded(doe, &mut m.blast_at, &m.blast_bounds, crate::real(age - 1), x, y);
    if out && let Some(a) = w.actors.actors.get_mut(id) {
        a.dead = true;
    }
}

/// ActivateObject @7048.
fn activate(w: &mut World, id: ActorId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    if a.dead {
        return;
    }
    let f = a.owner;
    if in_hitstun(w, f) {
        return; // Unverified: CheckHitstunActive is in_hitstun
    }
    let Some(m) = m_of(w, id) else { return };
    let (ay, az) = (m.ay, m.az);
    // @1104-1387: Jerry at Tom's location - 100 + 200 in Z, along ArrowAimVector (the spawner's mirror undone by the
    // FacingLeft select: UE Y of the world velocity = ay, sim vx = -ay x 3500).
    if actor_class_def(w, f, JERRY_N).is_none() {
        warn_text(w, &format!("spawned actor not exported ({JERRY})"));
    } else {
        let (fx, fy) = (w.fighter(f).body.x, actor_y(w, f));
        spawn_jerry(w, f, fx, fy + 100.0, -ay * JERRY_SPEED, az * JERRY_SPEED, false, JERRY_SPEED);
    }
    if let Some(a) = w.actors.actors.get_mut(id) {
        a.dead = true; // K2_DestroyActor @1531
    }
}

/// ReceiveEndPlay: HideUI (the view's). The pawn's record of it cleared.
fn end(w: &mut World, id: ActorId) {
    let Some(f) = w.actors.actors.get(id).map(|a| a.owner) else { return };
    if let Some(s) = tj_mut(w, f)
        && s.sling_aimer == Some(ActorRef::Actor(id))
    {
        s.sling_aimer = None;
    }
}

/// Its Capsule has no collision; the Sphere and the SkillShotIndicator are no bodies the traces meet.
fn bodies(_w: &World, _id: ActorId, _out: &mut Vec<TraceBody>) {}

/// This file's registry entries (TomAndJerry_SlingShot_Aim_02_C).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let hooks = WorldActorHooks {
        spawn: Some(spawn),
        spawned: Some(spawned),
        step: Some(step),
        activate: Some(activate),
        end: Some(end),
        bodies: Some(bodies),
        ..WorldActorHooks::default()
    };
    // Reads of the owner: its location, facing, stick, montage, hitstun (any fighter's).
    let entry = ActorEntry {
        foreign_owner: true,
        ..ActorEntry::new(ActorClass::World(WorldActorClass::TomAndJerrySlingShotAim02), ActorHooks::World(hooks))
    };
    r.actors.enter(SLING_AIM, entry)?;
    Ok(())
}
