//! JakeStretch_C ([`Stretch`], a DealerActor): the body Jake leaves behind in his
//! Stretch special (Jake_Stretch_Montage: Spawner at 0.1296 s with DestroyAllActorsOnNotifyEnd, ActivateSpawned at
//! 1.7989 s), its spline (`actors::spline`), the pull of the hit or the ledge, the Quick Stretch perk's test. Offsets
//! are the class ubergraph's unless a function is named. Its state on the pawn: `JakeState::stretch_actor`; the
//! AnimBP's StretchOn / CalculateStretchAngle are pawn.rs's setters.
//!
//! Place. Comp_Spawner's Translation (0, 0, 100), no socket (`spawn_place`: the owner's actor location); the actor is
//! static (no movement). Its yaw is the spawn's +-90 (the actor's local X is the owner's facing), so the Spline,
//! attached to the root at identity, has Local = actor-local (X along the facing, Z up): SplineCurve sx = the owner's
//! facing at the spawn.
//!
//! ReceiveBeginPlay @21400 -> @4273: the main body first, then AddSplineMesh (@1265), then the pushed flows in turn:
//!   main @4308: JakeMainMesh (BPI_GetMainMesh: the owner's Mesh), the KnockedBack binding (EX_BindDelegate @4695: the
//!     owner's KnockedBack -> @24169 -> ActivateMulticast), then for a Pawn_Jake Instigator (@4801) LocalBaseFighter,
//!     MeshScaleAmount = Mesh.RelativeScale3D.Y and the bottom half scaled by it, IsCake (AppliedSkin C005_S03 or
//!     Jake_S01, @5198-5616), AddInvisibility, SetSubmeshAlwaysVisible(TopHalf, true), UnhidEquippedWeapon @5862
//!     (weapons.rs), OwnerMat (a material: the view's);
//!   @4051 FacingLeft = LocalBaseFighter.FacingLeft (and the bottom half's AnimBP's);
//!   @6118 BPI_SetStretchOn(true), BPI_CalculateStretchAngle(true) on Jake's anim instance (pawn.rs);
//!   @6477 AddArmorMesh: its body @30201 is a bare pop_flow, and nothing sets AddedArmorMesh or calls ArmorBreak:
//!     armor stays false;
//!   @6552 BounceHousePerkActive = the Instigator has Buff_Jake_SafetyNet_C; @6952 IsSticky = the owner has
//!     Buff_Jake_QuickStretch_C; @7294 Delay(0.033) -> CanBounce = true (read only by ActorBeginOverlap's first gate,
//!     which together with the GetOwner gate leaves Jake out either way: not kept);
//!   @27 LastAddedSplinePosition = the actor's location.
//!
//! Tick @8410 (the pushes make it run 8430, 12687, 11657, 10515, 9421):
//!   @8430 a new point every IncementSize of length (UpdateSpline @30897-31359: NewSplinePointCount + 1,
//!     AddSplinePoint(World, no update), the one before it set to the same place, AddSplineMesh) while the end is
//!     within 600 of the last added one; the end is the StretchGoal socket when it is within 300 of Jake's location,
//!     else his location;
//!   @12687 the last point to the end, its tangent unit(end -> head socket, Y/Z) x 10 (World), the one before it
//!     unit(P[n-3] -> P[n-1]) x (the length before this tick's update - (meshes - 1) x IncementSize) (World, update);
//!     with two points (@8201) point 0 to the bottom half's StretchLegGoal socket and its tangent (0, 0, 50) (Local);
//!   @11657 every mesh i: SetStartAndEnd(point i, point i + 1, Local), StartScale CalculateTaper(i), EndScale
//!     CalculateTaper(i + 1);
//!   @10515 the wiggles: per record, age > 0.5 its point back to the saved place (update), else the saved place +
//!     WiggleDirection x WiggleCurve(age) x 60 (no update); records are never removed; then UpdateSpline;
//!   @9421 RTPCs cha_jake_length (the length), cha_jake_pitch (the head bone's rotation: not here, mvs-app's
//!     audio/jake.rs sends it from the drawn pose), cha_jake_velocity (PF_Velocity_Ak on an AkComponent added to Jake:
//!     |his location - the last| / delta).
//!
//! ActivateObject / the KnockedBack binding -> ActivateMulticast @20459: SetActorTickEnabled(false), Knock to
//! QueryOnly (its begin overlaps now), BPI_CalculateStretchAngle(false), the bottom half's Retracting, TotalMeshes,
//! LegReletiveLocation, Return.PlayFromStart, RTPCs velocity 2000 / pitch 2 / length 0 (interp 50).
//! Return (a timeline of 0.2 s, ReturnFloat linear 0 -> 1) Update @21420: the last point to Jake's StretchGoal socket
//! (update), the last mesh's end there; @17298 the meshes the travelled length (L x ReturnFloat) has passed are
//! destroyed from the front; CurrentLaunchVector = unit(spline at trav - 140 -> at trav) (World); the bottom half moved
//! to the spline at trav - 140 (Local; below 140 the saved place raised by trav) and pitched along that direction
//! (@20173: DegAtan2 of the World direction's Y, Z, negated facing left); mesh 0's start to the spline at trav, its
//! tangents (@14843); every mesh's scales CalculateTaperReturn (@1280-2553). Finished @20444: K2_DestroyActor.
//!
//! Knock (a sphere of 160 at (0, 0, 143) on the bottom half; HitBox, overlaps Pawn and HurtBox; NoCollision until the
//! activation) begin overlap @22005: an enemy of LocalBaseFighter takes pfgDoDamage(null HitBox) of a struct built
//! there (Damage Round(5 a), BaseKnockBack 850 a with a = 1 + NormalizeToRange(clamp(L, 600, 1700), 600, 1700),
//! the launch vector's Y / Z or (0, 0, 1) when it is zero).
//! ActorBeginOverlap @83 -> BounceServer -> BounceMulticast @25507, then BounceHouseServer.
//
// Unverified: the spline meshes' collision. The port takes each mesh's body as spheres along its Hermite curve (5 per
// mesh), radius Jake_StretchMesh's bounds extent Y/Z 83.8 x the scale lerped start -> end, and the root Capsule (22 /
// 44, Pawn only); a cooked build's USplineMeshComponent may keep the undeformed collision.
// Deviation: the overlaps are tested once a tick, after the actor's own tick or Return update (begin overlaps of a
// fighter's own move come in its tick in the build); the Knock's at the activation too.
// Unverified: the Return timeline ticks from the frame after its PlayFromStart (as Item_Sandwich's timeline); stock
// UTimelineComponent::PlayFromStart also fires the Update at position 0 inside the activation (SetPlaybackPosition
// with bFireUpdate), which the port runs on the next frame with the first tick's position.
// Deviation: StretchLegGoal is the bottom half's reference-pose socket (6.10376, 179.51) x MeshScaleAmount above the
// mesh's (0, 0, -143): JakeBottomHalfBlueprint's pose is not sampled.
// Unverified: GetLastUpdateVelocity (the spike branch) is taken as the Velocity.
// Deviation: KnockedBack by the owner's knockedBackCount, read at the top of the actor's step (as fastball.rs) and,
// for a hit that also ends the Stretch montage (its Spawner's DestroyAllActorsOnNotifyEnd: World.step's despawn pass
// runs before the actors' step), at the actor's end: the build's ActivateMulticast runs inside the KnockedBack
// broadcast (the Knock's begin overlaps and hits there), the port's later in the same frame, then the actor goes.
// HitEachActorOnlyOnce with a null HitBox: pfgDoDamage still checks (CheckIfActorHasBeenHit @2883) and records
// (TrackActorsHitByTag @5290, past the invulnerable gate @4682) the victim under AttackTag 'None' in the actor's own
// Comp_Pawn_HitBoxHandler, which nothing here clears: each victim is hit by the Knock once per actor (`hit_ids`).
// The dealer's own hit pause (ApplyHitPauseToDamageDealer) is not ported port-wide.
// Unverified: the spline's points are set in world space and so stay; its local-space reads (GetSplineLength,
// GetLocationAtDistanceAlongSpline(Local)) and the bottom half's relative place are taken in world units at any
// root scale (`size_scale`).

