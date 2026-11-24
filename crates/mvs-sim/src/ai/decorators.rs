//! The bot Blueprint decorators: the BTDecorator_BlueprintBase classes under Panda_Main/Blueprints/AI and the
//! characters' AI folders, ported from their bytecode.
//!
//! A decorator's test is PerformConditionCheckAI before bInverseCondition; the runtime handles the inversion and the
//! abort mode. Coordinates as in `ai/util.rs`: world Y = -x, Z = y; fighters and projectiles stand at world X = 0
//! (the spawn points' X).
//!
//! Decoded:
//! - traces use TraceTypeQuery4 = ProjectileGroundTrace (GameTraceChannel6, default Ignore). The Floor / BlockAll /
//!   Platform profiles block it; the fighter capsule (Pawn profile, no response set for it) does not. The stage
//!   block (`World::stage.ground`, profile Floor) is the map's only blocker: the sandwich projectile (capsule
//!   ProjectileNavNoPlatform, hurt box ProjectileDestructible) and the Item_Sandwich (profile Item) set no response
//!   for the channel either. The type numbering fits every trace of the build: 2 in ValidateJumpStart /
//!   ValidateWallSkate / BPI_IsOnWall (WallJump), 3 in the ground checks, 4 in pfgDoDamage (ProjectileExplode);
//! - one blast box: the map's single BlastBoxPlayerExit actor registers in PandaGameState BlastBoxes;
//! - GetInputEnabled stays true: the Lab runs TrainGameMode (a PandaGameMode), whose ring-out path does not call
//!   BPI_DisableInput for a player character (the intro, victory and debug camera callers are not in the port);
//! - the persona key: the build's training modes and custom game pass a literal None to MatchPlayerData 'Set Is
//!   Bot', so PersonaTimedChance never loads a chance there; Deviation: the port gives every bot Default
//!   (`BtBot::lab_bot`), so each node loads Default's ActionMap value.
//!
//! Scope:
//! - teams: BotDetectEnemy, FindEnemyInRange and BotThrowItemAtAnyEnemy compare team indices;
//! - the Lab is game mode type 3: in the build BotUpdateDifficulty leaves NumericDifficulty alone (the port sets a
//!   fight bot's to its level: BotUpdateDifficulty_BTS), the match timer is not started and KillPlayerFighter skips
//!   HandleWinLoss, so MatchOver stays false;
//! - classes of other characters and items (C020 blaster, abilities) do not exist in the port: their checks read
//!   false.

#![allow(clippy::neg_cmp_op_on_partial_ord, reason = "the comparison is `!(a > b)`: NaN takes the negated branch")]

use crate::Real;
use crate::actors::ActorRef;
use crate::actors::world_actor::{actor_cls, actor_dead};
use crate::ai::cx::{Cx, Dec, Res};
use crate::ai::props;
use crate::ai::sweep::trace_sweep;
use crate::ai::util::{actor, corners, facing_left, fighter, loc, loc_y, loc_z, num, player_team, sq, valid, vec, vel};
use crate::consts::DT;
use crate::fighter::FighterId;
use crate::fighter::body::{ch_cap_half, ch_cap_radius, def};
use crate::fighter::common::warn_once;
use crate::math::{self, max, min, ue};
use crate::name;
use crate::names::Name;
use crate::world::World;
use mvs_data::value::{Props, Value};

// ---- the node's reads

/// `selfLoc`: the controlled pawn's location.
fn self_loc(cx: &Cx) -> crate::ai::blackboard::BbVec {
    loc(cx.w, ActorRef::Fighter(cx.me))
}

/// `p[k] ?? 0`.
fn pnum(cx: &Cx, k: &str) -> Real {
    props::num(cx.props, k).unwrap_or(0.0)
}

/// `bbFighter`: `cast<Base_Fighter>(GetBlackboardValueAsActor(key))`.
fn bb_fighter(cx: &Cx, prop: &str) -> Option<FighterId> {
    fighter(actor(cx, prop)).filter(|f| cx.w.fighters.get(*f).is_some())
}

/// `subject`: SelectObject(BB actor, pawn, IsValid(BB actor)) cast to the fighter.
fn subject(cx: &Cx, prop: &str) -> Option<FighterId> {
    let t = actor(cx, prop);
    if valid(cx.w, t) { fighter(t) } else { Some(cx.me) }
}

/// `f.dead`: frames until respawn, 0 alive.
fn is_dead(w: &World, f: FighterId) -> bool {
    w.fighter(f).life.dead != 0.0
}

/// `f.team`.
fn team(w: &World, f: FighterId) -> i32 {
    w.fighter(f).team.team
}

/// A struct member of an object property as a number: a member the object lacks reads `undefined`, NaN in the
/// arithmetic.
fn member(o: &Props, k: &str) -> Real {
    crate::math::narrow(o.get(k).and_then(Value::as_f64).unwrap_or(f64::from(Real::NAN)))
}

/// `p[k] ?? default` for an object property, whole: `None` for a missing one.
fn obj<'a>(cx: &Cx<'a>, k: &str) -> Option<&'a Props> {
    props::get(cx.props, k).and_then(Value::as_obj)
}

/// BPI_GetInputEnabled: InputDisabledCounter == 0. Nothing in the port's scope raises the counter: BaseFighter's kill
/// path does it for NotPlayerCharacter only, and PandaGameMode / TrainGameMode never do (the handler that does
/// belongs to TrainingGameMode, which the Lab does not use); the other callers are the intro, the victory / defeat
/// animations and the debug camera.
fn input_enabled(_w: &World, _f: FighterId) -> bool {
    true
}

/// `sweep(c, y0, z0, y1, z1, r, skip)`: the ground sweep against the stage and the spawned boxes.
fn sweep(
    cx: &mut Cx,
    y0: Real,
    z0: Real,
    y1: Real,
    z1: Real,
    r: Real,
    skip: Option<FighterId>,
) -> Option<crate::ai::sweep::SweepHit> {
    trace_sweep(cx.w, y0, z0, y1, z1, r, skip)
}

/// A graph's cast of a spawned actor to a Blueprint class: its class or a parent of it in the data
/// A class the data has no chain for warns and takes no cast.
pub fn cast_to(w: &World, a: ActorRef, cls: Name, parent: Name) -> bool {
    let owner = crate::fighter::common::lookup_owner(w, a);
    match crate::fighter::common::actor_is_a(w, ActorRef::Fighter(owner), cls, parent) {
        Some(r) => r,
        None => {
            warn_once(w, name!("bots: a class has no class parents in the data: no cast takes it"));
            false
        }
    }
}

// ---- difficulty, chance, time

/// NotEasy_BTD_C. A stock controller (`BtBot::stock`): the cast<Base_AIController_C> fails and
/// these three return false.
#[derive(Clone, Debug, Default)]
pub struct NotEasy;

impl Dec for NotEasy {
    fn test(&mut self, cx: &mut Cx) -> bool {
        !cx.bot.stock && cx.bot.difficulty > 2.1
    }
}

/// NotVeryEasy_BTD_C: true for VeryEasy only (the name is the inverse of the check).
#[derive(Clone, Debug, Default)]
pub struct NotVeryEasy;

impl Dec for NotVeryEasy {
    fn test(&mut self, cx: &mut Cx) -> bool {
        !cx.bot.stock && cx.bot.difficulty < 0.9
    }
}

/// BotDifficulty_BTD_C.
#[derive(Clone, Debug, Default)]
pub struct BotDifficulty;

impl Dec for BotDifficulty {
    fn test(&mut self, cx: &mut Cx) -> bool {
        if cx.bot.stock {
            return false;
        }
        let nd = cx.bot.difficulty;
        let p = cx.props;
        if nd > 0.0 {
            let k = if nd < 0.1 {
                "VeryEasy"
            } else if nd < 1.1 {
                "Easy"
            } else if nd < 2.1 {
                "Medium"
            } else {
                "Hard"
            };
            return props::is_true(p, k);
        }
        // NumericDifficulty 0: by the controller's BotDifficulty (EBotBehavior); the node's bool of that name.
        props::is_true(p, cx.bot.behavior.split("::").nth(1).unwrap_or(""))
    }
}

/// DifficultyCooldown_C: ReadyTime is the bare cooldown, compared with the world time (not
/// now + cooldown).
#[derive(Clone, Debug, Default)]
pub struct DifficultyCooldown {
    ready_time: Real,
}

impl Dec for DifficultyCooldown {
    fn test(&mut self, cx: &mut Cx) -> bool {
        self.ready_time < cx.bot.now
    }

    fn finish(&mut self, cx: &mut Cx, res: Res) {
        let p = cx.props;
        if res != Res::Succeeded && !(props::truthy(p, "AllowAborted") && res == Res::Aborted) {
            return;
        }
        if cx.bot.stock {
            return; // the cast fails: no ReadyTime write
        }
        let t = [pnum(cx, "VeryEasy"), pnum(cx, "Easy"), pnum(cx, "Medium"), pnum(cx, "Hard")];
        let nd = cx.bot.difficulty;
        // t[i]: an index past the table reads undefined, 0
        let at = |i: Real| [0.0, 1.0, 2.0, 3.0].iter().position(|&x| x == i).map_or(0.0, |k| t[k]);
        let (lo, hi) = (at(nd.floor()), at(nd.ceil()));
        self.ready_time = lo + (hi - lo) * (nd - nd.floor());
    }
}

/// `timedChance`: a false roll is kept for Duration +- variation; after a true window one evaluation reads false and
/// NextCheckTime becomes the bare Cooldown value.
#[derive(Clone, Debug, Default)]
struct Timed {
    next_check_time: Real,
    cached_result: bool,
}

impl Timed {
    fn roll(&mut self, cx: &mut Cx, chance: Real) -> bool {
        if !(cx.bot.now > self.next_check_time) {
            return self.cached_result;
        }
        if self.cached_result {
            self.next_check_time = pnum(cx, "Cooldown");
            self.cached_result = false;
            return false;
        }
        let dv = pnum(cx, "DurationVariation");
        let dur = pnum(cx, "Duration");
        let r = cx.bot.rfir(-dv, dv);
        self.next_check_time = (cx.bot.now + dur) + r;
        self.cached_result = chance > cx.bot.rand();
        self.cached_result
    }
}

