//! The bot Blueprint services: the BTService_BlueprintBase classes under Panda_Main/Blueprints/AI and Velma's
//! police, ported from their bytecode.
//!
//! Coordinates as in `ai/util.rs`: world Y = -x, Z = y.

#![allow(clippy::neg_cmp_op_on_partial_ord, reason = "the comparison is `!(a > b)`: NaN takes the negated branch")]

use crate::Real;
use crate::actors::world_actor::{actor_cls, actor_dead};
use crate::actors::{ActorRef, ItemExt, ProjectileExt};
use crate::ai::blackboard::{BbType, BbVal, BbVec};
use crate::ai::cx::{Cx, Svc};
use crate::ai::decorators::cast_to;
use crate::ai::props;
use crate::ai::sweep::{SweepHit, trace_sweep};
use crate::ai::util::{
    actor, corners, facing_left, fighter, loc, loc_y, loc_z, num, sq, valid, vec, vec_opt, vel, waypoints,
};
use crate::fighter::FighterId;
use crate::fighter::body::def;
use crate::fighter::common::warn_once;
use crate::math::{max, min, ue};
use crate::name;
use crate::names::Name;
use crate::world::World;

// ---- the services' reads

/// `bbFighter`: `cast<Base_Fighter>(GetBlackboardValueAsActor(key))`.
fn bb_fighter(cx: &Cx, prop: &str) -> Option<FighterId> {
    fighter(actor(cx, prop)).filter(|f| cx.w.fighters.get(*f).is_some())
}

fn is_dead(w: &World, f: FighterId) -> bool {
    w.fighter(f).life.dead != 0.0
}

fn team(w: &World, f: FighterId) -> i32 {
    w.fighter(f).team.team
}

/// `dist2`: Vector_DistanceSquared of two actor locations (float; both at world X 0).
fn dist2(w: &World, a: ActorRef, b: ActorRef) -> Real {
    let dy = loc_y(w, Some(a)) - loc_y(w, Some(b));
    let dz = loc_z(w, Some(a)) - loc_z(w, Some(b));
    (dy * dy) + (dz * dz)
}

/// `sweep(c, ...)`.
fn sweep(cx: &mut Cx, y0: Real, z0: Real, y1: Real, z1: Real, r: Real) -> Option<SweepHit> {
    trace_sweep(cx.w, y0, z0, y1, z1, r, None)
}

/// `impact(c, h)`: ImpactPoint of a sweep hit: the touching point on the box (on a polygon solid: its point nearest
/// the sweep's centre at the hit).
fn impact(h: &SweepHit) -> BbVec {
    match &h.shape {
        crate::world::stage_query::StageShape::Box(g) => BbVec {
            x: 0.0,
            y: min(crate::math::narrow(-g.left), max(crate::math::narrow(-g.right), h.y)),
            z: min(crate::math::narrow(g.top), max(crate::math::narrow(g.bottom), h.z)),
        },
        p => {
            let (x, z) = p.closest(-h.y, h.z);
            BbVec { x: 0.0, y: -x, z }
        }
    }
}

// ---- Velma's police

/// VelmaPolice_DetectEnemy_BTS_C: ReceiveTickAI @1433: EnemyActor valid and a BaseFighter ->
/// dead ? ClearBlackboardValue @1145 and search : keep it; valid and no fighter, or not valid -> search (@1165, no
/// clear). The search: the controlled pawn a BaseFighter, L_ClosestDistSquared = 9999999, then over
/// GetAllActorsOfClass(BP_FighterCharacter: the match's fighters in spawn order, minions with them) the one of
/// another TeamIndex nearest by Vector_DistanceSquared (strict <: the first of a tie), dead or not;
/// SetBlackboardValueAsObject(EnemyActor, it or None) @1041.
#[derive(Clone, Debug, Default)]
pub struct VelmaPoliceDetectEnemy;

impl Svc for VelmaPoliceDetectEnemy {
    fn tick(&mut self, cx: &mut Cx, _dt: Real) {
        let e = actor(cx, "EnemyActor");
        if valid(cx.w, e)
            && let Some(f) = fighter(e)
        {
            if !is_dead(cx.w, f) {
                return;
            }
            let k = cx.key("EnemyActor");
            cx.clear(k);
        }
        let me = cx.me;
        let mut best = None;
        let mut best_d: Real = 9_999_999.0;
        for t in cx.w.all_ids() {
            let d = dist2(cx.w, ActorRef::Fighter(t), ActorRef::Fighter(me));
            if team(cx.w, t) != team(cx.w, me) && d < best_d {
                best_d = d;
                best = Some(ActorRef::Fighter(t));
            }
        }
        let k = cx.key("EnemyActor");
        cx.set_obj(k, best);
    }
}

/// UpdateTargetPosition_BTS_C: TargetPoint = the EnemyActor's location while it is valid.
#[derive(Clone, Debug, Default)]
pub struct UpdateTargetPosition;

impl Svc for UpdateTargetPosition {
    fn tick(&mut self, cx: &mut Cx, _dt: Real) {
        let e = actor(cx, "EnemyActor");
        if let Some(e) = e
            && valid(cx.w, Some(e))
        {
            let v = loc(cx.w, e);
            let k = cx.key("TargetPoint");
            cx.set(k, BbVal::Vec(v));
        }
    }
}

