//! Marvin's swap: C018SwapProjectile_C (a plain projectile),
//! Buff_C018ProjectileWarpMove_C and its child Buff_C018ProjectileWarpEnemy_C (Buff_DynamicMoveToActor children:
//! `combat::dynamic_move`'s buffs, their child fields [`WarpExt`] in the dynamic move's `child`) and
//! Buff_C018WarpFighterHit_C. Its state is the buffs' own; nothing on Marvin (MarvinSwapState stays empty). No pawn hook.
//!
//! The live path: the side special's C018SwapProjectile hits an enemy fighter; its On Hit Buffs row
//! (Buff_C018WarpFighterHit_C) puts that buff on the victim with Marvin as Instigator; it spawns
//! Buff_C018ProjectileWarpEnemy on the victim and starts its move toward Marvin (+ the Offset), then destroys itself.
//! WarpEnemy's EndPlay deals PostMoveKnockback through Marvin's hit box handler.
//!
//! C018SwapProjectile's own graph (BeginPlay @375 / Reflected @326 / EndPlay @248: OriginalInstigator, ClearMoveToPositions
//! destroying C018WarpToPosition_C actors, which no live graph spawns) does nothing the sim can see beyond its
//! parent's, so it is entered with no hooks.

use crate::Real;
use crate::State;
use crate::actors::{ActorClass, ActorRef, ProjectileClass};
use crate::combat::buffs::{buff, end_buff, spawn_buff};
use crate::combat::dynamic_move::{
    DynamicMoveChild, DynamicMoveClass, begin_move_to_actor, dest_place, dynamic_move_buffs, dynamic_move_mut,
    dynamic_move_of, handle_move,
};
use crate::combat::hits::direct_hit;
use crate::combat::packet::HitPacket;
use crate::combat::{BuffClass, BuffRef};
use crate::events::Event;
use crate::fighter::common::{buff_def, warn_text};
use crate::fighter::match_side::pawn_gone;
use crate::fighter::{FighterId, actor_y, set_grounded};
use crate::math::ue::{safe_normal2d, size2d};
use crate::math::{max, min};
use crate::name;
use crate::registry::entries::{ActorBuffHooks, ActorEntry, ActorHooks, BuffEntry, BuffHooks, ProjectileHooks};
use crate::registry::props::{num, obj};
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use mvs_data::value::{Props, Value};

/// C018SwapProjectile's class.
pub const SWAP: &str = "C018SwapProjectile_C";

/// Buff_C018WarpFighterHit's class.
pub const WARP_FIGHTER_HIT: &str = "Buff_C018WarpFighterHit_C";

/// Buff_C018ProjectileWarpMove's class.
pub const PROJECTILE_WARP_MOVE: &str = "Buff_C018ProjectileWarpMove_C";

/// Buff_C018ProjectileWarpEnemy's class.
pub const PROJECTILE_WARP_ENEMY: &str = "Buff_C018ProjectileWarpEnemy_C";

/// The warp buffs' fields beside Buff_DynamicMoveToActor's (`WarpExt extends DynamicMove`: the dynamic move's
/// `child`). Sim axes: x = -UE Y, y = UE Z.
#[derive(State, Copy, PartialEq, Debug, Default)]
pub struct WarpExt {
    /// StartingLocation (the owner's actor location at BeginMoveToActor @1707).
    pub sx: Real,
    pub sy: Real,
    /// MaxDistance (class default: 1500 WarpMove, 1000 WarpEnemy).
    pub max_dist: Real,
    /// MinimumTime.
    pub min_t: Real,
    /// MaximumTime.
    pub max_t: Real,
    /// GetInstigator (SpawnBuffActor's Instigator: Marvin).
    pub inst: Option<FighterId>,
    /// WarpEnemy: PostMoveKnockback.AttackID @2130.
    pub attack_id: i32,
}

