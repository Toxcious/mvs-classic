//! Bugs Bunny's rockets: BugsBunnyRocketFProjectile and
//! BugsBunnyRocketUpProjectile ([`Rocket`], a projectile's `ext`). Both are ParentProjectilePhysics children with a
//! NoCollision root Capsule (10 / 10): no blocking hit, so DestroyOnCollideWithEnvironment never fires for them
//! (`Projectile.step`); they end at their CapsuleNav's begin overlap with the stage, a fighter hit, a block, a
//! projectile's hit on their HurtBox, or (RocketF, from 1 s on) the blast box. Their hit and hurt boxes sit ahead of
//! the root along its X (the forward / up the velocity turns it to: bRotationFollowsVelocity).
//!
//! The overrides of hitX / hitY / hitNear / hitShapes / hurtX / hurtY are the methods below; the shared
//! `actors::projectile` accessors call them for a `ProjectileExt::Rocket`.

use crate::Real;
use crate::State;
use crate::actors::projectile::{ProjectileExt, class_def, class_p, set_hitbox, yaw_x};
use crate::actors::{ActorClass, ActorRef, Projectile, ProjectileClass, ProjectileId};
use crate::combat::boxes::WorldSphere;
use crate::combat::elem::{charge_pct, end_actor_children};
use crate::combat::hits::{actor_hit_enemy, apply_projectile_hit, dealer, projectile_blocks};
use crate::combat::spawned_hits::damage_spawned_actor;
use crate::combat::{DataEnum, DataVec, HitPacket, VacuumInfo};
use crate::consts::DELTA_SECONDS;
use crate::events::{Event, SoundKey};
use crate::fighter::common::{latent_frames, owner_team, warn_text};
use crate::fighter::control::attack_dodged;
use crate::fighter::moves::V2;
use crate::fighter::traces::solids::QUERY_UNKNOWN;
use crate::fighter::traces::{OBJ_HURT_BOX, OBJ_PAWN, OBJ_PHYSICS_BODY, TraceBody, body_info, sphere_overlap_actors};
use crate::fighter::{FighterId, invulnerable, unequip_object};
use crate::math::{max, min};
use crate::name;
use crate::registry::entries::{ActorEntry, ActorHooks, ProjectileHooks};
use crate::registry::props::{num, obj};
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use crate::world::blast::{blast_box_loop, check_blast_box_exit, spawn_box};
use crate::world::flow::reset_pass;
use crate::world::pawns::match_fighters;
use crate::world::spawned_platform::projectile_box;
use crate::world::spawning::destroy_projectile;
use mvs_data::value::Props;

pub const ROCKET_F: &str = "BugsBunnyRocketFProjectile_C";
pub const ROCKET_UP: &str = "BugsBunnyRocketUpProjectile_C";

// ---------------------------------------------------------------- the boxes on the root

/// A place on the root in its local X (forward) and Z (up), unscaled.
#[derive(State, Copy, Debug, Default)]
pub struct At {
    pub ox: Real,
    pub oz: Real,
}

/// A capsule along local X (pitch 90 in the data): its centre, the half length of its core segment (half height less
/// radius) and its radius.
#[derive(State, Copy, Debug, Default)]
pub struct Seg {
    pub ox: Real,
    pub oz: Real,
    pub half: Real,
    pub r: Real,
}

impl Seg {
    /// A place on the core (`u`: -1 one end, 1 the other).
    fn on_core(&self, u: Real) -> At {
        At { ox: self.ox + self.half * u, oz: self.oz }
    }
}

/// `Geo`: per class, from its data (read at the first make).
#[derive(State, Debug, Default)]
pub struct Geo {
    /// The dealing HitBox's (RocketF: the HitBox sphere; HitBox2D is at the same place).
    pub hit: At,
    /// The HurtBox sphere's.
    pub hurt: At,
    /// CapsuleNav.
    pub nav: Seg,
    /// RocketUp: the CapsuleHitbox (tag HitBox), the 3D dealer in place of a sphere.
    pub dealer: Option<Seg>,
}

fn at_of(c: Option<&Props>) -> At {
    let l = c.and_then(|c| obj(c, "RelativeLocation"));
    At { ox: l.and_then(|l| num(l, "X")).unwrap_or(0.0), oz: l.and_then(|l| num(l, "Z")).unwrap_or(0.0) }
}

fn seg_of(w: &World, cls: &str, c: Option<&Props>) -> Seg {
    let Some(c) = c else {
        warn_text(w, &format!("{cls}: a capsule of the class data is missing"));
        return Seg::default();
    };
    let pitch = obj(c, "RelativeRotation").and_then(|r| num(r, "Pitch")).unwrap_or(0.0);
    if (pitch.abs() - 90.0).abs() > 0.01 {
        warn_text(w, &format!("{cls}: a capsule not along the root's X (pitch {pitch})"));
    }
    let r = num(c, "CapsuleRadius").unwrap_or(22.0);
    let hh = num(c, "CapsuleHalfHeight").unwrap_or(44.0);
    let a = at_of(Some(c));
    Seg { ox: a.ox, oz: a.oz, half: max(hh - r, 0.0), r }
}

