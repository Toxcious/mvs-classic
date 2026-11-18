//! The carry hooks of a spawned actor: what a buff's graph does to the
//! spawned actor it holds (its place, its velocity, SetActorEnableCollision, its MaxLifetimeTimer), through the actor's
//! `CarryHooks` (`registry::entries`): a class's own `carry` entry (HarleyBomb's), else the shared Projectile and Item
//! hooks here. Buff_GarnetStopProjectile and Buff_C015HitProjectile call [`carry_hooks`]; what a kind keeps across
//! the carry lives on their extensions ([`CarryExt`]).
//!
//! Buff_ProjectileThroughTunnel itself (the buff on an actor carried from one of Bugs Bunny's tunnels to another:
//! canTunnel, carryTeam, carryVelocity, setCarryVelocity, throughTunnel, startMove, carryTrail and its begin / tick /
//! end; its extension [`ThroughTunnelExt`]) is Bugs Bunny's port's: his tunnel is its only caller.

use crate::Real;
use crate::State;
use crate::actors::ActorRef;
use crate::combat::buff_class::BuffClass;
use crate::combat::elem::{ElemExt, elem_buff, elem_buff_mut, spawn_actor_buff};
use crate::events::Event;
use crate::fighter::FighterId;
use crate::fighter::common::actor_sound_from;
use crate::fighter::common::owner_team;
use crate::fighter::moves::V2;
use crate::fighter::traces::edges;
use crate::registry::entries::CarryHooks;
use crate::registry::entries::{ActorBuffHooks, BuffEntry, BuffHooks};
use crate::registry::{Registry, RegistryError};
use crate::world::World;

/// What a kind keeps across the carry on its buff's extension (`ElemBuff.ext`): the paused timer's ticks left (`life`)
/// and a projectile's HurtBox radius (`hurt`); `None`: the key absent.
#[derive(State, Copy, PartialEq, Debug, Default)]
pub struct CarryExt {
    /// The paused MaxLifetimeTimer's ticks left.
    pub life: Option<Real>,
    /// A projectile's HurtBox radius the collision-off keeps.
    pub hurt: Option<Real>,
}

/// Buff_ProjectileThroughTunnel's extension on the carried actor (`ElemBuff.ext`): the buff's own location (`bx`,
/// `by`), StartPosition (`sx`, `sy`), EndPosition (`ex`, `ey`), the VFX's lerp ends (`vsx`, `vsy` -> `vdx`, `vdy`;
/// `trail`: the VFX is valid), MoveTimeline's position and rate (`t`, `rate`; `play`: playing), `edge` (the rider
/// detached from its edge: WasEdgeAttached) and, in `carry`, what a kind keeps across the carry. `started`: StartMove
/// ran, so the keys it writes exist (the record writes them only then).
#[derive(State, Copy, PartialEq, Debug, Default)]
pub struct ThroughTunnelExt {
    pub bx: Real,
    pub by: Real,
    pub started: bool,
    pub rate: Real,
    pub sx: Real,
    pub sy: Real,
    pub ex: Real,
    pub ey: Real,
    pub trail: bool,
    pub vsx: Real,
    pub vsy: Real,
    pub vdx: Real,
    pub vdy: Real,
    pub t: Real,
    pub play: bool,
    pub edge: bool,
    pub carry: CarryExt,
}

/// The carry keys of the actor's buff `b` (a hook's index), `None` for a class whose extension has none.
fn carry_ext(w: &mut World, a: ActorRef, b: u32) -> Option<&mut CarryExt> {
    elem_buff_mut(w, a, b)?.ext.carry_mut()
}

fn projectile(w: &World, a: ActorRef) -> Option<&crate::actors::Projectile> {
    match a {
        ActorRef::Projectile(id) => w.actors.projectiles.get(id),
        _ => None,
    }
}