use crate::Real;
use crate::State;
use crate::actors::spline::{
    REPARAM_STEPS, SplineCurve, SplineSpace, add_spline_point, find_location_closest_to_world_location,
    get_location_at_distance_along_spline, get_location_at_spline_point, get_tangent_at_spline_point, new_spline_curve,
    set_location_at_spline_point, set_tangent_at_spline_point, spline_length, update_spline_curve,
};
use crate::actors::world_actor::WorldActorState;
use crate::actors::{ActorClass, ActorId, ActorRef, WorldActor, WorldActorClass};
use crate::combat::boxes::WorldSphere;
use crate::combat::elem::charge_pct;
use crate::combat::hit::{
    add_hit_stun_from_knockback, hurt_hit, remove_hitstun_from_knock_back, set_hit_pause, timer_count,
};
use crate::combat::hits::{actor_hit_enemy, apply_projectile_hit, dealer, projectile_blocks};
use crate::combat::phases::{alive, give_vbuf, victim_pose};
use crate::combat::{DataEnum, DataVec, HitPacket};
use crate::consts::{AIR_EVADE_MAX, DELTA_SECONDS, GRAVITY_SCALE};
use crate::events::Event;
use crate::fighter::anim::sampled_boxes;
use crate::fighter::anim::slot_mix::set_sub_mesh;
use crate::fighter::anim::{add_invisibility, remove_invisibility, set_submesh_always_visible};
use crate::fighter::body::{cap_radius, capsule_half, no_collision};
use crate::fighter::common::{actor_data, warn_text};
use crate::fighter::control::{attack_dodged, dodge_over};
use crate::fighter::match_side::SpawnRequest;
use crate::fighter::moves::fast_fall_end;
use crate::fighter::moves::max_jumps;
use crate::fighter::moves::reset_jump_counts;
use crate::fighter::moves::{Impulse, V2};
use crate::fighter::{FighterId, actor_y, facing, invulnerable};
use crate::math::ue::{SMALL_NUMBER, deg_atan2, normal, round_to_int, safe_normal, size, turn_by_pitch};
use crate::math::{max, min};
use crate::name;
use crate::names::Name;
use crate::registry::entries::{SizeHooks, WorldActorHooks};
use crate::registry::props::{num, num_or, obj};
use crate::registry::{ActorEntry, ActorHooks, Registry, RegistryError};
use crate::world::World;
use crate::world::flow::reset_pass;
use crate::world::pawns::match_fighters;
use crate::world::spawn_spot::spawn_place;
use crate::world::spawning::request_props;

use super::pawn::{calculate_stretch_angle, set_stretch_on};
use super::perks::{QUICK_STRETCH, SAFETY_NET};
use super::state::{jake_mut, jake_of};
use super::weapons::unhid_equipped_weapon;

/// JakeStretch's class.
pub const STRETCH: &str = "JakeStretch_C";

const KNOCK_RADIUS: Real = 160.0; // Knock.SphereRadius
const KNOCK_UP: Real = 143.0; // Knock RelativeLocation Z on the bottom half
const LEG_Z: Real = -143.0; // Jake_BotHalf_SkelMesh RelativeLocation Z
const LEG_GOAL_X: Real = 6.10376; // StretchLegGoal in the bottom half's reference pose (Deviation above)
const LEG_GOAL_Z: Real = 179.51;
const MESH_HALF: Real = 83.8; // Jake_StretchMesh bounds extent Y / Z
const ROOT_R: Real = 22.0; // the root Capsule (CapsuleComponent defaults)
const ROOT_HALF: Real = 44.0;
const RETURN_LEN: Real = 0.2; // Return timeline length
const WIGGLE_SCALE: Real = 60.0; // @11331
const BODY_SAMPLES: i32 = 5;

/// A WiggleInfo record.
#[derive(State, Debug, Default)]
pub struct Wiggle {
    /// PointNumber.
    pub pt: i32,
    /// SavedLocation (World).
    pub sx: Real,
    pub sy: Real,
    /// WiggleDirection.
    pub dx: Real,
    pub dy: Real,
    /// HitTime.
    pub t: Real,
}

/// `Stretch` (JakeStretch_C): the class's fields, under their names (the view's and its own). Sim axes.
#[derive(State, Debug)]
pub struct Stretch {
    /// The `hit` (KNOCK_HIT: a sparse packet; the Knock's hit is built per hit from it).
    pub hit: HitPacket,
    /// The AttackID of the spawn request.
    pub attack_id: i32,
    /// Its root's place.
    pub x: Real,
    pub y: Real,
    /// The owner's facing at the spawn (the actor's yaw side).
    pub facing: Real,
    /// FacingLeft.
    pub facing_left: bool,
    /// The Spline component's points (sim x, y per point, flat), as UpdateSpline leaves them this tick, and their
    /// count.
    pub points: Vec<Real>,
    pub count: i32,
    /// Per added spline mesh its start and end scale, flat.
    pub scales: Vec<Real>,
    /// AddedArmorMesh shown (never).
    pub armor: bool,
    /// IsSticky.
    pub sticky: bool,
    /// Per added spline mesh, what its calls last left, in sim world: start place, start tangent, end place, end
    /// tangent (flat, 8 each).
    pub ends: Vec<Real>,
    /// Per mesh its Diffuse_Mult.
    pub dims: Vec<Real>,
    /// Jake_BotHalf_SkelMesh: its origin in sim world and its relative pitch in degrees.
    pub leg_x: Real,
    pub leg_y: Real,
    pub leg_pitch: Real,
    /// The bottom half's AnimBP Retracting.
    pub retracting: bool,
    /// GetSplineLength.
    pub length: Real,
    /// Its Spline component.
    pub spline: SplineCurve,
    /// The Instigator is a Pawn_Jake (LocalBaseFighter set).
    pub jake: bool,
    /// MeshScaleAmount.
    pub mesh_scale: Real,
    /// NewSplinePointCount.
    pub new_count: Real,
    /// LastAddedSplinePosition.
    pub last_x: Real,
    pub last_y: Real,
    /// BounceHousePerkActive.
    pub bounce_house: bool,
    /// The actor's tick (SetActorTickEnabled(false) at the activation).
    pub ticking: bool,
    /// AddedSplineMeshes, per mesh: start / end place and tangent (Local, 8), scales (2), Diffuse_Mult.
    pub mesh_ends: Vec<Real>,
    pub mesh_scales: Vec<Real>,
    pub mesh_dims: Vec<Real>,
    /// WiggleInfo.
    pub wiggles: Vec<Wiggle>,
    /// BouncedFighters (fighter ids).
    pub bounced: Vec<i32>,
    /// The HitBoxHandler's ActorsHitByTag['None'] (fighter ids).
    pub hit_ids: Vec<i32>,
    /// The fighters the actor overlaps (any component).
    pub touching: Vec<i32>,
    /// Knock's overlapping fighters.
    pub knock_in: Vec<i32>,
    /// Knock's collision.
    pub knock_on: bool,
    /// The owner's knockedBackCount at the bind.
    pub kb_seq: i32,
    /// TotalMeshes.
    pub total: i32,
    /// Jake_BotHalf_SkelMesh RelativeLocation (local X, Z).
    pub leg_rel_x: Real,
    pub leg_rel_y: Real,
    /// LegReletiveLocation.
    pub leg_save_x: Real,
    pub leg_save_y: Real,
    /// The bottom half's up axis in sim world (its pitch direction).
    pub pitch_dx: Real,
    pub pitch_dy: Real,
    /// CurrentLaunchVector in sim world (class default (0, 0, 1)).
    pub launch_x: Real,
    pub launch_y: Real,
    /// The Return timeline's position, whether it plays, and the tick of its PlayFromStart.
    pub tl: Real,
    pub playing: bool,
    pub act_tick: i32,
    /// SFX_Velocity: the AkComponent's previous location (the owner's, zero at first).
    pub vel_x: Real,
    pub vel_y: Real,
    /// IncementSize, TaperMin, StrechTaperDistance, BounceMultiplier, MinBounceVelocity, MaxBounceVelocity,
    /// SpikeMultiplier, MinimumSpikeVelocity.
    pub inc: Real,
    pub taper_min: Real,
    pub taper_dist: Real,
    pub bounce_mul: Real,
    pub bounce_min: Real,
    pub bounce_max: Real,
    pub spike_mul: Real,
    pub spike_min: Real,
}