// ---------------------------------------------------------------- the actor

/// `Rocket extends Projectile`: what the class adds.
#[derive(State, Debug)]
pub struct Rocket {
    /// CurrentSpeed (RocketF's ReceiveTick): BeginPlay's ProjectileMovement.InitialSpeed. BeginPlay runs inside
    /// FinishSpawningActor, ahead of BPI_SetInitialMovement (HandleActorPostSpawn), so it is the template's 3600, not
    /// the notify's.
    pub speed: Real,
    /// Actors to Ignore (fighters): HitEnemyPlayer's HitPlayer and Blocked's BlockingActor, left out of the burst.
    pub ignored: Vec<FighterId>,
    /// The burst ran (ReceiveDestroyed; the end runs it once more otherwise).
    pub exploded: bool,
    /// The age CheckForBlastBox set CheckBlastBoxTimer at (-1: not set).
    pub blast_from: i32,
    /// The timer's next expiry, in seconds of counted deltas.
    pub blast_next: crate::Clock,
    pub geo: Geo,
}

impl Default for Rocket {
    /// The field initial values.
    fn default() -> Self {
        Rocket {
            speed: 0.0,
            ignored: Vec::new(),
            exploded: false,
            blast_from: -1,
            blast_next: f64::from(0.1_f32),
            geo: Geo::default(),
        }
    }
}

impl Rocket {
    /// The capsule dealer while the 3D hit boxes deal (RocketUp), else `None`.
    fn capsule(&self, p: &Projectile) -> Option<Seg> {
        if p.sphere_3d { self.geo.dealer } else { None }
    }

    /// A place on the root in the world: its local X along the facing side turned by the pitch, as the platform box's
    /// (`spawned_platform::projectile_box_follow`).
    fn wx(&self, p: &Projectile, a: At) -> Real {
        let k = p.scale;
        p.x + yaw_x(p) * crate::math::ue::turn_by_pitch(p.pitch, [a.ox, a.oz])[0] * k
    }

    fn wy(&self, p: &Projectile, a: At) -> Real {
        let k = p.scale;
        p.y + crate::math::ue::turn_by_pitch(p.pitch, [a.ox, a.oz])[1] * k
    }

    /// `hitX` / `hitY`.
    pub fn hit_xy(&self, p: &Projectile) -> (Real, Real) {
        match self.capsule(p) {
            Some(d) => {
                let a = At { ox: d.ox, oz: d.oz };
                (self.wx(p, a), self.wy(p, a))
            }
            None => (self.wx(p, self.geo.hit), self.wy(p, self.geo.hit)),
        }
    }

    /// `hitNear`: the point of the capsule's core segment nearest (x, y), for its tests against another actor's sphere
    /// or box; the hit sphere's centre without a capsule dealer.
    pub fn hit_near(&self, p: &Projectile, x: Real, y: Real) -> V2 {
        let Some(d) = self.capsule(p) else {
            let (hx, hy) = self.hit_xy(p);
            return V2 { x: hx, y: hy };
        };
        let (ea, eb) = (d.on_core(-1.0), d.on_core(1.0));
        let (ax, ay, bx, by) = (self.wx(p, ea), self.wy(p, ea), self.wx(p, eb), self.wy(p, eb));
        let (ex, ey) = (bx - ax, by - ay);
        let l2 = ex * ex + ey * ey;
        let t = if l2 > 0.0 { max(0.0, min(1.0, ((x - ax) * ex + (y - ay) * ey) / l2)) } else { 0.0 };
        V2 { x: ax + ex * t, y: ay + ey * t }
    }

    /// `hurtX`.
    pub fn hurt_x(&self, p: &Projectile) -> Real {
        self.wx(p, self.geo.hurt)
    }

    /// `hurtY`.
    pub fn hurt_y(&self, p: &Projectile) -> Real {
        self.wy(p, self.geo.hurt)
    }

    /// `hitShapes`: RocketUp's dealer is its CapsuleHitbox (r 80, half height 300, along the root's X at X 12).
    // Deviation: against a fighter's hurt capsules it is met as nine spheres of its radius along its core segment (the
    // union is at least 0.93 of the radius thick between two of them); the hurt-capsule test has no capsule dealer
    // shape.
    pub fn hit_shapes(&self, p: &Projectile, out: &mut Vec<WorldSphere>) {
        let Some(d) = self.capsule(p) else {
            let (x, y) = self.hit_xy(p);
            out.push(WorldSphere { x, y, z: p.sphere_3d.then_some(0.0), r: p.hit_radius, ..Default::default() });
            return;
        };
        let k = p.scale;
        for i in 0..9 {
            let at = d.on_core(crate::real(i) / 4.0 - 1.0);
            out.push(WorldSphere {
                x: self.wx(p, at),
                y: self.wy(p, at),
                z: Some(0.0),
                r: d.r * k,
                ..Default::default()
            });
        }
    }