fn projectile_mut(w: &mut World, a: ActorRef) -> Option<&mut crate::actors::Projectile> {
    match a {
        ActorRef::Projectile(id) => w.actors.projectiles.get_mut(id),
        _ => None,
    }
}

fn item(w: &World, a: ActorRef) -> Option<&crate::actors::item::Item> {
    match a {
        ActorRef::Item(id) => w.actors.items.get(id),
        _ => None,
    }
}

fn item_mut(w: &mut World, a: ActorRef) -> Option<&mut crate::actors::item::Item> {
    match a {
        ActorRef::Item(id) => w.actors.items.get_mut(id),
        _ => None,
    }
}

const ZERO: V2 = V2 { x: 0.0, y: 0.0 };

// ---- ParentProjectilePhysics (every Projectile of the port)
//
// Its root (the Capsule) never simulates physics, so IsActive of the ProjectileMovement is the whole test of
// `moving`; GetVelocity is the root's ComponentVelocity, the movement's Velocity for a root it moves, so the select
// gives (vx, vy) either way.

fn p_moving(w: &World, a: ActorRef) -> bool {
    projectile(w, a).is_some_and(crate::actors::projectile::movement_active)
}

fn p_at(w: &World, a: ActorRef) -> V2 {
    projectile(w, a).map_or(ZERO, |p| V2 { x: p.x, y: p.y })
}

fn p_place(w: &mut World, a: ActorRef, x: Real, y: Real) {
    if let Some(p) = projectile_mut(w, a) {
        p.x = x;
        p.y = y;
    }
}

fn p_velocity(w: &World, a: ActorRef) -> V2 {
    projectile(w, a).map_or(ZERO, |p| V2 { x: p.vx, y: p.vy })
}

/// SetPhysicsLinearVelocity on a root that does not simulate: nothing.
fn p_set_velocity(w: &mut World, a: ActorRef, vx: Real, vy: Real) {
    if let Some(p) = projectile_mut(w, a)
        && crate::actors::projectile::movement_active(p)
    {
        p.vx = vx;
        p.vy = vy;
    }
}

/// Off: Projectile.no_collision (no hit box, no blocker) and the HurtBox's radius 0, kept on the buff; on: the radius
/// back. The class's own writes of the radius during the carry are kept by [`p_tick`].
fn p_collision(w: &mut World, a: ActorRef, b: u32, on: bool) {
    let kept = carry_ext(w, a, b).and_then(|x| x.hurt);
    let Some(p) = projectile_mut(w, a) else { return };
    if on {
        p.no_collision = false;
        p.hurt_radius = kept.unwrap_or(p.hurt_radius);
        return;
    }
    p.no_collision = true;
    let r = p.hurt_radius;
    p.hurt_radius = 0.0;
    if let Some(x) = carry_ext(w, a, b) {
        x.hurt = Some(r);
    }
}

fn p_pause_life(w: &mut World, a: ActorRef, b: u32, paused: bool) {
    if paused {
        let Some(p) = projectile_mut(w, a) else { return };
        let left = crate::actors::projectile::pause_max_lifetime(p);
        if let Some(x) = carry_ext(w, a, b) {
            x.life = Some(crate::real(left));
        }
        return;
    }
    let left = carry_ext(w, a, b).and_then(|x| x.life).unwrap_or(0.0);
    if let Some(p) = projectile_mut(w, a) {
        crate::actors::projectile::unpause_max_lifetime(p, crate::math::to_int32(left));
    }
}

/// A class that writes its HurtBox's radius in its move (C017_Plasma_Shot's grow): the write kept for the end and
/// the HurtBox off again, ahead of the frame's hit tests.
fn p_tick(w: &mut World, a: ActorRef, b: u32) {
    let Some(p) = projectile_mut(w, a) else { return };
    if p.hurt_radius == 0.0 {
        return;
    }
    let r = p.hurt_radius;
    p.hurt_radius = 0.0;
    if let Some(x) = carry_ext(w, a, b) {
        x.hurt = Some(r);
    }
}

