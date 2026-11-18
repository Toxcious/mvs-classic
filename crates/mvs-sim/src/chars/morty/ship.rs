//! The ship: C019 ship, an `actors::lists::Ship` of the World's typed lists and a Platform
//! (its pieces, their sensors and collision; `world::spawned_platform` reads it as a platform).
//!
//! C019_Ship_C (Morty's up special; C019_Ship, a SpawnedPlatformParent). Sim space: x = -UE Y, y = UE Z.
//! ReceiveBeginPlay sets the actor's rotation to yaw -90 whatever the owner faces, so a component's local X is sim +x.
//! AnimNotify_C019_Ship_Spawner Received_NotifyBegin picks the spawn point: a line trace on trace type 5
//! (ProjectileGroundNoPlatform: the stage block, not platforms) from the owner's location + (0, 0, 50) to its
//! location + the notify's translation + (0, 0, -450); a hit gives the impact point + (0, 0, 450), no hit the
//! location + the translation; with an ally below the owner within 400 to either side (the lowest one), Z is at
//! least that ally's; then Y is clamped to the first BlastBoxPlayerExit's corners pulled in by 100 and Z to
//! [bottom + 450, top - 100]. The offset from the owner replaces the notify's translation in CompSpawnActor, which
//! mirrors its Y by the facing and starts from the owner's location - (0, 0, 100) (socket None). The trace and the
//! clamp use the unmirrored translation, so they are made on the UE +Y side whatever the facing.
//! The actor then rises with its ProjectileMovement (the notify's direction x InitialSpeed, no gravity), which moves
//! a root without collision.
//! ReceiveBeginPlay: sfx_cha_C019_portal_cast; Delay(0.25) -> sfx_cha_C019_spaceship_appear at the actor and
//! sfx_cha_C019_spaceship_loop; Delay(TimeTillDestroy - AudioDelayTime - 2) -> sfx_cha_C019_spaceship_death at the
//! mesh; Delay(TimeTillDestroy - AudioDelayTime), then Delay(AudioDelayTime) -> sfx_cha_C019_spaceship_loop_stop and
//! K2_DestroyActor. TimeTillDestroy there is SelectFloat(SociableEaglePerkTimeTillDestroy, TimeTillDestroy, the
//! instigator's BPI_CheckIfHasBuff(BuffParentActor_C)) (ubergraph 863-1301, 2137-2338). CheckHasBuff
//! (Comp_State_Manager) walks the owner's Comp_Actor_Parent.Children and IsBuffOfClass accepts the class or any child
//! of it, so the base class asks "has any buff actor" (`world::reset::has_buff_actor`).
//! SpawnedActorParent's BeginPlay (ForceSpawnPositionInsideBlastBox, true here; ubergraph 9347-10808, 14195-15029):
//! for a blast box whose BPI_CheckIfPositionOverlaps (inclusive bounds) fails for the actor's location, the location
//! is clamped to the box's bounds and set without a sweep. The mirrored offset can leave the box the notify clamped
//! into only next to a side edge.
//! The pieces block the fighters' capsules from every side (profile Platform against Pawn, both Block; the class
//! overrides BPI_IgnorePlatformCollision with nothing, so no fighter is ever told to ignore them): the fighter side is
//! the movement's, through `world::spawned_platform`'s platform view.
//! The other actors meet the pieces through [`ship_boxes`] (`World::blockers`): see `BlockKind`.
//! BPI_OnTeleport (collision off for 0.2 s) has one caller, C020 PortalParent's teleport, and that drops actors tagged
//! IgnorePortal, which the ship is: it never runs.
//! The sensors (DisableCollisionCheck*: profile Custom, the shape default's object type WorldDynamic, Overlap for
//! WorldStatic and Ignore for the rest) meet a body of type WorldStatic that answers WorldDynamic and generates
//! overlap events: the stage block (profile Floor; a Box, or on Sky Arena (1 vs 1) the floor mesh, whose
//! bGenerateOverlapEvents the map sets). A fighter's FullBody sphere (WorldStatic, Ignore for WorldDynamic), the
//! maps' BlockPortalCollision box (object type Portal) and the stage's platforms (Platform) are not met.
//! Their handlers count per sensor and switch the piece at once (Increment / Decrement Collision), so the state after
//! any update is the overlap state at the transform of that update: BeginPlay's K2_SetActorRotation(yaw -90) turns a
//! ship spawned facing left around before its tick, and the root's move by the ProjectileMovement updates the
//! attached sensors' overlaps (6: UpdateOverlapsImpl walks the children).
// Unverified: the first overlaps are taken to be reported inside the spawn (the stock UpdateInitialOverlaps after
// BeginPlay, not read in the exe; facing left the rotation's own update comes first); the blast box of the clamp is
// taken as the port's one (the first BlastBoxPlayerExit); the lifetime with a buff actor (7.5 s) is from bytecode and
// data, not captured.

