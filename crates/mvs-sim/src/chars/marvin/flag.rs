//! Marvin's flag: C018Flag_C and C018AirFlag_C ([`C018Flag`], a projectile with no
//! ProjectileMovement that rides its floor; a hazard the Lab's clean removes), with the WallBP / DestructibleObjectBP
//! delegate OnStructureDestroyed (the wall blueprint: a no-op in the port). It spawns the homing mark
//! through homing.rs. No pawn hook and no pawn state.
//!
//! The class's graph, by entry (offsets of its ubergraph):
//! - ReceiveBeginPlay @8407 (the parent's first: NumberOfActorsAllowed 1 @5864-6515 in the kind's begin): (authority)
//!   AutoActivate -> ActivateObject @8461; else the Instigator's Comp_Pawn_DamageManager KnockedBack bound to
//!   OnOwnerKnockedBack @8570 -> @9411: !Activated -> DestroySpawnedActor.
//! - ActivateObject @2901: Activated = true @2911, GetChargeValuesFromInstigator @2922, the Instigator's
//!   TriggerAbilityServer(TriggerAbilityIndex) @3121; the Instigator a BP_FighterCharacter -> its
//!   CharacterMovement.K2_FindFloor(its root's location) @3386 and K2_AttachToComponent(the hit component, KeepWorld x3,
//!   weld) @3634 (the flag stays where it is and rides that component from then on); then the hit actor: a WallBP_C ->
//!   its StructureDestroyed += OnStructureDestroyed @4135; else a DestructibleObjectBP_C -> its ObjectDestroyed +=
//!   OnStructureDestroyed @4736; else its OnDestroyed += On Floor Destroyed @5165 (all three: DestroySpawnedActor
//!   @9538 / @9554). Then MulticastActivate @4176, HitBoxOn? = true and UpdateHitbox @4190-4201, Delay(HitBoxOnDuration)
//!   @4215 -> @15: HitBoxOn? = false, UpdateHitbox, PlayerOverlap.SetCollisionEnabled(QueryOnly) @40.
//! - MulticastActivate @5211: IsActiveCharging = false @5216, FlagMesh visible @5228,
//!   sfx_cha_C018_flag_impact_aura_start at the Capsule @5312, Delay(0.2) @5742 -> @79
//!   PF_Post_Event(sfx_cha_C018_flag_aura_pulse_loop, self) (stopped with the actor).
//! - ReceiveTick @1752: !IsMaxSize -> Scene scale += DeltaSeconds x (IsActiveCharging ? ChargingGrowthRate :
//!   PassiveGrowthRate) per axis, FMin MaxSize @1815-2273, IsMaxSize once the X is at MaxSize @2556; then (authority)
//!   QueuedFighters backwards @2598 -> @145-1013: a fighter no longer invulnerable -> the flag's own Comp_State_Manager
//!   SpawnBuffActor(Buff_C018HomingVictim_C, it, Instigator, 0, -1) @669, removed.
//! - PlayerOverlap's begin overlap @5797 (authority): a BP_FighterCharacter not in OverlappingEnemies of another
//!   TeamIndex @6142 -> AddUnique OverlappingEnemies @6234; not invulnerable -> its own Comp_State_Manager
//!   SpawnBuffActor(HomingVictim, it, Instigator, 0, -1) @6692; else AddUnique QueuedFighters @6777.
//! - Its end overlap @7847 (authority): no longer overlapping and in OverlappingEnemies -> removed from both
//!   @8037-8097, SpawnBuffActor(HomingVictim, it, Instigator, 0, Debuff Duration) @8318.
//! - ReceiveEndPlay @8612 (authority): the attach parent's StructureDestroyed / ObjectDestroyed / OnDestroyed and the
//!   Instigator's KnockedBack unbound.
//! - BPI Clean: Activated -> DestroySpawnedActor, Success.
//!
//! Model. A Projectile of the world's list (its kind's `construct`, as chars/garnet/marker.rs) whose movement never
//! runs: it stands where it was spawned, and after the activation rides its floor's move. The HitBox (radius 80 under
//! Scene) deals the class's HitBoxDamage while HitBoxOn? holds (hit radius 80 x the Capsule's scale x the Scene's);
//! PlayerOverlap has the same sphere. Its Delays count on the actor's age.
// Unverified: K2_FindFloor (a capsule sweep down 102.4 walking, 97.6 otherwise, a one-way platform the fighter ignores
// not found) is read as: a grounded instigator's floor is its base (the stage block without one); one in the air takes
// the highest floor, solid box, stage platform it does not ignore or spawned platform under its capsule's span whose
// top is within 97.6 under its feet; none -> no attach and no binding.
// Deviation: the ride is the floor's move of the frame (Platform.dx / dy) added in the flag's own step (behind the
// fighters' moves, ahead of the hit test); the build's weld moves the flag with its parent inside the parent's move.
// Deviation: PlayerOverlap's overlaps are tested once a step against the fighters' root capsules (`sphere_hits`), the
// Pawn channel; the fighters' HurtBox capsules it overlaps too are left out. A fighter waiting for its respawn keeps
// the state it had (no edge).
// Deviation: OwnerKnockedBack is read in the flag's step (the owner's knocked-back count moved), not inside the hit.
// Deviation: the Sphere component (HurtBox type, radius 80 under Scene) is not modelled: nothing hits the flag.
// Unverified: the flag's destroy is read as firing PlayerOverlap's end overlaps (DestroyComponent's
// OnComponentDestroyed runs ClearComponentOverlaps, Blueprint events included) into its still-bound graph @7847, after
// ReceiveEndPlay: each fighter it still overlaps and holds in OverlappingEnemies takes the mark's Debuff Duration tail
// @8318. It does not run in World.reset's pass over the spawned actors (a `!resetPass` test).