impl Default for Stretch {
    /// The field initial values.
    fn default() -> Self {
        Stretch {
            hit: knock_hit_packet(),
            attack_id: 0,
            x: 0.0,
            y: 0.0,
            facing: 0.0,
            facing_left: false,
            points: Vec::new(),
            count: 0,
            scales: Vec::new(),
            armor: false,
            sticky: false,
            ends: Vec::new(),
            dims: Vec::new(),
            leg_x: 0.0,
            leg_y: 0.0,
            leg_pitch: 0.0,
            retracting: false,
            length: 0.0,
            spline: SplineCurve::default(),
            jake: false,
            mesh_scale: 1.0,
            new_count: 1.0,
            last_x: 0.0,
            last_y: 0.0,
            bounce_house: false,
            ticking: true,
            mesh_ends: Vec::new(),
            mesh_scales: Vec::new(),
            mesh_dims: Vec::new(),
            wiggles: Vec::new(),
            bounced: Vec::new(),
            hit_ids: Vec::new(),
            touching: Vec::new(),
            knock_in: Vec::new(),
            knock_on: false,
            kb_seq: 0,
            total: 0,
            leg_rel_x: 0.0,
            leg_rel_y: LEG_Z,
            leg_save_x: 0.0,
            leg_save_y: LEG_Z,
            pitch_dx: 0.0,
            pitch_dy: 1.0,
            launch_x: 0.0,
            launch_y: 1.0,
            tl: 0.0,
            playing: false,
            act_tick: -1,
            vel_x: 0.0,
            vel_y: 0.0,
            inc: 150.0,
            taper_min: 0.4,
            taper_dist: 1500.0,
            bounce_mul: 1.2,
            bounce_min: 1200.0,
            bounce_max: 2400.0,
            spike_mul: 1.3,
            spike_min: 1200.0,
        }
    }
}

/// The Knock's HitBoxDamage struct (@22322-23890: `KNOCK_HIT`); Damage, BaseKnockBack and KnockBackDirectionVector are
/// set per hit. A packet built as an object literal.
fn knock_hit_packet() -> HitPacket {
    HitPacket {
        sparse: true,
        damage: Some(0.0),
        base_knock_back: Some(0.0),
        knock_back_scaling: Some(16.0),
        attack_tag: Some(name!("None")),
        hit_reaction: Some(DataEnum::Value(2)),
        hit_pause_time: Some(0.125),
        hit_stun_mult: Some(1.0),
        on_hit_audio: Some(name!("sfx_cha_jake_stretch_hit")),
        directional_knockback: Some(false),
        velocity_instead_of_facing: Some(false),
        knock_back_mirror: Some(false),
        ignore_facing: Some(true),
        attack_id: Some(0),
        do_los_check: Some(false),
        force_knockback: Some(true),
        queue_hit_pause_until_poses: Some(false),
        hit_only_one_fighter: Some(false),
        armor_pierce: Some(false),
        destroy_projectiles: Some(false),
        reflect_projectiles: Some(false),
        vacuum_knockback: Some(false),
        direct_knockback_towards_socket: Some(false),
        play_on_hit_effects: Some(true),
        apply_hit_pause_to_damage_dealer: Some(true),
        attack_decay_type: Some(DataEnum::Value(0)),
        additive_kb_mode: Some(DataEnum::Value(0)),
        kb_scaling_starting_dmg: Some(0.0),
        hit_each_actor_only_once: Some(true),
        ignore_kbi: Some(false),
        kbi_multiplier: Some(1.0),
        ..HitPacket::default()
    }
}

fn state(w: &World, id: ActorId) -> &Stretch {
    match &w.actors.actors.get(id).expect("a live world actor").state {
        WorldActorState::Stretch(s) => s,
        _ => panic!("a JakeStretch_C state"),
    }
}

fn state_mut(w: &mut World, id: ActorId) -> &mut Stretch {
    match &mut w.actors.actors.get_mut(id).expect("a live world actor").state {
        WorldActorState::Stretch(s) => s,
        _ => panic!("a JakeStretch_C state"),
    }
}

fn owner_of(w: &World, id: ActorId) -> FighterId {
    w.actors.actors.get(id).expect("a live world actor").owner
}

fn is_dead(w: &World, id: ActorId) -> bool {
    w.actors.actors.get(id).is_none_or(|a| a.dead)
}

fn size_scale(w: &World, id: ActorId) -> Real {
    w.actors.actors.get(id).and_then(|a| a.size_scale).unwrap_or(1.0)
}

/// The JakeStretch_C's place (its `x`, `y`).
pub fn place(w: &World, a: &WorldActor, s: &Stretch) -> Option<(Real, Real)> {
    let _ = (w, a);
    Some((s.x, s.y))
}

/// The JakeStretch_C as a dealer (`DealerActor`): its place, facing and AttackID.
pub fn dealer_parts(w: &World, a: &WorldActor, s: &Stretch) -> (Real, Real, Real, i32) {
    let _ = (w, a);
    (s.x, s.y, s.facing, s.attack_id)
}

/// The JakeStretch_C's hit packet (`hit`, KNOCK_HIT).
pub fn hit(s: &Stretch) -> &HitPacket {
    &s.hit
}

// ------------------------------------------------------------------ helpers

/// GetDirectionUnitVector(From, To): (To - From).GetSafeNormal().
fn unit(fx: Real, fy: Real, tx: Real, ty: Real) -> V2 {
    let [_, x, y] = safe_normal([0.0, tx - fx, ty - fy], SMALL_NUMBER);
    V2 { x, y }
}

/// GetDistanceAlongSplineAtSplinePoint.
// Unverified: the stock 4.26 reading: the reparam table's entry at the point, 0 out of range.
fn dist_at(s: &SplineCurve, i: i32) -> Real {
    let n = i32::try_from(s.pts.len()).unwrap_or(i32::MAX);
    if i < 0 || i >= n {
        return 0.0;
    }
    usize::try_from(i * REPARAM_STEPS).ok().and_then(|k| s.table.get(k)).map_or(0.0, |e| e.d)
}