/// BotUpdateDifficulty_BTS_C. The Lab is training (BPI_IsTrainingOrTutorial) and a custom
/// game is context 3: in the build the service returns before it touches NumericDifficulty (@541), which stays 0.
// Deviation: every match bot and every Lab fight bot skips that early-out and runs the rest as the build's
// non-custom matches do, with Min = Max = its level (`BtBot::lab_bot` / `match_bot`); a Lab recovery, evade, idle or
// patrolling bot has no range (`level_range` None) and returns here. The adaptive branches past @801 are ported as
// the build has them and not reached (Min == Max returns first).
// An arcade round's bots carry the run's ArcadeDifficulty (PandaGameMode SpawnAndInitPlayer @0 writes it to every
// player's MatchPlayerData) and the build returns at @541 (ArcadeDifficulty != 0, @254), NumericDifficulty 0
// whatever the round. Deviation: the port has no such early-out; an arcade bot is a match bot with the round's level.
// NumericDifficulty and the range are floats (Lerp is not truncated). The Interval is the tree's (none set) over the
// class's: UBTService's 0.5 +- 0.1.
// Persona: InitializeVariables looks the bot's 'Bot Persona Key' up in PersonaDataTable (@526); its rows are Default,
// Tony, Daniel, Figgy, Larissa, Long, Christina. The build's custom game and Lab pass None (no row); the port's bots
// carry Default: PersonaFound is true and Default's DisableAdaptiveDifficulty is false, so @608 goes on and the
// `if not (false)` at @732 jumps to @801: NumericDifficulty is still Min == Max, the bot's level. Only the Default
// row is in the data: another key warns.
// EnemyFighter / AllyFighter are the service's own variables, written only when the blackboard's actor casts to a
// fighter (@210, @338): a cleared key leaves the last one.
// Unverified: IsValid(AllyFighter) on a fighter that has left the match (a kicked or ringed-out pawn) is read as
// valid while the port keeps its Fighter; the TeamKnockouts index is the fighter's team (Comp_Actor_Destructible
// TeamIndex), an index past the array reads 0 (Array_Get).
#[derive(Clone, Debug, Default)]
pub struct BotUpdateDifficulty {
    enemy_fighter: Option<FighterId>,
    ally_fighter: Option<FighterId>,
}

impl Svc for BotUpdateDifficulty {
    fn tick(&mut self, cx: &mut Cx, _dt: Real) {
        let Some(range) = cx.bot.level_range else { return };
        // InitializeVariables
        if let Some(e) = bb_fighter(cx, "EnemyActor") {
            self.enemy_fighter = Some(e);
        }
        if let Some(a) = bb_fighter(cx, "AllyActor") {
            self.ally_fighter = Some(a);
        }
        // @526..@608: Default (and None, no row) leave the persona branches untaken.
        if cx.bot.persona_key != "Default" && cx.bot.persona_key != "None" {
            warn_once(cx.w, name!("BotUpdateDifficulty: a persona other than Default has no row in the data"));
        }
        let (lo, hi) = (range.min, range.max);
        // @801
        if lo == hi {
            cx.bot.difficulty = lo;
            return;
        }
        let w = &*cx.w;
        let ally = self.ally_fighter.filter(|f| w.fighters.get(*f).is_some());
        let enemy = self.enemy_fighter.filter(|f| w.fighters.get(*f).is_some());
        // @907-1026: a human team mate
        if let Some(a) = ally
            && !w.fighter(a).controls.bot_controlled
        {
            cx.bot.difficulty = hi;
            return;
        }
        let Some(enemy) = enemy else { return };
        let ko = |t: i32| usize::try_from(t).ok().and_then(|i| w.flow.team_knockouts.get(i)).copied().unwrap_or(0.0);
        let diff = ko(team(w, cx.me)) - ko(team(w, enemy));
        let lerp = |t: Real| lo + (t * (hi - lo));
        let to = if diff > 1.0 {
            lo
        } else if diff > 0.0 {
            lerp(0.33)
        } else if diff < 0.0 {
            hi
        } else {
            let dmg = w.fighter(enemy).stats.damage - w.fighter(cx.me).stats.damage;
            if dmg > 40.0 {
                lerp(0.33)
            } else if dmg < -40.0 {
                lerp(0.66)
            } else {
                lerp(0.5)
            }
        };
        if cx.bot.difficulty != to {
            cx.bot.difficulty = to;
        }
    }
}

/// UpdateFleeTime_BTS_C: while fleeing, ends the flee (and suppresses it for 3.5 s) when
/// there is no ground 1000 further away from the enemy, or when the blast box side the bot faces is closer than 2000;
/// then it steps toward the enemy.
#[derive(Clone, Debug, Default)]
pub struct UpdateFleeTime {
    right: Option<Real>,
    left: Option<Real>,
}

impl Svc for UpdateFleeTime {
    fn activate(&mut self, cx: &mut Cx) {
        let k = corners(cx.w);
        let right = self.right.unwrap_or_else(|| props::num(cx.props, "BlastBoundsRight").unwrap_or(0.0));
        let left = self.left.unwrap_or_else(|| props::num(cx.props, "BlastBoundsLeft").unwrap_or(0.0));
        self.right = Some(min(right, k.bottom_right[0]));
        self.left = Some(min(left, k.top_left[0]));
    }