/// `PROJECTILE`: the shared hooks of a Projectile (pub for a class whose own entry differs in a part, as the
/// Meeseeks' does).
pub const PROJECTILE: CarryHooks = CarryHooks {
    moving: p_moving,
    team: owner_team,
    at: p_at,
    place: p_place,
    velocity: p_velocity,
    set_velocity: p_set_velocity,
    collision: p_collision,
    pause_life: p_pause_life,
    tick: Some(p_tick),
};

// ---- ItemActorParent (every Item of the port, a SpawnedActorParent child)
//
// Its root Capsule simulates physics while it lies or falls out of a hand, its ProjectileMovement is active while it
// flies (Item simulating_physics / movement_active): either holds `moving`; in a hand neither (PickUp: Deactivate,
// SetSimulatePhysics(false)). TeamIndex: the Comp_Actor_Destructible's, the spawner's then the last grabber's
// (PickUp), which is the owner's. Velocity: the ProjectileMovement's while it flies; a deactivated one reads 0
// (UMovementComponent::Deactivate -> StopMovementImmediately; AActor::GetVelocity) and so does one a blocking hit
// stopped (StopSimulating), so the select falls to GetVelocity, the simulating body's velocity: (vx, vy) either way.
// Unverified: an item whose ProjectileMovement was never activated (spawned lying, never in a hand, never thrown) is
// taken to read 0 as well; the movement's Velocity after InitializeComponent of an inactive component is not read.
// MulticastProjectileVelocity: the active movement's Velocity, else SetPhysicsLinearVelocity on the simulating root.
// SetActorEnableCollision: Item.no_collision (no floor or platform contact in the move: it falls with gravity behind
// the carrier's placing; no Sphere, hit box or PickupArea). MaxLifetimeTimer: SpawnedActorParent's BeginPlay sets it
// only for a MaxLifetime > 0 (@342); no ported item has one (its LifeTimer, which no carry pauses, runs on). No tick
// hook (the tunnel buff's ReceiveTick acts on a ParentProjectilePhysics alone).
// Unverified: the root Capsule stays among the trace bodies of an Item-channel query while its collision is off
// (the traces read the item's holder, not no_collision), as a carried projectile's root does.

fn i_moving(w: &World, a: ActorRef) -> bool {
    item(w, a).is_some_and(|i| i.movement_active() || i.simulating_physics())
}

fn i_team(w: &World, a: ActorRef) -> i32 {
    item(w, a).map_or(-1, |i| i.destructible_team(w))
}

fn i_at(w: &World, a: ActorRef) -> V2 {
    item(w, a).map_or(ZERO, |i| V2 { x: i.x, y: i.y })
}

fn i_place(w: &mut World, a: ActorRef, x: Real, y: Real) {
    if let Some(i) = item_mut(w, a) {
        i.x = x;
        i.y = y;
    }
}

fn i_velocity(w: &World, a: ActorRef) -> V2 {
    item(w, a).map_or(ZERO, |i| V2 { x: i.vx, y: i.vy })
}

/// A root that does not simulate: nothing.
fn i_set_velocity(w: &mut World, a: ActorRef, vx: Real, vy: Real) {
    if let Some(i) = item_mut(w, a)
        && (i.movement_active() || i.simulating_physics())
    {
        i.vx = vx;
        i.vy = vy;
    }
}

fn i_collision(w: &mut World, a: ActorRef, _b: u32, on: bool) {
    if let Some(i) = item_mut(w, a) {
        i.no_collision = if on { None } else { Some(true) };
    }
}

fn i_pause_life(_w: &mut World, _a: ActorRef, _b: u32, _paused: bool) {}