    /// CapsuleNav over the stage block (WorldStatic; its only Overlap channel).
    // Unverified: the stage block alone is taken as the WorldStatic bodies that generate overlap events (as Morty's ship
    // sensors); another actor's WorldStatic body (none of the ported ones generates overlaps) is not met. A
    // PortalParent (the cast @3031 / @4191 that keeps the rocket alive) is never one of them: Rick's portal classes'
    // two boxes are of object types ProjectileDestructible / HitBox (GameTraceChannel8 / 3, rick.json PortalOverlap1 /
    // 2), channels CapsuleNav's Custom responses leave at their DefaultResponse ECR_Ignore (DefaultEngine.ini), so the
    // cast's success path is never reached with a portal.
    // An actor with its collision off (SetActorEnableCollision(false): Projectile.no_collision, the buffs that carry
    // or eat it) generates no overlap of its components.
    fn nav_ends(&self, p: &Projectile) -> Option<(Real, Real, Real, Real, Real)> {
        if p.no_collision {
            return None;
        }
        let n = self.geo.nav;
        let k = p.scale;
        let (a, b) = (At { ox: n.ox - n.half, oz: n.oz }, At { ox: n.ox + n.half, oz: n.oz });
        let rr = n.r * k * n.r * k;
        Some((self.wx(p, a), self.wy(p, a), self.wx(p, b), self.wy(p, b), rr))
    }
}

/// `navOverStage`: the CapsuleNav against the stage solids of the block's profile (`static_shapes`; a polygon solid by
/// the segment's distance to it).
fn nav_over_stage(w: &mut World, id: ProjectileId) -> bool {
    let p = pr(w, id);
    let ProjectileExt::Rocket(r) = &p.ext else { panic!("a rocket without its state") };
    let Some((ax, ay, bx, by, rr)) = r.nav_ends(p) else { return false };
    let gs = crate::world::stage_query::static_shapes(w, QUERY_UNKNOWN, name!("BugsBunnyRocket CapsuleNav overlap"));
    gs.polys.iter().any(|q| q.seg_dist2(ax, ay, bx, by) <= rr)
        || gs.boxes.iter().any(|g| {
            seg_box_dist2(
                ax,
                ay,
                bx,
                by,
                crate::math::narrow(g.left),
                crate::math::narrow(g.right),
                crate::math::narrow(g.top),
                crate::math::narrow(g.bottom),
            ) <= rr
        })
}

/// The squared distance from a segment to an axis-aligned box in the play plane; 0 where they meet.
#[allow(clippy::too_many_arguments, reason = "the call's parameters: the segment and the box")]
fn seg_box_dist2(ax: Real, ay: Real, bx: Real, by: Real, left: Real, right: Real, top: Real, bottom: Real) -> Real {
    let inside = |x: Real, y: Real| x >= left && x <= right && y >= bottom && y <= top;
    if inside(ax, ay) || inside(bx, by) {
        return 0.0;
    }
    // The segment against each edge: a crossing is a meet; else the nearest of the endpoints to the box and of the
    // corners to the segment.
    let cross = |cx: Real, cy: Real, dx: Real, dy: Real| {
        let d1 = (bx - ax) * (cy - ay) - (by - ay) * (cx - ax);
        let d2 = (bx - ax) * (dy - ay) - (by - ay) * (dx - ax);
        let d3 = (dx - cx) * (ay - cy) - (dy - cy) * (ax - cx);
        let d4 = (dx - cx) * (by - cy) - (dy - cy) * (bx - cx);
        d1 * d2 <= 0.0 && d3 * d4 <= 0.0
    };
    if cross(left, bottom, right, bottom)
        || cross(right, bottom, right, top)
        || cross(right, top, left, top)
        || cross(left, top, left, bottom)
    {
        return 0.0;
    }
    let pt = |x: Real, y: Real| {
        let dx = max(max(left - x, 0.0), x - right);
        let dy = max(max(bottom - y, 0.0), y - top);
        dx * dx + dy * dy
    };
    let corner = |x: Real, y: Real| {
        let (ex, ey) = (bx - ax, by - ay);
        let l2 = ex * ex + ey * ey;
        let t = if l2 > 0.0 { max(0.0, min(1.0, ((x - ax) * ex + (y - ay) * ey) / l2)) } else { 0.0 };
        let (qx, qy) = (ax + ex * t - x, ay + ey * t - y);
        qx * qx + qy * qy
    };
    min(
        min(min(pt(ax, ay), pt(bx, by)), min(corner(left, bottom), corner(right, bottom))),
        min(corner(right, top), corner(left, top)),
    )
}

fn pr(w: &World, id: ProjectileId) -> &Projectile {
    w.actors.projectiles.get(id).expect("a live projectile")
}

fn pm(w: &mut World, id: ProjectileId) -> &mut Projectile {
    w.actors.projectiles.get_mut(id).expect("a live projectile")
}