    fn tick(&mut self, cx: &mut Cx, _dt: Real) {
        if !(num(cx, "FleeTime") > cx.bot.now) {
            return;
        }
        let f = cx.me;
        let Some(ef) = bb_fighter(cx, "Enemy") else { return };
        let end = |cx: &mut Cx| {
            let k = cx.key("FleeTime");
            cx.set(k, BbVal::Num(0.0));
            let k = cx.key("SuppressFleeTime");
            cx.set(k, BbVal::Num(cx.bot.now + 3.5));
            cx.bot.pad.set_x(0.0);
        };
        let (fr, er) = (Some(ActorRef::Fighter(f)), Some(ActorRef::Fighter(ef)));
        let away = loc_y(cx.w, fr) > loc_y(cx.w, er);
        let y = loc_y(cx.w, fr) + if away { 1000.0 } else { -1000.0 };
        let z = loc_z(cx.w, fr);
        let r = def(cx.w, f).inner_cap_radius;
        if sweep(cx, y, z, y, -99999.0, crate::math::narrow(r)).is_none() {
            end(cx);
            return;
        }
        let left = self.left.unwrap_or_else(|| props::num(cx.props, "BlastBoundsLeft").unwrap_or(0.0));
        let right = self.right.unwrap_or_else(|| props::num(cx.props, "BlastBoundsRight").unwrap_or(0.0));
        let my = loc_y(cx.w, fr);
        if (if facing_left(cx.w, f) { left - my } else { my - right }) < 2000.0 {
            end(cx);
            cx.bot.pad.set_x(if away { 1.0 } else { -1.0 }); // BPI_MoveX after the MoveX(0): toward the enemy
        }
    }
}

/// UpdateRecoveryPoint_BTS_C: the recovery point is chosen once per activation: the nearest
/// waypoint (UseNearestWaypoint), the ground under the target fighter, or the target's location. The tick only
/// steers toward it (+- 50 dead zone).
#[derive(Clone, Debug, Default)]
pub struct UpdateRecoveryPoint {
    self_pos: Option<BbVec>,
    target_actor: Option<ActorRef>,
    target_fighter: Option<FighterId>,
    initialized: bool,
}

/// UpdateRecoveryPoint's soft class fields besides C019 Ship (a ship's piece is `SweepHit::ship`): the class names
/// (a soft path's object name; none: empty).
const PASS_FIELDS: [&str; 4] =
    ["ScoobyElevatorClass", "Rocket Class", "Steven Shield Actor U Special Platform", "Steven Shield Actor U"];

impl UpdateRecoveryPoint {
    /// Nearest waypoint (3D, from SelfPos) among those with ground below them.
    fn nearest_waypoint(&self, cx: &mut Cx) -> Option<BbVec> {
        let s = self.self_pos.expect("SelfPos is set at the activation");
        let list: Vec<_> = waypoints(cx.w).collect();
        let mut best: Option<(BbVec, Real)> = None;
        for p in list {
            if sweep(cx, p.y, p.z, p.y, p.z - 20000.0, 0.0).is_none() {
                continue;
            }
            let d = sq(p.x - s.x) + sq(p.y - s.y) + sq(p.z - s.z);
            if best.is_none_or(|(_, bd)| d < bd) {
                best = Some((BbVec { x: p.x, y: p.y, z: p.z }, d));
            }
        }
        best.map(|b| b.0)
    }

    fn go_to_waypoint(&mut self, cx: &mut Cx) {
        let key = cx.key("RecoveryPoint");
        if let Some(w) = self.nearest_waypoint(cx) {
            cx.set(key, BbVal::Vec(w));
        }
        self.initialized = true;
    }
}

impl Svc for UpdateRecoveryPoint {
    fn activate(&mut self, cx: &mut Cx) {
        let key = cx.key("RecoveryPoint");
        self.self_pos = Some(loc(cx.w, ActorRef::Fighter(cx.me)));
        let t = actor(cx, "Target");
        if valid(cx.w, t) {
            self.target_actor = t;
            self.target_fighter = fighter(t);
        }
        // No BotRecoveryRetargeter actor on the map.
        if props::is_true(cx.props, "UseNearestWaypoint") {
            self.go_to_waypoint(cx);
            return;
        }
        if let Some(tf) = self.target_fighter {
            // SphereTraceMulti (@2091); a hit whose actor's class is exactly one of the node's five soft class
            // fields (GetObjectClass, EqualEqual_SoftClassReference @3008-3274: C019 Ship, then ScoobyElevatorClass,
            // Rocket Class, Steven Shield Actor U Special Platform, Steven Shield Actor U) is passed over, and as a
            // blocking hit it is the list's last: Found stays false and the waypoint is taken (@5187).
            // Steven_ShieldActor_U_Special_C is reached only through its field (nothing spawns it);
            // Steven_ShieldActor_Up_C, his up special's shield, is not on the list, so its box is a Found hit. A
            // ScoobyElevator_C hit is its BoxCollision solid, whose host is the placed actor's class (`trace_sweep`).
            let tr = Some(ActorRef::Fighter(tf));
            let (y, z) = (loc_y(cx.w, tr), loc_z(cx.w, tr));
            let r = def(cx.w, tf).inner_cap_radius;
            let h = sweep(cx, y, z, y, z - 20000.0, crate::math::narrow(r));
            let passed = |cx: &Cx, h: &SweepHit| {
                h.host.is_some_and(|c| {
                    let c = cx.w.names.str(c);
                    PASS_FIELDS.iter().any(|k| props::str(cx.props, k).is_some_and(|s| !s.is_empty() && s == c))
                })
            };
            match h {
                Some(h) if !h.ship && !passed(cx, &h) => {
                    cx.set(key, BbVal::Vec(impact(&h)));
                    self.initialized = true;
                }
                h if h.is_some() || !self.initialized => self.go_to_waypoint(cx),
                _ => {}
            }
            return;
        }
        self.initialized = true;
        let v = match self.target_actor {
            Some(a) if valid(cx.w, Some(a)) => Some(loc(cx.w, a)),
            _ => vec_opt(cx, "Target"),
        };
        cx.set(key, v.map_or(BbVal::Null, BbVal::Vec));
    }