/// TimedChance_BTD_C.
#[derive(Clone, Debug, Default)]
pub struct TimedChance {
    t: Timed,
}

impl Dec for TimedChance {
    fn test(&mut self, cx: &mut Cx) -> bool {
        let chance = pnum(cx, "Chance");
        self.t.roll(cx, chance)
    }
}

/// PersonaTimedChance_BTD_C. PerformConditionCheckAI: Chance is loaded when the persona key
/// differs from InitializedPersonaKey (None at first): the key's PersonaDataTable row (@384), then the Default row
/// (@757); an action in neither reads 0 (@1130). Chance is ActionMap value * 0.01 and the key is kept (@1079). With
/// key None nothing is loaded and the node's own Chance is rolled. Only the Default row is bound
/// (`BotTrees::persona`) and every port bot carries Default; another key would take Default's value without its own
/// row first, so it warns.
#[derive(Clone, Debug, Default)]
pub struct PersonaTimedChance {
    t: Timed,
    chance: Option<Real>,
    initialized_persona_key: Option<&'static str>,
}

impl Dec for PersonaTimedChance {
    fn test(&mut self, cx: &mut Cx) -> bool {
        let key = cx.bot.persona_key;
        if key != self.initialized_persona_key.unwrap_or("None") {
            if key != "Default" {
                warn_once(
                    cx.w,
                    name!("PersonaTimedChance: a persona other than Default has no row in the data, Default read"),
                );
            }
            let action = props::str(cx.props, "PersonaAction").and_then(|s| s.split("::").nth(1));
            let v = action.and_then(|a| cx.bot.data.persona.get(a).copied()).unwrap_or(0.0);
            self.chance = Some(crate::math::narrow(v) * 0.01); // Multiply_FloatFloat
            self.initialized_persona_key = Some(key);
        }
        let chance = self.chance.unwrap_or_else(|| pnum(cx, "Chance"));
        self.t.roll(cx, chance)
    }
}

/// BotIsFleeing_BTD_C.
#[derive(Clone, Debug, Default)]
pub struct BotIsFleeing;

impl Dec for BotIsFleeing {
    fn test(&mut self, cx: &mut Cx) -> bool {
        num(cx, "FleeTime") > cx.bot.now
    }
}

/// BotIsFleeSuppressed_BTD_C.
#[derive(Clone, Debug, Default)]
pub struct BotIsFleeSuppressed;

impl Dec for BotIsFleeSuppressed {
    fn test(&mut self, cx: &mut Cx) -> bool {
        num(cx, "SuppressFleeTime") > cx.bot.now
    }
}

/// ShouldPause_BTD_C.
#[derive(Clone, Debug, Default)]
pub struct ShouldPause;

impl Dec for ShouldPause {
    fn test(&mut self, cx: &mut Cx) -> bool {
        num(cx, "PauseTimeBB") > 0.0 && crate::fighter::grounded(cx.w, cx.me)
    }
}

/// ActorOlderThan_BTD_C: GetGameTimeSinceCreation. A ring-out respawn resets the pawn
/// (PandaGameMode RespawnCharacter); only the Lab reset spawns new ones (`BtBot::fighter_born`).
// Unverified: a projectile no longer in its arena has no age here: 0 (the build's read of a destroyed actor is not
// decoded).
#[derive(Clone, Debug, Default)]
pub struct ActorOlderThan;

impl Dec for ActorOlderThan {
    fn test(&mut self, cx: &mut Cx) -> bool {
        let age = match actor(cx, "Actor") {
            Some(ActorRef::Projectile(id)) => cx.w.actors.projectiles.get(id).map_or(0.0, |p| crate::real(p.age) * DT),
            Some(_) => cx.bot.now - cx.bot.fighter_born,
            None => 0.0,
        };
        age > pnum(cx, "Age")
    }
}

/// IsMatchOver_BTD_C: Panda_Game_Mode MatchOver: set by HandleWinLoss and MatchEndImpl only,
/// which game mode type 3 does not reach.
#[derive(Clone, Debug, Default)]
pub struct IsMatchOver;

impl Dec for IsMatchOver {
    fn test(&mut self, cx: &mut Cx) -> bool {
        cx.w.flow.match_over
    }
}

// ---- fighter state

/// OnGround_BTD_C.
#[derive(Clone, Debug, Default)]
pub struct OnGround;

impl Dec for OnGround {
    fn test(&mut self, cx: &mut Cx) -> bool {
        bb_fighter(cx, "Target").is_some_and(|f| crate::fighter::grounded(cx.w, f))
    }
}

/// WallSliding_BTD_C.
#[derive(Clone, Debug, Default)]
pub struct WallSliding;

impl Dec for WallSliding {
    fn test(&mut self, cx: &mut Cx) -> bool {
        bb_fighter(cx, "Target").is_some_and(|f| cx.w.fighter(f).wall.wall_sliding)
    }
}

/// TargetIsAlive_BTD_C.
#[derive(Clone, Debug, Default)]
pub struct TargetIsAlive;

impl Dec for TargetIsAlive {
    fn test(&mut self, cx: &mut Cx) -> bool {
        bb_fighter(cx, "Target").is_some_and(|f| !is_dead(cx.w, f))
    }
}

/// TargetIsAttacking_BTD_C.
#[derive(Clone, Debug, Default)]
pub struct TargetIsAttacking;

impl Dec for TargetIsAttacking {
    fn test(&mut self, cx: &mut Cx) -> bool {
        bb_fighter(cx, "Target").is_some_and(|f| crate::fighter::is_attacking(cx.w, f))
    }
}

/// TargetHasBuff_BTD_C: PerformConditionCheckAI @225-373: BPI_CheckIfHasBuff's HasBuff && its
/// stacks >= Stacks (the class default Stacks is 1, so a buff at 0 stacks fails unless the node sets Stacks 0).
#[derive(Clone, Debug, Default)]
pub struct TargetHasBuff;

impl Dec for TargetHasBuff {
    fn test(&mut self, cx: &mut Cx) -> bool {
        let Some(f) = bb_fighter(cx, "Target") else { return false };
        let Some(b) = props::str(cx.props, "Buff").filter(|b| !b.is_empty()) else { return false };
        // (a class no name of the match holds is on no fighter)
        let Some(cls) = cx.w.names.get(b) else { return false };
        crate::combat::buffs::has_buff(cx.w, f, cls)
            && crate::combat::buffs::buff_stacks(cx.w, f, b) >= pnum(cx, "Stacks")
    }
}

/// DamageRequired_BTD_C.
#[derive(Clone, Debug, Default)]
pub struct DamageRequired;

impl Dec for DamageRequired {
    fn test(&mut self, cx: &mut Cx) -> bool {
        bb_fighter(cx, "Target").is_some_and(|f| cx.w.fighter(f).stats.damage >= pnum(cx, "DamageRequired"))
    }
}

/// BotDamageGreaterThan_BTD_C: BotDamageGreaterThan_BTD (WonderWoman_LassoAI): the Target
/// key's actor cast to Base_Fighter (@0-95; a failed cast leaves ReturnValue unwritten: false, @210) ->
/// Comp_Actor_Damage.CurrentDamage > Damage (@109-191), strictly. Damage has no class default in the export (0); the
/// tree's one node sets 90.
#[derive(Clone, Debug, Default)]
pub struct BotDamageGreaterThan;

impl Dec for BotDamageGreaterThan {
    fn test(&mut self, cx: &mut Cx) -> bool {
        bb_fighter(cx, "Target").is_some_and(|f| cx.w.fighter(f).stats.damage > pnum(cx, "Damage"))
    }
}

/// BotIsEvading_BTD_C.
#[derive(Clone, Debug, Default)]
pub struct BotIsEvading;

impl Dec for BotIsEvading {
    fn test(&mut self, cx: &mut Cx) -> bool {
        bb_fighter(cx, "Actor").is_some_and(|f| crate::fighter::is_evading(cx.w, f))
    }
}

/// BotIsFalling_BTD_C.
#[derive(Clone, Debug, Default)]
pub struct BotIsFalling;

impl Dec for BotIsFalling {
    fn test(&mut self, cx: &mut Cx) -> bool {
        bb_fighter(cx, "Target").is_some_and(|f| !crate::fighter::grounded(cx.w, f) && cx.w.fighter(f).body.vy < 0.0)
    }
}

/// BotHasEvade_BTD_C: on the ground an evade is always there; in the air while
/// AirEvadeNumber < AirEvadeMax.
#[derive(Clone, Debug, Default)]
pub struct BotHasEvade;

impl Dec for BotHasEvade {
    fn test(&mut self, cx: &mut Cx) -> bool {
        let Some(f) = bb_fighter(cx, "TargetActor") else { return false };
        if !input_enabled(cx.w, f) || crate::fighter::is_evading(cx.w, f) {
            return false;
        }
        crate::fighter::grounded(cx.w, f)
            || crate::real(cx.w.fighter(f).air.air_evades) < crate::world::own_rules::air_evade_max(cx.w)
    }
}

/// BotHasAllEvades_BTD_C.
#[derive(Clone, Debug, Default)]
pub struct BotHasAllEvades;

impl Dec for BotHasAllEvades {
    fn test(&mut self, cx: &mut Cx) -> bool {
        let Some(f) = bb_fighter(cx, "TargetActor") else { return false };
        if !input_enabled(cx.w, f) || crate::fighter::is_evading(cx.w, f) {
            return false;
        }
        crate::fighter::grounded(cx.w, f) || cx.w.fighter(f).air.air_evades == 0
    }
}

/// BotHasAttack_BTD_C.
#[derive(Clone, Debug, Default)]
pub struct BotHasAttack;

impl Dec for BotHasAttack {
    fn test(&mut self, cx: &mut Cx) -> bool {
        let f = bb_fighter(cx, "TargetActor").unwrap_or(cx.me);
        let w = &*cx.w;
        if !input_enabled(w, f) || crate::fighter::in_hitstun(w, f) || crate::fighter::is_attacking(w, f) {
            return false;
        }
        crate::fighter::grounded(w, f)
            || crate::real(w.fighter(f).air.air_specials) < crate::fighter::air_special_max(w, f)
    }
}

