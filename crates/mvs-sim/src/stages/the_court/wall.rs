//! The Court's two side walls (m010_wall_C m010_wall_2 / m010_wall2_5). Offsets are
//! the class's ubergraph's unless named. A direct Actor child (no WallBP, no stages): three boxes on a nonprimitive
//! DefaultSceneRoot, WallCollision (Floor, the solid), WallHitCollision (DamageDestructible: the hurt box) and
//! WallTouchCollision (WorldStatic, Pawn overlap: the touch effect alone).
//! - BeginPlay @1374: amb_map_M010_emit_barrier on the Ak component (the drawing's), M010MaxWallHP = M010WallHP (30),
//!   WallStartLocation = the root, ConnectedBarrierWall attached (the drawing's).
//! - ReceiveAnyDamage @182 -> DamageM010Wall: HP - Damage (float32, no clamp, no gate), DamageVFXMulticast((max - HP) /
//!   max) @1582 (material Damage, PS_M010_WallImpact at root + (0, 0, 1000): `damage` / `hit_at`), then HP <= 0:
//!   DestroyWallMulticast @1909; then @205 sfx_map_M010_barrier_damaged on the actor.
//! - DestroyWallMulticast @1909: the three boxes NoCollision, DissolveBarrierWall @271 (BarrierDissolveTimeline from the
//!   start, sfx_map_M010_barrier_destroyed at the root, Comp_EdgeTrace.OnDestroyed: edges false for good), the floor
//!   glow hidden, K2_SetTimerDelegate(RespawnWall, WallRespawnTime 5, once) @2116: a running one is cleared and started
//!   again (`stage_set_timer`), so the wall comes back 5 s after the LAST fatal hit.
//! - RespawnWall @2910 -> @2346: K2_TeleportTo(root + (0, +2000, 0) left / (0, -2000, 0) right) from where the root is
//!   now, WallEndLocation = the root, HP = max, Damage / Dissolve 0, the three boxes QueryAndPhysics, WallMove from the
//!   start @72.
//! - WallMove (1 s, linear 0 -> 1): each update K2_SetActorLocation(VLerp(WallEndLocation, WallStartLocation, alpha),
//!   sweep) @522; the root is a scene component, so the boxes go to their places unswept; Finished @563 shows the floor
//!   glow @15. Collision is on all the way back. Neither a respawn nor a break stops a running timeline.
//! - BarrierDissolveTimeline: the material Dissolve 0 -> 1 over 2 s (keys 0 / 2 s), its length unstored: 5.0.
//! - WallTouchCollision begin overlap @569: PS_M010_Barrier_Touch and sfx_map_M010_barrier_touch at
//!   SweepResult.ImpactPoint, no actor filter (`touch_at` / `touch_x` / `touch_y`).
//!
//! Nothing resets: HP, the broken state and the removed edges carry on to the level's end.

use super::ids::WALL_EV;
use crate::Real;
use crate::State;
use crate::actors::{ActorRef, StageActorClass};
use crate::combat::boxes::{CH_DAMAGE, WorldSphere};
use crate::consts::DELTA_SECONDS;
use crate::events::{Event, SoundKey};
use crate::fighter::traces::edges::edges_removed;
use crate::math::{max, min};
use crate::name;
use crate::names::Name;
use crate::registry::entries::StageActorEntry;
use crate::registry::props::{all_fighters, num};
use crate::world::World;
use crate::world::stage::{StagePiece, stage_set_timer, stage_solid_enable, stage_teleport};
use crate::world::stage_actors::{StageActorOwn, StageHurt};

const EV_RESPAWN: i32 = WALL_EV; // RespawnWall (the K2_SetTimerDelegate's delegate)
const RESPAWN_KEY: i32 = 2910; // the delegate's entry, @2910

const MOVE_LEN: Real = 1.0; // WallMove: TL_LastKeyFrame, keys (0, 0) (1, 1)
const DISSOLVE_LEN: Real = 5.0; // BarrierDissolveTimeline: TL_TimelineLength with none stored: 5.0
const DISSOLVE_END: Real = 2.0; // its Dissolve keys (0, 0) (2, 1)
const TELEPORT: Real = 2000.0; // RespawnWall's offset, UE Y