fn rk(w: &World, id: ProjectileId) -> &Rocket {
    match &pr(w, id).ext {
        ProjectileExt::Rocket(r) => r,
        e => panic!("a Bugs Bunny rocket without its state: {e:?}"),
    }
}

fn rx(w: &mut World, id: ProjectileId) -> &mut Rocket {
    match &mut pm(w, id).ext {
        ProjectileExt::Rocket(r) => r,
        e => panic!("a Bugs Bunny rocket without its state: {e:?}"),
    }
}

/// The geometry of the rocket's class data (the constructor's).
pub fn geo_of(w: &World, p: &Projectile) -> Geo {
    let cs = class_def(w, p).map(|d| &d.components);
    let comp = |n: &str| cs.and_then(|c| obj(c, n));
    let cls = w.names.str(p.cls);
    Geo {
        hit: at_of(comp("HitBox")),
        hurt: at_of(comp("HurtBox")),
        nav: seg_of(w, cls, comp("CapsuleNav")),
        dealer: (cls == ROCKET_UP).then(|| seg_of(w, cls, comp("CapsuleHitbox"))),
    }
}

/// The constructor's tail: the speed of the template, the geometry of the class data, and RocketUp's dealer radius.
fn construct(w: &World, p: &mut Projectile) {
    let d = class_def(w, p);
    let speed =
        d.and_then(|d| obj(&d.components, "ProjectileMovement")).and_then(|c| num(c, "InitialSpeed")).unwrap_or(0.0);
    let geo = geo_of(w, p);
    // RocketUp's dealer is its CapsuleHitbox (3D hit boxes: Bugs has no Use 2D Hitboxes): its radius is the hit box's
    // radius in every test (the hit box over a hurt box, a damage box, a barrier).
    if let (Some(d), true) = (geo.dealer, p.sphere_3d) {
        p.hit_radius = d.r * p.scale;
    }
    p.ext = ProjectileExt::Rocket(Rocket { speed, geo, ..Rocket::default() });
}

// ---------------------------------------------------------------- shared parts

fn loop_key(p: &Projectile) -> SoundKey {
    SoundKey { tag: name!("proj"), n: p.seq }
}

// The key of the loop RocketUp's warning starts is `'proj' + id + 'd'`: the tag `proj#d`.
fn down_key(p: &Projectile) -> SoundKey {
    SoundKey { tag: name!("proj#d"), n: p.seq }
}

fn sound(w: &mut World, id: ProjectileId, ev: crate::names::Name) {
    let p = pr(w, id);
    let e = Event::Sound { fighter: p.owner_id, ev, x: Some(p.x), y: Some(p.y), hit: None, pooled: false, key: None };
    w.events.push(e);
}

fn sound_start(w: &mut World, id: ProjectileId, ev: crate::names::Name, key: SoundKey) {
    let fighter = pr(w, id).owner_id;
    w.events.push(Event::SoundStart { fighter, ev, key });
}

fn sound_stop(w: &mut World, id: ProjectileId, key: SoundKey) {
    let fighter = pr(w, id).owner_id;
    w.events.push(Event::SoundStop { fighter, key, fade: None, ev: None, curve: None, cut: false });
}