/// `ITEM`: the shared hooks of an Item.
const ITEM: CarryHooks = CarryHooks {
    moving: i_moving,
    team: i_team,
    at: i_at,
    place: i_place,
    velocity: i_velocity,
    set_velocity: i_set_velocity,
    collision: i_collision,
    pause_life: i_pause_life,
    tick: None,
};

/// `carryHooks`: the actor's carry hooks (its class's `carry` entry, else the shared Projectile or Item hooks), `None`
/// (warned) for an actor of no such kind (a world actor class without an entry).
pub fn carry_hooks(w: &World, a: ActorRef) -> Option<CarryHooks> {
    if let Some(k) = crate::actors::world_actor::actor_entry(w, a).and_then(|e| e.carry) {
        return Some(k);
    }
    match a {
        ActorRef::Projectile(_) => Some(PROJECTILE),
        ActorRef::Item(_) => Some(ITEM),
        _ => {
            crate::actors::world_actor::warn(
                "Buff_ProjectileThroughTunnel_C: an actor with no carry hooks (registry CarryHooks): not carried",
            );
            None
        }
    }
}

// ---- Buff_ProjectileThroughTunnel
//
// The buff (a BuffParentActor child; a child of the actor's Comp_State_Manager: `combat::elem` spawn_actor_buff;
// duration 0.0 from the caller: no LifeTimer). Each kind of actor answers through its CarryHooks: the shared
// Projectile and Item hooks above, a class's own `carry` entry in place of them, and for a world actor only its
// entry. An actor of no such kind is refused (warned).
// ReceiveBeginPlay @60 (the parent's first): the owner cast to SpawnedActorParent (every actor here is one) @90;
// Set Visibility For Relevant Components(owner, false) @165 (every visible child of the root, recursively, hidden
// and kept: @2614-3444; the root is not among them); SetActorEnableCollision(false) @189; a valid
// Comp_RunAlongEdgeTrace with a valid Attached detaches from the edge, WasEdgeAttached = true @222-519
// (`fighter::traces::edges`); sfx_cha_bugs_tunnel_enter posted on the owner @530; authority:
// K2_PauseTimerHandle(owner.MaxLifetimeTimer) @633.
// StartMove(Start, Destination, DestinationOffset, Time) @1114 (reliable multicast): MoveTimeline play rate 1 / Time;
// StartPosition = the buff's own location (the owner's transform at the spawn: Comp_State_Manager SpawnBuffActor
// @660 / @933, nothing moves the buff after), not Start; EndPosition = Destination + DestinationOffset; the owner
// put at StartPosition (teleport) @1345; PS_BugsBunnyTunnel_Trail attached to the owner's root, kept as VFX @1469;
// MoveTimeline.PlayFromStart @1584.
// MoveTimeline (length 1, Alpha linear 0 -> 1) update @2261: the owner put at VLerp(StartPosition, EndPosition,
// Alpha) (teleport: its velocity is kept); a valid VFX at VLerp(Start, Destination, Alpha) (the passed ends, no
// offset). Finished @3631: WasEdgeAttached false -> K2_SetActorLocation(EndPosition) on the buff itself @2216; true
// and the owner's Comp_RunAlongEdgeTrace valid -> its Attach To Edge(its Clockwise, EndPosition, its Initial
// Velocity, its Local Offset Position, its Local Offset Clockwise Degrees) @3830 (the buff stays where it is); then
// authority K2_DestroyActor @45.
// ReceiveTick @3583 -> @1993: authority, an owner that is a ParentProjectilePhysics with its StaticMesh visible ->
// MulticastVisibility(false): the visible children hidden again (kept), then VFX.SetVisibility(true) @3548.
// ReceiveEndPlay @676: a SpawnedActorParent owner: the buff put at EndPosition @790, SetActorEnableCollision(true)
// @830, the kept children shown and the list cleared @863 -> @1617, a valid VFX destroyed @930,
// sfx_cha_bugs_tunnel_projectile_exit at EndPosition @963, authority K2_UnPauseTimerHandle(owner.MaxLifetimeTimer)
// @1071. Nothing gives back a collision that was off before, and nothing stops the actor's own movement: an
// active ProjectileMovement and a simulating body go on through the carry with the velocity the tunnel wrote, the
// timeline putting the actor in place each frame behind them, and fly on from EndPosition with what they have then.
//
// Deviation: "hidden while the buff lives" stands for the kept list: a child the class's own graph shows during the
// carry is hidden here too (the build hides it again only on a ParentProjectilePhysics whose StaticMesh is
// visible), and a child hidden at the begin and shown during the carry comes back shown either way.
// MoveTimeline: PlayFromStart's update at 0 inside the call; PlayFromStart turns the timeline's tick on inside a
// tick group, so it ticks in the StartMove frame too (a tick function added inside a group ticks that frame). The
// tunnel's call sits in the actor's own move (ProjectileOverlap's begin overlap), ahead of the actor's buff pass
// (`actor_buff_tick` behind the move), where the first advance falls: a caller outside that pass calls in it, not
// after it. Position + DeltaSeconds x rate in float32; finished once the position is past the length (> 1, as the
// item's DeathBlinkWarning).
// Unverified: the order of the buff's TimelineComponent tick against the actor's own movement tick in later frames
// is not read (attachment does not tie them: the buff is not attached).
// Unverified: the warning for a class with a Comp_RunAlongEdgeTrace and no rider entry is not ported (the port's
// warnings are not state).