use indexmap::IndexSet;

use super::homing::spawn_homing_victim;
use crate::Real;
use crate::State;
use crate::actors::projectile::{MakeOpts, StepEnd, class_def, class_p, expiry, hitbox_active, set_hitbox};
use crate::actors::world_actor::{actor_dead, actor_p};
use crate::actors::{ActorClass, ActorRef, Projectile, ProjectileClass, ProjectileExt, ProjectileId};
use crate::combat::elem::{instigator_charge, receive_charge};
use crate::consts as C;
use crate::events::{Event, SoundKey};
use crate::fighter::body::{PlatformRef, ignores_platform, plat, span_near};
use crate::fighter::common::latent_frames;
use crate::fighter::control::trigger_ability;
use crate::fighter::match_side::SpawnRequest;
use crate::fighter::{FighterId, actor_y, cap_radius, capsule_half, grounded, invulnerable};
use crate::math::{min, to_int32};
use crate::name;
use crate::names::Name;
use crate::registry::entries::{ActorEntry, ActorHooks, HazardHooks, ProjectileHooks};
use crate::registry::props::{all_fighters, flag, num, obj};
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use crate::world::spawning::destroy_projectile;
use crate::world::wall_bp::{
    WallActor, WallDelegate, is_destructible_object, is_wall_bp, wall_bind, wall_delegate_fns, wall_unbind,
};

/// C018Flag's class.
pub const FLAG: &str = "C018Flag_C";

/// C018AirFlag's class.
pub const AIR_FLAG: &str = "C018AirFlag_C";

const IMPACT_SOUND: Name = name!("sfx_cha_C018_flag_impact_aura_start");
const LOOP_SOUND: Name = name!("sfx_cha_C018_flag_aura_pulse_loop");
/// HitBox / PlayerOverlap SphereRadius (the class template).
const SPHERE_RADIUS: Real = 80.0;
/// K2_FindFloor's sweep down off the ground.
const FIND_FLOOR_AIR: Real = 97.6;