/// A socket of the fighter's main mesh in sim world; the actor location (and false) when the boxes lack it.
fn socket(w: &World, f: FighterId, name: &str) -> (V2, bool) {
    let ch = w.fighter(f).ch;
    let si = w.names.get(&format!("socket:{name}")).and_then(|s| w.fixed.samplers.get(&ch)?.by_name.get(&s).copied());
    let off = si.and_then(|si| w.fixed.samplers.get(&ch)?.offsets.get(usize::try_from(si).ok()?).copied());
    if let Some(o) = off
        && let Some(pose) = sampled_boxes(w, f)
    {
        let o = usize::try_from(o).expect("a box offset");
        let at = V2 {
            x: crate::fighter::mesh_wx(w, f) + crate::real(pose[o]) * facing(w, f),
            y: crate::fighter::mesh_wy(w, f) + crate::real(pose[o + 1]),
        };
        crate::fighter::anim::put_pose(w, pose);
        return (at, true);
    }
    warn_text(w, &format!("{STRETCH}: socket {name} not sampled (the actor location taken)"));
    (V2 { x: w.fighter(f).body.x, y: actor_y(w, f) }, false)
}

/// The Tick's end: the StretchGoal socket when IsValid(JakeMainMesh) and it is within 300 of Jake's location, else his
/// location (@8631-8895).
fn end_point(w: &World, f: FighterId) -> V2 {
    let (at, _) = socket(w, f, "StretchGoal");
    if size([0.0, at.x - w.fighter(f).body.x, at.y - actor_y(w, f)]) > 300.0 {
        return V2 { x: w.fighter(f).body.x, y: actor_y(w, f) };
    }
    at
}

/// CalculateTaper(PointIndex).
fn taper(a: &Stretch, i: i32) -> Real {
    let l = spline_length(&a.spline);
    let half = l / 2.0;
    let center = 1.0 + (a.taper_min - 1.0) * min(max(l / a.taper_dist, 0.0), 1.0);
    let d = dist_at(&a.spline, i);
    let alpha = if d > half { (d - half) / half } else { (half - d) / half };
    a.mesh_scale * (center + (1.0 - center) * alpha)
}

/// CalculateTaperReturn(NewSplineLength, Index, IsFirst).
fn taper_return(a: &Stretch, trav: Real, i: i32, first: bool) -> Real {
    let l = spline_length(&a.spline);
    let r = l - trav;
    let half = r / 2.0;
    let center = 1.0 + (a.taper_min - 1.0) * min(max(l / a.taper_dist, 0.0), 1.0);
    let sel = if first { r } else { dist_at(&a.spline, i) - trav };
    let alpha = if sel > half { (sel - half) / half } else { (half - sel) / half };
    let v = a.mesh_scale * (center + (1.0 - center) * alpha);
    min(max(v, 0.0), a.mesh_scale)
}

/// GetLocationAndTangentAtSplinePoint(i, Local) into mesh entry `at` (+ 0 start, + 4 end).
fn set_mesh_end(a: &mut Stretch, at: usize, i: i32, o: usize) {
    let p = get_location_at_spline_point(&a.spline, i, SplineSpace::Local);
    let t = get_tangent_at_spline_point(&a.spline, i, SplineSpace::Local);
    let e = &mut a.mesh_ends;
    e[at + o] = p.x;
    e[at + o + 1] = p.y;
    e[at + o + 2] = t.x;
    e[at + o + 3] = t.y;
}

/// AddSplineMesh @2573: a mesh from point n + 1 to n + 2 (both clamped to the last point), Diffuse_Mult by the air
/// resources.
fn add_spline_mesh(w: &World, f: FighterId, a: &mut Stretch) {
    let n = i32::try_from(a.spline.pts.len()).unwrap_or(i32::MAX);
    let fi = w.fighter(f);
    let spent = crate::real(fi.air.air_specials) == crate::fighter::air_special_max(w, f)
        && crate::real(fi.jump.jumps) == max_jumps(w, f)
        && crate::real(fi.air.air_evades) == AIR_EVADE_MAX;
    a.mesh_dims.push(if spent { 0.7 } else { 1.0 });
    let at = a.mesh_ends.len();
    a.mesh_ends.resize(at + 8, 0.0);
    set_mesh_end(a, at, n + 1, 0);
    set_mesh_end(a, at, n + 2, 4);
    a.mesh_scales.extend([1.0, 1.0]);
}

/// UpdateSpline(SplinePosition) @31258 (the custom event, not the component call).
fn update_spline(w: &World, f: FighterId, a: &mut Stretch, x: Real, y: Real) {
    a.last_x = x;
    a.last_y = y;
    a.new_count += 1.0;
    add_spline_point(&mut a.spline, x, y, SplineSpace::World, false);
    let n = i32::try_from(a.spline.pts.len()).unwrap_or(i32::MAX);
    if n - 2 > 0 {
        set_location_at_spline_point(&mut a.spline, n - 2, x, y, SplineSpace::World, false);
    }
    add_spline_mesh(w, f, a);
}

/// The bottom half's local place (lx along the facing, lz up) in sim world.
fn leg_world(a: &Stretch, lx: Real, lz: Real) -> V2 {
    V2 { x: a.x + a.facing * lx, y: a.y + lz }
}

fn mesh_count(a: &Stretch) -> Real {
    crate::real(i32::try_from(a.mesh_dims.len()).unwrap_or(i32::MAX))
}

fn rtpc(w: &mut World, f: FighterId, n: Name, value: Real, interp_ms: Real) {
    let fighter = w.fighter(f).id;
    w.events.push(Event::Rtpc { fighter, name: n, value, interp_ms });
}

// ------------------------------------------------------------------ BeginPlay / EndPlay

/// ReceiveEndPlay @7365 (the part past the Lab reset's end pass).
fn end_play(w: &mut World, id: ActorId) {
    let f = owner_of(w, id);
    let jake = state(w, id).jake;
    if let Some(js) = jake_mut(w, f)
        && js.stretch_actor == Some(ActorRef::Actor(id))
    {
        js.stretch_actor = None;
    }
    if reset_pass(w) || !jake {
        return; // the Lab reset's end pass acts on the old pawns
    }
    remove_invisibility(w, f);
    set_submesh_always_visible(w, f, name!("TopHalf"), false);
    set_sub_mesh(w, f, name!("TopHalf"), false);
    set_stretch_on(w, f, false);
    calculate_stretch_angle(w, f, true);
}

// ------------------------------------------------------------------ Tick