const SFX_DAMAGED: Name = name!("sfx_map_M010_barrier_damaged");
const SFX_DESTROYED: Name = name!("sfx_map_M010_barrier_destroyed");
const SFX_TOUCH: Name = name!("sfx_map_M010_barrier_touch");

/// `M010Wall`: an m010_wall_C in the match: its sim state and what the drawing reads (the root is the actor's x / y).
#[derive(State, Debug, Default)]
pub struct M010Wall {
    /// Its WallCollision's StageData.solids index (-1 when the stage data has none).
    pub floor: i32,
    /// IsLeftWall (m010_wall2_5, UE +Y).
    pub left: bool,
    /// M010WallHP (float32; negative after a fatal hit).
    pub hp: Real,
    /// M010MaxWallHP (BeginPlay's copy).
    pub max_hp: Real,
    /// The root's UE Y now (float32: only Y moves).
    pub uy: Real,
    /// WallEndLocation.Y (the last RespawnWall's teleported root; the start until then).
    pub end_y: Real,
    /// The three boxes' collision (QueryAndPhysics / NoCollision).
    pub on: bool,
    /// Comp_EdgeTrace.Enabled: false once On Destroyed has run.
    pub edges: bool,
    /// ConnectedFloorGlow shown.
    pub glow: bool,
    /// The barrier material's Damage ((max - HP) / max at a hit, 0 at a respawn).
    pub damage: Real,
    /// Its Dissolve (the dissolve timeline's value, 0 at a respawn).
    pub dissolve: Real,
    /// The world tick of the last DamageVFXMulticast (PS_M010_WallImpact), -1.
    pub hit_at: i32,
    /// Of the last DestroyWallMulticast, -1.
    pub broken_at: i32,
    /// Of the last RespawnWall, -1.
    pub respawn_at: i32,
    /// Of the last touch (PS_M010_Barrier_Touch), -1.
    pub touch_at: i32,
    /// Its ImpactPoint, sim.
    pub touch_x: Real,
    pub touch_y: Real,
    /// The timelines: position, playing, the first world tick they update in.
    pub move_pos: Real,
    pub move_on: bool,
    pub move_from: i32,
    pub dissolve_pos: Real,
    pub dissolve_on: bool,
    pub dissolve_from: i32,
    /// The fighters (ids) WallTouchCollision overlaps, as its overlap events left them.
    pub inside: Vec<i32>,
}

/// The static part, read from the stage data (`WallGeo`).
struct Geo {
    start_y: Real,
    solids: Vec<i32>,
    hurt_x: Real,
    hurt: Option<WorldSphere>,
    /// WallTouchCollision at the start, sim: left, right, bottom, top.
    touch: Option<[Real; 4]>,
    respawn: Real,
}

fn geo(w: &World, index: u32) -> Geo {
    let sd = &w.data.stage;
    let d = &sd.placed.as_ref().expect("placed actors")[index as usize];
    let all = sd.solids.as_deref().unwrap_or(&[]);
    let ids: Vec<i32> = d.solids.clone().unwrap_or_default();
    let part = |name: &str| {
        let suffix = format!(".{name}");
        ids.iter()
            .copied()
            .find(|&i| usize::try_from(i).ok().and_then(|i| all.get(i)).is_some_and(|s| s.name.ends_with(&suffix)))
    };
    let mut hurt_x = 0.0;
    let mut hurt = None;
    if let Some(id) = part("WallHitCollision") {
        let s = &all[usize::try_from(id).expect("a solid index")];
        let (c, e) = (&s.center, &s.half_extent);
        hurt_x = -c[1];
        // (the right wall's yaw -179.99995 maps the box on itself to .002 cm: taken on the world's axes)
        hurt = Some(WorldSphere {
            x: crate::math::narrow(hurt_x),
            y: crate::math::narrow(c[2]),
            z: Some(crate::math::narrow(c[0])),
            r: 0.0,
            bx: Some([
                crate::math::narrow(e[1]),
                0.0,
                0.0,
                0.0,
                crate::math::narrow(e[2]),
                0.0,
                0.0,
                0.0,
                crate::math::narrow(e[0]),
            ]),
            ..WorldSphere::default()
        });
    }
    let touch = part("WallTouchCollision").map(|id| {
        let s = &all[usize::try_from(id).expect("a solid index")];
        let (c, e) = (&s.center, &s.half_extent);
        [-c[1] - e[1], -c[1] + e[1], c[2] - e[2], c[2] + e[2]].map(crate::math::narrow)
    });
    Geo {
        start_y: crate::math::narrow(d.loc[1]),
        solids: ids,
        hurt_x: crate::math::narrow(hurt_x),
        hurt,
        touch,
        respawn: num(&d.p, "WallRespawnTime").unwrap_or(0.0),
    }
}