/// `C018Flag extends Projectile` (C018Flag_C, C018AirFlag_C): what the view reads (`scene_scale`, `active_charging`,
/// `flag_visible`) and the class's own.
#[derive(State, Debug, Default)]
pub struct C018Flag {
    /// Scene RelativeScale3D.X (the growth; SpawnedVFX Size).
    pub scene_scale: Real,
    /// IsActiveCharging (true until MulticastActivate).
    pub active_charging: bool,
    /// FlagMesh visible (MulticastActivate @5228).
    pub flag_visible: bool,
    pub is_max_size: bool,
    /// Activated.
    pub activated: bool,
    /// PlayerOverlap's collision (NoCollision in the template, QueryOnly from @40).
    pub overlap_on: bool,
    /// OverlappingEnemies.
    pub enemies: Vec<FighterId>,
    /// QueuedFighters.
    pub queued: Vec<FighterId>,
    /// The fighters (ids) PlayerOverlap overlaps.
    pub over: IndexSet<i32>,
    /// The age Delay(HitBoxOnDuration) @4215 fires at, -1 none.
    pub hit_off_at: Real,
    /// The age Delay(0.2) @5742 fires at, -1 none.
    pub pulse_at: Real,
    /// The pulse loop posted.
    pub looping: bool,
    /// The owner's knocked-back count when the KnockedBack binding was made.
    pub kb_seq: i32,
    pub kb_bound: bool,
    /// The attach parent is set (`floor` is not null): the floor it rides, `floor` None being World.ground.
    pub has_floor: bool,
    pub floor: Option<PlatformRef>,
    /// The actor whose OnDestroyed it bound (a spawned platform's, a placed stage actor's that is no wall).
    pub floor_actor: Option<ActorRef>,
    /// `wall`: the placed wall (`ActorRef::Stage`) whose delegate OnStructureDestroyed is bound to.
    pub wall: Option<ActorRef>,
    /// `wallDelegate`: that delegate.
    pub wall_delegate: Option<WallDelegate>,
}

fn pr(w: &World, id: ProjectileId) -> &Projectile {
    w.actors.projectiles.get(id).expect("a live projectile")
}

fn pm(w: &mut World, id: ProjectileId) -> &mut Projectile {
    w.actors.projectiles.get_mut(id).expect("a live projectile")
}

fn fl(w: &World, id: ProjectileId) -> &C018Flag {
    match &pr(w, id).ext {
        ProjectileExt::C018Flag(s) => s,
        e => panic!("C018Flag_C without its state: {e:?}"),
    }
}

fn fl_w(w: &mut World, id: ProjectileId) -> &mut C018Flag {
    match &mut pm(w, id).ext {
        ProjectileExt::C018Flag(s) => s,
        e => panic!("C018Flag_C without its state: {e:?}"),
    }
}

fn owner(w: &World, id: ProjectileId) -> FighterId {
    pr(w, id).owner.expect("a flag has an owner")
}

fn pnum(w: &World, id: ProjectileId, k: &str) -> Real {
    num(actor_p(w, ActorRef::Projectile(id)), k).unwrap_or(0.0)
}

/// `flagRadius`: GetScaledSphereRadius of HitBox / PlayerOverlap: 80 x the smallest axis of the world scale (uniform
/// here: the Capsule's absolute scale x the Scene's).
fn flag_radius(p: &Projectile, scene_scale: Real) -> Real {
    (SPHERE_RADIUS * p.scale) * scene_scale
}

fn radius_of(w: &World, id: ProjectileId) -> Real {
    flag_radius(pr(w, id), fl(w, id).scene_scale)
}

/// `C018Flag.step`, the Projectile's step override: no movement; it rides its floor's move, then the timers
/// (`expiry`). Called by `projectile::step` for a flag, in place of the parent's.
pub fn step_override(w: &mut World, id: ProjectileId) -> Option<StepEnd> {
    if pr(w, id).dead {
        return None;
    }
    let ride = fl(w, id).floor.map(|r| plat(w, r)).filter(|p| !p.dead).map(|p| (p.dx, p.dy));
    let p = pm(w, id);
    p.prev_x = p.x;
    p.prev_y = p.y;
    p.bounced = false;
    if let Some((dx, dy)) = ride {
        p.x += dx;
        p.y += dy;
    }
    p.age += 1;
    expiry(w, id)
}

/// The constructor's body: the class's state, the Scene's scale, the hit radius.
fn construct(w: &World, p: &mut Projectile) {
    let scene = class_def(w, p)
        .and_then(|d| obj(&d.components, "Scene"))
        .and_then(|s| obj(s, "RelativeScale3D"))
        .and_then(|s| num(s, "X"))
        .unwrap_or(1.0);
    let charging = flag(class_p(w, p), "IsActiveCharging");
    let owner = p.owner.expect("a flag has an owner");
    p.hit_radius = flag_radius(p, scene);
    p.ext = ProjectileExt::C018Flag(C018Flag {
        scene_scale: scene,
        active_charging: charging,
        hit_off_at: -1.0,
        pulse_at: -1.0,
        kb_seq: w.fighter(owner).attackers.knocked_back_count,
        ..C018Flag::default()
    });
}

/// The C018Flag constructor's options: `{ ridden: true }`.
fn make_opts(_w: &World, _f: FighterId, _r: &SpawnRequest) -> MakeOpts {
    MakeOpts { ridden: true, ..MakeOpts::default() }
}