/// BotHasJump_BTD_C: in the air: CurrentJumpNumber < 2 (the literal, not MaxJumpNumber).
#[derive(Clone, Debug, Default)]
pub struct BotHasJump;

impl Dec for BotHasJump {
    fn test(&mut self, cx: &mut Cx) -> bool {
        let Some(f) = bb_fighter(cx, "TargetActor") else { return false };
        let w = &*cx.w;
        if !input_enabled(w, f) || crate::fighter::in_hitstun(w, f) || crate::fighter::is_jumping(w, f) {
            return false;
        }
        crate::fighter::grounded(w, f) || w.fighter(f).jump.jumps < 2
    }
}

/// BotNotHitstunned_BTD_C: reads the controlled pawn, not its key.
#[derive(Clone, Debug, Default)]
pub struct BotNotHitstunned;

impl Dec for BotNotHitstunned {
    fn test(&mut self, cx: &mut Cx) -> bool {
        !crate::fighter::in_hitstun(cx.w, cx.me)
    }
}

/// BotRecentlyHitstunned_BTD_C.
#[derive(Clone, Debug, Default)]
pub struct BotRecentlyHitstunned;

impl Dec for BotRecentlyHitstunned {
    fn test(&mut self, cx: &mut Cx) -> bool {
        crate::fighter::recently_hitstunned(cx.w, cx.me)
    }
}

/// BotNeedsToStruggle_BTD_C: BotNeedsToStruggle_BTD (@79-130):
/// Comp_Pawn_HitstunHandler.HitstunBuffActive of the controlled pawn, set by AddBuffHitStun for a buff with a stun
/// duration (a hold: Jake's eat, Iron Giant's grab, ...): the port's `buff_stunned`.
#[derive(Clone, Debug, Default)]
pub struct BotNeedsToStruggle;

impl Dec for BotNeedsToStruggle {
    fn test(&mut self, cx: &mut Cx) -> bool {
        cx.w.fighter(cx.me).buffs.buff_stunned
    }
}

/// BotIsAbilityAvailable_BTD_C: Comp_Pawn_AbilityManager.HasAbilityResources(AbilityIndex) of
/// the controlled pawn (@0-105). On nodes of C019's trees (CallUFO, GrenadeAI, RewindAI), WonderWoman_BuffAI /
/// LassoAI, Harley_JackInBoxAI and Reindog_VictoryAI.
#[derive(Clone, Debug, Default)]
pub struct BotIsAbilityAvailable;

impl Dec for BotIsAbilityAvailable {
    fn test(&mut self, cx: &mut Cx) -> bool {
        let slot = math::to_int32(pnum(cx, "AbilityIndex"));
        crate::fighter::control::has_ability_resources(cx.w, cx.me, slot)
    }
}

/// ShaggyEnraged_BTD_C.
#[derive(Clone, Debug, Default)]
pub struct ShaggyEnraged;

impl Dec for ShaggyEnraged {
    fn test(&mut self, cx: &mut Cx) -> bool {
        cx.w.fighter(cx.me).rage.rage_enabled
    }
}

/// ShaggyHasSandwich_BTD_C.
#[derive(Clone, Debug, Default)]
pub struct ShaggyHasSandwich;

impl Dec for ShaggyHasSandwich {
    fn test(&mut self, cx: &mut Cx) -> bool {
        crate::chars::shaggy::state::shaggy_of(cx.w, cx.me).is_some() && crate::chars::shaggy::state::rock(cx.w, cx.me)
    }
}

/// C016_BallEnabled_BTD_C: PerformConditionCheckAI @0-336: the controlled pawn as a
/// BP_FighterCharacter, its Comp_State_Manager.CheckHasBuff(Buff_C016_Ball_Conditional_C).HasBuff: the buff, not the
/// pawn's HasBall copy. On nodes of C016_Ball and his other three trees.
#[derive(Clone, Debug, Default)]
pub struct C016BallEnabled;

impl Dec for C016BallEnabled {
    fn test(&mut self, cx: &mut Cx) -> bool {
        crate::chars::lebron::state::has_ball_buff(cx.w, cx.me)
    }
}

/// ShaggyIsChargingRage_BTD_C: true on an evaluation where the charge has grown since the
/// previous one (and rage is not on); an evaluation without growth resets the stored charge.
#[derive(Clone, Debug, Default)]
pub struct ShaggyIsChargingRage {
    last_rage_charge: Real,
}

impl Dec for ShaggyIsChargingRage {
    fn test(&mut self, cx: &mut Cx) -> bool {
        let Some(t) = bb_fighter(cx, "Target") else { return false };
        let r = &cx.w.fighter(t).rage;
        if self.last_rage_charge < r.rage {
            self.last_rage_charge = r.rage;
            return !r.rage_enabled && r.rage > 0.0;
        }
        self.last_rage_charge = 0.0;
        false
    }
}

// Iron Giant's decorators (Characters/C017/AI), each PerformConditionCheckAI on the controlled pawn. A pawn that is
// not Pawn_C017 (no Comp_C017_Hover_Rage: `giant_of`) reads false in all four.

/// C017_HasFuel_BTD_C: GetComponentByClass(Comp_C017_Hover_Rage) valid (@0-80, else false
/// @297) -> FuelAmount >= FuelFraction x FuelMaxAmount (@145-273). FuelFraction is a fraction of FuelMaxAmount (100),
/// class default 0 (always true with the component); C017_WallSide's three nodes set 40, which asks for 4000 of 100
/// and never passes: the build's own values.
#[derive(Clone, Debug, Default)]
pub struct C017HasFuel;

impl Dec for C017HasFuel {
    fn test(&mut self, cx: &mut Cx) -> bool {
        let fraction = pnum(cx, "FuelFraction");
        crate::chars::iron_giant::state::giant_of(cx.w, cx.me)
            .and_then(|g| g.hover_rage.as_ref())
            .is_some_and(|h| h.has_fuel(fraction))
    }
}

/// C017_IsHoldingEnemy_BTD_C: cast to BPI_C017 (@0-65; a failed cast tests a null buff, @184
/// -> 125) -> IsValid(BPI_GetCurrentHoldBuff) (@79-154): true @168, false @200. `Fighter.holdBuff`, as
/// C017_HoldingEnemy_Condition.
#[derive(Clone, Debug, Default)]
pub struct C017IsHoldingEnemy;

impl Dec for C017IsHoldingEnemy {
    fn test(&mut self, cx: &mut Cx) -> bool {
        cx.w.fighter(cx.me).holds.hold_buff.is_some()
    }
}

/// C017_IsMaxRage_BTD_C: cast to Pawn_C017 (@0-65; a failed cast leaves ReturnValue unwritten:
/// false) -> IsMaxRage (@79).
#[derive(Clone, Debug, Default)]
pub struct C017IsMaxRage;

impl Dec for C017IsMaxRage {
    fn test(&mut self, cx: &mut Cx) -> bool {
        crate::chars::iron_giant::state::giant_of(cx.w, cx.me).is_some_and(|g| g.is_max_rage)
    }
}

/// C017_WeaponMode_BTD_C: cast to Pawn_C017 (@0-65, else false @125) -> AlternateMode (@79).
#[derive(Clone, Debug, Default)]
pub struct C017WeaponMode;

impl Dec for C017WeaponMode {
    fn test(&mut self, cx: &mut Cx) -> bool {
        crate::chars::iron_giant::state::giant_of(cx.w, cx.me).is_some_and(|g| g.alternate_mode)
    }
}

/// Arya_CanTeleport_BTD_C: PerformConditionCheckAI. One node, Arya_TeleportAI's second branch;
/// it serialises no property (no inverse, no abort mode, no observer), so it is asked only when the search reaches
/// the branch. A pawn that is no Base_Fighter leaves ReturnValue unwritten (@70: false; every pawn of the port is
/// one).
/// @156-810: Projectile, a member of the node, is written with the first SpawnedActors entry of the class
/// (`chars::arya::knife::listed_knife`) and keeps its last value when the list holds none.
/// @811-840 IsValid(Projectile), else false. @854-1016 VSizeSquared(GetVelocity) < Square(100), strict, in single
/// precision. @1030-1220 the return value of LineTraceSingle(its location -> the world origin (0,0,0),
/// TraceTypeQuery4 = ProjectileGroundTrace: `sweep`, no actor ignored; the knife's Capsule does not answer the
/// channel): on the port's five stages the origin is inside the floor box, so a knife outside it always hits.
/// Nothing else of the knife is read (not Attached, AttachedToEnemy, isTeleporting or a distance).
/// GetVelocity of a knife whose movement is stopped and that is attached (to a fighter or to the world) is zero, the
/// port's vx / vy after StopSimulating. A trace that starts inside the floor box is a hit at its start, as sweepBox
/// answers. A destroyed knife leaves SpawnedActors inside its own destroy: `listed_knife` passes over a dead entry.
#[derive(Clone, Debug, Default)]
pub struct AryaCanTeleport {
    projectile: Option<ActorRef>,
}

impl Dec for AryaCanTeleport {
    fn test(&mut self, cx: &mut Cx) -> bool {
        if let Some(k) = crate::chars::arya::knife::listed_knife(cx.w, cx.me) {
            self.projectile = Some(ActorRef::Projectile(k));
        }
        let p = self.projectile;
        if !valid(cx.w, p) {
            return false;
        }
        let (vy, vz) = vel(cx.w, p);
        if !(((vy * vy) + (vz * vz)) < 10000.0) {
            return false;
        }
        let (y, z) = (loc_y(cx.w, p), loc_z(cx.w, p));
        sweep(cx, y, z, 0.0, 0.0, 0.0, None).is_some()
    }
}