/// The burst of ReceiveDestroyed (RocketF @3111-3263 -> @613-1414, RocketUp @4314-4443 -> @2194-3784), after the
/// parent's: SphereOverlapActors(the actor's location, 400, [PhysicsBody, Pawn, HurtBox], Actors to Ignore), and for
/// each actor found, Owner's TeamIndex != its TeamIndex -> Comp_Pawn_HitBoxHandler.pfgDoDamage(no hit box, the actor,
/// packet, no elementals, no buffs); RocketF's same-team branch (@1286) posts sfx_cha_bugs_rocketride_impact at the
/// StaticMesh, RocketUp's pops. The loop runs on either way.
/// The handler's checks of pfgDoDamage as for Black Adam's Double Strike (`black_adam::double_strike` explode): a
/// fighter once, a dodge, a block; a spawned actor through `damage_spawned_actor`.
// Deviation: the actor's own bodies (its HurtBox, of its owner's team) are not in the port's overlap (it is dead by
// then): RocketF's same-team sound for itself is not posted. The StaticMesh's place is taken as the root's.
// Deviation: the order of the actors is the port's trace order, not PhysX's.
fn burst(w: &mut World, id: ProjectileId, packet: &HitPacket, ally_sound: bool) {
    if rk(w, id).exploded {
        return;
    }
    rx(w, id).exploded = true;
    if reset_pass(w) {
        return; // the Lab reset's end pass acts on the old pawns
    }
    // ReceiveEndPlay (RocketF @2992, RocketUp @4152) calls the parent's first, and EndPlay runs before ReceiveDestroyed
    // (UWorld::DestroyActor): SpawnedActorParent @15512 has destroyed the buff children (the passive rows kept) before
    // the burst's pfgDoDamage @954, so the burst's HitEnemyPlayer reaches none of them (`actor_hit_enemy`).
    let me = ActorRef::Projectile(id);
    end_actor_children(w, me);
    let Some(f) = pr(w, id).owner else { return };
    let (x, y, attack_id) = (pr(w, id).x, pr(w, id).y, pr(w, id).attack_id);
    let ignored = rk(w, id).ignored.clone();
    let ignore: Vec<Option<ActorRef>> =
        match_fighters(w).into_iter().filter(|o| ignored.contains(o)).map(|o| Some(ActorRef::Fighter(o))).collect();
    let types = [crate::real(OBJ_PHYSICS_BODY), crate::real(OBJ_PAWN), crate::real(OBJ_HURT_BOX)];
    let found = sphere_overlap_actors(w, x, y, 400.0, &types, &ignore, -1.0);
    let mut done: Vec<FighterId> = Vec::new();
    let team = w.fighter(f).team.team;
    let ally = |w: &mut World| {
        if ally_sound {
            sound(w, id, name!("sfx_cha_bugs_rocketride_impact"));
        }
    };
    for o in found {
        let vic = match o {
            ActorRef::Fighter(v) if match_fighters(w).contains(&v) => v,
            ActorRef::Projectile(b) => {
                if b == id {
                    continue;
                }
                if owner_team(w, o) == team {
                    ally(w);
                    continue;
                }
                damage_spawned_actor(w, me, b, packet);
                continue;
            }
            _ => {
                warn_text(
                    w,
                    &format!(
                        "{}: the burst met an actor that is no fighter and no projectile (not hit)",
                        w.names.str(pr(w, id).cls)
                    ),
                );
                continue;
            }
        };
        if w.fighter(vic).team.team == team {
            ally(w);
            continue;
        }
        if done.contains(&vic) || w.fighter(vic).life.dead > 0.0 || crate::combat::phases::no_hit_by_team(w, team, vic)
        {
            continue;
        }
        if projectile_blocks(w, &dealer(w, me), vic) {
            continue;
        }
        if invulnerable(w, vic) {
            let tick = w.flow.tick;
            attack_dodged(w, me, vic, attack_id, packet.base_knock_back.unwrap_or(0.0), tick, None, false);
            continue;
        }
        done.push(vic);
        let d = dealer(w, me);
        let at = V2 { x, y };
        if apply_projectile_hit(w, &d, vic, packet, &[], at, None, &[], charge_pct(w, me)) {
            actor_hit_enemy(w, &d, vic);
        }
    }
}

/// CheckBlastBoxTimer (SpawnedActorParent CheckBlastBoxExit, 0.1 s looping, from CheckForBlastBox): outside the blast
/// box K2_DestroyActor. As the parent's (`expiry`) it counts from the next frame's timer tick on.
fn blast_timer(w: &mut World, id: ProjectileId) -> bool {
    let r = rk(w, id);
    if r.blast_from >= 0 && crate::consts::clock(crate::real(pr(w, id).age - 1 - r.blast_from)) > r.blast_next {
        rx(w, id).blast_next += f64::from(0.1_f32);
        return true;
    }
    false
}

/// HitEnemyPlayer (RocketF @3268, RocketUp @4448; no parent call): HitPlayer added to Actors to Ignore, then
/// K2_DestroyActor, which the world's DestroyOnHitFighter does in the same pending call (the class data has it).
fn hit_enemy(w: &mut World, id: ProjectileId, vic: FighterId) {
    rx(w, id).ignored.push(vic);
}

/// Blocked (@3352, @4531; no parent call): BlockingActor added, then K2_DestroyActor at once (not the parent's
/// Delay(0.0) and DestroySpawnedActor). The list holds fighters (the blast's ignore list is read over the fighters): a
/// Steven_ShieldActor as the BlockingActor is not kept, so the burst meets its Hurtbox and deals it nothing.
// Deviation: the build ignores the shield actor; same damage.
fn blocked(w: &mut World, id: ProjectileId, by: ActorRef) {
    if let ActorRef::Fighter(f) = by {
        rx(w, id).ignored.push(f);
    }
    if !pr(w, id).dead {
        destroy_projectile(w, id, false);
    }
}

// ---------------------------------------------------------------- BugsBunnyRocketFProjectile

