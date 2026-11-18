//! The side special: GarnetGauntletsProjectile_C and its Air child
//! ([`GarnetGauntlets`], a projectile).
//!
//! The graph's entries (offsets of GarnetGauntletsProjectile's ubergraph), by what the port does with each:
//! - ReceiveBeginPlay @1064: the parent's, then with authority the Instigator's DamageManager.KnockedBack bound to
//!   OnInstigatorKnockedBack @1293 / @1316, then AutoActivate @1357 -> ActivateObject (false for both classes).
//! - OnInstigatorKnockedBack -> ReleaseToPool @6833 (no CanBePooled class: the destroy). Unverified: the KnockedBack
//!   broadcast is read at the top of the gloves' tick (the fighter's knocked-back count against the count at the
//!   bind), not inside the hit: a knockback dealt behind the projectiles' tick of the frame is met one frame late.
//! - ReceiveTick @1390: with authority and !Activated @1443, BPI_Get8WayDirection(false) @1657 (Get8WayDirectionNoFacing
//!   of the Instigator's input) picks a direction, Normal of it times VSize(Velocity) is the wanted velocity; Angle =
//!   DegAcos((2 s^2 - d^2) / (2 s^2)) @2678, the velocity turned by DeltaSeconds x GauntletAimMocifier x Angle about
//!   -X @2761 and +X @2888, the one nearer the wanted velocity @3015 (a tie: the -X one), MulticastVelocity(it,
//!   false, false) @3108.
//! - ActivateObject @3163: Activated = true @3207, Normal(Velocity) x VSize x ActivateMultiplier @3341,
//!   MulticastVelocity(it, true, true) @3433, UpdateMaxLifetimeWithAuthority(MaxLifetimeAfterRelease) @3458.
//! - MulticastVelocity @4519: the Velocity, then the Owner's BPI_SetAimDirection(Normal(Velocity).Z, ResetAim, false)
//!   @4777.
//! - ReceiveEndPlay @6809 -> @3486: the KnockedBack unbound @3728, then the perk branch (Perk_Garnet_Marker): the
//!   Instigator's Buff_Garnet_Marker @3868, AreGauntletsCast false @4134, TriggerAbilityCheck(1) @4450, then @10: a
//!   CapsuleTraceSingle from the gloves + (0, 0, the Instigator's capsule half height) down to the gloves,
//!   ProjectileGroundNoPlatform, ignoring self @416; the hit's Location, or the gloves' place for none @878;
//!   Comp_Spawner.CompSpawnActor(GarnetMarker2_C, no direction, speeds 0, AttackID 0) @933.
//! - GarnetGauntletProjectileOut reads ProjectileGauntlets, which no graph of the build writes (`setProjectileGauntlets`
//!   has no caller): the gloves never write it.

use crate::Real;
use crate::State;
use crate::actors::projectile::{ProjectileExt, class_p, update_max_lifetime};
use crate::actors::{ActorClass, ActorRef, Projectile, ProjectileClass, ProjectileId};
use crate::chars::garnet::marker::MARKER2;
use crate::chars::garnet::pawn::{are_gauntlets_cast, set_aim_direction};
use crate::chars::garnet::state::garnet_of;
use crate::combat::buffs::buff_of;
use crate::consts::DELTA_SECONDS;
use crate::fighter::FighterId;
use crate::fighter::control::trigger_ability_check;
use crate::fighter::traces::{TRACE_PROJECTILE_GROUND_NO_PLATFORM, capsule_trace_single};
use crate::input::{Dir, get8_way_no_facing};
use crate::math::ue::{KINDA_SMALL_NUMBER, deg_acos, dist2d, rotate_angle_axis, size2d};
use crate::name;
use crate::names::Name;
use crate::registry::entries::{ActorEntry, ActorHooks, ProjectileHooks};
use crate::registry::props::num;
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use crate::world::flow::reset_pass;
use crate::world::spawning::comp_spawn_projectile;

const MARKER_BUFF: Name = name!("Buff_Garnet_Marker_C"); // Perk_Garnet_Marker's AppliedBuffs

/// `GarnetGauntlets extends Projectile` (GarnetGauntletsProjectile_C, GarnetGauntletsProjectileAir_C).
#[derive(State, Debug, Default)]
pub struct GarnetGauntlets {
    /// The fighter whose KnockedBack BeginPlay bound (@1316), `None` once EndPlay unbound it.
    pub kb_bound: Option<FighterId>,
    /// Its knockedBackCount at the bind.
    pub kb_at: i32,
    /// ReceiveEndPlay ran (the world calls `destroyed` and `end`: once).
    pub end_ran: bool,
}

fn pr(w: &World, id: ProjectileId) -> &Projectile {
    w.actors.projectiles.get(id).expect("a live projectile")
}