    fn tick(&mut self, cx: &mut Cx, _dt: Real) {
        let d = vec(cx, "RecoveryPoint").y - loc_y(cx.w, Some(ActorRef::Fighter(cx.me)));
        cx.bot.pad.set_x(if d < -50.0 {
            1.0
        } else if d > 50.0 {
            -1.0
        } else {
            0.0
        });
    }
}

// ---- BotDetectEnemy

const STEVEN_TETHER: Name = name!("StevenTetherStunProjectile_C");
const TETHER: Name = name!("TetherProjectile_C");
const HEAL_GRENADE: Name = name!("HealGrenade_C");
const ICE_BREATH: Name = name!("Superman_IceBreath_C");
const ROCKET_UP: Name = name!("BugsBunnyRocketUpProjectile_C");
const ROCKET_F: Name = name!("BugsBunnyRocketFProjectile_C");
const PPP: Name = name!("ParentProjectilePhysics_C");
const FASTBALL: Name = name!("Fastball_C");
const IG_ART: Name = name!("Item_C017_Art_C");
const IG_SCRAP: Name = name!("Item_C017_Scrap_C");
const HARLEY_BOMB: Name = name!("HarleyBomb_C");
const CARRY_RUN: Name = name!("VelmaCarryRun_Actor_C");
const PORTAL_PARENT: Name = name!("PortalParent_C");

/// `World.matchTimerRemaining`: the match timer's seconds left, -1 when there is none (not running or over).
fn match_timer_remaining(w: &World) -> Real {
    match w.setup.rules.as_ref().map(|r| r.duration) {
        Some(d) if w.flow.match_started && !w.flow.match_over => {
            d - crate::real(w.flow.tick - w.flow.match_start_tick) / 60.0
        }
        _ => -1.0,
    }
}

/// ConsiderProjectile (@19-1533) with ClosestProjectileDistSq in and out: an actor of another team (`team`: the
/// actor's own Comp_Actor_Destructible.TeamIndex, for a projectile its spawner's) flying toward the bot and nearer
/// than any considered before becomes the closest; it is EnemyProjectile when its path passes within 300.
fn consider_projectile(cx: &mut Cx, pr: ActorRef, team_of: i32, closest: Real) -> Real {
    let me = ActorRef::Fighter(cx.me);
    if team_of == team(cx.w, cx.me) {
        return closest;
    }
    let w = &*cx.w;
    let (py, pz) = (loc_y(w, Some(pr)), loc_z(w, Some(pr)));
    let (sy, sz) = (loc_y(w, Some(me)), loc_z(w, Some(me)));
    let (vy, vz) = (py - sy, pz - sz);
    let d2 = vy * vy + vz * vz;
    // Dot(Normal(d, 0.0001), Normal(v, 0.0001)) < 0: a zero vector under the tolerance (a dot of 0: skipped).
    let v = vel(w, Some(pr));
    let nd = ue::normal([0.0, vy, vz], 1e-4);
    let nv = ue::normal([0.0, v.0, v.1], 1e-4);
    if !((nd[1] * nv[1]) + (nd[2] * nv[2]) < 0.0 && d2 < closest) {
        return closest;
    }
    let d = d2.sqrt();
    if sq(((nv[1] * d) + py) - sy) + sq(((nv[2] * d) + pz) - sz) < 90000.0 {
        let k = cx.key("EnemyProjectile");
        cx.set_obj(k, Some(pr));
    }
    d2
}

/// BotDetectEnemy_BTS_C: target choice and the perception keys. Every tick: the per-tick
/// keys are cleared and refilled from the actors the fighters of the last refresh have spawned; a dead EnemyActor is
/// cleared. Every 0.5 s: the fighter list is refreshed, the last live team mate in it becomes AllyActor (never
/// cleared) and the live fighters of other teams are scored; one roll in [0, total) picks the first enemy whose own
/// score is above it. With a live team mate the scores are 300 / 100 in turn along the list, the start flipping
/// every 30 s of the match timer and shifted by 30 s for the team mate later in the list, so the two prefer
/// different enemies.
// Unverified: the list is the fighters in player-index order. GetAllActorsOfClass (exe 142a7d120) walks the UObject
// class hash (GetObjectsOfClass 1415d78d0): the class, then its derived pawn classes, each in its hash set's order;
// that order was not read. The match timer's not-running values (0 for an invalid handle, -1 for a timer that is
// gone; K2_GetTimerRemainingTimeHandle 142b26730) give the same phase as the port's -1.
#[derive(Clone, Debug, Default)]
pub struct BotDetectEnemy {
    cached: Vec<FighterId>,
    next_update_time: Real,
    chosen_time: Real,
}