// ReceiveBeginPlay @1903: the parent's; @1724 with authority Delay(1.0) -> @587; SetInterpolatedComponent (the
// view's); CurrentSpeed = InitialSpeed; StartingScale (RocketSquash, the view's); Delay(0.067) -> @125.
// @125: StaticMesh visible (the view's), bRotationFollowsVelocity = true (the template's already), with the owner's
// authority Comp_Spawner.UnequipObject(Rocket) @339, HitBoxOn? = true and UpdateHitbox @388-399,
// CapsuleNav.SetCollisionEnabled(QueryOnly) @413 (its begin overlap fires there: @3031), the loop
// sfx_cha_bugs_rocketride_loop @451; then for every role Collision.SetCollisionEnabled(QueryAndPhysics) @516 (the
// platform box Bugs rides: `world::spawned_platform`) and RocketSquash (the view's).
// @587 (1 s): DestroyOnExitedArena? = true and CheckForBlastBox: outside the blast box (no
// ForceSpawnPositionInsideBlastBox in the data) DestroySpawnedActor, else the 0.1 s CheckBlastBoxTimer.
// ReceiveTick @2190: with CurrentSpeed > FinalSpeed (1200), sub = CurrentSpeed - dt x Deceleration (6000); sub <=
// FinalSpeed: CurrentSpeed = FinalSpeed and Velocity = Normal(Velocity, 1e-4) x FinalSpeed, else CurrentSpeed = sub
// and Velocity = Normal(Velocity) x CurrentSpeed. The next move's LimitVelocity clamps it to MaxSpeed (the notify's).
// EndPlay / OnReleasedToPool (@2911): mix_stop_all_fade_eighthofsecond on the actor (its loop: the stop by key).
// BPI_Teleport (@30: K2_SetActorLocation, Collision off, Delay(0.1), Collision on) has no caller that reaches a
// rocket (xref calls BPI_Teleport: BugsBunnySafe's own): not ported. BPI_IgnorePlatformCollision is empty and
// BPI_IsMovingPlatform false (the parent's, `world::spawned_platform`).
fn at_on() -> Real {
    latent_frames(0.067)
}

fn at_arm() -> Real {
    latent_frames(1.0)
}

/// Normal(V, 1e-4) x s.
fn set_speed(p: &mut Projectile, s: Real) {
    let [_, nx, ny] = crate::math::ue::normal([0.0, p.vx, p.vy], crate::math::ue::KINDA_SMALL_NUMBER);
    p.vx = nx * s;
    p.vy = ny * s;
}

/// The box each rocket's `begin` makes: `projectile_box`'s component and `turn`, by class.
pub fn box_kind(class: ProjectileClass) -> Option<(&'static str, bool)> {
    match class {
        ProjectileClass::BugsBunnyRocketFProjectile => Some(("Collision", false)),
        // along the flight, up or down
        ProjectileClass::BugsBunnyRocketUpProjectile => Some(("Box", true)),
        _ => None,
    }
}

fn begin_f(w: &mut World, id: ProjectileId) {
    let (comp, turn) = box_kind(pr(w, id).class).expect("a rocket's box");
    projectile_box(w, id, comp, turn); // off (NoCollision) until @516
}

fn step_f(w: &mut World, id: ProjectileId) -> bool {
    let a = pr(w, id).age;
    if blast_timer(w, id) {
        let p = pr(w, id);
        if check_blast_box_exit(p.x, p.y, &p.blast_bounds) {
            return true;
        }
    }
    // CapsuleNav's begin overlap inside this frame's move (on from @413 of an earlier frame): ReleaseToPool ->
    // K2_DestroyActor (no CanBePooled).
    if crate::real(a) > at_on() && nav_over_stage(w, id) {
        return true;
    }
    // ReceiveTick, then the latent actions of the frame.
    let (fin, decel) = {
        let p = class_p(w, pr(w, id));
        (num(p, "FinalSpeed").unwrap_or(0.0), num(p, "Deceleration").unwrap_or(0.0))
    };
    if rk(w, id).speed > fin {
        let sub = rk(w, id).speed - (DELTA_SECONDS * decel);
        let s = if sub <= fin { fin } else { sub };
        rx(w, id).speed = s;
        set_speed(pm(w, id), s);
    }
    if crate::real(a) == at_on() {
        if let Some(o) = pr(w, id).owner {
            unequip_object(w, o, name!("Rocket"));
        }
        set_hitbox(pm(w, id), true);
        if nav_over_stage(w, id) {
            return true;
        }
        let key = loop_key(pr(w, id));
        sound_start(w, id, name!("sfx_cha_bugs_rocketride_loop"), key);
        for b in pm(w, id).solids.iter_mut().flatten() {
            b.on = true;
        }
    }
    if crate::real(a) == at_arm() {
        // DestroyOnExitedArena? = true (@587), CheckForBlastBox (@598: SpawnedActorParent's BeginPlay loop run again).
        let sb = spawn_box(w);
        let Projectile { x, y, prev_x, prev_y, blast_bounds, .. } = pm(w, id);
        if blast_box_loop(&sb, x, y, Some((prev_x, prev_y)), false, blast_bounds, true) {
            return true;
        }
        rx(w, id).blast_from = a;
    }
    false
}

fn destroyed_f(w: &mut World, id: ProjectileId) {
    let hit = pr(w, id).hit.clone();
    burst(w, id, &hit, true);
}

fn end_f(w: &mut World, id: ProjectileId) {
    let hit = pr(w, id).hit.clone();
    burst(w, id, &hit, true);
    let key = loop_key(pr(w, id));
    sound_stop(w, id, key);
}

// ---------------------------------------------------------------- BugsBunnyRocketUpProjectile