/// K2_FindFloor of the instigator's CMC from its root (see the header): `None` no hit, `Some(None)` World.ground.
fn find_floor(w: &World, f: FighterId) -> Option<Option<PlatformRef>> {
    if grounded(w, f) {
        return Some(w.fighter(f).platforms.base);
    }
    let feet = actor_y(w, f) - capsule_half(w, f);
    let r = cap_radius(w, f);
    let x = w.fighter(f).body.x;
    let mut best: Option<(Real, Option<PlatformRef>)> = None;
    let mut meet = |top: Real, left: Real, right: Real, id: Option<PlatformRef>| {
        if x <= left - r || x >= right + r {
            return;
        }
        if top > feet || feet - top > FIND_FLOOR_AIR {
            return;
        }
        if best.is_none_or(|(t, _)| top > t) {
            best = Some((top, id));
        }
    };
    for (g, id) in crate::fighter::body::floor_boxes(w, f) {
        meet(crate::math::narrow(g.top), crate::math::narrow(g.left), crate::math::narrow(g.right), id);
    }
    for &i in &w.fighter(f).platforms.stage_platforms {
        let r = PlatformRef::Stage(i);
        if ignores_platform(w, f, r) {
            continue;
        }
        let g = plat(w, r).ground();
        meet(crate::math::narrow(g.top), crate::math::narrow(g.left), crate::math::narrow(g.right), Some(r));
    }
    for &r_ in &w.fighter(f).platforms.platforms {
        let p = plat(w, r_);
        if p.dead || p.wall || p.stage_solid || ignores_platform(w, f, r_) {
            continue;
        }
        if span_near(w, &p, x, r).is_none() {
            continue;
        }
        if p.top > feet || feet - p.top > FIND_FLOOR_AIR {
            continue;
        }
        if best.is_none_or(|(t, _)| p.top > t) {
            best = Some((p.top, Some(r_)));
        }
    }
    best.map(|(_, id)| id)
}

/// ActivateObject's attach and bindings @3175-5206.
fn attach(w: &mut World, id: ProjectileId) {
    let f = owner(w, id);
    let Some(g) = find_floor(w, f) else { return }; // no hit: the attach and the bindings take None
    let s = fl_w(w, id);
    s.has_floor = true; // K2_AttachToComponent(HitComponent, KeepWorld, weld) @3634
    s.floor = g;
    let a = solid_owner(w, g);
    let (wall, d) = match a {
        Some(a) if is_wall_bp(w, a) => (a, WallDelegate::StructureDestroyed), // @4135
        Some(a) if is_destructible_object(w, a).is_some() => (a, WallDelegate::ObjectDestroyed), // @4736
        _ => {
            // OnDestroyed += On Floor Destroyed @5165: a spawned platform's actor, a placed actor (destroyed: `dead`);
            // the stage block and the stage's platforms are never destroyed.
            let act = a.or(match g {
                Some(PlatformRef::Spawned(a, _)) => Some(a),
                _ => None,
            });
            if act.is_some() {
                fl_w(w, id).floor_actor = act;
            }
            return;
        }
    };
    let s = fl_w(w, id);
    s.wall = Some(wall);
    s.wall_delegate = Some(d);
    let ActorRef::Stage(i) = wall else { unreachable!("a wall is a stage actor") };
    wall_bind(w, WallActor(i), d, ActorRef::Projectile(id), ON_STRUCTURE_DESTROYED);
}

/// `solidOwner`: the placed stage actor whose component a stage solid's box or polygon is (`StagePlacedData.solids`),
/// none for another floor.
fn solid_owner(w: &World, g: Option<PlatformRef>) -> Option<ActorRef> {
    let run = w.stage_run.as_ref()?;
    let id = match g? {
        PlatformRef::Solid(b) => run.solids[run.boxes[b as usize].solid as usize].id,
        PlatformRef::Poly(i) => run.solids[i as usize].id,
        _ => return None,
    };
    let placed = w.data.stage.placed.as_deref().unwrap_or(&[]);
    let i = run.actors.iter().position(|a| {
        placed.get(a.index as usize).and_then(|p| p.solids.as_deref()).is_some_and(|s| s.contains(&id))
    })?;
    Some(ActorRef::Stage(u32::try_from(i).expect("a list index")))
}