/// BTD_BatmanLostBatarang_C: PerformConditionCheckAI.
/// One node, Batman_MoveToTargetAI (Result Actor = Projectile, FlowAbortMode Self, observing Projectile). The pawn's
/// Comp_Spawner is always there (BaseFighter component: the IsValid branch @85 is left out).
/// The first SpawnedActors entry whose GetObjectClass is exactly Item_BatarangV2_C (@493-560, no cast) stops the
/// loop: SetBlackboardValueAsObject(Result Actor, it) @702 before the trace, then the result of LineTraceSingle(its
/// actor location -> 3000 below, TraceTypeQuery4 = ProjectileGroundTrace, no actor ignored) @863-1099: `sweep` (the
/// batarang's own Capsule and hurt sphere do not answer the channel). Nothing else is read: flying or lying, the
/// thrower's or picked up. No entry: false @1139 and the key is left as it was. A destroyed batarang leaves the list
/// inside its own destroy (Comp_Spawner, as Arya's knife): a dead entry is passed over.
// Unverified: the decorator writes the key it observes; the port's Blackboard notifies its observers at once on a
// change (`bb_changed` re-enters this test, which writes the same value and so ends there). Whether the build's
// SetValue notifies synchronously from inside PerformConditionCheckAI is not read.
#[derive(Clone, Debug, Default)]
pub struct BatmanLostBatarang;

impl Dec for BatmanLostBatarang {
    fn test(&mut self, cx: &mut Cx) -> bool {
        let Some(b) = spawned_of_class(cx.w, cx.me, name!("Item_BatarangV2_C")) else { return false };
        let k = cx.key("Result Actor");
        cx.set_obj(k, Some(b));
        let (y, z) = (loc_y(cx.w, Some(b)), loc_z(cx.w, Some(b)));
        sweep(cx, y, z, y, z - 3000.0, 0.0, None).is_some()
    }
}

/// The first live entry of the fighter's SpawnedActors whose class is exactly `cls` (GetObjectClass ==, no cast).
fn spawned_of_class(w: &World, f: FighterId, cls: Name) -> Option<ActorRef> {
    w.fighter(f).items.spawned_actors.iter().copied().find(|&a| !actor_dead(w, a) && actor_cls(w, a) == Some(cls))
}

// ---- Harley's bomb checks (Harley_BatBombAI): no observer, no abort mode, no tick

/// HarleyHasBomb_BTD_C: PerformConditionCheckAI @94-258: Comp_Spawner.IsEquippedComponents
/// contains 'SatchelCharge'.
#[derive(Clone, Debug, Default)]
pub struct HarleyHasBomb;

impl Dec for HarleyHasBomb {
    fn test(&mut self, cx: &mut Cx) -> bool {
        crate::fighter::has_equipped(cx.w, cx.me, name!("satchelcharge"))
    }
}

/// HarleyThrownBomb_BTD_C: PerformConditionCheckAI @99-692: any SpawnedActors entry whose class
/// is exactly HarleyBomb_C (@493-560, no cast).
#[derive(Clone, Debug, Default)]
pub struct HarleyThrownBomb;

impl Dec for HarleyThrownBomb {
    fn test(&mut self, cx: &mut Cx) -> bool {
        spawned_of_class(cx.w, cx.me, name!("HarleyBomb_C")).is_some()
    }
}

/// HarleyBombAttachedToEnemy_BTD_C: as compiled the loop over SpawnedActors never reaches its
/// second entry (a failed cast @658 and IsAttached false @672 both go to @971, an attached bomb returns at @860), so
/// only the oldest live spawned actor is looked at: a HarleyBomb with IsAttached whose AttachedTo's TeamIndex differs
/// from the pawn's (@708-841); AttachedTo None reads TeamIndex 0.
#[derive(Clone, Debug, Default)]
pub struct HarleyBombAttachedToEnemy;

impl Dec for HarleyBombAttachedToEnemy {
    fn test(&mut self, cx: &mut Cx) -> bool {
        // The bomb's state value for attached (`chars::harley::bomb`'s private ATTACHED).
        const ATTACHED: i32 = 2;
        let w = &*cx.w;
        let Some(first) = w.fighter(cx.me).items.spawned_actors.iter().copied().find(|&a| !actor_dead(w, a)) else {
            return false;
        };
        let ActorRef::Actor(id) = first else { return false };
        let Some(a) = w.actors.actors.get(id) else { return false };
        if a.cls != name!("HarleyBomb_C") {
            return false;
        }
        let crate::actors::WorldActorState::HarleyBomb(b) = &a.state else { return false };
        if b.state != ATTACHED {
            return false;
        }
        b.carrier.map_or(0, |c| team(w, c)) != team(w, cx.me)
    }
}

// ---- classes the port does not have, and the map's

/// OnPlatform_BTD_C: the target fighter walks on a floor of object type Platform (a stage's
/// pass-through platform or a piece of Morty's ship): `on_platform_floor`.
#[derive(Clone, Debug, Default)]
pub struct OnPlatform;

impl Dec for OnPlatform {
    fn test(&mut self, cx: &mut Cx) -> bool {
        let t = actor(cx, "Target");
        let f = if valid(cx.w, t) { fighter(t) } else { None };
        f.is_some_and(|f| crate::fighter::body::on_platform_floor(cx.w, f))
    }
}

/// ShouldClimbWall_BTD_C: Map_ScoobyDoo only climbs below Z 200; every other map: true.
#[derive(Clone, Debug, Default)]
pub struct ShouldClimbWall;

impl Dec for ShouldClimbWall {
    fn test(&mut self, _cx: &mut Cx) -> bool {
        true
    }
}

/// IsBotInNeverRecoverZone_C: true while RecoverZoneUntil > now (@73); true when the map has
/// no BotRecoveryZone_C (@196); else true while the pawn overlaps one, which sets RecoverZoneUntil to now + 3 (@594);
/// false otherwise (@753).
#[derive(Clone, Debug, Default)]
pub struct IsBotInNeverRecoverZone {
    recover_zone_until: Real,
}

impl Dec for IsBotInNeverRecoverZone {
    fn test(&mut self, cx: &mut Cx) -> bool {
        if self.recover_zone_until > cx.bot.now {
            return true;
        }
        let zones = cx.w.data.stage.bot_zones.as_deref().unwrap_or(&[]);
        if zones.is_empty() {
            return true;
        }
        if zones.iter().any(|z| in_zone(cx.w, cx.me, z)) {
            self.recover_zone_until = cx.bot.now + 3.0;
            return true;
        }
        false
    }
}

/// Zone.IsOverlappingActor(SelfFighter) @504: the pawn's root capsule against the zone's box, volumetric. The pawn
/// stands at world X 0, inside each zone's depth.
// Unverified: a rotated zone is taken unrotated (none is: the three of Map_ScoobyDoo have no rotation).
fn in_zone(w: &World, f: FighterId, z: &mvs_data::stage::BotZone) -> bool {
    let (cy, cz) = (z.center[1], z.center[2]);
    let (hy, hz) = (z.half_extent[1], z.half_extent[2]);
    let r = crate::fighter::cap_radius(w, f);
    let a = max(0.0, crate::fighter::capsule_half(w, f) - r);
    let fr = Some(ActorRef::Fighter(f));
    let dx = max(0.0, crate::math::narrow(z.center[0].abs() - z.half_extent[0]));
    let dy = max(0.0, (loc_y(w, fr) - crate::math::narrow(cy)).abs() - crate::math::narrow(hy));
    let dz = max(0.0, (loc_z(w, fr) - crate::math::narrow(cz)).abs() - crate::math::narrow(hz) - a);
    dx * dx + dy * dy + dz * dz <= r * r
}

/// CanCrouchUnderProjectile_BTD_C: the cast to C020_Blaster_Projectile_Parent: the class is not
/// in the port.
#[derive(Clone, Debug, Default)]
pub struct CanCrouchUnderProjectile;

impl Dec for CanCrouchUnderProjectile {
    fn test(&mut self, _cx: &mut Cx) -> bool {
        false
    }
}

/// IsHoldingMeeseeksBox_BTD_C: the HeldObject key's actor cast to Item_Meeseek @0-109.
#[derive(Clone, Debug, Default)]
pub struct IsHoldingMeeseeksBox;

impl Dec for IsHoldingMeeseeksBox {
    fn test(&mut self, cx: &mut Cx) -> bool {
        match actor(cx, "HeldObject") {
            Some(ActorRef::Item(id)) => cx.w.actors.items.get(id).is_some_and(|i| i.cls == name!("Item_Meeseek_C")),
            _ => false,
        }
    }
}

/// IsBugsPortal_BTD_C: GetObjectClass(the Portal key's actor) == BugsBunnyTunnelParent_C,
/// exact @0-97; an empty key is false.
#[derive(Clone, Debug, Default)]
pub struct IsBugsPortal;

impl Dec for IsBugsPortal {
    fn test(&mut self, cx: &mut Cx) -> bool {
        actor(cx, "Portal").and_then(|a| actor_cls(cx.w, a)) == Some(name!("BugsBunnyTunnelParent_C"))
    }
}

/// Bugs_IsTunnelingBTD_C: BPI_CheckIfHasBuff(Buff_Tunneling) @79-221.
#[derive(Clone, Debug, Default)]
pub struct BugsIsTunneling;

impl Dec for BugsIsTunneling {
    fn test(&mut self, cx: &mut Cx) -> bool {
        crate::combat::buffs::has_buff(cx.w, cx.me, name!("Buff_Tunneling_C"))
    }
}

/// Bugs_PieEquipped_BTD_C @94-409: Comp_Spawner.IsEquippedComponents contains PieProp1 or Pie.
#[derive(Clone, Debug, Default)]
pub struct BugsPieEquipped;

impl Dec for BugsPieEquipped {
    fn test(&mut self, cx: &mut Cx) -> bool {
        crate::fighter::has_equipped(cx.w, cx.me, name!("pie"))
            || crate::fighter::has_equipped(cx.w, cx.me, name!("pieprop1"))
    }
}

/// HorizontalSpeedBelow_BTD_C @0-175: |GetVelocity().Y| < Speed, strict.
#[derive(Clone, Debug, Default)]
pub struct HorizontalSpeedBelow;

impl Dec for HorizontalSpeedBelow {
    fn test(&mut self, cx: &mut Cx) -> bool {
        let (vy, _) = vel(cx.w, Some(ActorRef::Fighter(cx.me)));
        vy.abs() < pnum(cx, "Speed")
    }
}

/// OnBugsRocket_BTD_C @0-575: the Target key's fighter, walking, whose floor hit actor's class
/// is exactly BugsBunnyRocketFProjectile_C.
#[derive(Clone, Debug, Default)]
pub struct OnBugsRocket;