/// The `StageActorEntry` of m010_wall_C (`wallKind`).
pub fn entry() -> StageActorEntry {
    StageActorEntry {
        class: StageActorClass::M010Wall,
        hurt_type: Some(i32::from(CH_DAMAGE)),
        friendly_fire: false,
        can_take_damage: true,
        hit_pause_multiplier: None,
        // Unverified: no PhysMaterialOverride on WallHitCollision (m010_wall): the light hit sound's SurfaceType 6.
        surface: Some(6),
        create: Some(create),
        any_damage: Some(any_damage),
        hit_by_enemy: None,
        hit_by_ally: None,
        knocked_back: None,
        hurt: Some(hurt),
        sounds: &[],
        damage_disabled: None,
        invulnerable: None,
        current_damage: None,
        victim_pause: None,
        mesh: None,
        // The Lab reset (TrainGameMode destroys the pawns, not the level's actors): the old pawns leave the overlap.
        reset: Some(|w, slot| wall_mut(w, slot).inside.clear()),
        solid_hit: None,
    }
}

fn run_actor(w: &World, slot: u32) -> &crate::world::stage_actors::StageActor {
    &w.stage_run.as_ref().expect("a stage state").actors[slot as usize]
}

fn run_actor_mut(w: &mut World, slot: u32) -> &mut crate::world::stage_actors::StageActor {
    &mut w.stage_run.as_mut().expect("a stage state").actors[slot as usize]
}

/// The wall of a slot.
pub fn wall(w: &World, slot: u32) -> &M010Wall {
    match &run_actor(w, slot).own {
        StageActorOwn::M010Wall(m) => m,
        _ => unreachable!("m010_wall_C holds an M010Wall"),
    }
}

fn wall_mut(w: &mut World, slot: u32) -> &mut M010Wall {
    match &mut run_actor_mut(w, slot).own {
        StageActorOwn::M010Wall(m) => m,
        _ => unreachable!("m010_wall_C holds an M010Wall"),
    }
}

fn create(w: &mut World, slot: u32) {
    let index = run_actor(w, slot).index;
    let (floor, hp, y, left) = {
        let sd = &w.data.stage;
        let d = &sd.placed.as_ref().expect("placed actors")[index as usize];
        let all = sd.solids.as_deref().unwrap_or(&[]);
        let floor = d
            .solids
            .iter()
            .flatten()
            .copied()
            .find(|&i| {
                usize::try_from(i).ok().and_then(|i| all.get(i)).is_some_and(|s| s.name.ends_with(".WallCollision"))
            })
            .unwrap_or(-1);
        let hp = num(&d.p, "M010WallHP").unwrap_or(0.0);
        let left = d.p.get("IsLeftWall").is_some_and(|v| v.as_bool() == Some(true));
        (floor, hp, crate::math::narrow(d.loc[1]), left)
    };
    run_actor_mut(w, slot).x = -y;
    let m = M010Wall {
        floor,
        left,
        hp,
        max_hp: hp,
        uy: y,
        end_y: y,
        on: true,
        edges: true,
        glow: true,
        hit_at: -1,
        broken_at: -1,
        respawn_at: -1,
        touch_at: -1,
        ..M010Wall::default()
    };
    run_actor_mut(w, slot).own = StageActorOwn::M010Wall(m);
}