/// `THROUGH_TUNNEL`.
pub const THROUGH_TUNNEL: &str = "Buff_ProjectileThroughTunnel_C";

/// The carry's extension of the actor's buff `b`.
fn tt(w: &World, a: ActorRef, b: u32) -> Option<ThroughTunnelExt> {
    match elem_buff(w, a, b)?.ext {
        ElemExt::ThroughTunnel(e) => Some(e),
        _ => None,
    }
}

fn tt_mut(w: &mut World, a: ActorRef, b: u32) -> Option<&mut ThroughTunnelExt> {
    match &mut elem_buff_mut(w, a, b)?.ext {
        ElemExt::ThroughTunnel(e) => Some(e),
        _ => None,
    }
}

/// `canTunnel`: the tunnel's tests on the actor, ahead of its team test (@10861-11196): a SpawnedActorParent whose
/// root simulates physics or whose ProjectileMovement is active, not tagged IgnorePortal (the class default's Tags).
/// False for an actor without carry hooks.
pub fn can_tunnel(w: &World, a: ActorRef) -> bool {
    let Some(k) = carry_hooks(w, a) else { return false };
    if !(k.moving)(w, a) {
        return false;
    }
    let tags = crate::actors::world_actor::actor_p(w, a).get("Tags").and_then(|v| v.as_arr());
    !tags.is_some_and(|t| t.iter().any(|x| x.as_str() == Some("IgnorePortal")))
}

/// `carryTeam`.
pub fn carry_team(w: &World, a: ActorRef) -> i32 {
    carry_hooks(w, a).map_or(-1, |k| (k.team)(w, a))
}

/// `carryVelocity`: the velocity the tunnel reads (zero for an actor without carry hooks).
pub fn carry_velocity(w: &World, a: ActorRef) -> V2 {
    carry_hooks(w, a).map_or(ZERO, |k| (k.velocity)(w, a))
}

/// `setCarryVelocity`.
pub fn set_carry_velocity(w: &mut World, a: ActorRef, vx: Real, vy: Real) {
    if let Some(k) = carry_hooks(w, a) {
        (k.set_velocity)(w, a, vx, vy);
    }
}