impl Dec for OnBugsRocket {
    fn test(&mut self, cx: &mut Cx) -> bool {
        let Some(f) = bb_fighter(cx, "Target") else { return false };
        if !crate::fighter::grounded(cx.w, f) {
            return false;
        }
        match crate::fighter::body::floor_actor(cx.w, f) {
            Some(crate::fighter::body::PlatformRef::Spawned(a, _)) => {
                !actor_dead(cx.w, a) && actor_cls(cx.w, a) == Some(name!("BugsBunnyRocketFProjectile_C"))
            }
            _ => false,
        }
    }
}

// ---- Reindog

/// ReindogUsingSpeedBoost_BTD_C: BPI_CheckIfHasBuff(ReindogSpeedBoost_C) @79 -> its Buff_Actor
/// cast to ReindogSpeedBoost @245 -> Active @324; no such buff: false @386.
// Unverified: Active read from the buff's definition props; the port may keep it elsewhere.
#[derive(Clone, Debug, Default)]
pub struct ReindogUsingSpeedBoost;

impl Dec for ReindogUsingSpeedBoost {
    fn test(&mut self, cx: &mut Cx) -> bool {
        let Some(b) = crate::combat::buffs::buff_of(cx.w, cx.me, name!("ReindogSpeedBoost_C")) else { return false };
        crate::combat::buffs::inst_def_of(cx.w, b).is_some_and(|d| matches!(d.get("Active"), Some(Value::Bool(true))))
    }
}

/// `getTetherInfo` of the controlled pawn: `None` for a pawn without the hook (the failed cast to BPI_Reindog).
fn tether_info(w: &World, f: FighterId) -> Option<crate::registry::char_hooks::TetherInfo> {
    w.fighter(f).hooks.char.get_tether_info.map(|h| h(w, f))
}

/// ReindogUsingTether_BTD_C: BPI_GetTetherInfo IsTetherOut @134; a failed cast: false.
#[derive(Clone, Debug, Default)]
pub struct ReindogUsingTether;

impl Dec for ReindogUsingTether {
    fn test(&mut self, cx: &mut Cx) -> bool {
        tether_info(cx.w, cx.me).is_some_and(|t| t.out)
    }
}

/// ReindogTetherAttachedToAlly_BTD_C: IsTetherAttachedToAlly.
#[derive(Clone, Debug, Default)]
pub struct ReindogTetherAttachedToAlly;

impl Dec for ReindogTetherAttachedToAlly {
    fn test(&mut self, cx: &mut Cx) -> bool {
        tether_info(cx.w, cx.me).is_some_and(|t| t.attached_to_ally)
    }
}

/// `allyItemAttached`: the first live Fastball_C of the Reindog key's fighter's SpawnedActors (exact class) and the
/// number of actors attached to it (the fighters it carries, `shapes.carried`); -1 with none (@1135-1298).
fn ally_item_attached(cx: &Cx) -> i32 {
    let Some(o) = fighter(actor(cx, "Reindog")) else { return -1 };
    let w = &*cx.w;
    let Some(ActorRef::Item(item)) = spawned_of_class(w, o, name!("Fastball_C")) else { return -1 };
    let n = w
        .all_ids()
        .into_iter()
        .filter(|&f| w.fighter(f).shapes.carried.as_ref().is_some_and(|c| c.item == item))
        .count();
    i32::try_from(n).expect("a count of fighters")
}

/// Reindog_AllyItemIsPickedUp_BTD_C: the Fastball has 1 attached actor.
#[derive(Clone, Debug, Default)]
pub struct ReindogAllyItemIsPickedUp;

impl Dec for ReindogAllyItemIsPickedUp {
    fn test(&mut self, cx: &mut Cx) -> bool {
        ally_item_attached(cx) == 1
    }
}

/// Reindog_AllyItemReadyToBePickedUp_BTD_C: the Fastball has none.
#[derive(Clone, Debug, Default)]
pub struct ReindogAllyItemReadyToBePickedUp;

impl Dec for ReindogAllyItemReadyToBePickedUp {
    fn test(&mut self, cx: &mut Cx) -> bool {
        ally_item_attached(cx) == 0
    }
}

// ---- ranges

/// `p.Offset ?? {0, 0, 0}`: the three members (a member the object lacks reads NaN).
fn offset3(cx: &Cx) -> (Real, Real, Real) {
    match obj(cx, "Offset") {
        None => (0.0, 0.0, 0.0),
        Some(o) => (member(o, "X"), member(o, "Y"), member(o, "Z")),
    }
}

/// TargetInAttackRange_BTD_C: a box in front of the bot against the target's capsule (the
/// template's OriginalCapsuleRadius / HalfHeight), now and at NumPredictions steps along its velocity.
#[derive(Clone, Debug, Default)]
pub struct TargetInAttackRange;

impl Dec for TargetInAttackRange {
    fn test(&mut self, cx: &mut Cx) -> bool {
        let Some(t) = bb_fighter(cx, "TargetBot") else { return false };
        let m = |k: &str, m: &str| obj(cx, k).map_or(0.0, |o| member(o, m));
        let (off_x, off_y, ext_x, ext_y) = (m("Offset", "X"), m("Offset", "Y"), m("Extents", "X"), m("Extents", "Y"));
        let w = &*cx.w;
        let me = Some(ActorRef::Fighter(cx.me));
        let cy = loc_y(w, me) + if facing_left(w, cx.me) { off_x } else { -off_x };
        let cz = loc_z(w, me) + off_y;
        let tr = Some(ActorRef::Fighter(t));
        let (mut y, mut z) = (loc_y(w, tr), loc_z(w, tr));
        let (cap_r, cap_h) = (ch_cap_radius(w, t), ch_cap_half(w, t));
        let hit = |y: Real, z: Real| (cy - y).abs() < ext_x + cap_r && (cz - z).abs() < ext_y + cap_r + cap_h;
        if hit(y, z) {
            return true;
        }
        let n = pnum(cx, "NumPredictions");
        let vs = pnum(cx, "VelocityScale");
        if n <= 0.0 {
            return false;
        }
        let (vy, vz) = vel(w, tr);
        let dy = (vy * vs) / n;
        let mut dz = (vz * vs) / n;
        let g = if crate::fighter::grounded(w, t) { 0.0 } else { (ue::size2d([vy * vs, vz * vs]) * -0.06) / n };
        let mut i = 0.0;
        while i < n {
            y += dy;
            z += dz;
            dz += g;
            if hit(y, z) {
                return true;
            }
            i += 1.0;
        }
        false
    }
}

/// TargetInRange_BTT_C: 3D distance to the target actor (+ Offset), from the capsule bottom
/// with FromCapsuleBottom (the template's half height, @293).
#[derive(Clone, Debug, Default)]
pub struct TargetInRange;

impl Dec for TargetInRange {
    fn test(&mut self, cx: &mut Cx) -> bool {
        let t = actor(cx, "Target");
        if !valid(cx.w, t) {
            return false;
        }
        let o = offset3(cx);
        let w = &*cx.w;
        let me = Some(ActorRef::Fighter(cx.me));
        let from_bottom = if props::is_true(cx.props, "FromCapsuleBottom") { ch_cap_half(w, cx.me) } else { 0.0 };
        let sz = loc_z(w, me) - from_bottom;
        sq(o.0) + sq(loc_y(w, me) - (loc_y(w, t) + o.1)) + sq(sz - (loc_z(w, t) + o.2)) < sq(pnum(cx, "Range"))
    }
}

/// TargetInRangeOfOther_BTT_C.
#[derive(Clone, Debug, Default)]
pub struct TargetInRangeOfOther;

impl Dec for TargetInRangeOfOther {
    fn test(&mut self, cx: &mut Cx) -> bool {
        let (t, o) = (actor(cx, "Target"), actor(cx, "Other"));
        if !valid(cx.w, t) || !valid(cx.w, o) {
            return false;
        }
        let off = offset3(cx);
        let w = &*cx.w;
        sq(off.0) + sq(loc_y(w, o) - (loc_y(w, t) + off.1)) + sq(loc_z(w, o) - (loc_z(w, t) + off.2))
            < sq(pnum(cx, "Range"))
    }
}

/// TargetInHorizontalRange_BTT_C @65-433: both the pawn and the Target key's actor are
/// fighters, else false; the full squared distance < Range squared (nothing in it is horizontal), single precision.
#[derive(Clone, Debug, Default)]
pub struct TargetInHorizontalRange;

impl Dec for TargetInHorizontalRange {
    fn test(&mut self, cx: &mut Cx) -> bool {
        let Some(t) = bb_fighter(cx, "Target") else { return false };
        let w = &*cx.w;
        let (me, tr) = (Some(ActorRef::Fighter(cx.me)), Some(ActorRef::Fighter(t)));
        let dy = loc_y(w, me) - loc_y(w, tr);
        let dz = loc_z(w, me) - loc_z(w, tr);
        let r = pnum(cx, "Range");
        ((dy * dy) + (dz * dz)) < (r * r)
    }
}

/// TargetInCone_BTD_C: VSizeSquared(d) > Range squared -> false; else
/// DegAtan(|d.Z| / |d.Y|) < Angle (@868-1155). The facing is not read. Single precision.
// Unverified: Divide_FloatFloat by 0 (d.Y == 0) is taken as stock 4.26 (returns 0, so DegAtan 0: in the cone), not
// read in the exe. DegAtan is taken as DegAcos / DegAsin are: the CRT atanf times RAD_TO_DEG.
#[derive(Clone, Debug, Default)]
pub struct TargetInCone;

impl Dec for TargetInCone {
    fn test(&mut self, cx: &mut Cx) -> bool {
        let t = actor(cx, "Target");
        if !valid(cx.w, t) {
            return false;
        }
        let w = &*cx.w;
        let me = Some(ActorRef::Fighter(cx.me));
        let dy = loc_y(w, t) - loc_y(w, me);
        let dz = loc_z(w, t) - loc_z(w, me);
        let r = pnum(cx, "Range");
        if ((dy * dy) + (dz * dz)) > (r * r) {
            return false;
        }
        let ay = dy.abs();
        let q = if ay == 0.0 { 0.0 } else { dz.abs() / ay };
        (ue::atanf(q) * ue::RAD_TO_DEG) < pnum(cx, "Angle")
    }
}