use crate::Real;
use crate::actors::ActorRef;
use crate::actors::ShipId;
use crate::actors::lists::{Ship, ShipPiece, ShipSensor};
use crate::consts::{DELTA_SECONDS, SOLID_NOPLATFORM};
use crate::fighter::body::Span;
use crate::fighter::common::{actor_components, actor_data, latent_frames};
use crate::fighter::match_side::SpawnRequest;
use crate::fighter::traces::solids::QUERY_UNKNOWN;
use crate::fighter::{FighterId, actor_y, facing};
use crate::math::{max, min, to_int32};
use crate::name;
use crate::names::Name;
use crate::registry::props::{num, obj};
use crate::world::World;
use crate::world::blast::{force_spawn_inside, spawn_box};
use crate::world::spawned_platform::{BlockKind, blocks, can_step_up_on};
use crate::world::stage_query::{Shapes, static_shapes};
use mvs_collision::Ground;
use mvs_data::value::{Props, Value};

/// Sensor -> piece (`SENSOR_OF`), from the class's ComponentDelegateBindings and the ubergraph's handlers (the event
/// names are older than the component names). CollisionFarFarRight's sensor (DisableCollisionCheckFarRight) has
/// empty handlers.
const SENSOR_OF: [(&str, &str); 6] = [
    ("CollisionFarFarLeft", "DisableCollisionCheckFarFarLeft"),
    ("CollisionFarLeft", "DisableCollisionCheckFarLeft"),
    ("CollisionMidLeft", "DisableCollisionCheckMidLeft"),
    ("Collision", "DisableCollisionCheckMid"),
    ("CollisionFarRight", "DisableCollisionCheckMidRight"),
    ("CollisionFarFarRight", ""),
];

/// A ship tick's ran-out Delays (`ShipEvent`).
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum ShipEvent {
    Appear,
    DeathSound,
    Destroy,
}

fn piece_name(s: &str) -> Name {
    match s {
        "CollisionFarFarLeft" => name!("CollisionFarFarLeft"),
        "CollisionFarLeft" => name!("CollisionFarLeft"),
        "CollisionMidLeft" => name!("CollisionMidLeft"),
        "Collision" => name!("Collision"),
        "CollisionFarRight" => name!("CollisionFarRight"),
        _ => name!("CollisionFarFarRight"),
    }
}