fn pm(w: &mut World, id: ProjectileId) -> &mut Projectile {
    w.actors.projectiles.get_mut(id).expect("a live projectile")
}

fn gl(w: &World, id: ProjectileId) -> &GarnetGauntlets {
    match &pr(w, id).ext {
        ProjectileExt::GarnetGauntlets(g) => g,
        e => panic!("a Garnet gauntlets projectile without its state: {e:?}"),
    }
}

fn gl_w(w: &mut World, id: ProjectileId) -> &mut GarnetGauntlets {
    match &mut pm(w, id).ext {
        ProjectileExt::GarnetGauntlets(g) => g,
        e => panic!("a Garnet gauntlets projectile without its state: {e:?}"),
    }
}

fn owner(w: &World, id: ProjectileId) -> FighterId {
    pr(w, id).owner.expect("gauntlets have an owner")
}

// The 8-way byte's direction (@1704-2227, the switch @2227) in sim (x, y): x = -UE Y. Neutral / Default: the
// velocity's own direction.
fn dir_xy(d: Dir) -> (Real, Real) {
    match d {
        Dir::Up => (0.0, 1.0),
        Dir::Down => (0.0, -1.0),
        Dir::Fwd => (1.0, 0.0),
        Dir::Back => (-1.0, 0.0),
        Dir::FwdUp => (1.0, 1.0),
        Dir::FwdDown => (1.0, -1.0),
        Dir::BackUp => (-1.0, 1.0),
        Dir::BackDown => (-1.0, -1.0),
        Dir::Neutral | Dir::Default => unreachable!("a direction byte with no vector"),
    }
}

/// The Normal node (V, 0.0001) of the sim vector (x, y) as UE (0, -x, y), back in sim terms.
fn normal(x: Real, y: Real) -> (Real, Real) {
    let n = crate::math::ue::normal([0.0, x, y], KINDA_SMALL_NUMBER);
    (n[1], n[2])
}

/// MulticastVelocity(Velocity, ResetAim, ScaleUpVFX) @4519: the velocity, then the Owner's BPI_SetAimDirection by
/// Normal(Velocity).Z @4777 (an Owner that is not a Garnet: the cast fails, nothing). ScaleUpVFX is the view's.
fn multicast_velocity(w: &mut World, id: ProjectileId, vx: Real, vy: Real, reset_aim: bool) {
    let p = pm(w, id);
    p.vx = vx;
    p.vy = vy;
    let f = owner(w, id);
    if garnet_of(w, f).is_some() {
        set_aim_direction(w, f, normal(vx, vy).1, reset_aim, false);
    }
}

/// ReceiveTick's steering @1443-3108.
// Unverified: the stick is the Instigator's input of this frame, as Rick's aimer reads BPI_Get8WayDirection(false).
fn steer(w: &mut World, id: ProjectileId) {
    let f = owner(w, id);
    let (ix, iy) = (w.fighter(f).controls.inp.x, w.fighter(f).controls.inp.y);
    let d = get8_way_no_facing(ix, iy);
    let (vx, vy) = (pr(w, id).vx, pr(w, id).vy);
    let (wx, wy) = if matches!(d, Dir::Neutral | Dir::Default) { normal(vx, vy) } else { dir_xy(d) };
    let n = normal(wx, wy); // Normal(the switch, 0.0001) @2227
    let s = size2d([vx, vy]); // VSize @1892
    let (tx, ty) = ((n.0 * s), (n.1 * s)); // @2435
    let two = (s * s) * 2.0; // @1951-1988
    let (dx, dy) = ((vx - tx), (vy - ty));
    let dist = size2d([dx, dy]); // Vector_Distance @2481
    // @2586-2632; Divide_FloatFloat returns 0 for B == 0 (exe 142f68da0): a zero speed turns 90 deg of a zero vector
    let cos_a = if two == 0.0 { 0.0 } else { (two - (dist * dist)) / two };
    let aim = num(class_p(w, pr(w, id)), "GauntletAimMocifier").unwrap_or(0.0);
    let deg = (DELTA_SECONDS * aim) * deg_acos(cos_a); // @2030, @2715
    // RotateAngleAxis of the velocity, UE (0, -x, y), about -X (@2761): UE (Y, Z) -> (Y cos + Z sin, Z cos - Y sin), a
    // turn of +deg in sim (x, y); about +X (@2888): the turn of -deg.
    let a = rotate_angle_axis([0.0, -vx, vy], deg, [-1.0, 0.0, 0.0]);
    let (ax, ay) = (-a[1], a[2]);
    let b = rotate_angle_axis([0.0, -vx, vy], deg, [1.0, 0.0, 0.0]);
    let (bx, by) = (-b[1], b[2]);
    let da = dist2d([tx, ty], [ax, ay]); // Vector_Distance @2842
    let db = dist2d([tx, ty], [bx, by]); // @2969
    if db < da {
        multicast_velocity(w, id, bx, by, false); // SelectVector(+X, -X, db < da) @3053, MulticastVelocity @3108
    } else {
        multicast_velocity(w, id, ax, ay, false);
    }
}

