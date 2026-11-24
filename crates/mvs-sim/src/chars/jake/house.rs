//! JakeHouse_Actor_C ([`JakeHouse`]): the down special's bouncy house, its montages
//! on its own anim instance ([`ActorAnim`], `actors::actor_montage`), the bounce and the fart hit box (a DealerActor of
//! its own: the "Fart Hit Box Damage" packet), the Safety Net perk's AddAirSuperiorityServer (perks.rs); joined to
//! hitStunStart (the live house destroyed). Its state on the pawn: `JakeState::house_actor`.
//!
//! Jake_House_Montage (data): the Spawner at .3526 (AttachToActor, translation Z -150, DestroyLastActorsOnNotifyEnd:
//! the world ends the house at the window's end), ActivateSpawned at .6678 (the last one, conditions {0 true, 1 false}:
//! the bounce), ConditionSectionJumpSingleCheck -> BouncyHouse at .6813, ActivateSpawned at 2.6668 (Exit) and 3.4168
//! (ExitAir) (all of them, conditions {1 true, 0 false}: the exit). Idle loops: the house's HouseTimer (HouseHoldTime
//! 3) jumps Jake's montage to Exit / ExitAir.
//!
//! ReceiveBeginPlay @3322, inside the spawn (the parent's BeginPlay first: HitBoxOn? with HitBoxTags [HitBox], which no
//! component has: no hit box), then in the sequence's order:
//! - @3462-3544 OwningFighter = the owner; authority ChangeMesh(RandomInteger(3)) -> @13200: the skin's MeshSwap
//!   submesh of that index on JakeHouse, Mesh_Index = it (`mesh_index`).
//! - @3568 Delay(0.3) -> @15: SetGenerateOverlapEvents(true) on Box1, Box2, Box. Their template has overlap events on
//!   already, so the Delay changes nothing and the boxes overlap from the spawn (the begin overlaps right after
//!   BeginPlay: `overlaps` in `spawned`).
//! - @3623-4065 the owner's InputTriggered -> InputeTriggered and HitStunStart -> HitStunned bound; the owner's
//!   FacingLeft: JakeHouse.SetRelativeScale3D((0.8, -0.8, 0.8)) @4020 (`mirrored`, the view's); the owner's
//!   Comp_FighterVisibilityManager.AddInvisibility @4065.
//! - @4124 the owner's CurrentArmorPacks not empty: ArmorBreak bound and the armor overlay mesh added @589 (`armor`).
//! - @4362 SFX_VO_Line @13662: by Mesh_Index a Delay(0.25) -> PF_Post_VO_Event(Instigator, ability_house (0) /
//!   ability_car (1) / ability_boat (2), no context, 0.2).
//! - @4777 -> @2325: the owner's HeldItem's attached actors, the first BPI_Reindog that BPI_IsBeingHeld: the owner's
//!   ThrowItem. Deviation: not ported (an item's attached actors are not kept); warned when the owner holds an item.
//! - @3184 authority: HouseTimer = K2_SetTimerDelegate(HouseTimerEnd, HouseHoldTime, no loop).
//!
//! ActivateObject @4877 -> ActivateObjectMulticast(ActivationConditions) @13056: [0] -> @4901 JakeHouse_Bounce_Montage
//! through a proxy; else [1] -> @5322 JakeHouse_Exit_Montage through a proxy, the owner's RemoveInvisibility @5841,
//! Delay(0.2) @5899 -> @115 JakeHouse.SetVisibility(false) (`hidden`). Every proxy's events only store their
//! NotifyName: `mesh.montage_event` does nothing.
//!
//! The clock of its Delays and timers is the owner's tick: a Delay made in frame T fires in the latent pass of frame T
//! + latentFrames - 1, a timer set in the tick groups is pending until the frame's timer pass and fires in frame
//!   T + timerTicks.
//
// Unverified: the cast of the MeshSwap submesh to a SkeletalMesh (@13370, pop when it fails, Mesh_Index left 0) is
// taken to succeed.
// Deviation: Box, Box1 and Box2 hang on JakeHouse's Root socket; the bone is taken at the mesh origin (the house's own
// animations, JakeHouse_Bounce / Exit and the AnimBP's Intro / Idle, are not sampled for it).
// Deviation: the fighters' root capsule alone is tested against the boxes (the hurt shapes on the mesh are not).
// Deviation: the begin overlaps are tested once a frame (at the spawn and in the house's tick), where the build raises
// them inside the moves of either side; LastUpdateVelocity is the velocity of the frame; the fighters before the
// projectiles; an Item (a thrown one is IsProjectile) and the World.actors classes are not tested.
// Deviation: the fart clouds against the other spawned actors are not tested.
// Deviation: the hitStunStart call marks the house dead; its EndPlay @12791 (Jake's visibility, weapons, houseActor)
// runs at the world's end-of-frame actor sweep, not inside the broadcast.
// Deviation: the Delays and the timer fire in the house's late pass, not in the world's leftover latent pass and timer
// pass at the end of the frame.
// Deviation: ArmorBreak @13448 (bound at BeginPlay with packs): the overlay goes with the last pack, sampled in the
// late pass; the build's is the ArmorBreak broadcast, so a pack taken off without a break keeps it.
// Deviation: the fart's hit box keeps its dodge variables (attackDodged's) on the actor, where a separate handler
// object holds them.

use indexmap::IndexSet;