impl BotDetectEnemy {
    /// ProcessSpawnedActors for one fighter's projectiles (one cast chain per spawned actor, each cast by the
    /// class's parents in the data): the ParentProjectilePhysics (@3724: the sandwich, Morty's blaster shots and
    /// splitters, the creature's spit and firebolts) -> ConsiderProjectile @3803: enemy projectiles that fly toward
    /// the bot; the nearest one whose path passes within 300. A projectile class that no cast of the chain takes
    /// (Morty's grenades and whip, Wonder Woman's lasso, SeismicReactor_C, FireWallHazard_C: SpawnedActorParent
    /// children of no class the chain names) sets no key.
    fn projectiles(&self, cx: &mut Cx, f: FighterId, closest: &mut Real) {
        let me_team = team(cx.w, cx.me);
        let f_team = team(cx.w, f);
        let ids: Vec<_> = cx.w.actors.projectiles.ids().collect();
        for id in ids {
            let (cls, safe_speed2, is_safe, tether_ally, steven_ally) = {
                let Some(pr) = cx.w.actors.projectiles.get(id) else { continue };
                if pr.dead || crate::actors::projectile::spawner(pr) != Some(f) {
                    continue;
                }
                // (safeVelocity is read only of a safe: `pr instanceof BugsBunnySafe`)
                let is_safe = matches!(pr.ext, ProjectileExt::BugsBunnySafe(_));
                let speed2 = if is_safe {
                    let (sy, sz) = crate::chars::bugs::safe::safe_velocity(pr);
                    let (sy, sz) = (sy, sz);
                    (sy * sy) + (sz * sz)
                } else {
                    0.0
                };
                (
                    pr.cls,
                    speed2,
                    is_safe,
                    matches!(&pr.ext, ProjectileExt::TetherProjectile(t) if t.ally.is_some()),
                    matches!(&pr.ext, ProjectileExt::StevenTether(t) if t.ally.is_some()),
                )
            };
            let a = ActorRef::Projectile(id);
            // BugsBunnySafe (@4387-5030, @8544-8768; BugsBunnyNeutralSafe, its data-only child, with it: one class
            // of the port). A SpawnedActorParent child that no other cast of the chain takes, so its place in the
            // walk changes nothing: it is taken first here. VSizeSquared(GetVelocity) < 20000 (float, strict): the
            // spawner (TestFighter) of another team -> ItemToDestroy when Vector_DistanceSquared(safe, the bot) <
            // Square(1000) (@4803-5030; the trees' selector names the key ItemToAttack), of the bot's team -> Safe
            // (@8768, the bot's own safe included). Faster: another team's -> AvoidableItem (@8738), the bot's
            // team's nothing (@8728).
            if is_safe {
                if safe_speed2 < 20000.0 {
                    if f_team == me_team {
                        let k = cx.key("Safe");
                        cx.set_obj(k, Some(a));
                    } else if dist2(cx.w, a, ActorRef::Fighter(cx.me)) < 1e6 {
                        let k = cx.key("ItemToDestroy");
                        cx.set_obj(k, Some(a));
                    }
                } else if f_team != me_team {
                    let k = cx.key("AvoidableItem");
                    cx.set_obj(k, Some(a));
                }
                continue;
            }
            // StevenTetherStunProjectile (@858, the chain's second cast, ahead of TetherProjectile's): AttachedToAlly
            // (@937: StevenTether.ally set) -> the team test @973-1157 -> TetherAttachedToSelf = true (@8498) for the
            // bot's team; not attached: the bot's team -> ApproachableItem (@1371). Another team's: nothing either
            // way, never ConsiderProjectile.
            if cast_to(cx.w, a, cls, STEVEN_TETHER) {
                if f_team != me_team {
                    continue;
                }
                if steven_ally {
                    self.tether_attached(cx);
                } else {
                    let k = cx.key("ApproachableItem");
                    cx.set_obj(k, Some(a));
                }
                continue;
            }
            // TetherProjectile (@1460, SuperSaveTetherProjectile with it: its child; a ParentProjectilePhysics child
            // whose own cast comes first, so a tether of any team is never ConsiderProjectile's). AttachedToAlly
            // (@1539) -> the team test (@973-1157): the spawner (TestFighter) of the bot's team ->
            // TetherAttachedToSelf = true (@8498), of another team nothing. Not attached: the bot's team ->
            // ApproachableItem (@1779), another team nothing (@1778). The bot's own tether counts as its team's.
            if cast_to(cx.w, a, cls, TETHER) {
                if f_team != me_team {
                    continue;
                }
                if tether_ally {
                    self.tether_attached(cx);
                } else {
                    let k = cx.key("ApproachableItem");
                    cx.set_obj(k, Some(a));
                }
                continue;
            }
            // HealGrenade (@1868, after TetherProjectile's cast): the spawner of the bot's team ->
            // ApproachableItem (@2146), another team's nothing (@2145); never ConsiderProjectile.
            if cast_to(cx.w, a, cls, HEAL_GRENADE) {
                if f_team == me_team {
                    let k = cx.key("ApproachableItem");
                    cx.set_obj(k, Some(a));
                }
                continue;
            }
            // Superman_Ice_Breath (@2639, ahead of the ParentProjectilePhysics cast; the pellet and its Educated
            // child, SpawnedActorParent children): the spawner (TestFighter) of another team -> SupermanIceBreath
            // (@2820-2912), of the bot's team nothing (@2902). A tree whose selector is left at TargetPoint (a
            // Vector) takes no object (`Cx::set_obj`).
            if cast_to(cx.w, a, cls, ICE_BREATH) {
                if f_team != me_team {
                    let k = cx.key("SupermanIceBreath");
                    cx.set_obj(k, Some(a));
                }
                continue;
            }
            // BugsBunnyRocketUpProjectile (@3001-3303, a ParentProjectilePhysics child whose own cast comes first):
            // the spawner of another team -> AvoidableItem (@3274); the branch ends either way, never
            // ConsiderProjectile.
            if cast_to(cx.w, a, cls, ROCKET_UP) {
                if f_team != me_team {
                    let k = cx.key("AvoidableItem");
                    cx.set_obj(k, Some(a));
                }
                continue;
            }
            // BugsBunnyRocketFProjectile (@3363-3636): the bot's team's ends there (@3626 pop_flow); another team's
            // -> BugsRocketPlatform (@3636, a key no tree reads), then the flow falls through to the
            // ParentProjectilePhysics cast @3724, its parent, and ConsiderProjectile @3803.
            if cast_to(cx.w, a, cls, ROCKET_F) {
                if f_team == me_team {
                    continue;
                }
                let k = cx.key("BugsRocketPlatform");
                cx.set_obj(k, Some(a));
            }
            if cast_to(cx.w, a, cls, PPP) {
                *closest = consider_projectile(cx, a, f_team, *closest);
            }
        }
    }