/// `new Ship(f, a, r.p, staticGrounds(SOLID_NOPLATFORM), w.blast, spawnBox(w), matchFighters(w),
/// w.hasBuffActor(f), staticGrounds(QUERY_UNKNOWN))`.
pub fn new_ship(w: &mut World, f: FighterId, r: &SpawnRequest) -> Ship {
    let notify = crate::world::spawning::request_props(w, f, r);
    // the World's arguments, in their order
    let ground = static_shapes(w, SOLID_NOPLATFORM, name!("C019_Ship spawn trace"));
    let blast = w.stage.blast;
    let spawn = spawn_box(w);
    let fighters = crate::world::pawns::match_fighters(w);
    let has_buff = crate::world::reset::has_buff_actor(w, f);
    let sensed = static_shapes(w, QUERY_UNKNOWN, name!("C019_Ship sensors"));
    let data = w.data.clone();
    let def = actor_data(w, &data, ActorRef::Fighter(f), r.cls).expect("the ship's class data");
    let (p, comps) = (&def.p, &def.components);
    let (fx, fy, fc, team) = (w.fighter(f).body.x, actor_y(w, f), facing(w, f), w.fighter(f).team.team);
    let t = obj(&notify, "Transforms").and_then(|t| obj(t, "Translation"));
    let tx = -t.and_then(|t| num(t, "Y")).unwrap_or(0.0);
    let ty = t.and_then(|t| num(t, "Z")).unwrap_or(0.0);
    // The trace: slab test of the segment against the stage block (a polygon solid: its own line trace).
    let (ax, ay) = (fx, fy + 50.0);
    let (bx, by) = (fx + tx, fy + ty - 450.0);
    let (mut cx, mut cy) = (fx + tx, fy + ty);
    let mut hit: Option<Real> = None;
    for g in &ground.boxes {
        if let Some(t) = segment_box(ax, ay, bx, by, g)
            && hit.is_none_or(|h| t < h)
        {
            hit = Some(t);
        }
    }
    for q in &ground.polys {
        if let Some(t) = q.ray(ax, ay, bx, by).map(|h| crate::math::narrow(h.time))
            && hit.is_none_or(|h| t < h)
        {
            hit = Some(t);
        }
    }
    if let Some(h) = hit {
        cx = ax + (bx - ax) * h;
        cy = ay + (by - ay) * h + 450.0;
    }
    let mut ally = 1000000.0;
    let mut use_ally = false;
    for &o in &fighters {
        let of = w.fighter(o);
        if o == f || of.team.team != team || of.life.dead != 0.0 {
            continue;
        }
        let oy = actor_y(w, o);
        if oy < ally && fy > oy && of.body.x >= fx - 400.0 && of.body.x <= fx + 400.0 {
            ally = oy;
            use_ally = true;
        }
    }
    if use_ally {
        cy = max(cy, ally);
    }
    cx = min(max(cx, blast.left + 100.0), blast.right - 100.0);
    cy = min(max(cy, blast.bottom + 450.0), blast.top - 100.0);
    // CompSpawnActor: the offset's Y mirrored by the facing (sim: kept facing left, turned around facing right).
    let mut x = fx - (cx - fx) * fc;
    let mut y = fy - 100.0 + (cy - fy);
    // ForceSpawnPositionInsideBlastBox: outside the jump-out area, clamped into it (@10738 / @14195-14989).
    force_spawn_inside(&spawn, &mut x, &mut y);
    let d = obj(&notify, "InitialMovementDirection");
    let speed = min(num(&notify, "InitialSpeed").unwrap_or(0.0), num(&notify, "MaxSpeed").unwrap_or(Real::INFINITY));
    let vx = d.and_then(|d| num(d, "Y")).unwrap_or(0.0) * fc * speed;
    let vy = d.and_then(|d| num(d, "Z")).unwrap_or(0.0) * speed;
    let mut pieces = Vec::new();
    for (name, sensor) in SENSOR_OF {
        let Some(c) = obj(comps, name) else { continue };
        let l = obj(c, "RelativeLocation");
        let e = obj(c, "BoxExtent");
        let g = |o: Option<&Props>, k: &str, d: Real| o.and_then(|o| num(o, k)).unwrap_or(d);
        let s = if sensor.is_empty() { None } else { obj(comps, sensor) };
        let sensor = s.map(|s| {
            let (sl, se) = (obj(s, "RelativeLocation"), obj(s, "BoxExtent"));
            ShipSensor { dx: g(sl, "X", 0.0), dy: g(sl, "Z", 0.0), hw: g(se, "X", 32.0), hh: g(se, "Z", 32.0) }
        });
        pieces.push(ShipPiece {
            name: piece_name(name),
            dx: g(l, "X", 0.0),
            dy: g(l, "Z", 0.0),
            hw: g(e, "X", 32.0),
            hh: g(e, "Z", 32.0),
            sensor,
            count: 0,
            on: true,
            trace: blocks(c, BlockKind::Trace),
            nav: blocks(c, BlockKind::Nav),
            item: blocks(c, BlockKind::Item),
        });
    }
    // `sort((a, b) => a.dx - b.dx)`: stable, NaN as equal.
    pieces.sort_by(|a, b| a.dx.partial_cmp(&b.dx).unwrap_or(std::cmp::Ordering::Equal));
    let mesh_dy =
        obj(comps, "Ship_Skelmesh").and_then(|m| obj(m, "RelativeLocation")).and_then(|l| num(l, "Z")).unwrap_or(0.0);
    let till = num(p, if has_buff { "SociableEaglePerkTimeTillDestroy" } else { "TimeTillDestroy" });
    let life = till.unwrap_or(0.0) - num(p, "AudioDelayTime").unwrap_or(0.0);
    let mut sh = Ship {
        cls: r.cls,
        seq: -1,
        x,
        y,
        vx,
        vy,
        dx: 0.0,
        dy: 0.0,
        age: 0,
        dead: false,
        pieces,
        mesh_dy,
        team,
        appear_tick: to_int32(latent_frames(0.25)),
        death_sound_tick: to_int32(latent_frames(life - 2.0)),
        stop_tick: to_int32(latent_frames(life)),
        destroy_tick: -1,
        edge_at: None,
        owner: f,
    };
    sense(&mut sh, sensed.view());
    sh
}