/// PointInRange_BTT_C: d = self - point; (d.X, |d.Y| - OriginalCapsuleRadius, |d.Z| -
/// OriginalCapsuleHalfHeight) inside Range.
#[derive(Clone, Debug, Default)]
pub struct PointInRange;

impl Dec for PointInRange {
    fn test(&mut self, cx: &mut Cx) -> bool {
        let t = vec(cx, "Target");
        let s = self_loc(cx);
        let w = &*cx.w;
        sq(s.x - t.x) + sq((s.y - t.y).abs() - ch_cap_radius(w, cx.me)) + sq((s.z - t.z).abs() - ch_cap_half(w, cx.me))
            < sq(pnum(cx, "Range"))
    }
}

/// TargetInBoxRange_BTD_C.
#[derive(Clone, Debug, Default)]
pub struct TargetInBoxRange;

impl Dec for TargetInBoxRange {
    fn test(&mut self, cx: &mut Cx) -> bool {
        let Some(t) = bb_fighter(cx, "Target") else { return false };
        let w = &*cx.w;
        let (me, tr) = (Some(ActorRef::Fighter(cx.me)), Some(ActorRef::Fighter(t)));
        (loc_y(w, me) - loc_y(w, tr)).abs() < pnum(cx, "HorizontalRange")
            && (loc_z(w, me) - loc_z(w, tr)).abs() < pnum(cx, "VerticalRange")
    }
}

/// TargetInVerticalColumn_BTD_C: a key without a fighter is read as a vector.
#[derive(Clone, Debug, Default)]
pub struct TargetInVerticalColumn;

impl Dec for TargetInVerticalColumn {
    fn test(&mut self, cx: &mut Cx) -> bool {
        let ty = match bb_fighter(cx, "Target") {
            Some(t) => loc_y(cx.w, Some(ActorRef::Fighter(t))),
            None => vec(cx, "Target").y,
        };
        (loc_y(cx.w, Some(ActorRef::Fighter(cx.me))) - ty).abs() < pnum(cx, "ColumnWidth")
    }
}

/// TargetInRotatedVerticalColumn_BTD_C @188-1010: d = self - target fighter's location,
/// rotated by Angle about (-1, 0, 0): Y' = C.dY + S.dZ; |Y'| < ColumnWidth. A key that is no fighter: d = self
/// location, and the rotated Y less the key read as a vector's Y. RotateAngleAxis (FMath::SinCos) @334 / @784.
#[derive(Clone, Debug, Default)]
pub struct TargetInRotatedVerticalColumn;

impl Dec for TargetInRotatedVerticalColumn {
    fn test(&mut self, cx: &mut Cx) -> bool {
        let t = bb_fighter(cx, "Target");
        let me = Some(ActorRef::Fighter(cx.me));
        let (my, mz) = (loc_y(cx.w, me), loc_z(cx.w, me));
        let (dy, dz) = match t {
            Some(t) => {
                let tr = Some(ActorRef::Fighter(t));
                ((my - loc_y(cx.w, tr)), (mz - loc_z(cx.w, tr)))
            }
            None => (my, mz),
        };
        let ry = ue::rotate_angle_axis([0.0, dy, dz], pnum(cx, "Angle"), [-1.0, 0.0, 0.0])[1];
        let v = if t.is_some() { ry } else { ry - vec(cx, "Target").y };
        v.abs() < pnum(cx, "ColumnWidth")
    }
}

/// TargetBelowMe_BTD_C.
#[derive(Clone, Debug, Default)]
pub struct TargetBelowMe;

impl Dec for TargetBelowMe {
    fn test(&mut self, cx: &mut Cx) -> bool {
        let t = actor(cx, "Target");
        loc_z(cx.w, Some(ActorRef::Fighter(cx.me))) - loc_z(cx.w, t) > pnum(cx, "Distance")
    }
}

/// TargetAboveMe_BTD_C @79-488: the Target key's actor cast to Base_Fighter (else false);
/// target.Z - self.Z > Distance, strict (single precision).
#[derive(Clone, Debug, Default)]
pub struct TargetAboveMe;

impl Dec for TargetAboveMe {
    fn test(&mut self, cx: &mut Cx) -> bool {
        let Some(t) = bb_fighter(cx, "Target") else { return false };
        let w = &*cx.w;
        (loc_z(w, Some(ActorRef::Fighter(t))) - loc_z(w, Some(ActorRef::Fighter(cx.me)))) > pnum(cx, "Distance")
    }
}

/// ActorInFrontOfMe_BTD_C: facing left (+Y): the target has the larger Y; facing right: not
/// (self.Y < target.Y), so equal counts.
#[derive(Clone, Debug, Default)]
pub struct ActorInFrontOfMe;

impl Dec for ActorInFrontOfMe {
    fn test(&mut self, cx: &mut Cx) -> bool {
        let t = actor(cx, "Target");
        let less = loc_y(cx.w, Some(ActorRef::Fighter(cx.me))) < loc_y(cx.w, t);
        if facing_left(cx.w, cx.me) { less } else { !less }
    }
}

/// PointInFrontOfMe_BTD_C @300-367: ActorInFrontOfMe's rule with the Vector key's Y; an unset
/// key reads FLT_MAX.
#[derive(Clone, Debug, Default)]
pub struct PointInFrontOfMe;

impl Dec for PointInFrontOfMe {
    fn test(&mut self, cx: &mut Cx) -> bool {
        let less = loc_y(cx.w, Some(ActorRef::Fighter(cx.me))) < vec(cx, "Point").y;
        if facing_left(cx.w, cx.me) { less } else { !less }
    }
}

/// ProjectileETALessThan_BTD_C: distance / speed of the projectile; a projectile at rest (or
/// None) reads false.
#[derive(Clone, Debug, Default)]
pub struct ProjectileETALessThan;

impl Dec for ProjectileETALessThan {
    fn test(&mut self, cx: &mut Cx) -> bool {
        let (p, t) = (actor(cx, "Projectile"), actor(cx, "TargetActor"));
        let w = &*cx.w;
        let (vy, vz) = vel(w, p);
        let speed = ue::size2d([vy, vz]);
        if !(speed > 0.0) {
            return false;
        }
        ue::size2d([loc_y(w, t) - loc_y(w, p), loc_z(w, t) - loc_z(w, p)]) / speed < pnum(cx, "Time")
    }
}

/// IsProjectileInbound_BTD_C @110-939: over the ParentProjectilePhysics_C actors (any team,
/// the bot's own included), the first with far = VSize(self - it) > ProjectileTestDistance and near =
/// Vector_Distance(FindClosestPointOnLine(Normal(its velocity, 0.0001), self, (0,0,1)), self) <
/// ProjectileHitTestDistance goes into Result, true. The Point argument is the velocity's direction (a unit vector),
/// so near is |direction.Z - self.Z|: nothing of the projectile's position is in it. The build's own rule.
// Deviation: the list is in creation order; GetAllActorsOfClass groups it by class. Projectiles waiting in the pool
// are not in the port's list.
#[derive(Clone, Debug, Default)]
pub struct IsProjectileInbound;

impl Dec for IsProjectileInbound {
    fn test(&mut self, cx: &mut Cx) -> bool {
        let me = Some(ActorRef::Fighter(cx.me));
        let (sy, sz) = (loc_y(cx.w, me), loc_z(cx.w, me));
        let (test_d, hit_d) = (pnum(cx, "ProjectileTestDistance"), pnum(cx, "ProjectileHitTestDistance"));
        let ids: Vec<_> = cx.w.actors.projectiles.ids().collect();
        for id in ids {
            let a = Some(ActorRef::Projectile(id));
            let w = &*cx.w;
            let pr = w.actors.projectiles.get(id).expect("a listed projectile");
            if pr.dead || !cast_to(w, ActorRef::Projectile(id), pr.cls, name!("ParentProjectilePhysics_C")) {
                continue;
            }
            let (vy, vz) = vel(w, a);
            let nz = ue::normal([0.0, vy, vz], 1e-4)[2];
            if !(ue::size2d([sy - loc_y(w, a), sz - loc_z(w, a)]) > test_d) {
                continue;
            }
            if !((((sz + (nz - sz)) - sz).abs()) < hit_d) {
                continue;
            }
            let k = cx.key("Result");
            cx.set_obj(k, a);
            return true;
        }
        false
    }
}

/// FindEnemyInRange_BTD_C: the first live fighter of another team (in actor order, not the
/// nearest) inside Range goes into Result; none clears it. A minion has no PlayerData: `player_team`.
#[derive(Clone, Debug, Default)]
pub struct FindEnemyInRange;

impl Dec for FindEnemyInRange {
    fn test(&mut self, cx: &mut Cx) -> bool {
        let mut tm = 0;
        let me = Some(ActorRef::Fighter(cx.me));
        for t in cx.w.all_ids() {
            if is_dead(cx.w, t) || t == cx.me {
                continue;
            }
            tm = player_team(cx.w, t, tm);
            if tm == team(cx.w, cx.me) {
                continue;
            }
            let tr = Some(ActorRef::Fighter(t));
            let w = &*cx.w;
            if sq(loc_y(w, me) - loc_y(w, tr)) + sq(loc_z(w, me) - loc_z(w, tr)) < sq(pnum(cx, "Range")) {
                let k = cx.key("Result");
                cx.set_obj(k, tr);
                return true;
            }
        }
        let k = cx.key("Result");
        cx.clear(k);
        false
    }
}

/// FindAllyNotOverPlatform_BTD_C @110-1770: the first fighter alive, not the bot, of its team
/// (`player_team`), nearer than MaxRange (strict), not walking, with nothing under it (a sphere trace 20000 down with
/// its Capsule radius) -> Result = it, true; none: Result = None, false.
#[derive(Clone, Debug, Default)]
pub struct FindAllyNotOverPlatform;