/// Tick @8410.
fn tick(w: &mut World, f: FighterId, a: &mut Stretch) {
    // @8430
    if (spline_length(&a.spline) / a.inc) > a.new_count {
        let e = end_point(w, f);
        if size([0.0, a.last_x - e.x, a.last_y - e.y]) < 600.0 {
            update_spline(w, f, a, e.x, e.y);
        }
    }
    // @12687
    let mut n = i32::try_from(a.spline.pts.len()).unwrap_or(i32::MAX);
    let e = end_point(w, f);
    set_location_at_spline_point(&mut a.spline, n - 1, e.x, e.y, SplineSpace::World, false);
    let (head, _) = socket(w, f, "fml_un_C_head");
    let v = unit(e.x, e.y, head.x, head.y);
    set_tangent_at_spline_point(&mut a.spline, n - 1, v.x * 10.0, v.y * 10.0, SplineSpace::World, false);
    n = i32::try_from(a.spline.pts.len()).unwrap_or(i32::MAX);
    let l = spline_length(&a.spline); // before this tick's update
    let mag = l - ((mesh_count(a) - 1.0) * a.inc);
    let last = get_location_at_spline_point(&a.spline, n - 1, SplineSpace::World);
    let back = get_location_at_spline_point(&a.spline, n - 3, SplineSpace::World);
    let v = unit(back.x, back.y, last.x, last.y);
    set_tangent_at_spline_point(&mut a.spline, n - 2, v.x * mag, v.y * mag, SplineSpace::World, true);
    if n - 1 == 1 {
        // @8201: the bottom half's StretchLegGoal (Deviation above), then the tangent (0, 0, 50) Local
        let ms = a.mesh_scale;
        let g = leg_world(a, LEG_GOAL_X * ms, LEG_Z + LEG_GOAL_Z * ms);
        set_location_at_spline_point(&mut a.spline, 0, g.x, g.y, SplineSpace::World, false);
        set_tangent_at_spline_point(&mut a.spline, 0, 0.0, 50.0, SplineSpace::Local, true);
    }
    // @11657 (UpdateSplineMeshesMulticast)
    for i in 0..a.mesh_dims.len() {
        let k = i32::try_from(i).unwrap_or(i32::MAX);
        set_mesh_end(a, i * 8, k, 0);
        set_mesh_end(a, i * 8, k + 1, 4);
        a.mesh_scales[i * 2] = taper(a, k);
        a.mesh_scales[i * 2 + 1] = taper(a, k + 1);
    }
    // @10515
    if !a.wiggles.is_empty() {
        let now = w.fighter(f).clock.time;
        for r in &a.wiggles {
            let age = now - r.t;
            if age > 0.5 {
                set_location_at_spline_point(&mut a.spline, r.pt, r.sx, r.sy, SplineSpace::World, true);
            } else {
                let k = wiggle_curve(age) * WIGGLE_SCALE;
                set_location_at_spline_point(
                    &mut a.spline,
                    r.pt,
                    r.sx + r.dx * k,
                    r.sy + r.dy * k,
                    SplineSpace::World,
                    false,
                );
            }
        }
        update_spline_curve(&mut a.spline);
    }
    // @9421
    rtpc(w, f, name!("cha_jake_length"), spline_length(&a.spline), 0.0);
    let (fx, fy) = (w.fighter(f).body.x, actor_y(w, f));
    let vel = size([0.0, fx - a.vel_x, fy - a.vel_y]) / DELTA_SECONDS;
    a.vel_x = fx;
    a.vel_y = fy;
    rtpc(w, f, name!("cha_jake_velocity"), vel, 0.0);
}

/// JakeStretchWiggleCurve: keys [time, value, arrive, leave]; keys 0..5 cubic, key 6 linear (FRichCurve::Eval).
const WIGGLE_KEYS: [[Real; 4]; 7] = [
    [0.0, -2.0489097e-8, 0.0, 0.0],
    [0.09, 1.0, -0.01895515, -0.01895515],
    [0.1568198, -0.8, -0.009055188, -0.009055188],
    [0.28272793, 0.6, 0.018950965, 0.018950965],
    [0.35394114, -0.39999998, -0.011204494, -0.011204494],
    [0.45394114, 0.20000005, -0.00019490762, -0.00019490762],
    [0.5, -5.0934723e-9, 0.0, 0.0],
];
const WIGGLE_CUBIC: usize = 6;

fn wiggle_curve(t: Real) -> Real {
    let k = &WIGGLE_KEYS;
    if t <= k[0][0] {
        return k[0][1];
    }
    if t >= k[k.len() - 1][0] {
        return k[k.len() - 1][1];
    }
    let mut i = 0;
    while k[i + 1][0] <= t {
        i += 1;
    }
    let (a, b) = (&k[i], &k[i + 1]);
    let gap = b[0] - a[0];
    let u = (t - a[0]) / gap;
    if i >= WIGGLE_CUBIC {
        return a[1] + (b[1] - a[1]) * u;
    }
    let p1 = a[1] + (a[3] * gap) / 3.0;
    let p2 = b[1] - (b[2] * gap) / 3.0;
    let v = 1.0 - u;
    v * v * v * a[1] + 3.0 * v * v * u * p1 + 3.0 * v * u * u * p2 + u * u * u * b[1]
}

// ------------------------------------------------------------------ activation and Return

/// ActivateMulticast @20459 (BPI_ActivateObject @14828, the KnockedBack binding @24169).
fn activate(w: &mut World, id: ActorId) {
    if is_dead(w, id) {
        return;
    }
    activate_now(w, id);
}

fn activate_now(w: &mut World, id: ActorId) {
    let f = owner_of(w, id);
    {
        let a = state_mut(w, id);
        a.ticking = false;
        a.knock_on = true; // Knock.SetCollisionEnabled(QueryOnly): its begin overlaps now
    }
    overlaps(w, id);
    let jake = state(w, id).jake;
    if jake {
        calculate_stretch_angle(w, f, false); // @20757
    }
    let tick = w.flow.tick;
    {
        let a = state_mut(w, id);
        a.retracting = true; // @21361
        a.total = i32::try_from(a.mesh_dims.len()).unwrap_or(i32::MAX); // @21076
        a.leg_save_x = a.leg_rel_x;
        a.leg_save_y = a.leg_rel_y;
        a.tl = 0.0; // Return.PlayFromStart
        a.playing = true;
        a.act_tick = tick;
    }
    // @20796
    rtpc(w, f, name!("cha_jake_velocity"), 2000.0, 50.0);
    rtpc(w, f, name!("cha_jake_pitch"), 2.0, 50.0);
    rtpc(w, f, name!("cha_jake_length"), 0.0, 50.0);
    view(state_mut(w, id));
}

/// Return's Update @21420 with ReturnFloat = rf.
fn return_update(w: &World, f: FighterId, a: &mut Stretch, rf: Real) {
    let n = i32::try_from(a.spline.pts.len()).unwrap_or(i32::MAX);
    let (g, _) = socket(w, f, "StretchGoal"); // LocalBaseFighter.Mesh StretchGoal (no 300 check here)
    set_location_at_spline_point(&mut a.spline, n - 1, g.x, g.y, SplineSpace::World, true);
    if !a.mesh_dims.is_empty() {
        let last = a.mesh_dims.len() - 1;
        let p = get_location_at_spline_point(&a.spline, n - 1, SplineSpace::Local);
        a.mesh_ends[last * 8 + 4] = p.x;
        a.mesh_ends[last * 8 + 5] = p.y;
    }
    // @17298
    if a.mesh_dims.is_empty() {
        return;
    }
    let l = spline_length(&a.spline);
    let trav = l * rf;
    let mut k = (i32::try_from(a.mesh_dims.len()).unwrap_or(i32::MAX) - a.total).abs();
    while !a.mesh_dims.is_empty() && trav >= dist_at(&a.spline, k + 1) {
        a.mesh_ends.drain(..8);
        a.mesh_scales.drain(..2);
        a.mesh_dims.remove(0);
        k = (i32::try_from(a.mesh_dims.len()).unwrap_or(i32::MAX) - a.total).abs();
    }
    // @18463: CurrentLaunchVector
    let at = get_location_at_distance_along_spline(&a.spline, trav, SplineSpace::World);
    let back = get_location_at_distance_along_spline(&a.spline, trav - 140.0, SplineSpace::World);
    let v = unit(back.x, back.y, at.x, at.y);
    a.launch_x = v.x;
    a.launch_y = v.y;
    // @19324-19764: the bottom half's place
    if trav >= 140.0 {
        let t = get_location_at_distance_along_spline(&a.spline, trav - 140.0, SplineSpace::Local);
        a.leg_rel_x = t.x;
        a.leg_rel_y = t.y;
    } else {
        a.leg_rel_x = a.leg_save_x;
        a.leg_rel_y = a.leg_save_y + trav;
    }
    // @19816-20363: its pitch, DegAtan2(Y, Z) of the World direction (UE Y = -sim x), negated facing left
    let (dx, dy) = (v.x, v.y);
    if dx == 0.0 && dy == 0.0 {
        a.leg_pitch = 0.0;
        a.pitch_dx = 0.0;
        a.pitch_dy = 1.0;
    } else {
        a.leg_pitch = deg_atan2(-a.facing * dx, dy);
        a.pitch_dx = dx;
        a.pitch_dy = dy;
    }
    // @14843: mesh 0
    if !a.mesh_dims.is_empty() {
        let at = get_location_at_distance_along_spline(&a.spline, trav, SplineSpace::Local);
        let back = get_location_at_distance_along_spline(&a.spline, trav - 140.0, SplineSpace::Local);
        let v = unit(back.x, back.y, at.x, at.y);
        let d1 = dist_at(&a.spline, k + 1);
        let sel_d = if trav >= d1 { dist_at(&a.spline, k + 2) } else { d1 };
        let fac = 200.0 * min(max((sel_d - trav) / a.inc, 0.0), 1.0);
        let t = get_tangent_at_spline_point(&a.spline, k + 1, SplineSpace::Local);
        let [_, tnx, tny] = normal([0.0, t.x, t.y], 1e-4);
        let e = &mut a.mesh_ends;
        e[0] = at.x;
        e[1] = at.y;
        e[2] = v.x * fac;
        e[3] = v.y * fac;
        e[6] = tnx * fac;
        e[7] = tny * fac;
    }
    // @1280-2553: the scales
    for i in 0..a.mesh_dims.len() {
        let k2 = i32::try_from(i).unwrap_or(i32::MAX) + k;
        a.mesh_scales[i * 2] = taper_return(a, trav, k2, i == 0);
        a.mesh_scales[i * 2 + 1] = taper_return(a, trav, k2 + 1, false);
    }
    let _ = w;
}