fn ship(w: &World, sh: ShipId) -> &Ship {
    w.actors.ships.get(sh).expect("a live ship")
}

/// `top`: the pieces' top face (all six sit at one height).
pub fn top(x: &Ship) -> Real {
    x.pieces.first().map_or(x.y, |pc| x.y + pc.dy + pc.hh)
}

/// `bottom`.
pub fn bottom(x: &Ship) -> Real {
    x.pieces.first().map_or(x.y, |pc| x.y + pc.dy - pc.hh)
}

/// `canPassThrough(team)`: the parent's CanPassThrough(TeamIndex) (`world::spawned_platform::can_pass_through`; the
/// ship: allies true, enemies false), read by BaseFighter's jump press. The pieces stay solid all the same
/// (BPI_IgnorePlatformCollision is empty here): an ally's down + jump on the ship does nothing but what that press
/// does. The team is taken as the owner's at the spawn.
pub fn can_pass_through(w: &World, x: &Ship, team: i32) -> bool {
    let p = crate::fighter::common::actor_def(w, ActorRef::Fighter(x.owner), x.cls).map(|d| &d.p);
    p.is_some_and(|p| crate::world::spawned_platform::can_pass_through(p, crate::real(x.team), team))
}

/// `spans`: the runs of pieces with collision on, left to right. Neighbours stand 12 or 24 apart; a capsule cannot
/// enter such a gap and the walking floor test is a flat-base sweep (bUseFlatBaseForFloorChecks), so a run is one
/// floor.
fn spans(x: &Ship) -> Vec<Span> {
    let mut out: Vec<Span> = Vec::new();
    let mut last: Option<usize> = None;
    for (i, pc) in x.pieces.iter().enumerate() {
        if !pc.on {
            continue;
        }
        let (left, right) = (x.x + pc.dx - pc.hw, x.x + pc.dx + pc.hw);
        match out.last_mut() {
            Some(s) if last.is_some_and(|l| l + 1 == i) => s.right = right,
            _ => out.push(Span { left, right }),
        }
        last = Some(i);
    }
    out
}

/// `spanNear`.
pub fn span_near(x: &Ship, px: Real, r: Real) -> Option<Span> {
    spans(x).into_iter().find(|s| px > s.left - r && px < s.right + r)
}

/// `spanAt`.
pub fn span_at(x: &Ship, px: Real) -> Option<Span> {
    spans(x).into_iter().find(|s| px >= s.left && px <= s.right)
}

/// `clampX`.
pub fn clamp_x(x: &Ship, px: Real) -> Real {
    let (mut best, mut d) = (px, Real::INFINITY);
    for s in spans(x) {
        let c = max(s.left, min(s.right, px));
        if (c - px).abs() < d {
            d = (c - px).abs();
            best = c;
        }
    }
    best
}

/// The class's component template `name` (`actorDef(owner, cls).components[name]`).
fn template<'a>(w: &'a World, x: &Ship, name: Name) -> Option<&'a Props> {
    actor_components(w, ActorRef::Fighter(x.owner), x.cls)?.get(w.names.str(name)).and_then(Value::as_obj)
}

/// `canStepUpOn`: whether a walker may step up onto the pieces with collision on within r of x (from each piece's
/// template: the Collision* boxes leave CanCharacterStepUpOn at the default, Yes).
pub fn ship_can_step_up_on(w: &World, x: &Ship, px: Real, r: Real) -> bool {
    for pc in &x.pieces {
        if !pc.on || px <= x.x + pc.dx - pc.hw - r || px >= x.x + pc.dx + pc.hw + r {
            continue;
        }
        if !can_step_up_on(w, template(w, x, pc.name), x.cls) {
            return false;
        }
    }
    true
}