/// `throughTunnel`: SpawnBuffActor(Buff_ProjectileThroughTunnel_C, a, by, 0, 0.0, BreakIfBlocked true) and, on the
/// buff it made, StartMove(start, dest, offset, time) (offset: (0, 0, 400) from the tunnel, sim (0, 400)). The buff's
/// serial, or `None` when the spawn was refused (no carry hooks, the class defaults missing, a blocker on the actor).
pub fn through_tunnel(
    w: &mut World,
    a: ActorRef,
    by: FighterId,
    start: V2,
    dest: V2,
    offset: V2,
    time: Real,
) -> Option<u32> {
    carry_hooks(w, a)?;
    let cls = crate::name!("Buff_ProjectileThroughTunnel_C");
    let s = spawn_actor_buff(w, a, cls, 0, 0.0, true, Some(by))?;
    // spawn_actor_buff gives the serial; start_move takes the entry's place in the list, as the hooks do.
    if let Some(i) = crate::combat::elem::buff_place(w, a, s) {
        start_move(w, a, i, start, dest, offset, time);
    }
    Some(s)
}

/// `startMove` @1114 (the caller's `start` is the VFX's alone); `b` the buff's place in the actor's list.
pub fn start_move(w: &mut World, a: ActorRef, b: u32, start: V2, dest: V2, offset: V2, time: Real) {
    let Some(k) = carry_hooks(w, a) else { return };
    let Some(x) = tt_mut(w, a, b) else { return };
    let t = time;
    x.rate = if t == 0.0 { 0.0 } else { 1.0 / t }; // Divide_FloatFloat by 0 returns 0
    x.sx = x.bx;
    x.sy = x.by;
    x.ex = dest.x + offset.x;
    x.ey = dest.y + offset.y;
    x.started = true;
    let (sx, sy) = (x.sx, x.sy);
    (k.place)(w, a, sx, sy);
    if let Some(x) = tt_mut(w, a, b) {
        x.trail = true;
        x.vsx = start.x;
        x.vsy = start.y;
        x.vdx = dest.x;
        x.vdy = dest.y;
        x.t = 0.0;
        x.play = true;
    }
    update(w, &k, a, b);
}

/// MoveTimeline's update @2261 at its position (Alpha = the position: a linear 0 -> 1 key pair over the length 1).
// Unverified: VLerp(A, B, t) = A + t (B - A), each part in float32.
fn update(w: &mut World, k: &CarryHooks, a: ActorRef, b: u32) {
    let Some(x) = tt(w, a, b) else { return };
    let (al, sx, sy) = (x.t, x.sx, x.sy);
    (k.place)(w, a, sx + (al * (x.ex - sx)), sy + (al * (x.ey - sy)));
}

/// `carryTrail`: the VFX's place (VLerp(Start, Destination, Alpha), the trail's), `None` without one: for the drawing.
pub fn carry_trail(w: &World, a: ActorRef) -> Option<V2> {
    let e = crate::actors::world_actor::actor_elem(w, a)?.as_ref()?;
    let b = e.buffs.iter().find(|o| o.class == BuffClass::BuffProjectileThroughTunnel)?;
    let ElemExt::ThroughTunnel(x) = b.ext else { return None };
    if !x.trail {
        return None;
    }
    Some(V2 { x: x.vsx + x.t * (x.vdx - x.vsx), y: x.vsy + x.t * (x.vdy - x.vsy) })
}

/// `carryHidden`: true while a carry hides the actor's components (all but its root).
pub fn carry_hidden(w: &World, a: ActorRef) -> bool {
    crate::actors::world_actor::actor_elem(w, a)
        .and_then(|e| e.as_ref())
        .is_some_and(|e| e.buffs.iter().any(|o| o.class == BuffClass::BuffProjectileThroughTunnel))
}

