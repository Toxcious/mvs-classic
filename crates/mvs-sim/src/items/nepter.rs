//! ItemHelper_Nepter_C: the TV's helper ([`Nepter`], a SpawnedActorParent child, a
//! world actor whose mesh plays montages), and Projectile_Neptr_Pie_C, the pie it throws (data only, no graph of its
//! own: ParentProjectilePhysics). Offsets are the ubergraph's unless a function is named.
//!
//! Defaults: FollowMaxDistance 500, LifeTime 15, PieMovementSpeed 2000, PieThrowSpeed 0.5, WallOffset -75; the tick
//! starts off; Capsule 80 / 80 NoCollision; PieCheck a 1500 sphere overlapping Pawn. EnableProjectileMovement is not
//! set: only its own tick moves it. No hit box (HitBoxOn? tags a component it has none of), nothing hits its
//! NoCollision capsule.
//! - ReceiveBeginPlay @1706: the parent's, Delay 0.2 @1741 (-> SetActorTickEnabled(true), Follow Component, Radius =
//!   the owner's root capsule radius, the owner's FacingChanged / OnIsWallSliding / OnIsJumping bound), Delay
//!   LifeTime @2165 -> K2_DestroyActor @750, sfx_cha_bananaguard_spawn @2361.
//! - ReceiveTick @2473 follows the owner (`tick`); PieCheck's begin / end overlaps start and stop the ThrowPie timer
//!   (0.5 s looping), ThrowPie spawns the pie through the owner's Comp_Spawner @1023 and plays Neptr_Throw_Montage
//!   @5283. The end: the parent's EndPlay (DeathVFX, 'Ak Death Location').
//!
//! Unverified: the follow point. FollowLocationComponent sits on CharacterMesh0 at (-52.5, 0, -0.8333): its world
//! place is the owner's place + its facing x (mesh x + mesh scale x -52.5) and (MeshStartZLocation + the mesh's z +
//! scale x -0.8333) over the standing capsule centre; the mesh's relative rotation is taken as 0 and the actor's
//! scale as 1. Radius is the root CollisionCylinder's. Unverified: its tick runs in the world's actor pass, behind
//! the fighters' moves of the frame. Unverified: PieCheck's overlaps are tested once a frame (after the move in its
//! step) and at the spawn, behind BeginPlay, the sphere taken in the play plane, the pawns the fighters' root
//! capsules (one without collision left out). Unverified: the ThrowPie timer is counted in the step (the timer pass
//! comes after it in the frame), a period of `timer_ticks(0.5)` each. Deviation: FacingChanged reaches it on a
//! change of the owner's facing only; the tread sound of the build's repeated broadcasts is not posted.
//! CheckBlastBoxExit (DestroyOnExitedArena? true): BeginPlay's 0.1 s looping timer, counted in the late pass.
//! Deviation: the kind registers no `spawn`: its only spawner is the TV (Item_Helper), whose Nepter branch goes
//! through [`spawn_at`] (the kind's `deferred`) with the owner's ability trigger and CompSpawnActor around it.

use mvs_data::value::Props;

use crate::Real;
use crate::State;
use crate::actors::actor_montage::{ActorAnim, montage_play};
use crate::actors::world_actor::{WorldActor, WorldActorState, actor_p};
use crate::actors::{ActorClass, ActorId, ActorRef, ProjectileClass, WorldActorClass};
use crate::combat::hit::knockback::timer_ticks;
use crate::consts::DELTA_SECONDS;
use crate::events::Event;
use crate::fighter::common::{actor_data, latent_frames};
use crate::fighter::{FighterId, actor_y, cap_radius, capsule_half, no_collision};
use crate::math::ue::finterp_constant_to;
use crate::math::{max, to_int32};
use crate::name;
use crate::registry::entries::{MeshHooks, ProjectileHooks, WorldActorHooks};
use crate::registry::props::{flag, num, obj, text};
use crate::registry::signals::OwnerSignal;
use crate::registry::{ActorEntry, ActorHooks, Registry, RegistryError};
use crate::world::World;
use crate::world::blast::{BlastBounds, blast_box_loop, check_blast_box_exit, new_blast_bounds, spawn_box};
use crate::world::flow::reset_pass;
use crate::world::spawning::{comp_spawn_projectile, new_world_actor};