impl Dec for FindAllyNotOverPlatform {
    fn test(&mut self, cx: &mut Cx) -> bool {
        let me = cx.me;
        let max_range = pnum(cx, "MaxRange");
        let mut tm = 0;
        for t in cx.w.all_ids() {
            if is_dead(cx.w, t) || t == me {
                continue;
            }
            tm = player_team(cx.w, t, tm);
            if tm != team(cx.w, me) {
                continue;
            }
            let (mr, tr) = (Some(ActorRef::Fighter(me)), Some(ActorRef::Fighter(t)));
            let (ty, tz) = (loc_y(cx.w, tr), loc_z(cx.w, tr));
            if ue::size2d([loc_y(cx.w, mr) - ty, loc_z(cx.w, mr) - tz]) >= max_range {
                continue;
            }
            if crate::fighter::grounded(cx.w, t) {
                continue;
            }
            let r = def(cx.w, t).inner_cap_radius;
            if sweep(cx, ty, tz, ty, tz - 20000.0, crate::math::narrow(r), None).is_some() {
                continue;
            }
            let k = cx.key("Result");
            cx.set_obj(k, tr);
            return true;
        }
        let k = cx.key("Result");
        cx.set_obj(k, None);
        false
    }
}

/// Steven_HasWatermelon_BTD_C @110-899: the first live fighter whose MinionCreator is the bot
/// and IsMinion -> Result = it, true; none: Result = None, false.
#[derive(Clone, Debug, Default)]
pub struct StevenHasWatermelon;

impl Dec for StevenHasWatermelon {
    fn test(&mut self, cx: &mut Cx) -> bool {
        for t in cx.w.all_ids() {
            if is_dead(cx.w, t) {
                continue;
            }
            let l = &cx.w.fighter(t).life;
            if l.minion_creator == Some(cx.me) && l.is_minion {
                let k = cx.key("Result");
                cx.set_obj(k, Some(ActorRef::Fighter(t)));
                return true;
            }
        }
        let k = cx.key("Result");
        cx.set_obj(k, None);
        false
    }
}

/// GarnetIsElectricityCharged_BTD_C @0-218: BPI_CheckChargeStatus: Charging or FullyCharged;
/// a failed cast: false.
#[derive(Clone, Debug, Default)]
pub struct GarnetIsElectricityCharged;

impl Dec for GarnetIsElectricityCharged {
    fn test(&mut self, cx: &mut Cx) -> bool {
        crate::chars::garnet::state::garnet_of(cx.w, cx.me)
            .is_some_and(|g| g.zone.charging_electricity || g.zone.electricity_fully_charged)
    }
}

/// GarnetIsReadyToSing_BTD_C: ReturnValue = false @0, as compiled.
#[derive(Clone, Debug, Default)]
pub struct GarnetIsReadyToSing;

impl Dec for GarnetIsReadyToSing {
    fn test(&mut self, _cx: &mut Cx) -> bool {
        false
    }
}

/// TargetBetweenMeAndActor_BTD_C @0-878: with S the pawn, E the Enemy key's actor, T the
/// Target's, D = T - S, F = E - S: Dot(F, D) >= 0 and |F|^2 < |D|^2 -> the distance of E from the line through S
/// with direction Normal(S - T) <= MaxDistance. A None key reads the zero vector. Single precision.
#[derive(Clone, Debug, Default)]
pub struct TargetBetweenMeAndActor;

impl Dec for TargetBetweenMeAndActor {
    fn test(&mut self, cx: &mut Cx) -> bool {
        let (e, t) = (actor(cx, "Enemy"), actor(cx, "Target"));
        let w = &*cx.w;
        let me = Some(ActorRef::Fighter(cx.me));
        let (sy, sz) = (loc_y(w, me), loc_z(w, me));
        let (ey, ez, ty, tz) = (loc_y(w, e), loc_z(w, e), loc_y(w, t), loc_z(w, t));
        let (dy, dz, fy, fz) = ((ty - sy), (tz - sz), (ey - sy), (ez - sz));
        let dot = (fy * dy) + (fz * dz);
        let dd = (dy * dy) + (dz * dz);
        let ff = (fy * fy) + (fz * fz);
        if !(ff < dd && dot >= 0.0) {
            return false;
        }
        // Normal(S - T, 0.0001), then FindClosestPointOnLine's own GetSafeNormal of it.
        let n = ue::safe_normal(ue::normal([0.0, sy - ty, sz - tz], 1e-4), ue::SMALL_NUMBER);
        let k = (fy * n[1]) + (fz * n[2]);
        let (py, pz) = ((sy + (n[1] * k)), (sz + (n[2] * k)));
        ue::size2d([py - ey, pz - ez]) <= pnum(cx, "MaxDistance")
    }
}

/// BotGetSpawnedActorOfClass_BTD_C @416-641: the first live SpawnedActors entry whose class is
/// exactly SpawnedActorClass -> TargetActor = it, true; none: false, the key left as it was.
#[derive(Clone, Debug, Default)]
pub struct BotGetSpawnedActorOfClass;

impl Dec for BotGetSpawnedActorOfClass {
    fn test(&mut self, cx: &mut Cx) -> bool {
        let Some(cls) = props::str(cx.props, "SpawnedActorClass").and_then(|s| cx.w.names.get(s)) else {
            return false;
        };
        let Some(a) = spawned_of_class(cx.w, cx.me, cls) else { return false };
        let k = cx.key("TargetActor");
        cx.set_obj(k, Some(a));
        true
    }
}

// ---- Finn, Stripe, Jake, Tom and Jerry, Velma

/// Finn_HasTreasure_BTD_C @0-234: the FinnTreasureBuff's stacks >= Treasure (and the buff
/// held).
#[derive(Clone, Debug, Default)]
pub struct FinnHasTreasure;

impl Dec for FinnHasTreasure {
    fn test(&mut self, cx: &mut Cx) -> bool {
        let n = crate::combat::buffs::buff_stacks(cx.w, cx.me, "FinnTreasureBuff_C");
        n > 0.0 && n >= pnum(cx, "Treasure")
    }
}

/// Finn_DashThrownTargetIsReady_BTD_C @126-971: the first live SpawnedActors entry of class
/// FinnDashThrownTarget_C with VSizeSquared(velocity) < 1 ends the loop with a line trace from its location 9999
/// below; none: false. A moving gem is passed over.
#[derive(Clone, Debug, Default)]
pub struct FinnDashThrownTargetIsReady;

impl Dec for FinnDashThrownTargetIsReady {
    fn test(&mut self, cx: &mut Cx) -> bool {
        let list = cx.w.fighter(cx.me).items.spawned_actors.clone();
        for a in list {
            if actor_dead(cx.w, a) || actor_cls(cx.w, a) != Some(name!("FinnDashThrownTarget_C")) {
                continue;
            }
            let (vy, vz) = vel(cx.w, Some(a));
            if !(((vy * vy) + (vz * vz)) < 1.0) {
                continue;
            }
            let (y, z) = (loc_y(cx.w, Some(a)), loc_z(cx.w, Some(a)));
            return sweep(cx, y, z, y, z - 9999.0, 0.0, None).is_some();
        }
        false
    }
}

/// C023B_HasBomb_C @0-165: Pawn_C023B HasBomb (`StripeBombState::equipped`).
#[derive(Clone, Debug, Default)]
pub struct C023BHasBomb;

impl Dec for C023BHasBomb {
    fn test(&mut self, cx: &mut Cx) -> bool {
        crate::chars::stripe::state::stripe_of(cx.w, cx.me).is_some_and(|s| s.bomb.equipped)
    }
}

/// C023B_IsAttached_C: Pawn_C023B Is Attached is written true only by ActorReachedLogic
/// (@265), which nothing calls: false for every pawn.
#[derive(Clone, Debug, Default)]
pub struct C023BIsAttached;

impl Dec for C023BIsAttached {
    fn test(&mut self, _cx: &mut Cx) -> bool {
        false
    }
}

/// BTD_JakeHasEatenEnemy_C @79-231: BPI_CheckIfHasBuff(Buff_JakEatSlow).
#[derive(Clone, Debug, Default)]
pub struct JakeHasEatenEnemy;

impl Dec for JakeHasEatenEnemy {
    fn test(&mut self, cx: &mut Cx) -> bool {
        crate::combat::buffs::has_buff(cx.w, cx.me, name!("Buff_JakEatSlow_C"))
    }
}

/// TomAndJerry_IsJerryOut_BTD_C @0-171: the pawn cast to Pawn_TomAndJerry (else false) ->
/// Not(JerryMesh.bVisible).
#[derive(Clone, Debug, Default)]
pub struct TomAndJerryIsJerryOut;

impl Dec for TomAndJerryIsJerryOut {
    fn test(&mut self, cx: &mut Cx) -> bool {
        crate::chars::tom_and_jerry::state::tj_of(cx.w, cx.me).is_some()
            && !crate::chars::tom_and_jerry::aim::jerry_mesh_visible(cx.w, cx.me)
    }
}

/// TargetBetweenMeAndJerry_BTD_C @0-830: the pawn is Tom and Jerry's with SpawnedJerryAcor and
/// the Target key's actor valid, and the target's Y strictly between his and Jerry's. Locations only (no Z).
#[derive(Clone, Debug, Default)]
pub struct TargetBetweenMeAndJerry;

impl Dec for TargetBetweenMeAndJerry {
    fn test(&mut self, cx: &mut Cx) -> bool {
        let Some(s) = crate::chars::tom_and_jerry::state::tj_of(cx.w, cx.me) else { return false };
        let j = s.pawn.spawned_jerry;
        let t = actor(cx, "Target");
        if !valid(cx.w, j) || !valid(cx.w, t) {
            return false;
        }
        let w = &*cx.w;
        let (sy, ty, jy) = (loc_y(w, Some(ActorRef::Fighter(cx.me))), loc_y(w, t), loc_y(w, j));
        ty < max(jy, sy) && ty > min(jy, sy)
    }
}

/// TomAndJerry_TennisBallInPlay_C @0-1660: over the Tom_and_Jerry_TennisBall_C actors (its
/// _Reflected child too) the one with the least squared distance below MaxDistance squared (strict, the first of a
/// tie; 1e9 at first) and in front of him -> the TennisBall key = it, true; none: ClearBlackboardValue, false.
// Unverified: GetAllActorsOfClass's order taken as spawn order (it decides only a tie in distance).
#[derive(Clone, Debug, Default)]
pub struct TomAndJerryTennisBallInPlay;