    /// TetherAttachedToSelf = true (@8498), for a Bool key only.
    fn tether_attached(&self, cx: &mut Cx) {
        let k = cx.key("TetherAttachedToSelf");
        if cx.bot.bb.ty(k) == Some(BbType::Bool) {
            cx.set(k, BbVal::Bool(true));
        }
    }

    /// Iron Giant's Item_C017_Art and Item_C017_Scrap take their own casts (below). Item_Actor_Parent (any team, the
    /// bot's own included): a held one is HeldObject, an idle one within 1000 is Item. The list is the Comp_Spawner's
    /// of the fighter that spawned the item (@8312): a pickup changes the item's owner (SetOwner,
    /// Comp_Actor_Child.UpdateParent), not that list.
    /// Item_BatarangV2 (@3886, an ItemActorParent child) is taken by its own cast first and never reaches the
    /// Item_Actor_Parent one (@6833): VSizeSquared(GetVelocity) < 20000 (@3965-4090, float, strict) -> the spawner's
    /// TeamIndex (TestFighter's, not the batarang's) != the bot's -> ItemToDestroy (@4104-4298; the trees' selector
    /// names the key ItemToAttack); faster -> ConsiderProjectile(it) @8520 with the batarang's own TeamIndex (the
    /// thrower's, then a grabber's; `BatarangState::team`). The bot's own and its allies' slow batarangs set
    /// nothing. Fastball (@5835, an ItemActorParent child: its cast comes ahead of Item_Actor_Parent's @6833, so it
    /// is never Item or HeldObject): the spawner of the bot's team -> ReindogFastball (@6016-6098, @8828), held or
    /// not; another team: nothing (@6112). The bot's own counts (Reindog's own Fastball).
    // Deviation: the build walks the fighter's SpawnedActors in order and casts each; the port walks its projectiles
    // first and its items after, so a fast batarang is considered after every projectile of its spawner
    // (ClosestProjectileDistSq is shared): it differs only with two candidates of one fighter in one tick.
    fn items(&self, cx: &mut Cx, f: FighterId, closest: &mut Real) {
        let me_team = team(cx.w, cx.me);
        let f_team = team(cx.w, f);
        let me = ActorRef::Fighter(cx.me);
        let ids: Vec<_> = cx.w.actors.items.ids().collect();
        for id in ids {
            let a = ActorRef::Item(id);
            let (cls, held, batarang_team) = {
                let Some(it) = cx.w.actors.items.get(id) else { continue };
                if it.spawner != Some(f) || it.dead {
                    continue;
                }
                let bt = match &it.ext {
                    ItemExt::Batarang(b) => Some(b.team),
                    _ => None,
                };
                (it.cls, it.held, bt)
            };
            if cls == FASTBALL {
                if f_team == me_team {
                    let k = cx.key("ReindogFastball");
                    cx.set_obj(k, Some(a));
                }
                continue;
            }
            if let Some(bt) = batarang_team {
                let v = vel(cx.w, Some(a));
                let (vy, vz) = (v.0, v.1); // GetVelocity: an FVector
                if ((vy * vy) + (vz * vz)) < 20000.0 {
                    if f_team != me_team {
                        let k = cx.key("ItemToDestroy");
                        cx.set_obj(k, Some(a));
                    }
                } else {
                    *closest = consider_projectile(cx, a, bt, *closest);
                }
                continue;
            }
            // Item_C017_Art (@6310) sets nothing; Item_C017_Scrap (@6617-6744) is Item when its spawner is the bot
            // itself (TestFighter == SelfFighter), with no distance test and never HeldObject. Both casts come ahead
            // of Item_Actor_Parent's (@6833).
            if cast_to(cx.w, a, cls, IG_ART) {
                continue;
            }
            if cast_to(cx.w, a, cls, IG_SCRAP) {
                if f == cx.me {
                    let k = cx.key("Item");
                    cx.set_obj(k, Some(a));
                }
                continue;
            }
            if held {
                let k = cx.key("HeldObject");
                cx.set_obj(k, Some(a));
            } else if sq(loc_y(cx.w, Some(a)) - loc_y(cx.w, Some(me)))
                + sq(loc_z(cx.w, Some(a)) - loc_z(cx.w, Some(me)))
                < 1e6
            {
                let k = cx.key("Item");
                cx.set_obj(k, Some(a));
            }
        }
    }