/// The dead flag of the attach parent whose OnDestroyed is bound: a placed stage actor's own, a spawned actor's.
fn floor_dead(w: &World, a: ActorRef) -> bool {
    match a {
        ActorRef::Stage(i) => w.stage_run.as_ref().is_some_and(|r| r.actors[i as usize].dead),
        _ => actor_dead(w, a),
    }
}

/// OnStructureDestroyed's bind name (`wall_delegate_fns`).
const ON_STRUCTURE_DESTROYED: &str = "C018Flag_C.OnStructureDestroyed";

/// OnStructureDestroyed @9554: DestroySpawnedActor(false).
fn on_structure_destroyed(w: &mut World, a: ActorRef, _wall: WallActor) {
    if let ActorRef::Projectile(id) = a
        && w.actors.projectiles.get(id).is_some_and(|p| !p.dead)
    {
        destroy_projectile(w, id, false);
    }
}

/// MulticastActivate @5211. `base`: the age its Delay counts from.
fn multicast_activate(w: &mut World, id: ProjectileId, base: Real) {
    let (fighter, x, y) = {
        let p = pr(w, id);
        (p.owner_id, p.x, p.y)
    };
    let s = fl_w(w, id);
    s.active_charging = false;
    s.flag_visible = true;
    // @5312
    w.events.push(Event::Sound {
        fighter,
        ev: IMPACT_SOUND,
        x: Some(x),
        y: Some(y),
        hit: None,
        pooled: false,
        key: None,
    });
    let s = fl_w(w, id);
    if s.pulse_at < 0.0 {
        s.pulse_at = base + latent_frames(0.2); // Delay(0.2) @5742
    }
}

/// ActivateObject @2901. `base`: the age its Delays count from.
fn activate_object(w: &mut World, id: ProjectileId, base: Real) {
    let f = owner(w, id);
    fl_w(w, id).activated = true;
    let c = instigator_charge(w, f);
    receive_charge(w, ActorRef::Projectile(id), c); // GetChargeValuesFromInstigator @2922
    trigger_ability(w, f, to_int32(pnum(w, id, "TriggerAbilityIndex"))); // @3121
    attach(w, id);
    multicast_activate(w, id, base); // @4176
    set_hitbox(pm(w, id), true); // HitBoxOn? = true, UpdateHitbox @4190-4201
    let dur = pnum(w, id, "HitBoxOnDuration");
    let s = fl_w(w, id);
    if s.hit_off_at < 0.0 {
        s.hit_off_at = base + latent_frames(dur); // Delay @4215
    }
}

/// PlayerOverlap's begin and end edges since the last test (see the header).
fn overlaps(w: &mut World, id: ProjectileId) {
    if !fl(w, id).overlap_on {
        return;
    }
    let r = radius_of(w, id);
    let inst = owner(w, id);
    let dur = pnum(w, id, "Debuff Duration");
    let team = w.fighter(inst).team.team;
    for v in all_fighters(w) {
        if w.fighter(v).life.dead > 0.0 {
            continue;
        }
        let (x, y) = (pr(w, id).x, pr(w, id).y);
        let on = crate::actors::tether_shape::sphere_hits(w, x, y, r, v);
        let vid = w.fighter(v).id;
        if on == fl(w, id).over.contains(&vid) {
            continue;
        }
        if on {
            fl_w(w, id).over.insert(vid);
            // @5797: not in OverlappingEnemies, another TeamIndex (the flag's: its owner's).
            if fl(w, id).enemies.contains(&v) || w.fighter(v).team.team == team {
                continue;
            }
            fl_w(w, id).enemies.push(v); // @6234
            if !invulnerable(w, v) {
                spawn_homing_victim(w, v, inst, -1.0, Some(v)); // @6692
            } else if !fl(w, id).queued.contains(&v) {
                fl_w(w, id).queued.push(v); // @6777
            }
            continue;
        }
        fl_w(w, id).over.shift_remove(&vid);
        // @7847: in OverlappingEnemies -> out of both lists, the mark's tail.
        let s = fl_w(w, id);
        let Some(i) = s.enemies.iter().position(|&e| e == v) else { continue };
        s.enemies.remove(i);
        if let Some(j) = s.queued.iter().position(|&e| e == v) {
            s.queued.remove(j);
        }
        spawn_homing_victim(w, v, inst, dur, Some(v)); // @8318
    }
}