/// The root to UE Y `y`: the actor and its three boxes (unswept: K2_TeleportTo, and K2_SetActorLocation on a scene root).
/// A fighter standing on a box follows its move in his next CMC tick (`stage_teleport`).
/// Deviation: a fighter the returning wall's WallCollision lands on is left to the box code's depenetration; the
/// build's overlap by the returning wall is not read.
fn move_root(w: &mut World, slot: u32, y: Real) {
    let g = geo(w, run_actor(w, slot).index);
    wall_mut(w, slot).uy = y;
    run_actor_mut(w, slot).x = -y;
    let dx = -(y - g.start_y);
    let n = w.stage_run.as_ref().map_or(0, |r| r.solids.len());
    for id in g.solids {
        if let Ok(i) = usize::try_from(id)
            && i < n
        {
            stage_teleport(w, StagePiece::Solid(u32::try_from(i).expect("a solid index")), dx, 0.0);
        }
    }
}

fn set_collision(w: &mut World, slot: u32, on: bool) {
    let g = geo(w, run_actor(w, slot).index);
    wall_mut(w, slot).on = on;
    let n = w.stage_run.as_ref().map_or(0, |r| r.solids.len());
    for id in g.solids {
        if let Ok(i) = usize::try_from(id)
            && i < n
        {
            stage_solid_enable(w, u32::try_from(i).expect("a solid index"), on);
        }
    }
    // (SetCollisionEnabled(NoCollision) ends the box's overlaps: no end-overlap handler is bound; a fighter still
    // inside when it comes on begins one again)
    if !on {
        wall_mut(w, slot).inside.clear();
    }
}

/// WallMove's update (its TimelinePostUpdateFunc).
fn move_update(w: &mut World, slot: u32) {
    let g = geo(w, run_actor(w, slot).index);
    let m = wall(w, slot);
    let alpha = if m.move_pos >= MOVE_LEN { 1.0 } else { m.move_pos / MOVE_LEN };
    // VLerp(WallEndLocation, WallStartLocation, alpha): A + alpha (B - A), float32
    let y = m.end_y + (alpha * (g.start_y - m.end_y));
    move_root(w, slot, y);
}

fn dissolve_update(w: &mut World, slot: u32) {
    let m = wall_mut(w, slot);
    m.dissolve = if m.dissolve_pos >= DISSOLVE_END { 1.0 } else { m.dissolve_pos / DISSOLVE_END };
}

/// PlayFromStart: SetPlaybackPosition(0) with its update at once (4.26 source, as the port's other timelines), then
/// playing from the timelines' next tick. `from`: the first world tick it ticks in.
/// Unverified: the timeline's first tick is taken from the 4.26 source, not read in the exe.
fn play_move(w: &mut World, slot: u32, from: i32) {
    wall_mut(w, slot).move_pos = 0.0;
    move_update(w, slot);
    let m = wall_mut(w, slot);
    m.move_on = true;
    m.move_from = from;
}

fn play_dissolve(w: &mut World, slot: u32, from: i32) {
    wall_mut(w, slot).dissolve_pos = 0.0;
    dissolve_update(w, slot);
    let m = wall_mut(w, slot);
    m.dissolve_on = true;
    m.dissolve_from = from;
}

/// The wall actor's own game object (PF_Post_Event's target): `m010wall` + its placed index.
fn ak_key(w: &World, slot: u32) -> Option<SoundKey> {
    Some(SoundKey { tag: name!("m010wall"), n: i32::try_from(run_actor(w, slot).index).expect("a placed index") })
}