const PIE_CHECK_R: Real = 1500.0; // PieCheck SphereRadius
const FOLLOW_FWD: Real = -52.5; // FollowLocationComponent RelativeLocation on CharacterMesh0
const FOLLOW_ABOVE: Real = 75.0; // + (0, 0, 75) @2597
const DEPTH_SPEED: Real = 1750.0; // @4547
const SPEED: Real = 2500.0; // Lerp(0, 2500, ..)
const PIE_UP: Real = 50.0; // S.Z + 50 @476

/// `Nepter` (ItemHelper_Nepter_C).
#[derive(State, Debug)]
pub struct Nepter {
    /// Sim x (-UE Y).
    pub x: Real,
    /// UE Z.
    pub y: Real,
    /// UE X.
    pub depth: Real,
    /// Comp_Actor_Destructible.TeamIndex (the owner's team).
    pub team: i32,
    /// The world tick the Delay(0.2) fires on (-1: fired).
    pub bind_tick: i32,
    /// The world tick the Delay(LifeTime) fires on.
    pub life_tick: i32,
    /// The world tick of the spawn.
    pub born: i32,
    /// CheckBlastBoxTimer's next expiry (s since the spawn).
    pub blast_at: crate::Clock,
    pub blast_bounds: BlastBounds,
    /// SetActorTickEnabled(true) has run.
    pub ticking: bool,
    /// Radius.
    pub radius: Real,
    /// The view's inputs: FacingLeft (the owner's negated, false until the first turn), the Capsule's world scale Y,
    /// Is Wall Sliding, the AnimBP's IsJumping and JumpCount, the material's TredAlpha and the oven Light.
    pub facing_left: bool,
    pub scale_y: Real,
    pub wall_slide: bool,
    pub is_jumping: bool,
    pub jump_count: i32,
    pub tred_alpha: Real,
    pub oven_light: bool,
    /// AimActors.
    pub aim_actors: Vec<FighterId>,
    /// The fighters PieCheck overlapped at the last test.
    pub inside: Vec<FighterId>,
    /// The world tick of the looping ThrowPie timer's next fire; -1: no timer.
    pub throw_at: i32,
    /// LastThrowTime (world clock).
    pub last_throw_time: Real,
    /// Its mesh's montages (`anim`, `actors::actor_montage::anim_of`).
    pub anim: ActorAnim,
}

impl Default for Nepter {
    fn default() -> Self {
        Nepter {
            x: 0.0,
            y: 0.0,
            depth: 0.0,
            team: 0,
            bind_tick: -1,
            life_tick: 0,
            born: 0,
            blast_at: f64::from(0.1_f32),
            blast_bounds: new_blast_bounds(),
            ticking: false,
            radius: 0.0,
            facing_left: false,
            scale_y: 1.0,
            wall_slide: false,
            is_jumping: false,
            jump_count: 0,
            tred_alpha: 0.0,
            oven_light: false,
            aim_actors: Vec::new(),
            inside: Vec::new(),
            throw_at: -1,
            last_throw_time: 0.0,
            anim: ActorAnim::new(name!("Neptr_Skelmesh")),
        }
    }
}

/// The ItemHelper_Nepter_C's place (its `x`, `y`).
pub fn place(w: &World, a: &WorldActor, s: &Nepter) -> Option<(Real, Real)> {
    let _ = (w, a);
    Some((s.x, s.y))
}

fn st(w: &World, id: ActorId) -> &Nepter {
    match &w.actors.actors.get(id).expect("a live actor").state {
        WorldActorState::Nepter(n) => n,
        _ => unreachable!("ItemHelper_Nepter_C holds a Nepter"),
    }
}