/// `Ship.boxes(kind, out)`: the ship's pieces with their collision on that answer Block to a query of the kind,
/// pushed to `out`. The gaps between neighbours are kept (a 0.5 sphere or a line passes a 12 / 24 gap). `Detect` and
/// `Dynamic` are read from the piece's template, not kept on the piece.
pub fn ship_boxes(w: &World, sh: ShipId, kind: BlockKind, out: &mut Vec<Ground>) {
    let x = ship(w, sh);
    for pc in &x.pieces {
        let k = match kind {
            BlockKind::Trace => pc.trace,
            BlockKind::Nav => pc.nav,
            BlockKind::Item => pc.item,
            BlockKind::Detect | BlockKind::Dynamic => template(w, x, pc.name).is_some_and(|c| blocks(c, kind)),
        };
        if !pc.on || !k {
            continue;
        }
        let (cx, cy) = (x.x + pc.dx, x.y + pc.dy);
        out.push(Ground {
            left: f64::from(cx - pc.hw),
            right: f64::from(cx + pc.hw),
            top: f64::from(cy + pc.hh),
            bottom: f64::from(cy - pc.hh),
        });
    }
}

/// The ship's pieces with their collision on that `take` keeps (the piece and its template) as a simulated body's
/// statics, pushed to `out`. A piece is a QueryAndPhysics BoxComponent of a Movable actor with bSimulatePhysics off:
/// a kinematic PxRigidDynamic (CreateShapesAndActors 142c6eb60, CreateActor 142c9cfc0, SetIsKinematic 142cae100),
/// which the ProjectileMovement moves by a kinematic target (TickComponent 1429a0e00 passes ETeleportType::None;
/// SetBodyTransform 142c7cba0 -> SetKinematicTarget 142cd0200). Each box is at its place after this tick's move
/// (`step` runs ahead of the bodies) with that move as its kinematic one: the body's step meets it at the pose the
/// step starts from, moving at the ship's velocity.
// Unverified: a ship spawned this frame (its first tick in the list pass) is met where it spawned with no move; the
// pieces' rows come after the stage platforms' in the solver's order.
pub fn ship_statics(
    w: &World,
    sh: ShipId,
    mut take: impl FnMut(&ShipPiece, Option<&Props>) -> bool,
    out: &mut Vec<mvs_collision::rigid_sphere::Static>,
) {
    let x = ship(w, sh);
    if x.dead {
        return;
    }
    let kinematic = Some((f64::from(x.dx), f64::from(x.dy)));
    for pc in &x.pieces {
        if !pc.on || !take(pc, template(w, x, pc.name)) {
            continue;
        }
        let (cx, cy) = (x.x + pc.dx, x.y + pc.dy);
        let g = Ground {
            left: f64::from(cx - pc.hw),
            right: f64::from(cx + pc.hw),
            top: f64::from(cy + pc.hh),
            bottom: f64::from(cy - pc.hh),
        };
        out.push(mvs_collision::rigid_sphere::Static { g, kinematic, static_solid: false, poly: None });
    }
}

/// `sense`: the sensors against the stage block (WorldStatic) and the stage solids of its profile (`gs`:
/// `static_grounds(QUERY_UNKNOWN)`, with the polygon solids, met by the shape traces' overlap).
// Unverified: a solid's bGenerateOverlapEvents is not in the data, taken as the block's.
fn sense(x: &mut Ship, gs: Shapes) {
    let (sx, sy) = (x.x, x.y);
    for pc in &mut x.pieces {
        let Some(s) = pc.sensor else { continue };
        let (cx, cy) = (sx + s.dx, sy + s.dy);
        let over = gs.boxes.iter().any(|g| {
            cx - s.hw < crate::math::narrow(g.right)
                && cx + s.hw > crate::math::narrow(g.left)
                && cy - s.hh < crate::math::narrow(g.top)
                && cy + s.hh > crate::math::narrow(g.bottom)
        }) || gs.polys.iter().any(|q| q.box_overlaps(cx, cy, s.hw, s.hh));
        pc.count = i32::from(over);
        pc.on = !over;
    }
}

/// `Ship.step(g)`: one actor tick: the movement component, then the latent Delays; the Delays that ran out, in
/// order, pushed to `out`.
pub fn step(w: &mut World, sh: ShipId, g: Shapes, out: &mut Vec<ShipEvent>) {
    let x = ship(w, sh);
    let audio = crate::fighter::common::actor_def(w, ActorRef::Fighter(x.owner), x.cls)
        .and_then(|d| num(&d.p, "AudioDelayTime"))
        .unwrap_or(0.0);
    let x = w.actors.ships.get_mut(sh).expect("a live ship");
    x.age += 1;
    x.dx = x.vx * DELTA_SECONDS;
    x.dy = x.vy * DELTA_SECONDS;
    x.x += x.dx;
    x.y += x.dy;
    sense(x, g);
    let age = x.age;
    if age == x.stop_tick {
        x.destroy_tick = age + to_int32(latent_frames(audio));
    }
    if age == x.appear_tick {
        out.push(ShipEvent::Appear);
    }
    if age == x.death_sound_tick {
        out.push(ShipEvent::DeathSound);
    }
    if age == x.destroy_tick {
        out.push(ShipEvent::Destroy);
    }
}