/// ActivateObject @3163 (authority).
fn activate(w: &mut World, id: ProjectileId) {
    pm(w, id).activated = true; // Activated @3207
    let (vx, vy) = (pr(w, id).vx, pr(w, id).vy);
    let n = normal(vx, vy);
    let mult = num(class_p(w, pr(w, id)), "ActivateMultiplier").unwrap_or(0.0);
    let s = size2d([vx, vy]) * mult; // VSize @3341
    multicast_velocity(w, id, n.0 * s, n.1 * s, true); // @3433 (ScaleUpVFX true: the view's)
    let life = num(class_p(w, pr(w, id)), "MaxLifetimeAfterRelease").unwrap_or(0.0); // @3458
    update_max_lifetime(pm(w, id), life, 1);
}

/// ReceiveEndPlay @6809 -> @3486 (authority; the Instigator's DamageManager is valid on every fighter).
// Deviation: not in the Lab reset's pass (an old pawn there, whose ability slot and marker the reset drops with it; the
// build's EndPlay of the reset's DestroyActor would run the perk branch on the old pawn).
fn end_play(w: &mut World, id: ProjectileId) {
    if gl(w, id).end_ran {
        return;
    }
    let g = gl_w(w, id);
    g.end_ran = true;
    g.kb_bound = None; // the unbind @3728
    if reset_pass(w) {
        return;
    }
    let f = owner(w, id);
    if buff_of(w, f, MARKER_BUFF).is_none() {
        return; // BPI_CheckIfHasBuff @3868 -> @4021
    }
    if garnet_of(w, f).is_some() && are_gauntlets_cast(w, f) {
        return; // @4134 -> @4180 (the cast's failure: false @4249)
    }
    if !trigger_ability_check(w, f, 1) {
        return; // @4450 -> @4500
    }
    // @10-933. TraceEnd for no hit is the trace's end, the gloves' place.
    // Unverified: FHitResult's TraceEnd (the stock 4.26 source fills it before the sweep).
    let (r, hh) = (w.fighter(f).capsule.cap_radius_u, w.fighter(f).capsule.cap_half_u);
    let (px, py) = (pr(w, id).x, pr(w, id).y);
    let hit = capsule_trace_single(
        w,
        px,
        py + hh,
        px,
        py,
        r,
        hh,
        TRACE_PROJECTILE_GROUND_NO_PLATFORM,
        &[],
        Some(ActorRef::Projectile(id)),
    );
    let (x, y) = match hit {
        Some(h) if h.blocking => (h.x, h.y),
        _ => (px, py),
    };
    comp_spawn_projectile(w, f, MARKER2, x, y, [Some(0.0); 3], 0.0, 0.0, None);
}

fn construct(_w: &World, p: &mut Projectile) {
    p.ext = ProjectileExt::GarnetGauntlets(GarnetGauntlets::default());
}

/// ReceiveBeginPlay @1064 (authority): the bind @1316, then AutoActivate @1357.
fn begin(w: &mut World, id: ProjectileId) {
    let f = owner(w, id);
    let kb = w.fighter(f).attackers.knocked_back_count;
    let g = gl_w(w, id);
    g.kb_bound = Some(f);
    g.kb_at = kb;
    if crate::registry::props::flag(class_p(w, pr(w, id)), "AutoActivate") {
        activate(w, id);
    }
}

fn step(w: &mut World, id: ProjectileId) -> bool {
    // OnInstigatorKnockedBack -> ReleaseToPool @6833: ahead of the tick's steering.
    if let Some(b) = gl(w, id).kb_bound
        && w.fighter(b).attackers.knocked_back_count != gl(w, id).kb_at
    {
        return true;
    }
    if !pr(w, id).activated {
        steer(w, id); // @1443
    }
    false
}

fn kind() -> ProjectileHooks {
    ProjectileHooks {
        construct: Some(construct),
        begin: Some(begin),
        step: Some(step),
        activate: Some(activate),
        destroyed: Some(end_play),
        end: Some(end_play),
        ..ProjectileHooks::default()
    }
}

/// This file's registry entries (the two gauntlet classes).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.actors.enter(
        "GarnetGauntletsProjectile_C",
        ActorEntry::new(
            ActorClass::Projectile(ProjectileClass::GarnetGauntletsProjectile),
            ActorHooks::Projectile(kind()),
        ),
    )?;
    r.actors.enter(
        "GarnetGauntletsProjectileAir_C",
        ActorEntry::new(
            ActorClass::Projectile(ProjectileClass::GarnetGauntletsProjectileAir),
            ActorHooks::Projectile(kind()),
        ),
    )?;
    Ok(())
}