fn st_mut(w: &mut World, id: ActorId) -> &mut Nepter {
    match &mut w.actors.actors.get_mut(id).expect("a live actor").state {
        WorldActorState::Nepter(n) => n,
        _ => unreachable!("ItemHelper_Nepter_C holds a Nepter"),
    }
}

fn owner(w: &World, id: ActorId) -> FighterId {
    w.actors.actors.get(id).expect("a live actor").owner
}

fn kill(w: &mut World, id: ActorId) {
    w.actors.actors.get_mut(id).expect("a live actor").dead = true;
}

/// The data's own number of the class, `d` when absent.
fn data_num(w: &World, id: ActorId, key: &str, d: Real) -> Real {
    num(actor_p(w, ActorRef::Actor(id)), key).unwrap_or(d)
}

fn sound(w: &mut World, id: ActorId, ev: crate::names::Name) {
    let (x, y) = (st(w, id).x, st(w, id).y);
    let fighter = w.fighter(owner(w, id)).id;
    w.events.push(Event::Sound { fighter, ev, x: Some(x), y: Some(y), hit: None, pooled: false, key: None });
}

/// MeshStartZLocation: CharacterMesh0's RelativeLocation.Z of the pawn's CDO (the pawn's components, else the base's).
fn mesh_start_z(w: &World, f: FighterId) -> Real {
    let c = &w.char_data(f).fighter.components;
    let z = |m: &mvs_data::fighter::Map<mvs_data::fighter::Component>| {
        let p = m.get("CharacterMesh0")?.p.as_obj()?;
        Some(num(obj(p, "RelativeLocation")?, "Z").unwrap_or(0.0))
    };
    z(&c.pawn).or_else(|| z(&c.base)).unwrap_or(0.0)
}

/// ReceiveTick @2473.
fn tick(w: &mut World, id: ActorId) {
    let o = owner(w, id);
    let fm = w.fighter(o);
    let s = fm.scaled.mesh_scale;
    let (ox, oy, ofacing, ovy) = (fm.body.x, fm.body.y, fm.body.facing_dir, fm.body.vy);
    let fx = ox + ofacing * (fm.hit.mesh_x + s * FOLLOW_FWD); // Unverified: the follow point, see the head comment
    let fy = oy + mesh_start_z(w, o) + fm.hit.mesh_z + s * (-0.8333333) + FOLLOW_ABOVE;
    let max_dist = data_num(w, id, "FollowMaxDistance", 500.0);
    let wall_off = data_num(w, id, "WallOffset", -75.0);
    let n = st_mut(w, id);
    let wall = n.wall_slide;
    let norm_y = (fx - n.x).abs() / max_dist;
    let norm_z = (fy - n.y).abs() / max_dist;
    n.tred_alpha = if !wall && SPEED * norm_y > 500.0 { 1.0 } else { 0.0 };
    let full_z = (n.y > fy && ovy > 50.0) || ovy.abs() <= 50.0 || wall;
    let vz = SPEED * if full_z { 1.0 } else { norm_z };
    let vy = SPEED * if wall { 1.0 } else { norm_y };
    let r = n.radius;
    let off = r + wall_off;
    n.depth = finterp_constant_to(n.depth, if wall { r } else { 0.0 }, DELTA_SECONDS, DEPTH_SPEED);
    let tx = if wall { ox + if n.facing_left { off } else { -off } } else { fx };
    n.x = finterp_constant_to(n.x, tx, DELTA_SECONDS, vy);
    n.y = finterp_constant_to(n.y, fy, DELTA_SECONDS, vz);
}

