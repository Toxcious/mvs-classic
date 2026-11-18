//! Marvin's blasters: C018UpBlasterProj_C, C018UpBlasterProjAir_C and
//! C018ComboBlasterProjVersion2_C ([`Blaster`], a projectile with a MuzzleFlashHitbox box beside its HitBox). No pawn
//! hook and no pawn state.
//!
//! What ParentProjectilePhysics and the data give with no code here: the flight, the hit box (HitBox2D), the packets,
//! OnHitAudio, the Destroy flags, the DeathVFX. C018UpBlasterProj's own graph: GetSpawnMaxLifetime =
//! Lerp(LowMaxLifetime, HighMaxLifetime, the Instigator's GetChargeValue(4)), no clamp (the constructor's `life`);
//! BeginPlay @433 binds HitBoxesActivated to OnHitboxesActivated @538 -> @316 (a DoOnce): timer AfterMuzzleDelay,
//! MuzzleHitBoxDuration (0.02), not looping, set inside the spawn and pending until that frame's timer tick
//! (`timer_frames`). AfterMuzzleDelay @543: HitBoxOn? false, UpdateHitbox (the hit lists cleared), then @15 HitBoxOn?
//! true, HitBoxTags = [HitBox]: the muzzle box is off from there and the HitBox starts a new hit set. ReceiveTick @573
//! -> @137 (a DoOnce: the first tick): MuzzleFlashHitbox detached (keep world) @213 and SetMobility(Static) @252: the box
//! stays where the first move left it (the ProjectileMovement ticks ahead of its owner, so behind the first move).
//! MuzzleFlashHitbox: BoxExtent (80, 200, 200) (Combo (80, 200, 100)) at relative X -75 under the Capsule, tags
//! MuzzleHitBox and MuzzleHitBox2D, on with the HitBox while HitBoxTags names it.
//!
//! C018ComboBlasterProjVersion2 (its own graph): BeginPlay @109: PostAkEvent(sfx_cha_C018_combo_03_projectile_loop) on
//! self @129 and sfx_cha_C018_combo_03_cast @195; then Delay(0.067) -> @70 MuzzleFlashHitbox.SetCollisionEnabled(
//! NoCollision): the box is already out of HitBoxTags from the 0.02 s timer, so the write changes nothing the port
//! reads. OnProjectileBounce -> @261: sfx_cha_C018_combo_03_projectile_bounce at the actor's place. The loop is posted
//! on the actor's own Ak component, which ends it with the actor (as `chars::bugs::pie`).
// Unverified: the box takes the velocity's frame from the spawn on (ProjectileMovement InitializeComponent's
// SetWorldRotation(Velocity.Rotation()) with bRotationFollowsVelocity, stock UE 4.26, not read in the exe; the port's
// Projectile.pitch is taken the same way at its constructor).
// Deviation: the muzzle box is tested against the fighters' hurt shapes alone (`World.projectileTest`); against another
// spawned actor's hurt sphere the port reads the HitBox alone (`Projectile.hitNear`).
// `hit_shapes` takes the World and the projectile by shared reference, so it computes the box's place and frame
// without storing them in the projectile (until the freeze); the stored frame is the freeze's (`step`).

use crate::Real;
use crate::State;
use crate::actors::projectile::{MakeOpts, hit_xy, set_hitbox};
use crate::actors::{ActorClass, ActorRef, Projectile, ProjectileClass, ProjectileExt, ProjectileId};
use crate::combat::boxes::WorldSphere;
use crate::events::{Event, SoundKey};
use crate::fighter::FighterId;
use crate::fighter::common::{actor_def, timer_frames};
use crate::fighter::match_side::SpawnRequest;
use crate::math::ue::{quat_rotate_vector, rotator_quaternion, vector_rotation};
use crate::name;
use crate::registry::entries::{ActorEntry, ActorHooks, ProjectileHooks};
use crate::registry::props::{flag, num, obj};
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use mvs_data::value::Value;

/// C018UpBlasterProj's class.
pub const UP_BLASTER: &str = "C018UpBlasterProj_C";

/// C018UpBlasterProjAir's class.
pub const UP_BLASTER_AIR: &str = "C018UpBlasterProjAir_C";

/// C018ComboBlasterProjVersion2's class.
pub const COMBO_BLASTER: &str = "C018ComboBlasterProjVersion2_C";

/// `Blaster extends Projectile`: the class's own fields (`ProjectileExt::Blaster`).
#[derive(State, Debug, Default)]
pub struct Blaster {
    /// HitBoxTags names MuzzleHitBox (with the HitBox's HitBoxOn?).
    pub muzzle_on: bool,
    /// The age AfterMuzzleDelay fires at (-1: never).
    pub muzzle_off_age: Real,
    /// Detached at the first tick.
    pub muzzle_frozen: bool,
    /// The box's place (sim x, y, depth) and the local X along (ux, uy, uz).
    pub mx: Real,
    pub my: Real,
    pub mz: Real,
    pub ux: Real,
    pub uy: Real,
    pub uz: Real,
    pub m_scale: Real,
    /// MuzzleFlashHitbox's BoxExtent (`ext`).
    pub ext_x: Real,
    pub ext_y: Real,
    pub ext_z: Real,
    /// MuzzleFlashHitbox's RelativeLocation.X.
    pub rel_x: Real,
}