/// PlayerOverlap's end overlaps at the flag's destroy (see the header): @7847's flow per fighter.
fn end_overlaps_at_destroy(w: &mut World, id: ProjectileId) {
    let dur = pnum(w, id, "Debuff Duration");
    let inst = owner(w, id);
    let over: Vec<i32> = fl(w, id).over.iter().copied().collect();
    for vid in over {
        let Some(v) = all_fighters(w).into_iter().find(|&o| w.fighter(o).id == vid) else { continue };
        if w.fighter(v).life.dead > 0.0 {
            continue;
        }
        let s = fl_w(w, id);
        let Some(i) = s.enemies.iter().position(|&e| e == v) else { continue };
        s.enemies.remove(i);
        if let Some(j) = s.queued.iter().position(|&e| e == v) {
            s.queued.remove(j);
        }
        spawn_homing_victim(w, v, inst, dur, Some(v)); // @8318
    }
    fl_w(w, id).over.clear();
}

/// ReceiveTick @1752.
fn tick(w: &mut World, id: ProjectileId) {
    if !fl(w, id).is_max_size {
        let charging = fl(w, id).active_charging;
        let rate = pnum(w, id, if charging { "ChargingGrowthRate" } else { "PassiveGrowthRate" });
        let max = pnum(w, id, "MaxSize");
        let s = fl_w(w, id);
        s.scene_scale = min(s.scene_scale + (C::DELTA_SECONDS * rate), max); // @1815-2273
        if s.scene_scale >= max {
            s.is_max_size = true; // @2556
        }
        let r = radius_of(w, id);
        pm(w, id).hit_radius = r;
    }
    // (authority) QueuedFighters backwards @2598.
    let inst = owner(w, id);
    let mut i = fl(w, id).queued.len();
    while i > 0 {
        i -= 1;
        let Some(&v) = fl(w, id).queued.get(i) else { continue };
        if invulnerable(w, v) {
            continue; // @483
        }
        spawn_homing_victim(w, v, inst, -1.0, None); // the flag's own Comp_State_Manager @669
        let s = fl_w(w, id);
        if let Some(j) = s.queued.iter().position(|&e| e == v) {
            s.queued.remove(j); // @812
        }
    }
}

fn unbind(w: &mut World, id: ProjectileId) {
    let s = fl_w(w, id);
    if let (Some(ActorRef::Stage(i)), Some(d)) = (s.wall, s.wall_delegate) {
        wall_unbind(w, WallActor(i), d, ActorRef::Projectile(id), ON_STRUCTURE_DESTROYED);
    }
    let s = fl_w(w, id);
    s.wall = None;
    s.wall_delegate = None;
    s.floor_actor = None;
    s.kb_bound = false;
}

/// `loopKey`: `'c018flag' + id`.
fn loop_key(w: &World, id: ProjectileId) -> SoundKey {
    SoundKey { tag: name!("c018flag"), n: pr(w, id).seq }
}

/// `kind.begin` (SpawnedActorParent ReceiveBeginPlay's NumberOfActorsAllowed came ahead: the world's parent begin).
fn begin(w: &mut World, id: ProjectileId) {
    // @8447 (authority).
    if flag(actor_p(w, ActorRef::Projectile(id)), "AutoActivate") {
        let age = crate::real(pr(w, id).age);
        activate_object(w, id, age);
        return;
    }
    let kb = w.fighter(owner(w, id)).attackers.knocked_back_count;
    let s = fl_w(w, id);
    s.kb_bound = true; // KnockedBack += OnOwnerKnockedBack @8570
    s.kb_seq = kb;
}

/// `kind.step`: `true` destroys the flag.
fn step(w: &mut World, id: ProjectileId) -> bool {
    let a = crate::real(pr(w, id).age);
    // On Floor Destroyed (a destroyed attach parent's OnDestroyed) @9538.
    if let Some(fa) = fl(w, id).floor_actor
        && floor_dead(w, fa)
    {
        return true;
    }
    // OnOwnerKnockedBack @9411 (see the header).
    let kb = w.fighter(owner(w, id)).attackers.knocked_back_count;
    if fl(w, id).kb_bound && kb != fl(w, id).kb_seq {
        let s = fl_w(w, id);
        s.kb_seq = kb;
        if !s.activated {
            return true; // DestroySpawnedActor(false) @9450
        }
    }
    overlaps(w, id);
    tick(w, id);
    // The latent actions.
    if a == fl(w, id).hit_off_at {
        let s = fl_w(w, id);
        s.hit_off_at = -1.0;
        s.overlap_on = true; // PlayerOverlap QueryOnly @40: its overlaps begin at once
        set_hitbox(pm(w, id), false); // HitBoxOn? = false, UpdateHitbox @15-26
        overlaps(w, id);
    }
    if a == fl(w, id).pulse_at {
        let fighter = pr(w, id).owner_id;
        let key = loop_key(w, id);
        let s = fl_w(w, id);
        s.pulse_at = -1.0;
        s.looping = true;
        w.events.push(Event::SoundStart { fighter, ev: LOOP_SOUND, key }); // @79
    }
    false
}