/// PieCheck's begin / end overlaps (unverified, see the head comment).
fn pie_check(w: &mut World, id: ActorId) {
    let (nx, ny, team) = {
        let n = st(w, id);
        (n.x, n.y, n.team)
    };
    let all: Vec<FighterId> = w.pawns.as_deref().unwrap_or(&w.players).to_vec();
    let mut now: Vec<FighterId> = Vec::new();
    for f in all {
        if no_collision(w, f) {
            continue;
        }
        let r = cap_radius(w, f);
        let s = max(capsule_half(w, f) - r, 0.0);
        let cy = actor_y(w, f);
        let dx = w.fighter(f).body.x - nx;
        let dy = max(max(cy - s - ny, 0.0), ny - (cy + s));
        let d = PIE_CHECK_R + r;
        if dx * dx + dy * dy <= d * d {
            now.push(f);
        }
    }
    let was = std::mem::take(&mut st_mut(w, id).inside);
    for &g in &was {
        if now.contains(&g) || w.fighter(g).team.team == team {
            continue;
        }
        let n = st_mut(w, id);
        if let Some(i) = n.aim_actors.iter().position(|&a| a == g) {
            n.aim_actors.remove(i);
        }
        if st(w, id).aim_actors.is_empty() {
            set_throw_timer(w, id, false);
        }
    }
    st_mut(w, id).inside.clone_from(&now);
    for &g in &now {
        if was.contains(&g) || w.fighter(g).team.team == team {
            continue;
        }
        let n = st_mut(w, id);
        if !n.aim_actors.contains(&g) {
            n.aim_actors.push(g);
        }
        if st(w, id).throw_at < 0 {
            set_throw_timer(w, id, true);
        }
    }
}

fn pie_throw_speed(w: &World, id: ActorId) -> Real {
    data_num(w, id, "PieThrowSpeed", 0.5)
}

/// The Rate of the ThrowPie timer in ticks.
fn timer_period(w: &World, id: ActorId) -> i32 {
    to_int32(timer_ticks(Some(pie_throw_speed(w, id))))
}

fn set_throw_timer(w: &mut World, id: ActorId, on: bool) {
    if !on {
        let n = st_mut(w, id);
        n.throw_at = -1;
        n.oven_light = false;
        return;
    }
    // Unverified: pending until this frame's timer tick (see the head comment).
    let at = w.flow.tick + timer_period(w, id);
    let rate = pie_throw_speed(w, id);
    let time = w.fighter(owner(w, id)).clock.time;
    let n = st_mut(w, id);
    n.throw_at = at;
    n.oven_light = true;
    if (time - n.last_throw_time) >= rate {
        throw_pie(w, id);
    }
}

fn throw_pie(w: &mut World, id: ActorId) {
    if st(w, id).aim_actors.is_empty() {
        return;
    }
    let o = owner(w, id);
    // FindClosestTarget: GetDistanceTo (actor locations; a fighter's is its actorY).
    let (nx, ny, depth) = {
        let n = st(w, id);
        (n.x, n.y, n.depth)
    };
    let aim = st(w, id).aim_actors.clone();
    let mut t = aim[0];
    let mut best = 0.0;
    for (i, &g) in aim.iter().enumerate() {
        let ax = w.fighter(g).body.x - nx;
        let ay = actor_y(w, g) - ny;
        // GetDistanceTo: Size() of (target - S) in UE axes (X depth, Y = -x, Z = y)
        let d = crate::math::ue::size([-depth, -ax, ay]);
        if i == 0 {
            best = d;
        } else if d < best {
            best = d;
            t = g;
        }
    }
    // d = unit(target - S) in UE axes (X depth, Y = -x, Z = y), its X dropped after the normal.
    let (tx, ty) = (w.fighter(t).body.x, actor_y(w, t));
    let (ux, uy, uz) = (-depth, -(tx - nx), ty - ny);
    // GetDirectionUnitVector (ItemHelper_Nepter @619): the safe normal, tolerance 1e-8
    let u = crate::math::ue::safe_normal([ux, uy, uz], crate::math::ue::SMALL_NUMBER);
    let (d_y, d_z) = (u[1], u[2]);
    let left = w.fighter(o).body.facing_dir < 0.0;
    let speed = data_num(w, id, "PieMovementSpeed", 2000.0);
    comp_spawn_projectile(
        w,
        o,
        name!("Projectile_Neptr_Pie_C"),
        nx,
        ny + PIE_UP,
        [Some(0.0), Some(if left { d_y } else { -d_y }), Some(d_z)],
        speed,
        speed,
        None,
    );
    let time = w.fighter(o).clock.time;
    st_mut(w, id).last_throw_time = time;
    // ThrowPieMontageMulticast @1209 -> @5283
    montage_play(w, ActorRef::Actor(id), name!("Neptr_Throw_Montage"), 1.0, 0.0, true);
}