// ReceiveBeginPlay @4021: the parent's (HitBoxOn? true in the data: the CapsuleHitbox on from the spawn), then
// Delay(0.067) -> @125: StaticMesh visible, bRotationFollowsVelocity, with the owner's authority UnequipObject(Rocket)
// and the loop sfx_cha_bugs_rocketride_loop @389; then (push_flow 455, every role) Delay(7.0) -> @510:
// Comp_Pawn_HitBoxHandler.UpdateHitBoxDamage(Hit Box Tags, the down packet @510-1543) @1640; Delay(7.0) -> @1819:
// PS_BugsBunnyRocket_DarkSmoke attached at ParticleSystem1 (the view's); Delay(2.0) -> @1970:
// sfx_cha_bugs_rocketride_warning and sfx_cha_bugs_rocketride_loop_down on the actor. No ReceiveTick of its own, no
// MaxLifetime, and DestroyOnExitedArena? is false and never written: it ends by its CapsuleNav (on from the spawn, as
// the data has it), a hit, a block or a hit on its HurtBox.
// The handler's UpdateHitBoxDamage keeps the AttackTag of the first HitBoxesToDamage entry and the actors already
// hit (Projectile.bombHit's rule): the packet is `pr.hit` with the tag kept.
// BPI_Teleport (@2101: Box off, Delay(0.1), @30) is not reached (as RocketF's).
// The Box (BoxExtent 230 / 60 / 60 at X -20, QueryAndPhysics from the template to the end, profile Custom with no
// ObjectType: WorldDynamic; Pawn and WorldStatic unlisted: Block; WallJump Block) is a solid of the fighters' movement
// (`world::spawned_platform` projectile_box: landed on and ridden, a wall from the sides and below, a push out of a
// capsule it moved into) and a wall of their WallJump traces, and a body of the traces ([`bodies_up`]). It turns with
// the flight: along the velocity, up or down (bRotationFollowsVelocity).
// The owner's capsule meets it as any other's (read: no owner or instigator exemption anywhere in the graphs or the
// sweep path; the Box rides a NoCollision root moved without a sweep, so the rocket never pushes): at the spawn it
// stands in Bugs's capsule and his own next move resolves it (MTD x (depth + 0.125), X dropped, clamp 500; zero
// adjustment: the move is held).
// Unverified: the spawn-frame pose decides whether the MTD is along world Y (the port's ~102 push) or world X (held);
// not read live.
fn at_down() -> Real {
    at_on() + latent_frames(7.0)
}

/// `AT_SMOKE`: PS_BugsBunnyRocket_DarkSmoke's frame (the view's).
pub fn at_smoke() -> Real {
    at_down() + latent_frames(7.0)
}

fn at_warn() -> Real {
    at_smoke() + latent_frames(2.0)
}

/// `PACKET_BASE`: the packet the Up rocket's down and burst hits share.
fn packet_base() -> HitPacket {
    HitPacket {
        damage: Some(10.0),
        base_knock_back: Some(1800.0),
        knock_back_scaling: Some(20.5),
        attack_tag: Some(name!("None")),
        hit_reaction: Some(DataEnum::Value(3)),
        hit_pause_time: Some(0.115),
        hit_stun_mult: Some(1.0),
        on_hit_audio: Some(name!("sfx_cha_bugs_rocket_hit")),
        directional_knockback: Some(false),
        velocity_instead_of_facing: Some(false),
        knock_back_mirror: Some(false),
        ignore_facing: Some(false),
        attack_id: Some(0),
        do_los_check: Some(false),
        force_knockback: Some(false),
        queue_hit_pause_until_poses: Some(false),
        hit_only_one_fighter: Some(false),
        armor_pierce: Some(false),
        destroy_projectiles: Some(false),
        reflect_projectiles: Some(false),
        vacuum_knockback: Some(false),
        direct_knockback_towards_socket: Some(false),
        knockback_target_socket: Some(name!("None")),
        knockback_socket_offset: Some(DataVec { x: Some(0.0), y: Some(0.0), z: None }),
        play_on_hit_effects: Some(true),
        apply_hit_pause_to_damage_dealer: Some(true),
        attack_decay_type: Some(DataEnum::Value(0)),
        attack_decay_name: Some(name!("None")),
        additive_kb_mode: Some(DataEnum::Value(0)),
        kb_scaling_starting_dmg: Some(0.0),
        hit_each_actor_only_once: Some(true),
        account_for_velocity_with_vacuum_knockback: Some(true),
        ignore_kbi: Some(false),
        kbi_multiplier: Some(1.0),
        vacuum_info: Some(VacuumInfo {
            use_vacuum_knock_back: Some(false),
            account_for_attacker_velocity: Some(true),
            ..VacuumInfo::default()
        }),
        ..HitPacket::default()
    }
}

/// @510-1543: (-0.05, -0.5).
fn down_packet() -> HitPacket {
    HitPacket { knock_back_direction_vector: Some(DataVec { x: Some(-0.05), y: Some(-0.5), z: None }), ..packet_base() }
}