// ------------------------------------------------------------------ overlaps

/// The meshes' bodies as spheres (Unverified above), in sim world.
fn body_spheres(a: &Stretch, scale: Real) -> Vec<WorldSphere> {
    let mut out = Vec::new();
    let (e, sc) = (&a.mesh_ends, &a.mesh_scales);
    for i in 0..a.mesh_dims.len() {
        let o = i * 8;
        let (p0x, p0y, t0x, t0y) = (e[o], e[o + 1], e[o + 2], e[o + 3]);
        let (p1x, p1y, t1x, t1y) = (e[o + 4], e[o + 5], e[o + 6], e[o + 7]);
        let (s0, s1) = (sc[i * 2], sc[i * 2 + 1]);
        for j in 0..BODY_SAMPLES {
            let u = crate::real(j) / crate::real(BODY_SAMPLES - 1);
            let u2 = u * u;
            let u3 = u2 * u;
            let h00 = 2.0 * u3 - 3.0 * u2 + 1.0;
            let h10 = u3 - 2.0 * u2 + u;
            let h01 = -2.0 * u3 + 3.0 * u2;
            let h11 = u3 - u2;
            let lx = h00 * p0x + h10 * t0x + h01 * p1x + h11 * t1x;
            let ly = h00 * p0y + h10 * t0y + h01 * p1y + h11 * t1y;
            out.push(WorldSphere {
                x: a.x + a.facing * lx,
                y: a.y + ly,
                z: Some(0.0),
                r: MESH_HALF * (s0 + (s1 - s0) * u).abs() * scale,
                ..WorldSphere::default()
            });
        }
    }
    out
}

/// A fighter's capsule (Pawn) against a sphere (`capsulesInSphere`).
fn capsule_in_sphere(w: &World, v: FighterId, x: Real, y: Real, r: Real) -> bool {
    let cr = cap_radius(w, v);
    let h = max(capsule_half(w, v) - cr, 0.0);
    let cy = actor_y(w, v);
    let dy = max(max(cy - h - y, 0.0), y - (cy + h));
    let gx = w.fighter(v).body.x - x;
    (gx * gx + dy * dy).sqrt() <= r + cr
}

/// Which fighters each component overlaps now; the begin overlaps that come of it: the Knock's (@22005) ahead of the
/// actor's (@83), as UPrimitiveComponent::BeginComponentOverlap broadcasts them.
fn overlaps(w: &mut World, id: ActorId) {
    let own = owner_of(w, id);
    let g = size_scale(w, id);
    let mut kn = WorldSphere { z: Some(0.0), ..WorldSphere::default() };
    let knock_on = state(w, id).knock_on;
    if knock_on {
        let a = state(w, id);
        let ms = a.mesh_scale * g; // (the bottom half's scale under the root's)
        let at = leg_world(a, a.leg_rel_x, a.leg_rel_y);
        // Knock at (0, 0, 143) on the bottom half, turned by its relative pitch (FRotator::Quaternion)
        let [ux, uz] = turn_by_pitch(a.leg_pitch, [0.0, KNOCK_UP * ms]);
        kn.x = at.x + a.facing * ux;
        kn.y = at.y + uz;
        kn.r = KNOCK_RADIUS * ms;
    }
    let mut built: Option<Vec<WorldSphere>> = None;
    for v in match_fighters(w) {
        if v == own || is_dead(w, id) {
            continue; // the owner passes no gate of either event
        }
        let vid = w.fighter(v).id;
        let off = !alive(w, v) || no_collision(w, v);
        let mut vp: Option<Vec<f32>> = None;
        let mut in_knock = false;
        if knock_on && !off {
            in_knock = capsule_in_sphere(w, v, kn.x, kn.y, kn.r);
            if !in_knock {
                vp = victim_pose(w, v);
                in_knock = vp.as_ref().is_some_and(|p| hurt_hit(w, v, std::slice::from_ref(&kn), p).is_some());
            }
        }
        let mut body = false;
        if !in_knock && !off && !state(w, id).bounced.contains(&vid) {
            // the root Capsule (Pawn only)
            let a = state(w, id);
            let (ax, ay) = (a.x, a.y);
            let cr = cap_radius(w, v);
            let h = max(capsule_half(w, v) - cr, 0.0);
            let cy = actor_y(w, v);
            let rh = (ROOT_HALF - ROOT_R) * g;
            let gap = max(max(0.0, ay - rh - (cy + h)), cy - h - (ay + rh));
            let gx = w.fighter(v).body.x - ax;
            body = (gx * gx + gap * gap).sqrt() <= ROOT_R * g + cr;
            if !body {
                if built.is_none() {
                    built = Some(body_spheres(state(w, id), g));
                }
                let spheres = built.as_ref().expect("built above");
                body = spheres.iter().any(|s| capsule_in_sphere(w, v, s.x, s.y, s.r));
                if !body && !spheres.is_empty() {
                    if vp.is_none() {
                        vp = victim_pose(w, v);
                    }
                    body = vp.as_ref().is_some_and(|p| hurt_hit(w, v, spheres, p).is_some());
                }
            }
        }
        if let Some(p) = vp {
            give_vbuf(w, p);
        }
        let a = state_mut(w, id);
        let ki = a.knock_in.iter().position(|&k| k == vid);
        let enter_knock = in_knock && ki.is_none();
        if enter_knock {
            a.knock_in.push(vid);
        } else if !in_knock && let Some(k) = ki {
            a.knock_in.remove(k);
        }
        if enter_knock {
            knock_hit(w, id, v);
        }
        let touch = in_knock || body;
        let a = state_mut(w, id);
        let ti = a.touching.iter().position(|&k| k == vid);
        if touch && ti.is_none() {
            a.touching.push(vid);
            actor_begin(w, id, v);
        } else if !touch && let Some(k) = ti {
            a.touching.remove(k);
        }
    }
}