use crate::Real;
use crate::State;
use crate::actors::actor_montage::{ActorAnim, MontageEvent, montage_play};
use crate::actors::carry_run::calculate_jump_stop_time;
use crate::actors::projectile::hurt_y;
use crate::actors::world_actor::{WorldActorState, actor_p};
use crate::actors::{ActorClass, ActorId, ActorRef, WorldActor, WorldActorClass};
use crate::combat::boxes::{ExtraHurt, WorldSphere, shape_over_capsule};
use crate::combat::buffs::buff_of;
use crate::combat::elem::charge_pct;
use crate::combat::hit::{
    add_hit_stun_from_knockback, hurt_hit, remove_hitstun_from_knock_back, set_hit_pause, timer_count, timer_ticks,
};
use crate::combat::hits::{actor_hit_enemy, apply_projectile_hit, dealer, projectile_blocks};
use crate::combat::phases::{alive, give_vbuf, los_blocked, victim_pose};
use crate::combat::reflect::reflect_projectile;
use crate::combat::spawned_hits::class_rows;
use crate::combat::{DataVec, HitPacket};
use crate::consts::GRAVITY_SCALE;
use crate::events::Event;
use crate::fighter::anim::MontageSlot;
use crate::fighter::anim::grouped::active_instance;
use crate::fighter::anim::{add_invisibility, remove_invisibility};
use crate::fighter::body::{cap_radius, capsule_half, no_collision};
use crate::fighter::common::{actor_class_def, actor_is_projectile, latent_frames, lookup_owner, warn_text};
use crate::fighter::control::{attack_dodged, dodge_over};
use crate::fighter::match_side::{SpawnRequest, post_vo};
use crate::fighter::moves::{Impulse, V2, fast_fall_end, nav_jump, net_impulse, reset_jump_counts};
use crate::fighter::{FighterId, actor_y, facing, grounded, in_air, in_hitstun, invulnerable};
use crate::math::max;
use crate::math::ue::{quat_rotate_vector, rotator_quaternion};
use crate::name;
use crate::names::Name;
use crate::registry::entries::{MeshHooks, SizeHooks, WorldActorHooks};
use crate::registry::props::{flag_or, num_or, obj, text};
use crate::registry::signals::FighterSignal;
use crate::registry::{ActorEntry, ActorHooks, Registry, RegistryError};
use crate::world::World;
use crate::world::flow::reset_pass;
use crate::world::pawns::match_fighters;
use crate::world::spawning::request_props;
use mvs_data::value::{Props, Value};

use super::perks::add_air_superiority_server;
use super::state::{jake_mut, jake_of};
use super::weapons::unhid_equipped_weapon;

/// JakeHouse_Actor's class.
pub const HOUSE: &str = "JakeHouse_Actor_C";

const SAFETY_NET: Name = name!("Buff_Jake_SafetyNet_C");
const BOUNCE: Name = name!("JakeHouse_Bounce_Montage");
const EXIT: Name = name!("JakeHouse_Exit_Montage");
const MONTAGE: Name = name!("Jake_House_Montage");
const SFX_BOUNCE: Name = name!("sfx_cha_jake_house_bounce");
const MESH_SCALE: Real = 0.8; // JakeHouse RelativeScale3D (the boxes and the clouds hang under it)
const ATTACH_Z: Real = -150.0; // the Spawner's Transforms.Translation.Z

/// ReflectedActors' entry.
#[derive(State, Debug)]
pub struct Reflected {
    pub pr: ActorRef,
    pub t: Real,
}

impl Default for Reflected {
    fn default() -> Self {
        Reflected { pr: ActorRef::Stage(0), t: 0.0 }
    }
}

/// `JakeHouse` (JakeHouse_Actor_C): the class's fields, under their names (the view's and its own). Sim axes.
#[derive(State, Debug)]
pub struct JakeHouse {
    /// Its place (the owner's actor location + (0, 0, -150), attached).
    pub x: Real,
    pub y: Real,
    /// Its yaw's side: the owner's.
    pub facing: Real,
    /// Mesh_Index.
    pub mesh_index: i32,
    /// AddedArmorMesh shown.
    pub armor: bool,
    /// HitBoxOn? with HitBoxTags [Fart].
    pub fart: bool,
    /// JakeHouse's scale (0.8, -0.8, 0.8): the owner faced left at BeginPlay (@4020).
    pub mirrored: bool,
    /// JakeHouse.SetVisibility(false).
    pub hidden: bool,
    /// Its montages (`anim = new ActorAnim('JakeHouse')`).
    pub anim: ActorAnim,
    /// ActivationConditions.
    pub conds: [bool; 2],
    /// Its Delays and timers, as the owner's tick they fire on (-1: none): SFX_VO_Line's Delay(0.25), HouseTimer,
    /// InputeTriggered's Delay(CalculateJumpStopTime), FartHitBox's RetriggerableDelay, the exit's Delay(0.2).
    pub vo_at: Real,
    pub house_at: Real,
    pub jump_stop_at: Real,
    pub fart_off_at: Real,
    pub hide_at: Real,
    /// JumpGate.
    pub jump_gate: bool,
    /// FartGameTime.
    pub fart_game_time: Real,
    /// The fighters (ids) whose root capsule overlaps a box.
    pub inside: Vec<i32>,
    /// The projectiles overlapping a box.
    pub inside_p: Vec<ActorRef>,
    /// ReflectedActors.
    pub reflected: Vec<Reflected>,
    /// The fart's HitEachActorOnlyOnce, by fighter id.
    pub hit_set: IndexSet<i32>,
    /// The fart's hit: its own handler (`dealer`: HitBoxHandlerFacingLeft false, so the packet's X alone gives the
    /// side: facing 1 and no velocity): its place (as the last `hits` left it, the construction's before), the
    /// packet and the request's AttackID.
    pub dealer_x: Real,
    pub dealer_y: Real,
    pub dealer_hit: HitPacket,
    pub attack_id: i32,
}