impl Dec for TomAndJerryTennisBallInPlay {
    fn test(&mut self, cx: &mut Cx) -> bool {
        let w = &*cx.w;
        let me = Some(ActorRef::Fighter(cx.me));
        let (sy, sz, left) = (loc_y(w, me), loc_z(w, me), facing_left(w, cx.me));
        let max2 = sq(pnum(cx, "MaxDistance"));
        let mut best: Real = 1_000_000_000.0;
        let mut found = None;
        for (id, pr) in w.actors.projectiles.iter() {
            if pr.dead
                || (pr.cls != name!("Tom_and_Jerry_TennisBall_C")
                    && pr.cls != name!("Tom_and_Jerry_TennisBall_Reflected_C"))
            {
                continue;
            }
            let a = Some(ActorRef::Projectile(id));
            let by = loc_y(w, a);
            let dy = by - sy;
            let dz = loc_z(w, a) - sz;
            let d = (dy * dy) + (dz * dz);
            if !(d < best && d < max2) || !(if left { sy < by } else { sy > by }) {
                continue;
            }
            best = d;
            found = a;
        }
        let k = cx.key("TennisBall");
        if found.is_some() {
            cx.set_obj(k, found);
            return true;
        }
        cx.clear(k);
        false
    }
}

/// Velma_IsCarryRunning_BTD_C @0-270: Array_Length(Get Spawned Actor(LastObject only)) == 1:
/// MakeArray of one entry whether or not a carry run is up, so true for every pawn.
#[derive(Clone, Debug, Default)]
pub struct VelmaIsCarryRunning;

impl Dec for VelmaIsCarryRunning {
    fn test(&mut self, _cx: &mut Cx) -> bool {
        true
    }
}

/// VelmaPolice_HasPerformedArrest_BTD_C: the pawn cast to Pawn_VelmaAIBotPolice -> its
/// PerformedArrest, else false.
#[derive(Clone, Debug, Default)]
pub struct VelmaPoliceHasPerformedArrest;

impl Dec for VelmaPoliceHasPerformedArrest {
    fn test(&mut self, cx: &mut Cx) -> bool {
        crate::chars::velma::state::police_of(cx.w, cx.me).is_some_and(|p| p.performed_arrest)
    }
}

/// VelmaPolice_InInitialDelay_BTD_C: the pawn cast -> InitialDelay > GetGameTimeSinceCreation
/// (strict), else false; the age as ActorOlderThan reads it (the bot's clock).
#[derive(Clone, Debug, Default)]
pub struct VelmaPoliceInInitialDelay;

impl Dec for VelmaPoliceInInitialDelay {
    fn test(&mut self, cx: &mut Cx) -> bool {
        if crate::chars::velma::state::police_of(cx.w, cx.me).is_none() {
            return false;
        }
        let pawn = &crate::fighter::common::data_of(cx.w, cx.me).fighter.pawn;
        let delay = props::num(pawn, "InitialDelay").unwrap_or(2.0);
        delay > (cx.bot.now - cx.bot.fighter_born)
    }
}

// ---- blast box, traces

/// HasBlastBoxBufferTopClearance_BTD_C. BlastBoundsTop: max over the boxes, from 0.
#[derive(Clone, Debug, Default)]
pub struct HasBlastBoxBufferTopClearance;

impl Dec for HasBlastBoxBufferTopClearance {
    fn test(&mut self, cx: &mut Cx) -> bool {
        let Some(f) = subject(cx, "Target") else { return false };
        let k = corners(cx.w);
        max(pnum(cx, "BlastBoundsTop"), k.top_left[1]) - loc_z(cx.w, Some(ActorRef::Fighter(f))) > pnum(cx, "Clearance")
    }
}

/// HasBlastBoxBottomClearance_BTD_C.
#[derive(Clone, Debug, Default)]
pub struct HasBlastBoxBottomClearance;

impl Dec for HasBlastBoxBottomClearance {
    fn test(&mut self, cx: &mut Cx) -> bool {
        let Some(f) = subject(cx, "Target") else { return false };
        let k = corners(cx.w);
        loc_z(cx.w, Some(ActorRef::Fighter(f))) - min(pnum(cx, "BlastBoundsBottom"), k.bottom_right[1])
            > pnum(cx, "Clearance")
    }
}

/// HasBlastBoxSideClearance_BTD_C: both bounds are minima over the boxes (from 1e8), as
/// compiled.
#[derive(Clone, Debug, Default)]
pub struct HasBlastBoxSideClearance;

impl Dec for HasBlastBoxSideClearance {
    fn test(&mut self, cx: &mut Cx) -> bool {
        let Some(f) = subject(cx, "Target") else { return false };
        let k = corners(cx.w);
        let left = min(pnum(cx, "BlastBoundsLeft"), k.top_left[0]);
        let right = min(pnum(cx, "BlastBoundsRight"), k.bottom_right[0]);
        let y = loc_y(cx.w, Some(ActorRef::Fighter(f)));
        let c = pnum(cx, "Clearance");
        left - y > c && y - right > c
    }
}

/// BotNotOverPlatform_BTD_C: true when nothing lies under the airborne fighter down to the
/// blast box bottom. The fallback subject is the SelfFighter member: the last fighter the check ran on.
/// SphereTraceMulti (@569) ignoring the fighter's own spawned actors; a hit on a BPI_SpawnedActor is passed over
/// (@2635), any other gives false; another fighter's ship piece is a blocking hit that ends the trace: true. An
/// elevator (ScoobyElevator_C @1212 / M008_Elevator_BP_C @1983, exact class) gives false only while its Z is above
/// -900 (@1582) / -1300 (@2353); a lower one is the list's last: true.
// Unverified: that a multi trace returns nothing past its first blocking hit is 4.26 source, not read in the exe.
// The sphere's radius here and in the other bot traces is BaseFighter's own Capsule.CapsuleRadius (`CharDef`'s
// `inner_cap_radius`: 42 on every pawn), not the root capsule's.
#[derive(Clone, Debug, Default)]
pub struct BotNotOverPlatform {
    self_fighter: Option<FighterId>,
}

impl Dec for BotNotOverPlatform {
    fn test(&mut self, cx: &mut Cx) -> bool {
        let t = actor(cx, "TargetActor");
        let f = if valid(cx.w, t) { fighter(t) } else { self.self_fighter };
        let Some(f) = f else { return false };
        self.self_fighter = Some(f);
        if crate::fighter::grounded(cx.w, f) {
            return false;
        }
        let bottom = min(0.0, corners(cx.w).bottom_right[1]);
        let fr = Some(ActorRef::Fighter(f));
        let (y, z) = (loc_y(cx.w, fr), loc_z(cx.w, fr));
        let r = def(cx.w, f).inner_cap_radius;
        let Some(h) = sweep(cx, y, z, y, bottom, crate::math::narrow(r), Some(f)) else { return true };
        if h.ship {
            return true;
        }
        if h.host == Some(name!("ScoobyElevator_C")) {
            return !(h.host_z > -900.0);
        }
        if h.host == Some(name!("M008_Elevator_BP_C")) {
            return !(h.host_z > -1300.0);
        }
        false
    }
}

/// BotOverPlatform_BTD_C: a sphere sweep 20000 down (and ForwardAngle / 90 x 20000 forward)
/// from ForwardOffset in front of the fighter.
#[derive(Clone, Debug, Default)]
pub struct BotOverPlatform;

impl Dec for BotOverPlatform {
    fn test(&mut self, cx: &mut Cx) -> bool {
        let Some(f) = subject(cx, "TargetActor") else { return false };
        let a = pnum(cx, "ForwardAngle") / 90.0;
        let off = pnum(cx, "ForwardOffset");
        let left = facing_left(cx.w, f);
        let fr = Some(ActorRef::Fighter(f));
        let y = loc_y(cx.w, fr) + if left { off } else { -off };
        let z = loc_z(cx.w, fr);
        let r = def(cx.w, f).inner_cap_radius;
        sweep(cx, y, z, y + (if left { a } else { -a }) * 20000.0, z - 20000.0, crate::math::narrow(r), None).is_some()
    }
}

/// RaycastTowardPoint_BTD_C: a line trace of Distance toward the point. An unset point
/// (FLT_MAX) normalizes to the zero vector.
// Deviation: the exe's Normal of an overflowing sum multiplies by InvSqrt(inf), NaN (rsqrtss 0, then inf x 0 in its
// Newton step); the port keeps the zero vector there.
#[derive(Clone, Debug, Default)]
pub struct RaycastTowardPoint;

impl Dec for RaycastTowardPoint {
    fn test(&mut self, cx: &mut Cx) -> bool {
        let (s, t) = (self_loc(cx), vec(cx, "TargetPoint"));
        let (dx, dy, dz) = (t.x - s.x, t.y - s.y, t.z - s.z);
        let inf = ((dx * dx) + (dy * dy)) + (dz * dz) == Real::INFINITY;
        let n = if inf { [0.0; 3] } else { ue::normal([dx, dy, dz], 1e-4) };
        let d = pnum(cx, "Distance");
        sweep(cx, s.y, s.z, s.y + n[1] * d, s.z + n[2] * d, 0.0, None).is_some()
    }
}

/// RaycastForward_BTD_C @0-1006: a line trace of Distance from the subject's location (the
/// BotBB actor, or the pawn), forward and turned down by Angle: (Y: d cos a, Z: -d sin a), both signs + when facing
/// left. A hit reads Distance > MinDistance, no hit false. The ignore list is empty. RotateAngleAxis((0, d, 0), a,
/// (-1, 0, 0)) (FMath::SinCos).
#[derive(Clone, Debug, Default)]
pub struct RaycastForward;

impl Dec for RaycastForward {
    fn test(&mut self, cx: &mut Cx) -> bool {
        let Some(f) = subject(cx, "BotBB") else { return false };
        let left = facing_left(cx.w, f);
        let (ang, dist) = (pnum(cx, "Angle"), pnum(cx, "Distance"));
        let a = if left { ang } else { -ang };
        let d = if left { dist } else { -dist };
        let fr = Some(ActorRef::Fighter(f));
        let (y, z) = (loc_y(cx.w, fr), loc_z(cx.w, fr));
        let v = ue::rotate_angle_axis([0.0, d, 0.0], a, [-1.0, 0.0, 0.0]);
        match sweep(cx, y, z, y + v[1], z + v[2], 0.0, None) {
            Some(h) => ue::size2d([h.y - y, h.z - z]) > pnum(cx, "MinDistance"),
            None => false,
        }
    }
}