/// DamageM010Wall (function @0-250), then the damage sound @205.
fn any_damage(w: &mut World, slot: u32, dmg: Real, _causer: ActorRef) {
    let tick = w.flow.tick;
    let m = wall_mut(w, slot);
    m.hp -= dmg;
    // DamageVFXMulticast((max - HP) / max) @1582-1843
    m.damage = (m.max_hp - m.hp) / m.max_hp;
    m.hit_at = tick;
    let dead = m.hp <= 0.0;
    if dead {
        wall_destroy(w, slot);
    }
    // PF_Post_Event(sfx_map_M010_barrier_damaged, self): the actor's own game object at its root
    let (x, y) = (run_actor(w, slot).x, run_actor(w, slot).y);
    let key = ak_key(w, slot);
    w.events.push(Event::Sound { fighter: -1, ev: SFX_DAMAGED, x: Some(x), y: Some(y), hit: None, pooled: false, key });
}

/// DestroyWallMulticast @1909-2173 (a repeated call runs it all again).
fn wall_destroy(w: &mut World, slot: u32) {
    set_collision(w, slot, false);
    // DissolveBarrierWall @271: the timeline from the start. A Play inside the tick groups (a hit) is ticked in the same
    // frame (the port's timelines tick in the leftover pass: `tick`); one from later in the frame waits a frame.
    // Deviation: the timelines tick in TG_PrePhysics, their order against the pawns not fixed by code; the port ticks
    // them in the leftover pass, after the fighters.
    let tick = w.flow.tick;
    play_dissolve(w, slot, tick);
    let (x, y) = (run_actor(w, slot).x, run_actor(w, slot).y);
    w.events.push(Event::Sound {
        fighter: -1,
        ev: SFX_DESTROYED,
        x: Some(x),
        y: Some(y),
        hit: None,
        pooled: true,
        key: None,
    });
    // Comp_EdgeTrace.On Destroyed: its polygon leaves the EdgeTraceGenerator, the riders bound to it detach, Enabled
    // false; a respawn does not register it again.
    wall_mut(w, slot).edges = false;
    edges_removed(w, run_actor(w, slot).index);
    let respawn = geo(w, run_actor(w, slot).index).respawn;
    let m = wall_mut(w, slot);
    m.glow = false;
    m.broken_at = tick;
    let owner = i32::try_from(run_actor(w, slot).index).expect("a placed index");
    stage_set_timer(w, owner, RESPAWN_KEY, respawn, false, EV_RESPAWN);
}

/// RespawnWall @2910 -> @2346-2701, then WallMove.PlayFromStart @72 (in the timer pass: the timeline ticks from the next
/// frame).
fn respawn_wall(w: &mut World, slot: u32) {
    let m = wall(w, slot);
    let y = m.uy + if m.left { TELEPORT } else { -TELEPORT };
    move_root(w, slot, y);
    let tick = w.flow.tick;
    let m = wall_mut(w, slot);
    m.end_y = m.uy;
    m.hp = m.max_hp;
    m.damage = 0.0;
    m.dissolve = 0.0;
    set_collision(w, slot, true);
    wall_mut(w, slot).respawn_at = tick;
    play_move(w, slot, tick + 1);
}

/// `hurt`: the WallHitCollision box where the root is (none while the boxes' collision is off).
fn hurt(w: &World, slot: u32, out: &mut Vec<StageHurt>) -> bool {
    let m = wall(w, slot);
    if !m.on {
        return false;
    }
    let g = geo(w, run_actor(w, slot).index);
    let Some(mut b) = g.hurt else { return false };
    b.x = g.hurt_x - (m.uy - g.start_y);
    out.push(StageHurt::Box(b));
    true
}

/// A due timer of one wall (ev `WALL_EV` and up, owner its placed index).
pub fn timer(w: &mut World, slot: u32, ev: i32) {
    if ev == EV_RESPAWN {
        respawn_wall(w, slot);
    } else {
        crate::fighter::common::warn_text(w, &format!("m010_wall_C: a timer event it does not have ({ev})"));
    }
}