impl Default for JakeHouse {
    /// The field initial values.
    fn default() -> Self {
        JakeHouse {
            x: 0.0,
            y: 0.0,
            facing: 1.0,
            mesh_index: 0,
            armor: false,
            fart: false,
            mirrored: false,
            hidden: false,
            anim: ActorAnim::new(name!("JakeHouse")),
            conds: [false, false],
            vo_at: -1.0,
            house_at: -1.0,
            jump_stop_at: -1.0,
            fart_off_at: -1.0,
            hide_at: -1.0,
            jump_gate: true,
            fart_game_time: 0.0,
            inside: Vec::new(),
            inside_p: Vec::new(),
            reflected: Vec::new(),
            hit_set: IndexSet::new(),
            dealer_x: 0.0,
            dealer_y: 0.0,
            dealer_hit: HitPacket::default(),
            attack_id: 0,
        }
    }
}

fn state(w: &World, id: ActorId) -> &JakeHouse {
    match &w.actors.actors.get(id).expect("a live world actor").state {
        WorldActorState::JakeHouse(s) => s,
        _ => panic!("a JakeHouse_Actor_C state"),
    }
}

fn state_mut(w: &mut World, id: ActorId) -> &mut JakeHouse {
    match &mut w.actors.actors.get_mut(id).expect("a live world actor").state {
        WorldActorState::JakeHouse(s) => s,
        _ => panic!("a JakeHouse_Actor_C state"),
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

/// The JakeHouse_Actor_C's place (its `x`, `y`).
pub fn place(w: &World, a: &WorldActor, s: &JakeHouse) -> Option<(Real, Real)> {
    let _ = (w, a);
    Some((s.x, s.y))
}

/// The JakeHouse_Actor_C's fart hit box as a dealer (its `dealer`, a DealerActor of its own, id -1): its place, facing
/// 1 and the request's AttackID.
pub fn dealer_parts(w: &World, a: &WorldActor, s: &JakeHouse) -> (Real, Real, Real, i32) {
    let _ = (w, a);
    (s.dealer_x, s.dealer_y, 1.0, s.attack_id)
}

/// The JakeHouse_Actor_C dealer's packet (`dealer.hit`: "Fart Hit Box Damage", knockback direction (1, 1)).
pub fn hit(s: &JakeHouse) -> &HitPacket {
    &s.dealer_hit
}

// ---------------------------------------------------------------- the shapes

/// A box in the house's frame (UE local of the actor, times JakeHouse's 0.8): x along the owner's facing, y up, d depth
/// (UE local Y), and its three half extent vectors as (x, y, d). The actor's yaw is the owner's (+-90) and a left-facing
/// BeginPlay's mirror flips local Y back, so depth keeps its sign and only x follows the facing.
struct LocalBox {
    x: Real,
    y: Real,
    d: Real,
    ax: [Real; 3],
    ay: [Real; 3],
    az: [Real; 3],
}

fn local_box(c: &Props) -> LocalBox {
    let k = MESH_SCALE;
    let l = obj(c, "RelativeLocation");
    let e = obj(c, "BoxExtent");
    let at = |o: Option<&Props>, key: &str| o.map_or(0.0, |o| num_or(o, key, 0.0));
    let roll = obj(c, "RelativeRotation").map_or(0.0, |r| num_or(r, "Roll", 0.0));
    // The component's rotator (0, 0, Roll) by FRotator::Quaternion, its local Y and Z axes turned by it: about
    // (0, cos, -sin) and (0, sin, cos) in UE (X, Y, Z); here UE X -> x, UE Z -> y, UE Y -> d. (Pitch and yaw are below
    // 0.001 degree in the data.)
    let q = rotator_quaternion([0.0, 0.0, roll]);
    let [_, cr, msr] = quat_rotate_vector(q, [0.0, 1.0, 0.0]);
    let [_, sr, cr_z] = quat_rotate_vector(q, [0.0, 0.0, 1.0]);
    LocalBox {
        x: at(l, "X") * k,
        y: at(l, "Z") * k,
        d: at(l, "Y") * k,
        ax: [at(e, "X") * k, 0.0, 0.0],
        ay: [0.0, msr * at(e, "Y") * k, cr * at(e, "Y") * k],
        az: [0.0, cr_z * at(e, "Z") * k, sr * at(e, "Z") * k],
    }
}

struct Locals {
    boxes: Vec<LocalBox>,
    clouds: Vec<LocalBox>,
    spheres: Vec<(Real, Real, Real)>,
}

/// The class's components as local shapes (static data).
fn locals(w: &World, id: ActorId) -> Locals {
    let a = w.actors.actors.get(id).expect("a live world actor");
    let mut out = Locals { boxes: Vec::new(), clouds: Vec::new(), spheres: Vec::new() };
    let Some(def) = actor_class_def(w, a.owner, a.cls) else { return out };
    let comps = &def.components;
    for n in ["Box", "Box1", "Box2"] {
        if let Some(c) = obj(comps, n) {
            out.boxes.push(local_box(c));
        }
    }
    for (_, c) in comps {
        let Value::Obj(c) = c else { continue };
        let fart = c.get("ComponentTags").is_some_and(|t| match t {
            Value::Arr(t) => t.iter().any(|v| v.as_str() == Some("Fart")),
            _ => false,
        });
        if !fart {
            continue;
        }
        if c.contains_key("BoxExtent") {
            out.clouds.push(local_box(c));
        } else if c.get("SphereRadius").is_some_and(crate::registry::props::truthy) {
            let l = obj(c, "RelativeLocation");
            let at = |key: &str| l.map_or(0.0, |o| num_or(o, key, 0.0));
            let s = obj(c, "RelativeScale3D").map_or(1.0, |o| num_or(o, "X", 1.0));
            out.spheres.push((
                at("X") * MESH_SCALE,
                at("Z") * MESH_SCALE,
                num_or(c, "SphereRadius", 0.0) * s * MESH_SCALE,
            ));
        }
    }
    out
}

/// `place`: a box in world (g: the root's scale over the boxes' places and extents).
fn place_box(b: &LocalBox, a: &JakeHouse, g: Real) -> WorldSphere {
    let s = a.facing * g;
    WorldSphere {
        x: a.x + b.x * s,
        y: a.y + b.y * g,
        z: Some(b.d * g),
        r: 0.0,
        bx: Some([
            b.ax[0] * s,
            b.ax[1] * g,
            b.ax[2] * g,
            b.ay[0] * s,
            b.ay[1] * g,
            b.ay[2] * g,
            b.az[0] * s,
            b.az[1] * g,
            b.az[2] * g,
        ]),
        ..WorldSphere::default()
    }
}

fn house_boxes(w: &World, id: ActorId) -> Vec<WorldSphere> {
    let (a, g) = (state(w, id), size_scale(w, id));
    locals(w, id).boxes.iter().map(|b| place_box(b, a, g)).collect()
}

/// FartCloud_01-03 where they are now (the hits).
fn fart_shapes(w: &World, id: ActorId) -> Vec<WorldSphere> {
    let l = locals(w, id);
    let (a, g) = (state(w, id), size_scale(w, id));
    let mut out = Vec::new();
    for &(sx, sy, r) in &l.spheres {
        out.push(WorldSphere {
            x: a.x + sx * a.facing * g,
            y: a.y + sy * g,
            z: Some(0.0),
            r: r * g,
            bx: None,
            ..WorldSphere::default()
        });
    }
    for b in &l.clouds {
        out.push(place_box(b, a, g));
    }
    out
}

/// The fighter's root capsule (the Pawn profile against OverlapAllDynamic).
fn over_capsule(w: &World, shapes: &[WorldSphere], f: FighterId) -> bool {
    let seg = capsule_half(w, f) - cap_radius(w, f);
    let e = ExtraHurt { x: w.fighter(f).body.x, y0: actor_y(w, f) - seg, y1: actor_y(w, f) + seg, r: cap_radius(w, f) };
    shapes.iter().any(|s| shape_over_capsule(s, &e))
}

#[allow(clippy::neg_cmp_op_on_partial_ord, reason = "!(r > 0): a NaN radius tests nothing")]
fn over_sphere(shapes: &[WorldSphere], x: Real, y: Real, r: Real) -> bool {
    if !(r > 0.0) {
        return false;
    }
    let e = ExtraHurt { x, y0: y, y1: y, r };
    shapes.iter().any(|s| shape_over_capsule(s, &e))
}

/// A projectile's HitBox (HitBox2D or sphere) or HurtBox against the boxes.
fn projectile_over(w: &World, shapes: &[WorldSphere], pr: ActorRef) -> bool {
    let ActorRef::Projectile(id) = pr else { return false };
    let Some(p) = w.actors.projectiles.get(id) else { return false };
    over_sphere(shapes, p.x, p.y, p.hit_radius) || over_sphere(shapes, p.x, hurt_y(p), p.hurt_radius)
}

// ---------------------------------------------------------------- the overlaps

/// The begin overlaps of this frame: each fighter and projectile that touches a box now and did not at the last test,
/// in the world's lists' order.
fn overlaps(w: &mut World, id: ActorId) {
    let shapes = house_boxes(w, id);
    for f in match_fighters(w) {
        if is_dead(w, id) {
            return;
        }
        let fid = w.fighter(f).id;
        let i = state(w, id).inside.iter().position(|&k| k == fid);
        if !(alive(w, f) && !no_collision(w, f) && over_capsule(w, &shapes, f)) {
            if let Some(i) = i {
                state_mut(w, id).inside.remove(i);
            }
            continue;
        }
        if i.is_some() {
            continue;
        }
        state_mut(w, id).inside.push(fid);
        fighter_overlap(w, id, f);
    }
    if is_dead(w, id) {
        return;
    }
    let shapes2 = house_boxes(w, id);
    let mut i = state(w, id).inside_p.len();
    while i > 0 {
        i -= 1;
        let pr = state(w, id).inside_p[i];
        let gone = match pr {
            ActorRef::Projectile(p) => w.actors.projectiles.get(p).is_none_or(|p| p.dead),
            _ => true,
        };
        if gone || !projectile_over(w, &shapes2, pr) {
            state_mut(w, id).inside_p.remove(i);
        }
    }
    let mut k = 0;
    while let Some(p) = w.actors.projectiles.nth(k) {
        k += 1;
        if is_dead(w, id) {
            return;
        }
        let pr = ActorRef::Projectile(p);
        if w.actors.projectiles.get(p).is_none_or(|p| p.dead)
            || state(w, id).inside_p.contains(&pr)
            || !projectile_over(w, &shapes2, pr)
        {
            continue;
        }
        state_mut(w, id).inside_p.push(pr);
        projectile_overlap(w, id, pr);
    }
}

fn bounce_montage(w: &mut World, id: ActorId) {
    montage_play(w, ActorRef::Actor(id), BOUNCE, 1.0, 0.0, true);
}

fn sound(w: &mut World, id: ActorId, ev: Name) {
    let own = owner_of(w, id);
    let (x, y) = {
        let a = state(w, id);
        (a.x, a.y)
    };
    let fighter = w.fighter(own).id;
    w.fighter_mut(own).events.push(Event::Sound {
        fighter,
        ev,
        x: Some(x),
        y: Some(y),
        hit: None,
        pooled: false,
        key: None,
    });
}

fn now(w: &World, own: FighterId) -> Real {
    w.fighter(own).clock.time // GetGameTimeInSeconds
}

/// LaunchCharacter(v, xyOverride, zOverride): PendingLaunchVelocity, taken in the fighter's next CMC tick. An axis not
/// overridden adds the velocity at the call (ACharacter::LaunchCharacter): given here as an override of that sum.
fn launch(w: &mut World, f: FighterId, vx: Real, vy: Real, xy: bool, z: bool) {
    let b = &w.fighter(f).body;
    let (bvx, bvy) = (b.vx, b.vy);
    let m = &mut w.fighter_mut(f).moves;
    m.impulse = Some(Impulse {
        vx: if xy { vx } else { vx + bvx },
        vy: if z { vy } else { vy + bvy },
        xy: true,
        z: true,
        kind: 0,
        d: None,
    });
    m.hit_launch = true; // a direct LaunchCharacter: the CMC takes it under the stun too
}

#[allow(clippy::neg_cmp_op_on_partial_ord, reason = "!(o.vy <= 0): a NaN velocity is not falling")]
fn fighter_overlap(w: &mut World, id: ActorId, o: FighterId) {
    let own = owner_of(w, id);
    if o == own {
        return; // @6053
    }
    if invulnerable(w, o) || grounded(w, o) {
        return; // IsInvulnerable? or IsWalking @6101-6313
    }
    let p = actor_p(w, ActorRef::Actor(id));
    let buffer = num_or(p, "Buffer", 75.0);
    let (min_spike, spike_mul) = (num_or(p, "MinimumSpikeVelocity", 2500.0), num_or(p, "SpikeMultiplier", 1.5));
    let (bounce_v, ff_mul) = (num_or(p, "BounceVelocity", 2000.0), num_or(p, "FastFallMultiplier", 1.25));
    let fart_delay = num_or(p, "FartDelayTime", 0.75);
    let above = actor_y(w, o) >= actor_y(w, own) + buffer;
    let pushing = grounded(w, own) && w.fighter(own).body.vx.abs() > 50.0;
    if !(above || pushing) || !(w.fighter(o).body.vy <= 0.0) {
        // @9726: ApplyBounceHouseEffects(other, true, true); Success -> the bounce montage and the sound.
        if bounce_house_effects(w, id, o, true, true) {
            bounce_montage(w, id);
            sound(w, id, SFX_BOUNCE);
        }
        return;
    }
    let same = w.fighter(o).team.team == w.fighter(own).team.team;
    if in_hitstun(w, o) {
        // @7345-8379: the spike.
        let vy = w.fighter(o).body.vy;
        let z = max(min_spike, (vy * if same { 0.5 } else { spike_mul }).abs());
        launch(w, o, 0.0, z, false, true);
        add_hit_stun_from_knockback(w, o, 0.5, 0.0, z, 1.0, 1.0);
        set_hit_pause(w, o, crate::real(timer_count(0.1)), 0.1);
    } else {
        // @9393-9679 (authority).
        let ff = w.fighter(o).fast_fall.fast_falling;
        let z = if ff { ff_mul * bounce_v } else { bounce_v };
        launch(w, o, 0.0, z, false, true);
        if ff {
            // IsFastFalling = false, OnIsFastFalling(false): the port's flag and the build's.
            w.fighter_mut(o).fast_fall.fast_falling = false;
            fast_fall_end(w, o);
        }
    }
    // @8442-9322.
    bounce_montage(w, id);
    sound(w, id, SFX_BOUNCE);
    // BFReset Gravity @8931: NetGravity(BFDefaultGravityScale), GravityModifierCounter = 0.
    let m = &mut w.fighter_mut(o).moves;
    m.net_gravity = Some(GRAVITY_SCALE);
    m.gravity_mod_count = 0;
    bounce_house_effects(w, id, o, false, false);
    let t = now(w, own);
    if same && (t - state(w, id).fart_game_time) >= fart_delay {
        state_mut(w, id).fart_game_time = t;
        fart_hit_box(w, id);
    }
}

/// ApplyBounceHouseEffects @784 (the call sites pass both flags alike). Returns Success.
fn bounce_house_effects(w: &mut World, id: ActorId, o: FighterId, clamp: bool, only_if_hitstun: bool) -> bool {
    let own = owner_of(w, id);
    if w.fighter(o).team.team != w.fighter(own).team.team {
        return false;
    }
    reset_jump_counts(w, o); // ResetJump(0) @118
    let was = in_hitstun(w, o);
    if was {
        remove_hitstun_from_knock_back(w, o);
    }
    if clamp {
        // LastUpdateVelocity x 0.3 @426
        let (vx, vy) = (w.fighter(o).body.vx, w.fighter(o).body.vy);
        launch(w, o, vx * 0.3, vy * 0.3, true, true);
    }
    if (was || !only_if_hitstun)
        && let Some(b) = buff_of(w, own, SAFETY_NET)
    {
        add_air_superiority_server(w, own, b, o);
    }
    true
}

#[allow(clippy::neg_cmp_op_on_partial_ord, reason = "!(F(t - r.t) > ReflectBuffer)")]
fn projectile_overlap(w: &mut World, id: ActorId, pr: ActorRef) {
    let own = owner_of(w, id);
    let t = now(w, own);
    let reflect_buffer = num_or(actor_p(w, ActorRef::Actor(id)), "ReflectBuffer", 1.0);
    let r = state(w, id).reflected.iter().position(|e| e.pr == pr);
    if let Some(i) = r
        && !((t - state(w, id).reflected[i].t) > reflect_buffer)
    {
        return;
    }
    let ActorRef::Projectile(p) = pr else { return };
    let Some((cls, vx, vy)) = w.actors.projectiles.get(p).map(|p| (p.cls, p.vx, p.vy)) else { return };
    let owner = lookup_owner(w, pr);
    if !actor_is_projectile(w, ActorRef::Fighter(owner), cls) {
        return; // @10610-10661
    }
    // KnockBackDirectionVector (-Velocity.Y, Velocity.Z): sim (vx, vy). The owner's Comp_ReflectProjectile.
    if !reflect_projectile(w, own, pr, V2 { x: vx, y: vy }, None) {
        return;
    }
    bounce_montage(w, id);
    if let Some(i) = r {
        state_mut(w, id).reflected[i].t = t;
    } else {
        state_mut(w, id).reflected.push(Reflected { pr, t });
    }
}

// ---------------------------------------------------------------- the fart

fn fart_hit_box(w: &mut World, id: ActorId) {
    let own = owner_of(w, id);
    // FartCloudVisuals @15311: PS_JakeGroundImpact at FartCloud_03 +- 200 on UE Y (sim x -+ 200), scale 0.5.
    let l = locals(w, id);
    let c = l.clouds.first();
    let a = state(w, id);
    let (cx, cy) = (a.x + c.map_or(0.0, |c| c.x * a.facing), a.y + c.map_or(0.0, |c| c.y));
    let ox = c.map_or(0.0, |c| c.d);
    let fighter = w.fighter(own).id;
    for dx in [-200.0, 200.0] {
        w.events.push(Event::ProjectileDeath {
            cls: name!("JakeHouse_Actor_C"),
            fighter,
            vfx: Some(name!("PS_JakeGroundImpact")),
            x: cx + dx,
            y: cy,
            ox,
            rot: [0.0, 0.0, 0.0],
            scale: 0.5,
            tint: None,
            to: None,
        });
    }
    // @14916-15046: the packet, its X by the owner's facing at the call.
    let side = if facing(w, own) < 0.0 { -1.0 } else { 1.0 };
    let fart_time = num_or(actor_p(w, ActorRef::Actor(id)), "FartHitBoxTime", 0.2);
    let tick = crate::real(w.fighter(own).clock.tick);
    let a = state_mut(w, id);
    a.dealer_hit.knock_back_direction_vector = Some(DataVec { x: Some(side), y: Some(1.0), z: None });
    // HitBoxOn? = true, UpdateHitbox (HitBoxesOn; the list of the actors hit is the one HitBoxesOff emptied).
    a.fart = true;
    a.fart_off_at = tick + latent_frames(fart_time) - 1.0; // RetriggerableDelay: restarts
}

/// The fart clouds against the enemies' hurt shapes: the handler's pfgDoDamage gates in World.projectileTest's order
/// (as Rick's air flap).
fn hits(w: &mut World, id: ActorId) {
    if !state(w, id).fart {
        return;
    }
    let me = ActorRef::Actor(id);
    let own = owner_of(w, id);
    {
        let a = state_mut(w, id);
        (a.dealer_x, a.dealer_y) = (a.x, a.y);
    }
    let (x, y, attack_id, hit) = {
        let a = state(w, id);
        (a.x, a.y, a.attack_id, a.dealer_hit.clone())
    };
    let team = w.fighter(own).team.team;
    let mut shapes: Option<Vec<WorldSphere>> = None;
    for vic in match_fighters(w) {
        let vid = w.fighter(vic).id;
        if w.fighter(vic).team.team == team
            || !alive(w, vic)
            || crate::combat::phases::no_hit_by_team(w, team, vic)
            || state(w, id).hit_set.contains(&vid)
        {
            continue;
        }
        let Some(vp) = victim_pose(w, vic) else { continue };
        if shapes.is_none() {
            shapes = Some(fart_shapes(w, id));
        }
        let met = shapes.as_ref().is_some_and(|sh| hurt_hit(w, vic, sh, &vp).is_some());
        give_vbuf(w, vp);
        if !met {
            continue;
        }
        let (vx, vy) = (w.fighter(vic).body.x, w.fighter(vic).body.y);
        if hit.do_los_check.unwrap_or(false) && los_blocked(w, x, y, vx, vy) {
            continue;
        }
        if projectile_blocks(w, &dealer(w, me), vic) {
            continue;
        }
        if invulnerable(w, vic) {
            let tick = w.flow.tick;
            attack_dodged(w, me, vic, attack_id, hit.base_knock_back.unwrap_or(0.0), tick, None, false);
            continue;
        }
        dodge_over(w, me, vic);
        if hit.hit_each_actor_only_once.unwrap_or(false) {
            state_mut(w, id).hit_set.insert(vid);
        }
        let rows = class_rows(w, me);
        let d = dealer(w, me);
        if apply_projectile_hit(w, &d, vic, &hit, &rows, V2 { x, y }, None, &[], charge_pct(w, me)) {
            actor_hit_enemy(w, &d, vic);
        }
    }
}

// ---------------------------------------------------------------- the owner's broadcasts

/// house.rs's part of the joined hitStunStart, before horse.rs's: the owner's HitstunHandler.HitStunStart ->
/// HitStunned @12748: K2_DestroyActor, inside the broadcast (AddDecayedHitStun's start @3703 and replaced timer
/// @1398, AddBuffHitStun @2372). A zero hitstun and a kept timer broadcast OnHitStunExtended alone, which leaves the
/// house up. The house bound is the live one (`JakeState::house_actor`).
pub fn hit_stun_start(w: &mut World, f: FighterId) {
    let Some(ActorRef::Actor(id)) = jake_of(w, f).and_then(|j| j.house_actor) else { return };
    if let Some(a) = w.actors.actors.get_mut(id)
        && matches!(a.state, WorldActorState::JakeHouse(_))
        && !a.dead
    {
        a.dead = true;
    }
}

/// InputeTriggered @14109.
fn input_triggered(w: &mut World, id: ActorId, kind: Real) {
    let o = owner_of(w, id);
    if !(kind == 2.0 && state(w, id).jump_gate && grounded(w, o)) {
        return;
    }
    state_mut(w, id).jump_gate = false;
    let p = actor_p(w, ActorRef::Actor(id));
    let (jv, mh, jm) = (
        num_or(p, "JumpVelocity", 2300.0),
        num_or(p, "MaxJumpHeight", 400.0),
        num_or(p, "JumpStopVelocityMultiplier", 0.3),
    );
    let vx = w.fighter(o).body.vx;
    net_impulse(w, o, vx, jv, true, true, 0.0, false, 0.0); // (0, Velocity.Y, JumpVelocity) @14553
    sound(w, id, SFX_BOUNCE);
    // CalculateJumpStopTime(JumpVelocity, MaxJumpHeight) @856 (float32 per node; BFDefaultGravityScale the pawns').
    let stop = calculate_jump_stop_time(jv, mh, jm, GRAVITY_SCALE);
    state_mut(w, id).jump_stop_at = crate::real(w.fighter(o).clock.tick) + latent_frames(stop) - 1.0;
}

// ---------------------------------------------------------------- the kind

fn spawn(w: &mut World, f: FighterId, r: &SpawnRequest) -> (WorldActorState, bool) {
    let data = w.data.clone();
    let cls = w.names.str(r.cls);
    let ad = crate::fighter::common::actor_data(w, &data, ActorRef::Fighter(f), r.cls)
        .unwrap_or_else(|| panic!("{cls}: no actor data for its spawn (the importer gives every spawned class)"));
    let p = &ad.p;
    let t = crate::real(w.fighter(f).clock.tick);
    let np = request_props(w, f, r);
    let tr = obj(&np, "Transforms").and_then(|t| obj(t, "Translation"));
    let nonzero = |k: &str| tr.is_some_and(|t| crate::registry::props::truthy(t.get(k).unwrap_or(&Value::Null)));
    let z_ok = tr.is_some_and(|t| num_or(t, "Z", 0.0) == ATTACH_Z);
    if !crate::registry::props::flag(&np, "AttachToActor")
        || text(&np, "Socket").is_some()
        || crate::registry::props::flag(&np, "WorldTransforms")
        || nonzero("X")
        || nonzero("Y")
        || !z_ok
    {
        warn_text(w, &format!("{HOUSE}: a spawn other than attached to the actor at (0, 0, -150) is not ported"));
    }
    let side = facing(w, f);
    let (x, y) = (w.fighter(f).body.x, actor_y(w, f) + ATTACH_Z); // follow()
    let packet = obj(p, "Fart Hit Box Damage").cloned().unwrap_or_default();
    let mut dealer_hit = HitPacket::from_props(&w.names, &packet, None);
    dealer_hit.knock_back_direction_vector = Some(DataVec { x: Some(1.0), y: Some(1.0), z: None });
    let a = JakeHouse {
        x,
        y,
        facing: side,
        // ChangeMesh(RandomInteger(3)) @3511-3544 (authority).
        mesh_index: crate::math::to_int32(w.random_int(3.0)),
        mirrored: side < 0.0,
        armor: !w.fighter(f).armor.armor_packs.is_empty(),
        jump_gate: flag_or(p, "JumpGate", true),
        vo_at: t + latent_frames(0.25) - 1.0,
        house_at: t + timer_ticks(Some(num_or(p, "HouseHoldTime", 3.0))),
        dealer_x: x,
        dealer_y: y,
        dealer_hit,
        attack_id: r.attack_id,
        ..JakeHouse::default()
    };
    // @4065 AddInvisibility; @4777 the held Reindog's throw (not ported).
    add_invisibility(w, f);
    if w.fighter(f).items.held_item.is_some() {
        warn_text(w, &format!("{HOUSE}: BeginPlay's throw of a held Reindog (@2325-2621) is not ported"));
    }
    (WorldActorState::JakeHouse(a), false)
}

/// The rest of the spawn that needs the actor's id: `JakeState.houseActor`, then the begin overlaps right after
/// BeginPlay.
fn spawned(w: &mut World, id: ActorId) {
    let f = owner_of(w, id);
    if let Some(js) = jake_mut(w, f) {
        js.house_actor = Some(ActorRef::Actor(id));
    }
    overlaps(w, id);
}

/// Attached to the owner's root at (0, 0, -150): the yaw is the owner's.
fn follow(w: &mut World, id: ActorId) {
    let o = owner_of(w, id);
    let (side, x, y) = (facing(w, o), w.fighter(o).body.x, actor_y(w, o) + ATTACH_Z);
    let a = state_mut(w, id);
    a.facing = side;
    a.x = x;
    a.y = y;
}

fn step(w: &mut World, id: ActorId) {
    follow(w, id);
    overlaps(w, id);
}

/// The Delays and the timer, behind the mesh's tick (World.step).
fn late(w: &mut World, id: ActorId) {
    let o = owner_of(w, id);
    let t = crate::real(w.fighter(o).clock.tick);
    let p = actor_p(w, ActorRef::Actor(id));
    let (jv, jm) = (num_or(p, "JumpVelocity", 2300.0), num_or(p, "JumpStopVelocityMultiplier", 0.3));
    let a = state(w, id);
    if a.vo_at >= 0.0 && t >= a.vo_at {
        let alias = match a.mesh_index {
            0 => name!("ability_house"),
            1 => name!("ability_car"),
            _ => name!("ability_boat"),
        };
        state_mut(w, id).vo_at = -1.0;
        post_vo(w, o, alias, None, 0.2);
    }
    let a = state(w, id);
    if a.jump_stop_at >= 0.0 && t >= a.jump_stop_at {
        state_mut(w, id).jump_stop_at = -1.0;
        net_impulse(w, o, 0.0, jv * jm, false, true, 0.0, false, 0.0); // @450
        state_mut(w, id).jump_gate = true;
    }
    let a = state_mut(w, id);
    if a.fart_off_at >= 0.0 && t >= a.fart_off_at {
        a.fart_off_at = -1.0;
        a.fart = false; // HitBoxOn? = false, UpdateHitbox: HitBoxesOff, which forgets the actors hit
        a.hit_set.clear();
    }
    if a.hide_at >= 0.0 && t >= a.hide_at {
        a.hide_at = -1.0;
        a.hidden = true;
    }
    // ArmorBreak @13448 (bound at BeginPlay with packs): the overlay goes with the last pack.
    if a.armor && w.fighter(o).armor.armor_packs.is_empty() {
        state_mut(w, id).armor = false;
    }
    let a = state(w, id);
    if a.house_at >= 0.0 && t >= a.house_at {
        state_mut(w, id).house_at = -1.0;
        // HouseTimerEnd @15864 -> JumpToSectionLogic(section, Jake_House_Montage): that montage while it plays, else
        // the active one.
        let sec = if in_air(w, o) { name!("ExitAir") } else { name!("Exit") }; // BPI_GetIsInAir @15963
        let plays = w
            .fighter(o)
            .montages
            .montage
            .as_ref()
            .is_some_and(|m| m.montage.name == MONTAGE && !m.ended && !m.terminated);
        let m = if plays { Some(MontageSlot::Main) } else { active_instance(w, o) };
        if let Some(m) = m {
            nav_jump(w, o, m, sec);
        }
    }
}

fn set_condition(w: &mut World, id: ActorId, n: i32, v: bool) {
    if let Ok(i) = usize::try_from(n)
        && i < 2
    {
        state_mut(w, id).conds[i] = v;
    }
}

/// ActivateObjectMulticast @13056.
fn activate(w: &mut World, id: ActorId) {
    let o = owner_of(w, id);
    let conds = state(w, id).conds;
    if conds[0] {
        bounce_montage(w, id);
    } else if conds[1] {
        montage_play(w, ActorRef::Actor(id), EXIT, 1.0, 0.0, true);
        remove_invisibility(w, o);
        state_mut(w, id).hide_at = crate::real(w.fighter(o).clock.tick) + latent_frames(0.2) - 1.0;
    }
}

/// InputTriggered of its owner.
#[allow(clippy::too_many_arguments)]
fn fighter_signal(
    w: &mut World,
    id: ActorId,
    f: FighterId,
    sig: FighterSignal,
    flag: bool,
    num: Real,
    n: Name,
    by: Option<FighterId>,
) {
    let _ = (flag, n, by);
    if sig == FighterSignal::InputTriggered && f == owner_of(w, id) && !is_dead(w, id) {
        input_triggered(w, id, num);
    }
}

/// ReceiveEndPlay @12791.
fn end(w: &mut World, id: ActorId) {
    let o = owner_of(w, id);
    let jake = jake_of(w, o).is_some();
    if let Some(js) = jake_mut(w, o)
        && js.house_actor == Some(ActorRef::Actor(id))
    {
        js.house_actor = None;
    }
    if reset_pass(w) {
        return;
    }
    remove_invisibility(w, o);
    if jake {
        unhid_equipped_weapon(w, o); // Comp_WeaponSwap.UnhidEquippedWeapon @12995
    }
}

/// This file's registry entry (JakeHouse_Actor_C).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let mut e = ActorEntry::new(
        ActorClass::World(WorldActorClass::JakeHouseActor),
        ActorHooks::World(WorldActorHooks {
            // place and the fart shapes read the scale; the view too
            size: Some(SizeHooks::default()),
            spawn: Some(spawn),
            spawned: Some(spawned),
            step: Some(step),
            late: Some(late),
            hits: Some(hits),
            set_condition: Some(set_condition),
            activate: Some(activate),
            fighter_signal: Some(fighter_signal),
            end: Some(end),
            ..WorldActorHooks::default()
        }),
    );
    // The five proxies' events store their NotifyName alone.
    e.mesh = Some(MeshHooks {
        montage_event: Some(|_: &mut World, _: ActorRef, _: MontageEvent, _: Name| {}),
        ..MeshHooks::default()
    });
    r.actors.enter(HOUSE, e)
}