/// The kind's `deferred` (and Item_Helper's CompSpawnActor spawn): the Nepter made at (x, y) for `f`, then
/// ReceiveBeginPlay's own share and PieCheck's first overlaps (unverified, see the head comment).
pub fn spawn_at(w: &mut World, f: FighterId, cls: crate::names::Name, x: Real, y: Real) -> ActorId {
    let data = std::sync::Arc::clone(&w.data);
    let a = actor_data(w, &data, ActorRef::Fighter(f), cls).unwrap_or_else(|| {
        panic!("ItemHelper_Nepter_C: no actor data for its spawn (the importer gives every spawned class)")
    });
    let tick = w.flow.tick;
    let life = num(&a.p, "LifeTime").unwrap_or(15.0);
    let facing = w.fighter(f).body.facing_dir;
    let mut n = Nepter {
        x,
        y,
        team: w.fighter(f).team.team,
        born: tick,
        scale_y: if facing < 0.0 { 1.0 } else { -1.0 },
        // A Delay made in the tick groups counts its frame in the leftover latent pass.
        bind_tick: tick + to_int32(latent_frames(0.2)) - 1,
        life_tick: tick + to_int32(latent_frames(life)) - 1,
        ..Nepter::default()
    };
    let sb = spawn_box(w);
    let force = flag(&a.p, "ForceSpawnPositionInsideBlastBox");
    let gone =
        blast_box_loop(&sb, &mut n.x, &mut n.y, None, force, &mut n.blast_bounds, flag(&a.p, "DestroyOnExitedArena?"));
    let actor = new_world_actor(w, cls, WorldActorClass::ItemHelperNepter, f, gone, WorldActorState::Nepter(n));
    let id = w.actors.actors.insert(actor);
    sound(w, id, name!("sfx_cha_bananaguard_spawn"));
    pie_check(w, id);
    id
}

/// The kind's `step`: the follow, PieCheck, then the Delays (the leftover latent pass).
fn step(w: &mut World, id: ActorId) {
    if st(w, id).ticking {
        tick(w, id);
    }
    pie_check(w, id);
    let now = w.flow.tick;
    if now >= st(w, id).life_tick {
        kill(w, id); // K2_DestroyActor @750
        return;
    }
    let n = st(w, id);
    if n.bind_tick >= 0 && now >= n.bind_tick {
        let r = cap_radius(w, owner(w, id));
        let n = st_mut(w, id);
        n.bind_tick = -1;
        n.ticking = true; // its first tick is the next frame's
        n.radius = r;
    }
}

/// The timers, behind its mesh's tick: CheckBlastBoxExit's, then ThrowPie's.
fn late(w: &mut World, id: ActorId) {
    let doe = flag(actor_p(w, ActorRef::Actor(id)), "DestroyOnExitedArena?");
    let now = w.flow.tick;
    let n = st_mut(w, id);
    if doe && crate::consts::clock(crate::real(now - n.born)) > n.blast_at {
        n.blast_at += f64::from(0.1_f32);
        if check_blast_box_exit(n.x, n.y, &n.blast_bounds) {
            kill(w, id); // DestroySpawnedActor(true)
            return;
        }
    }
    // The timer pass (unverified, see the head comment).
    let n = st(w, id);
    if n.throw_at >= 0 && now >= n.throw_at {
        let period = timer_period(w, id);
        st_mut(w, id).throw_at += period;
        throw_pie(w, id);
    }
}