/// `warpOf`: the buff's WarpExt (a copy).
fn warp_of(w: &World, b: BuffRef) -> Option<WarpExt> {
    match dynamic_move_of(w, b)?.child {
        DynamicMoveChild::C018Warp(x) => Some(x),
        DynamicMoveChild::None => None,
    }
}

/// The WarpExt, to write.
fn warp_mut(w: &mut World, b: BuffRef) -> Option<&mut WarpExt> {
    match &mut dynamic_move_mut(w, b)?.child {
        DynamicMoveChild::C018Warp(x) => Some(x),
        DynamicMoveChild::None => None,
    }
}

/// The buff's class defaults (`d`).
fn defs_of(w: &World, f: FighterId, b: BuffRef) -> Option<Props> {
    let cls = buff(w, b)?.cls;
    buff_def(w, ActorRef::Fighter(f), cls).cloned()
}

/// `warpMoveBegin` (Buff_C018ProjectileWarpMove ReceiveBeginPlay @783, behind the parent's): its `DynamicMoveClass::begin`;
/// it sets the dynamic move's `child`. SetMovementMode(Falling) as rick's teleport takes it, the swaprifle hit sound.
// Unverified: the buff actor's place is taken as its owner's actor location (SpawnBuffActor's spawn transform not read).
pub fn warp_move_begin(w: &mut World, f: FighterId, b: BuffRef, from: Option<FighterId>, _duration: Option<Real>) {
    let d = defs_of(w, f, b).unwrap_or_default();
    let x = WarpExt {
        sx: 0.0, // StartingLocation's class default (0, 0, 0)
        sy: 0.0,
        max_dist: num(&d, "MaxDistance").unwrap_or(0.0),
        min_t: num(&d, "MinimumTime").unwrap_or(0.0),
        max_t: num(&d, "MaximumTime").unwrap_or(0.0),
        inst: from,
        attack_id: 0,
    };
    // Offset: the parent's variable, which a child's class defaults set (WarpEnemy (0, 0, 300)).
    let o = obj(&d, "Offset");
    let (oy, oz) = (o.and_then(|o| num(o, "Y")).unwrap_or(0.0), o.and_then(|o| num(o, "Z")).unwrap_or(0.0));
    if let Some(e) = dynamic_move_mut(w, b) {
        e.child = DynamicMoveChild::C018Warp(x);
        e.ox = -oy;
        e.oy = oz;
    }
    if d.get("MaxDistance").is_none() {
        let c = buff(w, b).map_or_else(String::new, |x| w.names.str(x.cls).to_owned());
        warn_text(w, &format!("{c}: no MaxDistance in its class defaults"));
    }
    let fx = w.fighter_mut(f);
    fx.moves.flying = false;
    fx.hit.kb_mode = false;
    set_grounded(w, f, false);
    let (fid, px, py) = (w.fighter(f).id, w.fighter(f).body.x, actor_y(w, f));
    // (the fighter's own events, flushed into the World's at the end of the step: `f.events.push`)
    w.fighter_mut(f).events.push(Event::Sound {
        fighter: fid,
        ev: name!("sfx_cha_C018_swaprifle_hit"),
        x: Some(px),
        y: Some(py),
        hit: None,
        pooled: false,
        key: None,
    });
}

/// `warpEnemyBegin` (Buff_C018ProjectileWarpEnemy ReceiveBeginPlay @2199): WarpMove's, then the AttackID = the
/// Instigator's attack count (0 when the cast fails).
pub fn warp_enemy_begin(w: &mut World, f: FighterId, b: BuffRef, from: Option<FighterId>, duration: Option<Real>) {
    warp_move_begin(w, f, b, from, duration);
    let id = from.map_or(0, |i| w.fighter(i).decay.attack_count);
    if let Some(x) = warp_mut(w, b) {
        x.attack_id = id;
    }
}