/// The burst's @2472-3505: (-0.05, 0.5); the rest the same.
fn burst_packet() -> HitPacket {
    HitPacket { knock_back_direction_vector: Some(DataVec { x: Some(-0.05), y: Some(0.5), z: None }), ..packet_base() }
}

fn begin_up(w: &mut World, id: ProjectileId) {
    let (comp, turn) = box_kind(pr(w, id).class).expect("a rocket's box");
    projectile_box(w, id, comp, turn); // on (QueryAndPhysics) from the spawn
}

fn step_up(w: &mut World, id: ProjectileId) -> bool {
    let a = crate::real(pr(w, id).age);
    // CapsuleNav's begin overlap (@4191 -> @4271 ReleaseToPool -> K2_DestroyActor).
    // Deviation: met at the end of each move; one already over the stage at the spawn ends in the spawn frame's tick.
    if nav_over_stage(w, id) {
        return true;
    }
    if a == at_on() {
        if let Some(o) = pr(w, id).owner {
            unequip_object(w, o, name!("Rocket"));
        }
        let key = loop_key(pr(w, id));
        sound_start(w, id, name!("sfx_cha_bugs_rocketride_loop"), key);
    } else if a == at_down() {
        let tag = pr(w, id).hit.attack_tag;
        pm(w, id).hit = HitPacket { attack_tag: tag, ..down_packet() };
    } else if a == at_warn() {
        sound(w, id, name!("sfx_cha_bugs_rocketride_warning"));
        let key = down_key(pr(w, id));
        sound_start(w, id, name!("sfx_cha_bugs_rocketride_loop_down"), key);
    }
    false
}

fn destroyed_up(w: &mut World, id: ProjectileId) {
    burst(w, id, &burst_packet(), false);
}

fn end_up(w: &mut World, id: ProjectileId) {
    burst(w, id, &burst_packet(), false);
    let (k, d) = (loop_key(pr(w, id)), down_key(pr(w, id)));
    sound_stop(w, id, k);
    sound_stop(w, id, d);
}

/// The Box as a body of the traces (WorldDynamic; it blocks the WallJump channel), where the movement meets it (its
/// projectile box). The class's other bodies beyond the parent's gather (CapsuleNav: a Custom shape, WorldDynamic;
/// Sphere: DetectPawn, WorldDynamic) are of an object type the traces do not gather. CapsuleNav's Custom profile leaves
/// the engine channels unlisted, so it blocks Visibility and Camera (an unlisted engine channel is Block); no port
/// query traces those channels yet.
fn bodies_up(w: &World, a: ActorRef, out: &mut Vec<TraceBody>) {
    let ActorRef::Projectile(id) = a else { return };
    let p = pr(w, id);
    let info = body_info(class_def(w, p).and_then(|d| obj(&d.components, "Box")), None);
    for s in p.solids.iter().flatten() {
        let Some(info) = info else { continue };
        if !s.on || p.no_collision {
            continue;
        }
        out.push(TraceBody {
            ax: s.x,
            ay: s.y,
            bx: s.x,
            by: s.y,
            hx: s.hx,
            hy: s.hy,
            r: 0.0,
            z: Real::NAN,
            object_type: info.kind,
            profile: name!("Custom"),
            component: name!("Box"),
            actor: Some(a),
            platform: -1,
            blocks: info.blocks,
            poly: None,
            floor: None,
        });
    }
}

/// This file's registry entries (the two rockets). `foreign_owner` (Taz's burp: a rocket he ate is spawned again with
/// him as its owner): the classes' code reads of the owner only what any fighter answers: its id and team, its actor
/// data (`actor_class_def` falls back to the match's other fighters), BPI_UnequipObject('Rocket') (a tag he has no mesh
/// for: nothing). The burst hits the actors of another team than the owner's, as the build's GetOwner().
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let rocket_f = ProjectileHooks {
        construct: Some(construct),
        begin: Some(begin_f),
        step: Some(step_f),
        hit_enemy: Some(hit_enemy),
        blocked: Some(blocked),
        destroyed: Some(destroyed_f),
        end: Some(end_f),
        ..ProjectileHooks::default()
    };
    let rocket_up = ProjectileHooks {
        construct: Some(construct),
        bodies: Some(bodies_up),
        begin: Some(begin_up),
        step: Some(step_up),
        hit_enemy: Some(hit_enemy),
        blocked: Some(blocked),
        destroyed: Some(destroyed_up),
        end: Some(end_up),
        ..ProjectileHooks::default()
    };
    r.actors.enter(
        ROCKET_F,
        ActorEntry {
            foreign_owner: true,
            ..ActorEntry::new(
                ActorClass::Projectile(ProjectileClass::BugsBunnyRocketFProjectile),
                ActorHooks::Projectile(rocket_f),
            )
        },
    )?;
    r.actors.enter(
        ROCKET_UP,
        ActorEntry {
            foreign_owner: true,
            ..ActorEntry::new(
                ActorClass::Projectile(ProjectileClass::BugsBunnyRocketUpProjectile),
                ActorHooks::Projectile(rocket_up),
            )
        },
    )
}