/// ReceiveBeginPlay. True: refused (an actor the port cannot carry, as a failed cast would leave it).
// Unverified: the refusal of an actor with no carry hooks.
fn begin(w: &mut World, a: ActorRef, b: u32, _from: Option<FighterId>) -> bool {
    let Some(k) = carry_hooks(w, a) else { return true };
    let at = (k.at)(w, a);
    if let Some(e) = elem_buff_mut(w, a, b) {
        e.ext = ElemExt::ThroughTunnel(ThroughTunnelExt { bx: at.x, by: at.y, ..ThroughTunnelExt::default() });
    }
    (k.collision)(w, a, b, false);
    // @222-519: a valid Comp_RunAlongEdgeTrace with a valid Attached: Detach From Edge, WasEdgeAttached (`edge`)
    if edges::rider_attached(w, a) == Some(true) {
        edges::detach_rider(w, a);
        if let Some(x) = tt_mut(w, a, b) {
            x.edge = true;
        }
    }
    let from = actor_sound_from(w, a);
    w.events.push(Event::Sound {
        fighter: from.fighter,
        ev: crate::name!("sfx_cha_bugs_tunnel_enter"),
        x: Some(at.x),
        y: Some(at.y),
        hit: None,
        pooled: from.pooled,
        key: None,
    });
    (k.pause_life)(w, a, b, true);
    false
}

/// ReceiveTick: the kind's tick, then the timeline. True: finished (the buff destroys itself).
fn tick(w: &mut World, a: ActorRef, b: u32) -> bool {
    let Some(k) = carry_hooks(w, a) else { return false };
    if let Some(t) = k.tick {
        t(w, a, b);
    }
    let Some(x) = tt(w, a, b) else { return false };
    if !x.play {
        return false;
    }
    let t = x.t + (crate::consts::DELTA_SECONDS * x.rate);
    let done = t > 1.0;
    if let Some(x) = tt_mut(w, a, b) {
        x.t = if done { 1.0 } else { t };
    }
    update(w, &k, a, b);
    if !done {
        return false;
    }
    // Finished: the buff itself to EndPosition, or the rider attached again at EndPosition @3631-3999, then
    // K2_DestroyActor (its EndPlay below).
    let Some(x) = tt_mut(w, a, b) else { return false };
    x.play = false;
    let (edge, ex, ey) = (x.edge, x.ex, x.ey);
    if edge && edges::rider_attached(w, a).is_some() {
        edges::attach_rider_at(w, a, f64::from(ex), f64::from(ey));
        return true;
    }
    if let Some(x) = tt_mut(w, a, b) {
        x.bx = ex;
        x.by = ey;
    }
    true
}

/// ReceiveEndPlay.
fn end(w: &mut World, a: ActorRef, b: u32) {
    let Some(k) = carry_hooks(w, a) else { return };
    // EndPosition: (0, 0, 0) when StartMove never ran (the keys then default to 0)
    let Some(x) = tt_mut(w, a, b) else { return };
    x.bx = x.ex;
    x.by = x.ey;
    let (bx, by) = (x.bx, x.by);
    (k.collision)(w, a, b, true);
    if let Some(x) = tt_mut(w, a, b) {
        x.trail = false;
    }
    let from = actor_sound_from(w, a);
    w.events.push(Event::Sound {
        fighter: from.fighter,
        ev: crate::name!("sfx_cha_bugs_tunnel_projectile_exit"),
        x: Some(bx),
        y: Some(by),
        hit: None,
        pooled: from.pooled,
        key: None,
    });
    (k.pause_life)(w, a, b, false);
}

/// This file's registry entry: Buff_ProjectileThroughTunnel_C, on an actor only (on a fighter no caller does that: the
/// owner cast fails in BeginPlay, ReceiveTick and EndPlay alike, so the buff does nothing there).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let actor = ActorBuffHooks { begin: Some(begin), tick: Some(tick), end: Some(end), ..ActorBuffHooks::default() };
    r.buffs.enter(
        THROUGH_TUNNEL,
        BuffEntry {
            actor: Some(actor),
            ..BuffEntry::new(BuffClass::BuffProjectileThroughTunnel, BuffHooks::default())
        },
    )
}