/// `kind.activate` (ActivateObject, an ActivateSpawned of the owner's: the fighter pass, ahead of the projectile pass).
fn activate(w: &mut World, id: ProjectileId) {
    let age = crate::real(pr(w, id).age);
    activate_object(w, id, age);
}

/// `kind.destroyed` (ReceiveEndPlay @8612; the world plays Ak Death Location, sfx_cha_C018_flag_aura_end, and the
/// death VFX).
fn destroyed(w: &mut World, id: ProjectileId) {
    unbind(w, id);
    if fl(w, id).looping {
        let fighter = pr(w, id).owner_id;
        let key = loop_key(w, id);
        w.events.push(Event::SoundStop { fighter, key, fade: Some(0.0), ev: None, curve: None, cut: false });
    }
    fl_w(w, id).looping = false;
    if !crate::world::flow::reset_pass(w) {
        end_overlaps_at_destroy(w, id); // the components' end overlaps (see the header)
    }
}

fn flag_id(a: ActorRef) -> Option<ProjectileId> {
    match a {
        ActorRef::Projectile(id) => Some(id),
        _ => None,
    }
}

/// `hazard.overlaps` (BPI_EnvironmentalHazard: C019ScrubShield's sphere meets the HitBox while it is on and
/// PlayerOverlap from @40: both overlap ProjectileDestructible).
fn hazard_overlaps(w: &World, a: ActorRef, x: Real, y: Real, r: Real) -> bool {
    let Some(id) = flag_id(a) else { return false };
    let Some(p) = w.actors.projectiles.get(id) else { return false };
    let ProjectileExt::C018Flag(s) = &p.ext else { return false };
    if !hitbox_active(p) && !s.overlap_on {
        return false;
    }
    let (dx, dy, rr) = (x - p.x, y - p.y, r + flag_radius(p, s.scene_scale));
    dx * dx + dy * dy <= rr * rr
}

/// `hazard.clean` (BPI Clean): DestroySpawnedActor(false), Success = true.
fn hazard_clean(w: &mut World, a: ActorRef) -> bool {
    let Some(id) = flag_id(a) else { return false };
    if !fl(w, id).activated {
        return false;
    }
    destroy_projectile(w, id, false);
    true
}

/// This file's registry entries: the two flags (foreignOwner, the hazard), the Air one with parents [C018Flag_C].
// (foreignOwner: the reads of `owner` are its team, knocked-back count, triggerAbility, its position, capsule, floors
// and platforms (`find_floor`), its spawned actors and data and the charge values: each a fighter's own, whoever spawned
// the flag; nothing reads Marvin's state.)
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let kind = ProjectileHooks {
        construct: Some(construct),
        make_opts: Some(make_opts),
        begin: Some(begin),
        step: Some(step),
        activate: Some(activate),
        destroyed: Some(destroyed),
        ..ProjectileHooks::default()
    };
    let hazard = HazardHooks { overlaps: hazard_overlaps, clean: hazard_clean };
    r.actors.enter(
        FLAG,
        ActorEntry {
            foreign_owner: true,
            hazard: Some(hazard),
            ..ActorEntry::new(ActorClass::Projectile(ProjectileClass::C018Flag), ActorHooks::Projectile(kind))
        },
    )?;
    r.actors.enter(
        AIR_FLAG,
        ActorEntry {
            parents: &[FLAG],
            foreign_owner: true,
            hazard: Some(hazard),
            ..ActorEntry::new(ActorClass::Projectile(ProjectileClass::C018AirFlag), ActorHooks::Projectile(kind))
        },
    )?;
    wall_delegate_fns(r, &[(ON_STRUCTURE_DESTROYED, on_structure_destroyed)])
}