/// `frame`: the box's frame from the projectile's velocity and place: (ux, uy, uz, mScale, mx, my, mz). The actor's
/// rotation is Velocity.Rotation() (bRotationFollowsVelocity), its local X turned by that rotator's quaternion
/// (FRotator::Quaternion): sim (x, y, depth) = UE (-Y, Z, X). A zero velocity gives the zero rotation (local X the
/// depth axis).
fn frame(p: &Projectile, rel_x: Real) -> [Real; 7] {
    let q = rotator_quaternion(vector_rotation([0.0, -p.vx, p.vy]));
    let [x, y, z] = quat_rotate_vector(q, [1.0, 0.0, 0.0]);
    let (ux, uy, uz) = (-y, z, x);
    let ms = p.scale;
    let off = rel_x * ms;
    [ux, uy, uz, ms, p.x + ux * off, p.y + uy * off, uz * off]
}

/// `Blaster.hitShapes`: the HitBox's shape, then MuzzleFlashHitbox while it is on: an oriented box. Axes: X along the
/// velocity; for a velocity with a sim-x part (UE yaw +-90) local Y is the depth and local Z the in-plane normal; for a
/// vertical one (yaw 0) local Y is the in-plane normal (sim -x) and local Z the depth. Called by
/// `projectile::hit_shapes`.
// Deviation: local Y and Z are formed from local X's in-plane part (its normal in the plane, the depth axis), not
// turned by the rotator's quaternion as X is: the frozen frame keeps X alone.
pub fn hit_shapes(w: &World, p: &Projectile, out: &mut Vec<WorldSphere>) {
    let (x, y) = hit_xy(w, p);
    out.push(WorldSphere { x, y, z: p.sphere_3d.then_some(0.0), r: p.hit_radius, ..Default::default() });
    let ProjectileExt::Blaster(b) = &p.ext else { return };
    if !b.muzzle_on {
        return;
    }
    let [ux, uy, uz, s, mx, my, mz] =
        if b.muzzle_frozen { [b.ux, b.uy, b.uz, b.m_scale, b.mx, b.my, b.mz] } else { frame(p, b.rel_x) };
    let (ex, ey, ez) = (b.ext_x, b.ext_y, b.ext_z);
    let mut bx = [0.0; 9];
    bx[0] = ux * ex * s;
    bx[1] = uy * ex * s;
    bx[2] = uz * ex * s;
    if ux == 0.0 && uy == 0.0 {
        // The zero rotation: local Y = UE Y (sim -x), local Z = up.
        bx[3] = -ey * s;
        bx[7] = ez * s;
    } else {
        let in_plane = if ux != 0.0 { ez } else { ey };
        let depth = if ux != 0.0 { ey } else { ez };
        bx[3] = -uy * in_plane * s;
        bx[4] = ux * in_plane * s;
        bx[8] = depth * s;
    }
    out.push(WorldSphere { x: mx, y: my, z: Some(mz), r: 0.0, bx: Some(bx), ..Default::default() });
}

/// `make: new Blaster(f, r, a)`: the class state (MuzzleFlashHitbox's data, the HitBoxTags, the AfterMuzzleDelay).
fn make(w: &World, f: FighterId, r: &SpawnRequest) -> ProjectileExt {
    let a = actor_def(w, ActorRef::Fighter(f), r.cls);
    let m = a.and_then(|a| obj(&a.components, "MuzzleFlashHitbox"));
    let p = a.map(|a| &a.p);
    let ext = m.and_then(|m| obj(m, "BoxExtent"));
    let g = |k: &str| ext.and_then(|e| num(e, k)).unwrap_or(0.0);
    let muzzle_on = p
        .and_then(|p| p.get("HitBoxTags"))
        .and_then(Value::as_arr)
        .is_some_and(|t| t.iter().any(|v| v.as_str() == Some("MuzzleHitBox")));
    // OnHitboxesActivated: the post-spawn UpdateHitbox with HitBoxOn? (the class data's true).
    let muzzle_off_age = match p {
        Some(p) if flag(p, "HitBoxOn?") => timer_frames(Some(num(p, "MuzzleHitBoxDuration").unwrap_or(0.0))),
        _ => -1.0,
    };
    ProjectileExt::Blaster(Blaster {
        muzzle_on,
        muzzle_off_age,
        m_scale: 1.0,
        ext_x: g("X"),
        ext_y: g("Y"),
        ext_z: g("Z"),
        rel_x: m.and_then(|m| obj(m, "RelativeLocation")).and_then(|l| num(l, "X")).unwrap_or(0.0),
        ..Blaster::default()
    })
}