/// `warpTimer` (HandleMoveOnTimer override @898-1636): the distance the owner has travelled against MaxDistance.
pub fn warp_timer(w: &mut World, f: FighterId, b: BuffRef) -> bool {
    let Some(x) = warp_of(w, b) else { return false };
    let (fx, fy) = (w.fighter(f).body.x, actor_y(w, f));
    let rem = x.max_dist - size2d([fx - x.sx, fy - x.sy]);
    if rem.is_nan() || rem <= 0.0 {
        return true;
    }
    handle_move(w, f, b);
    false
}

/// `warpEnemyEnd` (Buff_C018ProjectileWarpEnemy ReceiveEndPlay @2214, behind the parent's): PostMoveKnockback through
/// the Instigator's hit box handler.
// Unverified: the end at a defeat (DestroyBuffWhenOwnerDefeated) or with a knocked-out owner or Instigator deals
// nothing here; what the build's pfgDoDamage does on a defeated pawn is not read.
pub fn warp_enemy_end(w: &mut World, f: FighterId, b: BuffRef) {
    let Some(x) = warp_of(w, b) else { return };
    let Some(inst) = x.inst else { return };
    if pawn_gone(w) || w.fighter(f).life.dead > 0.0 || w.fighter(inst).life.dead > 0.0 {
        return;
    }
    let d = defs_of(w, f, b).unwrap_or_default();
    let Some(pk) = obj(&d, "PostMoveKnockback").cloned() else {
        let c = buff(w, b).map_or_else(String::new, |x| w.names.str(x.cls).to_owned());
        warn_text(w, &format!("{c}: no PostMoveKnockback in its class defaults"));
        return;
    };
    let arrived = num(&d, "DistanceToConsiderArrived").unwrap_or(0.0);
    let kd = obj(&pk, "KnockBackDirectionVector");
    let ax = kd.and_then(|k| num(k, "X")).unwrap_or(0.0).abs();
    // UE Y = -sim x: the Instigator's Y above the owner's -> +Abs (@515-553)
    let (ix, fx, fy) = (w.fighter(inst).body.x, w.fighter(f).body.x, actor_y(w, f));
    let sx = if -ix > -fx { ax } else { -ax };
    let (ox, oy, dest) = dynamic_move_of(w, b).map_or((0.0, 0.0, None), |e| (e.ox, e.oy, e.dest));
    let (dx, dy) = dest.map_or((0.0, 0.0), |d| dest_place(w, d));
    let (tx, ty) = ((dx + ox), (dy + oy));
    let (ey, ez) = ((-(tx - fx)), (ty - fy)); // D - owner, in UE Y and Z
    let dist = size2d([ey, ez]);
    let (dir_x, dir_y);
    if dist <= arrived {
        dir_x = sx;
        dir_y = kd.and_then(|k| num(k, "Y")).unwrap_or(0.0);
    } else {
        // Normal2D: a zero vector at a squared size of 1e-8 or under, else times InvSqrt
        [dir_x, dir_y] = safe_normal2d([ey, ez]);
    }
    let mut dir = Props::new();
    dir.insert("X".into(), Value::Num(f64::from(dir_x)));
    dir.insert("Y".into(), Value::Num(f64::from(dir_y)));
    let mut q = pk.clone();
    q.insert("KnockBackDirectionVector".into(), Value::Obj(dir.clone()));
    q.insert("KnockBackDirection".into(), Value::Obj(dir));
    q.insert("AttackID".into(), Value::Num(f64::from(crate::real(x.attack_id))));
    if let Some(v) = pk.get("OnHitAudio") {
        q.insert("OnHitSound".into(), v.clone());
    }
    let hp = HitPacket::from_props(&w.names, &q, None);
    direct_hit(w, inst, f, &hp);
}