/// `segmentBox`: the entry time (0..1) of the segment a -> b into the box, or `None`. A start inside the box hits at
/// 0.
fn segment_box(ax: Real, ay: Real, bx: Real, by: Real, g: &Ground) -> Option<Real> {
    let (mut t0, mut t1) = (0.0, 1.0);
    let (dx, dy) = (bx - ax, by - ay);
    for (p, d, lo, hi) in [(ax, dx, g.left, g.right), (ay, dy, g.bottom, g.top)] {
        if d == 0.0 {
            if p < crate::math::narrow(lo) || p > crate::math::narrow(hi) {
                return None;
            }
            continue;
        }
        let (mut a, mut b) = ((lo - f64::from(p)) / f64::from(d), (hi - f64::from(p)) / f64::from(d));
        if a > b {
            std::mem::swap(&mut a, &mut b);
        }
        if a > t0 {
            t0 = a;
        }
        if b < t1 {
            t1 = b;
        }
        if t0 > t1 {
            return None;
        }
    }
    Some(crate::math::narrow(t0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn piece(dx: Real, on: bool) -> ShipPiece {
        ShipPiece { dx, dy: 10.0, hw: 50.0, hh: 5.0, on, ..ShipPiece::default() }
    }

    fn ship_of(pieces: Vec<ShipPiece>) -> Ship {
        Ship {
            cls: Name::NONE,
            seq: 0,
            x: 1000.0,
            y: 0.0,
            vx: 0.0,
            vy: 0.0,
            dx: 0.0,
            dy: 0.0,
            age: 0,
            dead: false,
            pieces,
            mesh_dy: 0.0,
            team: 0,
            appear_tick: 0,
            death_sound_tick: 0,
            stop_tick: 0,
            destroy_tick: -1,
            edge_at: None,
            owner: FighterId::new(0, 0),
        }
    }

    #[test]
    fn spans_join_neighbouring_pieces_and_split_at_one_off() {
        let s = ship_of(vec![piece(-212.0, true), piece(-100.0, true), piece(12.0, false), piece(124.0, true)]);
        let v = spans(&s);
        assert_eq!(v.len(), 2);
        assert_eq!((v[0].left, v[0].right), (1000.0 - 262.0, 1000.0 - 50.0));
        assert_eq!((v[1].left, v[1].right), (1074.0, 1174.0));
        assert_eq!(span_at(&s, 1000.0), None);
        assert_eq!(span_at(&s, 1100.0).map(|s| s.left), Some(1074.0));
        assert_eq!(span_near(&s, 1050.0, 30.0).map(|s| s.left), Some(1074.0));
        assert_eq!(span_near(&s, 1000.0, 30.0), None);
        assert_eq!(clamp_x(&s, 1000.0), 950.0);
        assert_eq!((top(&s), bottom(&s)), (15.0, 5.0));
    }

    #[test]
    fn sense_turns_a_piece_off_over_the_block() {
        let mut p = piece(0.0, true);
        p.sensor = Some(ShipSensor { dx: 0.0, dy: -50.0, hw: 50.0, hh: 10.0 });
        let mut s = ship_of(vec![p, piece(200.0, true)]);
        let g = Ground { left: 900.0, right: 1100.0, bottom: -500.0, top: -45.0 };
        sense(&mut s, Shapes::boxes(&[g]));
        assert!(!s.pieces[0].on && s.pieces[0].count == 1);
        assert!(s.pieces[1].on);
        s.y = 20.0;
        sense(&mut s, Shapes::boxes(&[g]));
        assert!(s.pieces[0].on && s.pieces[0].count == 0);
    }

    #[test]
    fn segment_box_gives_the_entry_time() {
        let g = Ground { left: -10.0, right: 10.0, bottom: -100.0, top: 0.0 };
        assert_eq!(segment_box(0.0, 50.0, 0.0, -50.0, &g), Some(0.5));
        assert_eq!(segment_box(0.0, -50.0, 0.0, -150.0, &g), Some(0.0));
        assert_eq!(segment_box(20.0, 50.0, 20.0, -50.0, &g), None);
    }
}