/// The Blaster constructor's options (`{ life, byFighter }`; the spawn sets `by_fighter`): Lerp(LowMaxLifetime,
/// HighMaxLifetime, the fighter's percentCharged) in float32, no clamp.
fn make_opts(w: &World, f: FighterId, r: &SpawnRequest) -> MakeOpts {
    let p = actor_def(w, ActorRef::Fighter(f), r.cls).map(|a| &a.p);
    let g = |k: &str| p.and_then(|p| num(p, k)).unwrap_or(0.0);
    let (lo, hi) = (g("LowMaxLifetime"), g("HighMaxLifetime"));
    let life = lo + ((hi - lo) * crate::fighter::percent_charged(w, f));
    MakeOpts { life: Some(life), ..MakeOpts::default() }
}

/// `blasterStep`: ReceiveTick's first-tick detach, then the AfterMuzzleDelay timer (the frame's timer tick, behind the
/// actors' ticks).
fn step(w: &mut World, pr: ProjectileId) -> bool {
    let Some(p) = w.actors.projectiles.get_mut(pr) else { return false };
    let ProjectileExt::Blaster(b) = &p.ext else { return false };
    let (frozen, off, rel_x) = (b.muzzle_frozen, b.muzzle_off_age, b.rel_x);
    if !frozen {
        let [ux, uy, uz, ms, mx, my, mz] = frame(p, rel_x);
        if let ProjectileExt::Blaster(b) = &mut p.ext {
            (b.ux, b.uy, b.uz, b.m_scale, b.mx, b.my, b.mz) = (ux, uy, uz, ms, mx, my, mz);
            b.muzzle_frozen = true;
        }
    }
    if crate::real(p.age) == off {
        if let ProjectileExt::Blaster(b) = &mut p.ext {
            b.muzzle_on = false; // HitBoxTags = [HitBox]
        }
        set_hitbox(p, false);
        set_hitbox(p, true);
    }
    false
}

fn loop_key(w: &World, pr: ProjectileId) -> SoundKey {
    SoundKey { tag: name!("proj"), n: w.actors.projectiles.get(pr).map_or(-1, |p| p.seq) }
}

/// `comboBlaster.begin`: the loop and the cast sound.
fn combo_begin(w: &mut World, pr: ProjectileId) {
    let Some(p) = w.actors.projectiles.get(pr) else { return };
    let (fighter, x, y) = (p.owner_id, p.x, p.y);
    let key = loop_key(w, pr);
    w.events.push(Event::SoundStart { fighter, ev: name!("sfx_cha_C018_combo_03_projectile_loop"), key });
    w.events.push(Event::Sound {
        fighter,
        ev: name!("sfx_cha_C018_combo_03_cast"),
        x: Some(x),
        y: Some(y),
        hit: None,
        pooled: false,
        key: None,
    });
}

/// `comboBlaster.bounceImpact`: the bounce sound at the actor's place.
fn combo_bounce_impact(w: &mut World, pr: ProjectileId, _nx: Real, _ny: Real, _ivx: Real, _ivy: Real) -> bool {
    let Some(p) = w.actors.projectiles.get(pr) else { return false };
    let (fighter, x, y) = (p.owner_id, p.x, p.y);
    w.events.push(Event::Sound {
        fighter,
        ev: name!("sfx_cha_C018_combo_03_projectile_bounce"),
        x: Some(x),
        y: Some(y),
        hit: None,
        pooled: false,
        key: None,
    });
    false
}

/// `comboBlaster.end`: the loop ends with the actor.
fn combo_end(w: &mut World, pr: ProjectileId) {
    let Some(p) = w.actors.projectiles.get(pr) else { return };
    let fighter = p.owner_id;
    let key = loop_key(w, pr);
    w.events.push(Event::SoundStop { fighter, key, fade: None, ev: None, curve: None, cut: false });
}

/// This file's registry entries: the three blasters (foreignOwner), the Air one and the combo one with parents
/// [C018UpBlasterProj_C].
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let up = ProjectileHooks {
        make: Some(make),
        make_opts: Some(make_opts),
        step: Some(step),
        ..ProjectileHooks::default()
    };
    let combo = ProjectileHooks {
        begin: Some(combo_begin),
        bounce_impact: Some(combo_bounce_impact),
        end: Some(combo_end),
        ..up
    };
    let enter = |r: &mut Registry,
                 cls: &'static str,
                 class: ProjectileClass,
                 k: ProjectileHooks,
                 parents: &'static [&'static str]| {
        r.actors.enter(
            cls,
            ActorEntry {
                parents,
                foreign_owner: true,
                ..ActorEntry::new(ActorClass::Projectile(class), ActorHooks::Projectile(k))
            },
        )
    };
    enter(r, UP_BLASTER, ProjectileClass::C018UpBlasterProj, up, &[])?;
    enter(r, UP_BLASTER_AIR, ProjectileClass::C018UpBlasterProjAir, up, &[UP_BLASTER])?;
    enter(r, COMBO_BLASTER, ProjectileClass::C018ComboBlasterProjVersion2, combo, &[UP_BLASTER])
}