/// One wall's frame in the leftover pass: its two timelines (TickTimeline in the template's order is the component
/// order: WallMove, then the dissolve; Unverified: their order is not read), then WallTouchCollision's begin overlaps.
pub fn tick(w: &mut World, slot: u32) {
    let t = w.flow.tick;
    let y0 = wall(w, slot).uy;
    let m = wall(w, slot);
    if m.move_on && t >= m.move_from {
        let p = m.move_pos + DELTA_SECONDS;
        let m = wall_mut(w, slot);
        m.move_pos = if p > MOVE_LEN { MOVE_LEN } else { p };
        move_update(w, slot);
        if p > MOVE_LEN {
            let m = wall_mut(w, slot);
            m.move_on = false;
            m.glow = true; // Finished @563 -> @15
        }
    }
    let m = wall(w, slot);
    if m.dissolve_on && t >= m.dissolve_from {
        let p = m.dissolve_pos + DELTA_SECONDS;
        let m = wall_mut(w, slot);
        m.dissolve_pos = if p > DISSOLVE_LEN { DISSOLVE_LEN } else { p };
        dissolve_update(w, slot);
        if p > DISSOLVE_LEN {
            wall_mut(w, slot).dissolve_on = false; // Finished: nothing
        }
    }
    let moved = wall(w, slot).uy != y0;
    touch_test(w, slot, moved);
}

/// WallTouchCollision's begin overlaps with the fighters' root capsules (Pawn), tested once a tick after the timelines.
/// SweepResult.ImpactPoint: a fighter's own swept move into the box hands the box the reversed sweep hit (its point on
/// the box: the box point nearest his capsule here); a begin from the box's own unswept move, or its collision coming
/// on, carries an empty result, ImpactPoint (0, 0, 0). Unverified: the reversed hit and the empty result are stock UE
/// 4.26's BeginComponentOverlap / UpdateOverlaps, not read in the exe; the build's events come inside the movement,
/// the port's after the leftover pass (a pass through the box inside one tick is missed).
fn touch_test(w: &mut World, slot: u32, moved: bool) {
    let g = geo(w, run_actor(w, slot).index);
    let Some([bl, br, bb, bt]) = g.touch else { return };
    if !wall(w, slot).on {
        return;
    }
    let ox = -(wall(w, slot).uy - g.start_y);
    let (l, r) = (bl + ox, br + ox);
    let tick = w.flow.tick;
    for f in all_fighters(w) {
        let (id, x) = (w.fighter(f).id, w.fighter(f).body.x);
        let ay = crate::fighter::actor_y(w, f);
        let on = touches(w, f, l, r, bb, bt);
        let k = wall(w, slot).inside.iter().position(|&i| i == id);
        if on && k.is_none() {
            wall_mut(w, slot).inside.push(id);
            // (the collision came on in the last frame's timer pass, or the box moved in this pass)
            let fresh = moved || wall(w, slot).respawn_at == tick - 1;
            let (tx, ty) = if fresh { (0.0, 0.0) } else { (max(l, min(r, x)), max(bb, min(bt, ay))) };
            let m = wall_mut(w, slot);
            m.touch_at = tick;
            m.touch_x = tx;
            m.touch_y = ty;
            w.events.push(Event::Sound {
                fighter: -1,
                ev: SFX_TOUCH,
                x: Some(tx),
                y: Some(ty),
                hit: None,
                pooled: true,
                key: None,
            });
        } else if !on && let Some(k) = k {
            wall_mut(w, slot).inside.remove(k);
        }
    }
}

/// A fighter's root capsule (upright, on the play plane: the box spans the depth) over the box.
fn touches(w: &World, f: crate::fighter::FighterId, l: Real, r: Real, b: Real, t: Real) -> bool {
    if w.fighter(f).life.dead > 0.0 || crate::fighter::no_collision(w, f) {
        return false;
    }
    let x = w.fighter(f).body.x;
    let cr = crate::fighter::cap_radius(w, f);
    let h = max(crate::fighter::capsule_half(w, f) - cr, 0.0);
    let cy = crate::fighter::actor_y(w, f);
    let dx = max(max(l - x, 0.0), x - r);
    let dy = max(max(b - (cy + h), 0.0), cy - h - t);
    dx * dx + dy * dy <= cr * cr
}