/// Knock's ComponentBeginOverlap @22005.
fn knock_hit(w: &mut World, id: ActorId, vic: FighterId) {
    let own = owner_of(w, id);
    if vic == own || w.fighter(vic).team.team == w.fighter(own).team.team {
        return;
    }
    let me = ActorRef::Actor(id);
    let vid = w.fighter(vic).id;
    let a = state(w, id);
    let l = spline_length(&a.spline);
    let al = 1.0 + ((min(max(l, 600.0), 1700.0) - 600.0) / 1100.0);
    let (mut lx, mut ly) = (a.launch_x, a.launch_y);
    if lx.abs() <= 1e-4 && ly.abs() <= 1e-4 {
        lx = 0.0;
        ly = 1.0;
    }
    let mut p = a.hit.clone();
    p.damage = Some(crate::math::real(round_to_int(5.0 * al)));
    p.base_knock_back = Some(850.0 * al);
    p.knock_back_direction_vector = Some(DataVec { x: Some(-lx), y: Some(ly), z: None });
    let (attack_id, seen) = (a.attack_id, a.hit_ids.contains(&vid));
    if seen {
        return; // CheckIfActorHasBeenHit @2883
    }
    if crate::combat::phases::no_hit_by(w, own, vic) || projectile_blocks(w, &dealer(w, me), vic) {
        return;
    }
    if invulnerable(w, vic) {
        let tick = w.flow.tick;
        attack_dodged(w, me, vic, attack_id, p.base_knock_back.unwrap_or(0.0), tick, None, false);
    } else {
        dodge_over(w, me, vic);
        state_mut(w, id).hit_ids.push(vid); // TrackActorsHitByTag @5290
        let at = V2 { x: w.fighter(vic).body.x, y: actor_y(w, vic) };
        let d = dealer(w, me);
        let pct = charge_pct(w, me);
        if apply_projectile_hit(w, &d, vic, &p, &[], at, Some(V2 { x: lx, y: ly }), &[], pct) {
            actor_hit_enemy(w, &d, vic);
        }
    }
}

/// ReceiveActorBeginOverlap @83.
fn actor_begin(w: &mut World, id: ActorId, v: FighterId) {
    let own = owner_of(w, id);
    let vid = w.fighter(v).id;
    let a = state(w, id);
    if invulnerable(w, v) || v == own || a.knock_in.contains(&vid) || a.bounced.contains(&vid) {
        return;
    }
    // Deviation: the port's spare switch spares an opponent the bounce (its launch, stun and hit pause).
    if crate::fighter::spared_by(w, own, v) {
        return;
    }
    state_mut(w, id).bounced.push(vid); // BouncedFighters (the cooldown branch is never reached: one bounce per fighter)
    bounce(w, id, v); // BounceServer -> BounceMulticast
    bounce_house(w, id, v); // BounceHouseServer @1241
}

/// LaunchCharacter(v, true, true) of a UE (0, Y, Z): sim (vx, vy).
fn launch(w: &mut World, f: FighterId, vx: Real, vy: Real) {
    let m = &mut w.fighter_mut(f).moves;
    m.impulse = Some(Impulse { vx, vy, xy: true, z: true, kind: 0, d: None });
    m.hit_launch = true; // a direct LaunchCharacter: the CMC takes it under the stun too
}

/// Comp_HitPause SetHitPauseTime.
fn hit_pause(w: &mut World, f: FighterId, sec: Real) {
    set_hit_pause(w, f, crate::real(timer_count(sec)), sec);
}

fn stunned(w: &World, v: FighterId) -> bool {
    let fi = w.fighter(v);
    fi.attackers.stun_source || fi.buffs.buff_stunned // CheckHitstunActive
}

fn speed_of(w: &World, v: FighterId) -> Real {
    let b = &w.fighter(v).body;
    size([0.0, b.vx, b.vy])
}

/// BounceMulticast @25507: the body, then the wiggle record (@25479 -> @24174), then BFReset Gravity (@29429).
fn bounce(w: &mut World, id: ActorId, v: FighterId) {
    let own = owner_of(w, id);
    let stun = stunned(w, v);
    let (vx, vy) = (w.fighter(v).body.x, actor_y(w, v));
    let a = state(w, id);
    // outward: unit(the spline's nearest place -> the fighter)
    let near = find_location_closest_to_world_location(&a.spline, vx, vy, SplineSpace::World);
    let out = unit(near.x, near.y, vx, vy);
    let (ox, oy) = (out.x, out.y);
    let (sticky, spike_min, spike_mul, bounce_mul, bounce_min, bounce_max) =
        (a.sticky, a.spike_min, a.spike_mul, a.bounce_mul, a.bounce_min, a.bounce_max);
    if w.fighter(v).team.team != w.fighter(own).team.team {
        if sticky {
            launch(w, v, ox * -50.0, oy * -50.0);
            add_hit_stun_from_knockback(w, v, 0.5, 0.0, 500.0, 1.0, 1.0);
            hit_pause(w, v, 0.1);
        } else if stun {
            let sp = max(spike_min, speed_of(w, v) * spike_mul);
            launch(w, v, ox * sp, oy * sp);
            add_hit_stun_from_knockback(w, v, 0.5, ox * sp, oy * sp, 1.0, 1.0);
            hit_pause(w, v, 0.1);
        } else {
            let c = min(max(speed_of(w, v) * bounce_mul, bounce_min), bounce_max);
            let sp = min(max(c, bounce_min), 2000.0);
            launch(w, v, ox * sp, oy * sp);
            add_hit_stun_from_knockback(w, v, 0.5, ox * sp, oy * sp, 0.2, 1.0);
            hit_pause(w, v, 0.033);
            if w.fighter(v).fast_fall.fast_falling {
                w.fighter_mut(v).fast_fall.fast_falling = false; // @28788
                fast_fall_end(w, v);
            }
        }
    } else if stun {
        let sp = min(max(speed_of(w, v) * bounce_mul, bounce_min), bounce_max);
        launch(w, v, ox * sp, oy * sp);
        if w.fighter(v).fast_fall.fast_falling {
            w.fighter_mut(v).fast_fall.fast_falling = false;
            fast_fall_end(w, v);
        }
    }
    // @24174: the nearest of points 1 .. n - 2 (strictly nearer than 5000)
    let a = state(w, id);
    let mut best = 5000.0;
    let mut pt = 0;
    let (mut sx, mut sy) = (0.0, 0.0);
    let n = i32::try_from(a.spline.pts.len()).unwrap_or(i32::MAX);
    for i in 1..=(n - 2) {
        let p = get_location_at_spline_point(&a.spline, i, SplineSpace::World);
        let d = size([0.0, p.x - vx, p.y - vy]);
        if d < best {
            best = d;
            pt = i;
            sx = p.x;
            sy = p.y;
        }
    }
    if pt != 0 {
        let near = find_location_closest_to_world_location(&a.spline, vx, vy, SplineSpace::World);
        let dir = unit(near.x, near.y, vx, vy);
        let t = w.fighter(own).clock.time;
        state_mut(w, id).wiggles.push(Wiggle { pt, sx, sy, dx: dir.x, dy: dir.y, t });
        // BounceSoundMulticast @29590: on GetOwner
        let fighter = w.fighter(own).id;
        for ev in [name!("sfx_cha_jake_stretch_attack_a"), name!("sfx_cha_jake_stretch_wiggle")] {
            w.events.push(Event::Sound { fighter, ev, x: None, y: None, hit: None, pooled: false, key: None });
        }
    }
    // @29429 BFReset Gravity
    let m = &mut w.fighter_mut(v).moves;
    m.net_gravity = Some(GRAVITY_SCALE);
    m.gravity_mod_count = 0;
}