    /// The fighter's spawned actors of the world-actor family (`f.spawnedActors`): the ally tunnel pass (ubergraph
    /// @8379-9865, after TestFighter's ProcessSpawnedActors @8312): TestFighter of the bot's team (the bot itself
    /// included; another team pops @8577) with a Comp_Pawn_Tunnel whose AlliedTunnels has exactly 2 entries (@8800):
    /// each tunnel, in the array's order, whose Vector_DistanceSquared to the bot is below ClosestPortalDistSq
    /// becomes the bound (@9706) and Portal (@9865).
    /// The PortalParent branch of ProcessSpawnedActors (@249-695, the first cast of the chain): a PortalParent_C of
    /// any fighter, any team (no team test), whose Vector_DistanceSquared to the bot is below ClosestPortalDistSq
    /// becomes the bound (@668) and Portal (@695), in SpawnedActors' order. Rick's four portal classes (the tunnels
    /// are SpawnedActorParent_C children, not this cast). A dead entry is one the build's list no longer holds
    /// (Comp_Spawner drops it from SpawnedActors at its EndPlay, @5582 / @6635).
    /// VelmaCarryRun_Actor (@7037, a SpawnedActorParent child; no class of the build derives from it): TestFighter
    /// of the bot's team (Comp_Actor_Destructible TeamIndex @7116-7300; the bot's own Velma included) ->
    /// VelmaCarryRunActor (@9115), the last one in the lists staying; another team's: nothing (@7314).
    /// HarleyBomb (@5479, after the Bugs safe and BasketBallProjectile_Pass casts, ahead of Fastball's @5835; a
    /// SpawnedActorParent child that no earlier cast of the chain takes; GetObjectClass exact is the same here, the
    /// build has no child class): TestFighter of another team (@5558-5742) -> ConsiderProjectile(the bomb) @5752,
    /// which tests the bomb's own Comp_Actor_Destructible TeamIndex (its spawner's: nothing changes it) and its
    /// GetVelocity: the body's while it simulates, 0 once attached. Another team's bomb attached to the bot itself
    /// sits at its leg with no velocity, so the dot test drops it.
    // Deviation: this walk of the fighter's actors comes after its projectiles and items, and
    // ClosestProjectileDistSq is shared with them: it differs only with a bomb and another candidate of one fighter
    // in one tick. The portal walk shares no key or bound with them, so the order does not show.
    fn actors(&self, cx: &mut Cx, f: FighterId, closest: &mut Real, closest_portal: &mut Real) {
        let me_team = team(cx.w, cx.me);
        let f_team = team(cx.w, f);
        let list: Vec<ActorRef> = cx.w.fighter(f).items.spawned_actors.clone();
        for a in list {
            let ActorRef::Actor(id) = a else { continue };
            if actor_dead(cx.w, a) {
                continue;
            }
            let cls = actor_cls(cx.w, a);
            if cls == Some(HARLEY_BOMB) {
                if f_team != me_team {
                    let owner = cx.w.actors.actors.get(id).map_or(f, |x| x.owner);
                    let t = team(cx.w, owner);
                    *closest = consider_projectile(cx, a, t, *closest);
                }
                continue;
            }
            if cls == Some(CARRY_RUN) && f_team == me_team {
                let k = cx.key("VelmaCarryRunActor");
                cx.set_obj(k, Some(a));
            }
            let Some(c) = cls else { continue };
            if !cast_to(cx.w, a, c, PORTAL_PARENT) {
                continue;
            }
            let d2 = dist2(cx.w, a, ActorRef::Fighter(cx.me));
            if d2 < *closest_portal {
                *closest_portal = d2;
                let k = cx.key("Portal");
                cx.set_obj(k, Some(a));
            }
        }
        if f_team == me_team
            && let Some(ts) = crate::chars::bugs::tunnel::allied_tunnels(cx.w, f)
            && ts.len() == 2
        {
            for t in ts {
                let a = ActorRef::Actor(t);
                let d2 = dist2(cx.w, a, ActorRef::Fighter(cx.me));
                if d2 < *closest_portal {
                    *closest_portal = d2;
                    let k = cx.key("Portal");
                    cx.set_obj(k, Some(a));
                }
            }
        }
    }
}