/// The owner's broadcasts it binds at the Delay(0.2).
fn owner_signal(w: &mut World, id: ActorId, f: FighterId, sig: OwnerSignal, v: bool) {
    if !st(w, id).ticking {
        return; // bound at the Delay(0.2)
    }
    match sig {
        OwnerSignal::Facing => {
            let (x, y) = (st(w, id).x, st(w, id).y);
            let n = st_mut(w, id);
            n.scale_y = if v { 1.0 } else { -1.0 };
            n.facing_left = !v;
            // (on the owner's own event list, flushed with its tick)
            crate::fighter::control::emit(w, f, |fighter| Event::Rtpc {
                fighter,
                name: name!("cha_neptr_turn_pitch"),
                value: if v { 0.0 } else { 1.0 },
                interp_ms: 750.0,
            });
            crate::fighter::control::emit(w, f, |fighter| Event::Sound {
                fighter,
                ev: name!("sfx_cha_neptr_foley_tread_start"),
                x: Some(x),
                y: Some(y),
                hit: None,
                pooled: false,
                key: None,
            });
        }
        OwnerSignal::WallSliding => st_mut(w, id).wall_slide = v,
        OwnerSignal::Jumping => {
            let n = st_mut(w, id);
            n.is_jumping = v;
            if v {
                n.jump_count = 1 - n.jump_count;
            }
        }
    }
}

/// SpawnedActorParent's EndPlay (DestroyVFXBlocked false): DeathVFX and the 'Ak Death Location' sound.
fn end(w: &mut World, id: ActorId) {
    if reset_pass(w) {
        return;
    }
    let p: &Props = actor_p(w, ActorRef::Actor(id));
    let off = obj(p, "Death VFX Offset");
    let (oz, ox) = (off.and_then(|o| num(o, "Z")).unwrap_or(0.0), off.and_then(|o| num(o, "X")).unwrap_or(0.0));
    let vfx = text(p, "DeathVFX").filter(|s| !s.is_empty()).map(str::to_owned);
    let scale = obj(p, "DeathVFXScale").and_then(|s| num(s, "X")).unwrap_or(1.0);
    let ak = text(p, "Ak Death Location").filter(|s| !s.is_empty()).map(str::to_owned);
    let (x, y, depth) = (st(w, id).x, st(w, id).y, st(w, id).depth);
    let a = w.actors.actors.get(id).expect("a live actor");
    let (cls, fighter) = (a.cls, a.owner_id);
    if let Some(v) = vfx {
        let vfx = Some(w.lit(&v));
        w.events.push(Event::ProjectileDeath {
            cls,
            fighter,
            vfx,
            x,
            y: y + oz,
            ox: depth + ox,
            rot: [0.0, 0.0, 0.0],
            scale,
            tint: None,
            to: None,
        });
    }
    if let Some(s) = ak {
        let ev = w.lit(&s);
        w.events.push(Event::Sound { fighter, ev, x: Some(x), y: Some(y), hit: None, pooled: false, key: None });
    }
}

/// This file's registry entries (ItemHelper_Nepter_C, Projectile_Neptr_Pie_C).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let hooks = WorldActorHooks {
        deferred: Some(spawn_at),
        step: Some(step),
        late: Some(late),
        owner_signal: Some(owner_signal),
        end: Some(end),
        // Its Capsule is NoCollision and PieCheck overlaps Pawn alone: no trace or overlap of another meets it.
        bodies: Some(|_, _, _| {}),
        ..WorldActorHooks::default()
    };
    r.actors.enter(
        "ItemHelper_Nepter_C",
        ActorEntry {
            // The throw montage's proxy: every handler empty (@816-928).
            mesh: Some(MeshHooks { montage_event: Some(|_, _, _, _| {}), ..MeshHooks::default() }),
            ..ActorEntry::new(ActorClass::World(WorldActorClass::ItemHelperNepter), ActorHooks::World(hooks))
        },
    )?;
    r.actors.enter(
        "Projectile_Neptr_Pie_C",
        ActorEntry::new(
            ActorClass::Projectile(ProjectileClass::ProjectileNeptrPie),
            ActorHooks::Projectile(ProjectileHooks::default()),
        ),
    )
}