/// BounceHouseServer: an ally with the SafetyNet perk gets its jumps back and its hitstun ended.
fn bounce_house(w: &mut World, id: ActorId, v: FighterId) {
    let own = owner_of(w, id);
    if w.fighter(v).team.team != w.fighter(own).team.team || !state(w, id).bounce_house {
        return;
    }
    reset_jump_counts(w, v);
    if stunned(w, v) {
        remove_hitstun_from_knock_back(w, v);
    }
}

// ------------------------------------------------------------------ the view

/// What the view reads (`JakeStretchFields`), as the actor's tick and activation leave it.
fn view(a: &mut Stretch) {
    let sx = a.facing;
    a.count = i32::try_from(a.spline.pts.len()).unwrap_or(i32::MAX);
    a.points.clear();
    for p in &a.spline.pts {
        a.points.push(a.x + sx * p.x);
        a.points.push(a.y + p.y);
    }
    let m = a.mesh_dims.len();
    a.ends.clear();
    for i in 0..m {
        let (o, e) = (i * 8, &a.mesh_ends);
        a.ends.extend([
            a.x + sx * e[o],
            a.y + e[o + 1],
            sx * e[o + 2],
            e[o + 3],
            a.x + sx * e[o + 4],
            a.y + e[o + 5],
            sx * e[o + 6],
            e[o + 7],
        ]);
    }
    a.scales.clone_from(&a.mesh_scales);
    a.scales.truncate(m * 2);
    a.dims.clone_from(&a.mesh_dims);
    a.leg_x = a.x + sx * a.leg_rel_x;
    a.leg_y = a.y + a.leg_rel_y;
    a.length = spline_length(&a.spline);
}

// ------------------------------------------------------------------ the kind

fn spawn(w: &mut World, f: FighterId, r: &SpawnRequest) -> (WorldActorState, bool) {
    let data = w.data.clone();
    let cls = w.names.str(r.cls);
    let ad = actor_data(w, &data, ActorRef::Fighter(f), r.cls)
        .unwrap_or_else(|| panic!("{cls}: no actor data for its spawn (the importer gives every spawned class)"));
    let np = request_props(w, f, r);
    let at = spawn_place(w, f, &np);
    let p = &ad.p;
    let owner_facing = facing(w, f);
    let mut a = Stretch {
        attack_id: r.attack_id,
        x: at.x,
        y: at.y,
        facing: owner_facing,
        facing_left: owner_facing < 0.0,
        last_x: at.x,
        last_y: at.y,
        new_count: num_or(p, "NewSplinePointCount", 1.0),
        inc: num_or(p, "IncementSize", 150.0),
        taper_min: num_or(p, "TaperMin", 0.4),
        taper_dist: num_or(p, "StrechTaperDistance", 1500.0),
        bounce_mul: num_or(p, "BounceMultiplier", 1.2),
        bounce_min: num_or(p, "MinBounceVelocity", 1200.0),
        bounce_max: num_or(p, "MaxBounceVelocity", 2400.0),
        spike_mul: num_or(p, "SpikeMultiplier", 1.3),
        spike_min: num_or(p, "MinimumSpikeVelocity", 1200.0),
        ..Stretch::default()
    };
    if let Some(cv) = obj(p, "CurrentLaunchVector") {
        a.launch_x = -num(cv, "Y").unwrap_or(0.0);
        a.launch_y = num(cv, "Z").unwrap_or(0.0);
    }
    let tpl = obj(&ad.components, "Spline")
        .and_then(|s| obj(s, "SplineCurves"))
        .and_then(|s| obj(s, "Position"))
        .and_then(|s| s.get("Points"))
        .and_then(|v| v.as_arr());
    if tpl.is_none() {
        warn_text(w, &format!("{STRETCH}: no Spline template points in the data"));
    }
    a.spline = new_spline_curve(tpl, at.x, at.y, owner_facing);
    a.jake = jake_of(w, f).is_some();
    a.kb_seq = w.fighter(f).attackers.knocked_back_count;
    // ReceiveBeginPlay @21400
    if a.jake {
        a.mesh_scale = w.fighter(f).scaled.mesh_scale;
        add_invisibility(w, f);
        set_submesh_always_visible(w, f, name!("TopHalf"), true);
        unhid_equipped_weapon(w, f); // @5862 (Comp_WeaponSwap's)
    }
    add_spline_mesh(w, f, &mut a); // @1265
    // @6118
    if a.jake {
        set_stretch_on(w, f, true);
        calculate_stretch_angle(w, f, true);
    }
    a.bounce_house = buff_named(w, f, SAFETY_NET); // @6552
    a.sticky = buff_named(w, f, QUICK_STRETCH); // @6952
    a.last_x = a.x; // @27
    a.last_y = a.y;
    view(&mut a);
    (WorldActorState::Stretch(a), false)
}

fn buff_named(w: &World, f: FighterId, cls: &str) -> bool {
    w.names.get(cls).is_some_and(|c| crate::combat::buffs::buff_of(w, f, c).is_some())
}

/// The rest of the spawn that needs the actor's id: `JakeState.stretchActor`.
fn spawned(w: &mut World, id: ActorId) {
    let f = owner_of(w, id);
    if let Some(js) = jake_mut(w, f) {
        js.stretch_actor = Some(ActorRef::Actor(id));
    }
}

fn step(w: &mut World, id: ActorId) {
    if is_dead(w, id) {
        return;
    }
    let f = owner_of(w, id);
    // the KnockedBack binding (Deviation above)
    let count = w.fighter(f).attackers.knocked_back_count;
    if count != state(w, id).kb_seq {
        state_mut(w, id).kb_seq = count;
        activate(w, id);
    }
    let mut finished = false;
    let (ticking, playing, act_tick) = {
        let a = state(w, id);
        (a.ticking, a.playing, a.act_tick)
    };
    if ticking {
        let mut a = std::mem::take(state_mut(w, id));
        tick(w, f, &mut a);
        *state_mut(w, id) = a;
    } else if playing && w.flow.tick > act_tick {
        let mut a = std::mem::take(state_mut(w, id));
        a.tl += DELTA_SECONDS;
        if a.tl >= RETURN_LEN {
            a.tl = RETURN_LEN;
            finished = true;
        }
        let rf = a.tl / RETURN_LEN;
        return_update(w, f, &mut a, rf);
        *state_mut(w, id) = a;
    }
    overlaps(w, id);
    view(state_mut(w, id));
    if finished {
        state_mut(w, id).playing = false;
        if let Some(a) = w.actors.actors.get_mut(id) {
            a.dead = true; // Finished @20444: K2_DestroyActor
        }
    }
}

fn end(w: &mut World, id: ActorId) {
    let f = owner_of(w, id);
    // A KnockedBack the step has not met yet: the hit's montage end destroyed the actor first (Deviation above).
    let count = w.fighter(f).attackers.knocked_back_count;
    if !reset_pass(w) && count != state(w, id).kb_seq {
        state_mut(w, id).kb_seq = count;
        if let Some(a) = w.actors.actors.get_mut(id) {
            a.dead = false; // its components still overlap until K2_DestroyActor ends it
        }
        activate_now(w, id);
        if let Some(a) = w.actors.actors.get_mut(id) {
            a.dead = true;
        }
    }
    end_play(w, id);
}

/// This file's registry entry (JakeStretch_C).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.actors.enter(
        STRETCH,
        ActorEntry::new(
            ActorClass::World(WorldActorClass::JakeStretch),
            ActorHooks::World(WorldActorHooks {
                // overlaps and the body spheres read the scale, the view too
                size: Some(SizeHooks::default()),
                spawn: Some(spawn),
                spawned: Some(spawned),
                activate: Some(activate),
                step: Some(step),
                end: Some(end),
                ..WorldActorHooks::default()
            }),
        ),
    )
}