impl Svc for BotDetectEnemy {
    fn tick(&mut self, cx: &mut Cx, _dt: Real) {
        let me = cx.me;
        let now = cx.bot.now;
        // ClearBlackboardValues. A selector left at its default points at the first key (TargetPoint) and clears it.
        let k = cx.key("TetherAttachedToSelf");
        if cx.bot.bb.ty(k) == Some(BbType::Bool) {
            cx.set(k, BbVal::Bool(false));
        }
        for k in [
            "Portal",
            "Item",
            "SupermanIceBreath",
            "AvoidableItem",
            "BugsRocketPlatform",
            "ItemToDestroy",
            "EnemyProjectile",
            "Safe",
            "ApproachableItem",
            "VelmaCarryRunActor",
            "ReindogFastball",
            "HeldObject",
            "UFO",
        ] {
            let k = cx.key(k);
            cx.clear(k);
        }
        let mut closest: Real = 1e10; // ClosestProjectileDistSq
        let mut closest_portal: Real = 1e10; // ClosestPortalDistSq: ClearBlackboardValues @224
        // ProcessSpawnedActors for each fighter of the last refresh.
        for f in self.cached.clone() {
            if cx.w.fighters.get(f).is_none() {
                continue;
            }
            self.projectiles(cx, f, &mut closest);
            // C019_Ship (@6449): UFO, with no team or distance test; the last one in the lists stays.
            let ships: Vec<_> =
                cx.w.actors.ships.iter().filter(|(_, s)| s.owner == f && !s.dead).map(|(id, _)| id).collect();
            for sh in ships {
                let k = cx.key("UFO");
                cx.set_obj(k, Some(ActorRef::Ship(sh)));
            }
            self.items(cx, f, &mut closest);
            self.actors(cx, f, &mut closest, &mut closest_portal);
        }
        let enemy = bb_fighter(cx, "EnemyActor");
        if let Some(e) = enemy
            && is_dead(cx.w, e)
        {
            let k = cx.key("EnemyActor");
            cx.clear(k);
        }
        if !(self.next_update_time <= now) {
            return;
        }
        self.next_update_time = now + 0.5;
        // CachedActors = GetAllActorsOfClass(BP_FighterCharacter) @11088: the minions too (`World::all_ids`, after
        // the players). A live minion of the bot's team is an ally like a player (the AllyActor loop @77-2603 tests
        // no IsMinion), and its spawned actors are walked above.
        self.cached = cx.w.all_ids();
        let cached = self.cached.clone();
        let self_i = cached.iter().position(|&t| t == me);
        // Unverified: AllyActor is the last live teammate, and the 30 s shift below is against that one alone: in a
        // team of three (the port's own big matches) the first two bots both shift 0 and prefer the same enemy. Kept
        // as the build's graph.
        let mut ally: Option<usize> = None;
        for (i, &t) in cached.iter().enumerate() {
            if t == me || is_dead(cx.w, t) || team(cx.w, t) != team(cx.w, me) {
                continue;
            }
            let k = cx.key("AllyActor");
            cx.set_obj(k, Some(ActorRef::Fighter(t)));
            ally = Some(i);
        }
        let mut targets: Vec<FighterId> = Vec::new();
        let mut scores: Vec<Real> = Vec::new();
        let mut total: Real = 0.0;
        // fmod keeps the sign of the timer's -1 (not running): false.
        let shifted = match (self_i, ally) {
            (Some(s), Some(a)) if s > a => 30.0,
            _ => 0.0,
        };
        let mut high = ally.is_some() && ue::fmod(match_timer_remaining(cx.w) + shifted, 60.0) > 30.0;
        for &t in &cached {
            if t == me || is_dead(cx.w, t) || team(cx.w, t) == team(cx.w, me) {
                continue;
            }
            let w = &*cx.w;
            let tf = w.fighter(t);
            let mut s: Real;
            if ally.is_some() {
                // @5984: a minion scores 400 within 1200 on world Y (|dY| < 1200, strict), else 50 (@6020-6376),
                // and leaves the 300 / 100 turn and the +500 alone (@7646-7883 are the player's branch).
                // Not ported: the team mate's SpawnedActors pass (@6403-7431, both branches): +1000 when it holds a
                // MarkTeleportProjectile attached to the candidate or any HarleyBomb_C.
                if tf.life.is_minion {
                    s = if (loc_y(w, Some(ActorRef::Fighter(me))) - loc_y(w, Some(ActorRef::Fighter(t)))).abs() < 1200.0
                    {
                        400.0
                    } else {
                        50.0
                    };
                } else {
                    s = if high { 300.0 } else { 100.0 };
                    high = !high;
                    if tf.stats.damage > 180.0 {
                        s += 500.0;
                    }
                }
            } else {
                let dy = (loc_y(w, Some(ActorRef::Fighter(me))) - loc_y(w, Some(ActorRef::Fighter(t)))).abs();
                s = if dy < 500.0 {
                    300.0
                } else if dy < 1200.0 {
                    200.0
                } else if dy < 2500.0 {
                    100.0
                } else {
                    1.0
                };
                if w.fighter(me).attackers.most_recent_attacker == Some(t) {
                    s += 500.0;
                }
                // A human at 0 damage scores 1000, anyone else their damage.
                s += if tf.stats.damage > 0.0 || tf.controls.bot_controlled { tf.stats.damage } else { 1000.0 };
                if enemy == Some(t) {
                    s += (self.chosen_time - now) * 30.0 + 200.0;
                }
            }
            targets.push(t);
            scores.push(s);
            total += s;
        }
        let roll = cx.bot.rfir(0.0, total);
        for (i, &t) in targets.iter().enumerate() {
            if roll - scores[i] < 0.0 {
                self.chosen_time = now;
                let k = cx.key("EnemyActor");
                cx.set_obj(k, Some(ActorRef::Fighter(t)));
                break;
            }
        }
    }
}