/// `warpBeginMoveToActor` (the BeginMoveToActor override @1637-2116): StartingLocation = the owner's location, the time
/// by distance in place of the caller's, then the parent's BeginMoveToActor.
// Unverified: the pull's length and arrival are the graph's; how they look against the live game is not checked.
#[allow(clippy::too_many_arguments, reason = "the function's parameters")]
pub fn warp_begin_move_to_actor(
    w: &mut World,
    f: FighterId,
    b: BuffRef,
    dest: FighterId,
    ease_in: bool,
    ease_out: bool,
    launch_at_end: bool,
    vel: (Real, Real),
) {
    let (fx, fy) = (w.fighter(f).body.x, actor_y(w, f));
    let Some(x) = warp_mut(w, b) else { return };
    x.sx = fx;
    x.sy = fy;
    let x = *x;
    let (dx, dy) = (w.fighter(dest).body.x, actor_y(w, dest));
    let dist = size2d([dx - fx, dy - fy]);
    let a = min(max(dist, 0.0), x.max_dist) / x.max_dist;
    let t = x.min_t + (a * (x.max_t - x.min_t));
    if begin_move_to_actor(w, f, b, Some(ActorRef::Fighter(dest)), t, ease_in, ease_out, launch_at_end, [vel.0, vel.1])
    {
        end_buff(w, f, b);
    }
}

/// Buff_C018WarpFighterHit's begin @10-851: the WarpEnemy buff on the hit fighter (not its Instigator), its move toward
/// the Instigator (SwapMoveTime 0.35, ease in and out, no launch), then the buff destroys itself.
fn warp_fighter_hit_begin(w: &mut World, f: FighterId, b: BuffRef, from: Option<FighterId>, _duration: Option<Real>) {
    if let Some(i) = from.filter(|&i| i != f) {
        let nb = spawn_buff(w, f, name!("Buff_C018ProjectileWarpEnemy_C"), 0.0, Some(0), Some(i), true, None);
        if let Some(nb) = nb
            && buff(w, nb).is_some_and(|x| x.frames > 0.0)
        {
            warp_begin_move_to_actor(w, f, nb, i, true, true, false, (0.0, 0.0));
        }
    }
    end_buff(w, f, b);
}

/// Buff_C018WarpFighterHit on a spawned actor: BeginPlay with an owner that is not its Instigator and no
/// BP_FighterCharacter (@231) goes to K2_DestroyActor @837; the buff ends at once (true).
// The class has an actor entry, so the spawn runs; the begin-then-end
// leaves the actor's list and the sweep's blocks as they were after the spawn.
fn warp_fighter_hit_actor_begin(_w: &mut World, _a: ActorRef, _i: u32, _from: Option<FighterId>) -> bool {
    true
}

/// This file's registry entries: the swap projectile, the two warp buffs through `dynamic_move_buffs` (WarpEnemy is a
/// child of WarpMove: WarpMove's overrides, its own begin and end behind them) and Buff_C018WarpFighterHit_C.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.actors.enter(
        SWAP,
        ActorEntry::new(
            ActorClass::Projectile(ProjectileClass::C018SwapProjectile),
            ActorHooks::Projectile(ProjectileHooks::default()),
        ),
    )?;
    dynamic_move_buffs(
        r,
        &[
            (
                PROJECTILE_WARP_MOVE,
                BuffClass::BuffC018ProjectileWarpMove,
                DynamicMoveClass {
                    handle_move_on_timer: Some(warp_timer),
                    begin: Some(warp_move_begin),
                    ..DynamicMoveClass::default()
                },
            ),
            (
                PROJECTILE_WARP_ENEMY,
                BuffClass::BuffC018ProjectileWarpEnemy,
                DynamicMoveClass {
                    handle_move_on_timer: Some(warp_timer),
                    begin: Some(warp_enemy_begin),
                    end: Some(warp_enemy_end),
                    ..DynamicMoveClass::default()
                },
            ),
        ],
    )?;
    let hit = BuffHooks { begin: Some(warp_fighter_hit_begin), ..BuffHooks::default() };
    let on_actor = ActorBuffHooks { begin: Some(warp_fighter_hit_actor_begin), ..ActorBuffHooks::default() };
    r.buffs.enter(
        WARP_FIGHTER_HIT,
        BuffEntry { actor: Some(on_actor), ..BuffEntry::new(BuffClass::BuffC018WarpFighterHit, hit) },
    )
}
