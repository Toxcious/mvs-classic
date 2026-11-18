//! Pawn_C016's own logic (and `state.rs` setHasBall): LeBron's (content lebron, Pawn_C016 of
//! package c016/Pawn_c016, fighter byte 21) own classes: the pawn's hooks, the ball as a state of the pawn, his
//! conditions and his one notify. His attacks themselves are data: the start tables, branch groups and montages of
//! the build, played by the shared code.
//!
//! The ball is not a variable: "he has the ball" is a Buff_C016_Ball_Conditional among the pawn's buffs
//! (Comp_State_Manager.CheckHasBuff: `has_ball_buff`). The start tables, branch groups and bot trees ask the buff
//! through his conditions. The pawn's HasBall is a copy that lags it by one latent step, and with the copy go the
//! navigation montages, the hold pack and the AnimBP's state:
//!   - the buff's BeginPlay, EndPlay and Destroyed, Buff_C016_RemoveBall and the pawn's AttackStarted / AttackEnded
//!     all call BPI_SetHasBall(x) @2257, which is Delay(0) @2287 and nothing else: the argument is dropped;
//!   - the delay's resume @35 reads CheckHasBuff and calls SetHasBall(HasBuff) @2342 -> @2874.
//!
//! A Delay ignores a call while its action exists, and an action made outside the pawn's latent pass fires in the
//! pass after it (execDelay; AActor::Tick runs ReceiveTick, then the latent actions): every caller above runs
//! outside it (a buff's spawn or destroy inside a notify, a hit or the game mode's reset; the BranchManager's notify
//! dispatch), so the copy is written at the pawn tick after the call, the same frame when the call came ahead of that
//! tick and the next frame otherwise.
// Unverified: where a ring-out's buff destroy and the respawn's buff spawn stand in the frame against the pawn's tick
// is the port's World.step order, not read from the build; the copy can be a frame early or late there.

use super::state::{BALL, DUNK, has_ball_buff, lebron, lebron_of, lebron_w, request_has_ball};
use super::state::{LebronState, set_is_teleport_dunking, set_teleport_dunk_available};
use crate::chars::CharState;
use crate::combat::BuffClass;
use crate::combat::BuffRef;
use crate::combat::buffs::{buff, buff_of, end_buff, spawn_buff};
use crate::events::{Event, NotifyRef};
use crate::fighter::anim::montages::start_montage;
use crate::fighter::anim::update::{FALL_RATE, anim_set_switched, set_loco};
use crate::fighter::anim::{SetLocoOpt, SetRef};
use crate::fighter::moves::{is_falling, is_flying};
use crate::fighter::{FighterId, actor_y};
use crate::name;
use crate::names::Name;
use crate::registry::entries::{BuffEntry, BuffHooks};
use crate::registry::{CharHooks, Condition, NotifyHandler, Registry, RegistryError};
use crate::world::World;
use mvs_anim::montage::PROXY_NONE;

const IGNITED: Name = name!("Buff_C016_IgnitedDunk_C");
/// The navigation set SetHasBall(true) writes and the one it writes for false (`navSets`' keys).
const NAV_BALL: Name = name!("ball");
const NAV_NO_BALL: Name = name!("noBall");
/// The anim set of c016_AnimBP's second instance (`animSets.noBall`).
const SET_NO_BALL: Name = name!("noBall");

/// The hooks of this file in LeBron's CharHooks (`mod.rs` own: init, animSet, attackStarted, attackEnded,
/// tick, matchReset). The files' joined hooks are all empty.
///
/// A match reset spawns a new pawn: a new state (HasBall at its class default, BeginPlay's delay pending, no delay of
/// BPI_SetHasBall, TeleportDunkReady and IsTeleportDunking false) and its BeginPlay again; the fighter's reset has
/// put the class defaults' navigation set back. The world gives the passive row's buff after this call, whose
/// BeginPlay asks for the copy. An attack that was on at the reset fires no `attack_ended` at the reset's montage stop
/// (`world::reset::new_pawn` clears the count first).
///
/// The pawn has no EndPlay, death or reset event of its own: HasBall, a pending delay, TeleportDunkReady and
/// IsTeleportDunking go on through a ring-out and the respawn. The ball buff ends with the defeat
/// (DestroyBuffWhenOwnerDefeated) and the passive row gives it again at ResetFighterLogic, each with its call.
/// BPI_ReadyForAI @3036 (AssignDefaultBehaviors, then SetDynamicSubtree of AI.CustomRecovery, AI.CustomMeleeCombo,
/// Ai.CustomAirMelee and AI.CustomSpecialLogic) belongs to the bots' crate.
pub fn hooks(h: &mut CharHooks) {
    h.init = Some(begin_play);
    h.anim_set = Some(anim_set);
    h.attack_started = Some(request_has_ball);
    h.attack_ended = Some(request_has_ball);
    h.tick = Some(tick);
    h.match_reset = Some(match_reset);
}

/// ReceiveBeginPlay @2366, after the parent's: the AnimBP reference @2386, the capsule's size saved @2523, and
/// Delay(0) @2622 (the state's `begin_delay`, pending from the first).
fn begin_play(w: &mut World, f: FighterId) {
    let (half, radius) = {
        let c = &w.fighter(f).capsule;
        (c.cap_half_u, c.cap_radius_u)
    };
    let s = lebron_w(w, f);
    s.cap_half = half;
    s.cap_radius = radius;
}

/// The pawn's latent actions, behind ReceiveTick @2844 (the parent's tick and SetBallSquashAndStretch, which is
/// drawing). The Delay of BPI_SetHasBall first, then BeginPlay's: its resume @224 is BPI_SetHasBall(true), a Delay
/// made inside the latent pass, which counts from the next frame.
// Unverified: the order of two delay actions of one actor in one pass is not read from the exe. With BeginPlay's
// first, its call would find the other action and do nothing: one SetHasBall at the first tick instead of one at
// each of the first two. The values written are the same either way (each reads the buff as it is then); the count
// of OnSetHasBall calls (`ball_sets`) differs by one.
fn tick(w: &mut World, f: FighterId) {
    if std::mem::take(&mut lebron_w(w, f).ball_delay) {
        let has = has_ball_buff(w, f);
        set_has_ball(w, f, has);
    }
    if std::mem::take(&mut lebron_w(w, f).begin_delay) {
        request_has_ball(w, f);
    }
}

/// `matchReset`. c016_AnimBP is new too, with its HasBall false: World.reset's `spawn` has put the machines at their
/// start on the old state's set, so when that was the ball set the start clip is put again from the new one (the
/// line of Life.spawn; no switch, the old instance is gone).
fn match_reset(w: &mut World, f: FighterId) {
    let from = crate::fighter::anim_set(w, f);
    w.fighter_mut(f).cs = CharState::Lebron(LebronState::default());
    begin_play(w, f);
    if crate::fighter::anim_set(w, f) == from {
        return;
    }
    let grounded = crate::fighter::anim::update::grounded(w.fighter(f));
    let set = crate::fighter::anim_set_def(w, f);
    let clip = if grounded { set.idle.as_deref() } else { set.fall.as_deref() };
    let a = clip.and_then(|c| w.names.get(c)).unwrap_or(Name::NONE);
    let rate = if grounded { 1.0 } else { FALL_RATE };
    set_loco(w, f, a, rate, None, Some(SetLocoOpt { restart: true, ..SetLocoOpt::default() }));
}

/// c016_AnimBP's BlendListByBool @14: the LinkedAnimLayer of Base_AnimBP_C016 (the data file's animSet) on HasBall ||
/// "Is In Menu", else the LinkedAnimGraph of Base_AnimBP_NoBall_C016 (`animSets.noBall`). The HasBall is the AnimBP's
/// own (`anim_has_ball`): false on a new pawn, so he stands on the no-ball set until the first SetHasBall, the pawn
/// tick after his spawn, and blends to the ball set from there over the node's 0.1 s (`set_has_ball` calls
/// `anim_set_switched`).
fn anim_set(w: &World, f: FighterId) -> SetRef {
    if lebron(w, f).anim_has_ball { SetRef::Base } else { SetRef::Named(SET_NO_BALL) }
}

/// The Delay's resume @35-200: CheckHasBuff(Buff_C016_Ball_Conditional_C) -> MultiCastHasBall(HasBuff) ->
/// SetHasBall(HasBuff) @2342.
/// SetHasBall @2874: HasBall = the value; AnimBP.OnSetHasBall(HasBall) @2893; then (@536) the six navigation
/// variables: DashAnim @593, DashTurnAnim @713, Evades @795, FWD_Evade @923, SpotDodgeAnim @1043, WallJumpAnim @1163;
/// then (@324) the pawn's PickUpOverride copy and Comp_ItemManager.HoldingItemOverride @387 / @486. All of it at every
/// call, whether the value changed or not. A montage already playing is not replaced; an item in the hand keeps the
/// pack of its pick-up (`hold_pack`).
/// OnSetHasBall (c016_AnimBP @7110) writes the AnimBP's HasBall, the input of its BlendListByBool: a change of it is a
/// switch of the anim set (`anim_set_switched`, with that function's unverified notes: the instance that becomes
/// relevant is taken to start at its entry states, and one set of machine state stands for the two).
fn set_has_ball(w: &mut World, f: FighterId, has: bool) {
    let from = crate::fighter::anim_set(w, f);
    let s = lebron_w(w, f);
    s.has_ball = has;
    s.anim_has_ball = has;
    s.ball_sets += 1;
    w.fighter_mut(f).anim.nav = SetRef::Named(if has { NAV_BALL } else { NAV_NO_BALL });
    anim_set_switched(w, f, from);
}

/// MulticastPlayBallRecieveAnim @1605-1792: the PlayMontage proxy on the Mesh (rate 1.0, start 0, no section) of
/// C016_NoBall_Air_BallReturn_Montage when CharacterMovement.IsFalling or IsFlying, else
/// C016_NoBall_BallReturn_Montage. The graph asks the movement mode, not the floor (@1643-1735): Falling and Flying
/// take the air montage, every other mode the ground one: Walking, and Custom 2 of a knockback (`kb_mode`: off the
/// floor and not Falling). The proxy is a plain Montage_Play, past the branch manager's attack start: the montage
/// replaces the attack slot's as any attack's does and BeginAttack is not set, so he counts as attacking from the
/// montage's own isAttackingFlag on (its NotifyBegin, the first dispatch) as an evade's montage does.
// Unverified: what else the port's start_montage does for an attack start rides along: the attack count, the charge
// values and the hit flag reset (none read by these two montages) and the player's ground / air section choice by
// `grounded` (the proxy names no section: the montage's first).
fn play_ball_receive_anim(w: &mut World, f: FighterId) {
    let air = is_falling(w, f) || is_flying(w, f);
    let m = if air { name!("C016_NoBall_Air_BallReturn_Montage") } else { name!("C016_NoBall_BallReturn_Montage") };
    start_montage(w, f, m, &PROXY_NONE, None);
    w.fighter_mut(f).charge.begin_attack = false;
}

/// `getBallBack` (BPI_GetBallBack @3540 -> @1231): Comp_State_Manager.SpawnBuffActor(Buff_C016_Ball_Conditional_C,
/// self, self, stack count 0, duration 0.0, true, false), then MulticastPlayBallRecieveAnim @1299, whatever the pawn
/// is doing and whether or not the spawn made a buff. No authority test, no test of the buff. Called by the fence's
/// BlockedBuffandMontage (bubble.rs); the ball's catch does not come here: it makes the two calls itself, the montage
/// first and behind its own tests (ball.rs).
pub fn get_ball_back(w: &mut World, f: FighterId) {
    if lebron_of(w, f).is_none() {
        return; // the callers' cast to BPI_C016
    }
    spawn_buff(w, f, BALL, 0.0, Some(0), Some(f), true, None);
    play_ball_receive_anim(w, f);
}

/// `playBallReceiveAnimIfNotInAttack` (BPI_PlayBallRecieveAnimIfNotInAttack @1314-1590): nothing with the ball buff or
/// while the mesh's current montage is C016_Special_Dunk1_Montage; else MulticastPlayBallRecieveAnim. The name says
/// more than the graph tests: any other attack is replaced. Its callers are the ball's (ball.rs).
pub fn play_ball_receive_anim_if_not_in_attack(w: &mut World, f: FighterId) {
    if lebron_of(w, f).is_none() || has_ball_buff(w, f) {
        return;
    }
    if w.fighter(f).montages.montage.as_ref().is_some_and(|m| m.montage.name == DUNK) {
        return;
    }
    play_ball_receive_anim(w, f);
}

// ---- the conditions and the notify

/// c016BallEnabledCondition Check (c016BallEnabledCondition) @0-336: the actor as a BP_FighterCharacter, its
/// Comp_State_Manager.CheckHasBuff(Buff_C016_Ball_Conditional_C).HasBuff; a failed cast false @325. Any fighter's
/// buffs, not the pawn's HasBall. On 46 rows of his data: the with-ball rows of the four start tables and of his
/// branch groups, and the with-ball entries of Comp_ItemManager.PickUpOverride.
fn ball_enabled(w: &mut World, f: FighterId) -> bool {
    has_ball_buff(w, f)
}

/// c016BallNotEnabledCondition Check @0-365: the same read, negated @295; a failed cast false @365 (every fighter of
/// the port is a BP_FighterCharacter). The no-ball rows of BranchGroup_C016Base, the no-ball entries of PickUpOverride
/// and C016_General_Emote_Montage's first notify.
fn ball_not_enabled(w: &mut World, f: FighterId) -> bool {
    !has_ball_buff(w, f)
}

/// c016IgniteDunkEnabledCondition Check @0-309: BPI_CheckIfHasBuff(Buff_C016_IgnitedDunk_C).hasBuff of a
/// BPI_Base_Fighter. The buff is given by DashToBasketball's completion alone, with HotHands and a burning ball
/// (later.rs): never in a 1v1. Dunk1's ConditionalSpawner of FireWallHazard carries it.
fn ignite_dunk_enabled(w: &mut World, f: FighterId) -> bool {
    buff_of(w, f, IGNITED).is_some()
}

/// C016TeleportDunkActiveCondition Check @0-165: the actor's BPI_GetIsTeleportDunking (IsTeleportDunking); no
/// BPI_C016: false @149. The variable is set by DashToBasketball's completion alone (later.rs), which needs a team
/// mate's AllyPass ball: false in every 1v1. Dunk1's ConditionalSpawner of C016_DunkShockwave and its
/// Hitframe_Conditional carry it.
fn teleport_dunk_active(w: &mut World, f: FighterId) -> bool {
    lebron_of(w, f).is_some_and(|s| s.is_teleport_dunking)
}

/// c016TeleportDunkEnabledCondition Check @0-436: BPI_CheckTeleportDunkAvailable (TeleportDunkReady; false with no
/// BPI_C016 @409, and the buff is still asked @125) or CheckHasBuff(Buff_C016_Ball_Conditional_C).HasBuff. Two rows of
/// AttacksSpecialGround and two of AttacksSpecialAir carry it (the dunk: with the ball in hand, or with a thrown ball
/// that has said it can be dunked). TeleportDunkReady's writers are the ball's (ball.rs) and Buff_C016_RemoveBall's
/// false. The ball's true comes from the parent class's hit response alone, and nothing of his data spawns the parent
/// class (ball.rs): in this build's 1v1 it reads as the buff alone.
fn teleport_dunk_enabled(w: &mut World, f: FighterId) -> bool {
    lebron_of(w, f).is_some_and(|s| s.teleport_dunk_ready) || has_ball_buff(w, f)
}

/// c016TeleportDunkedCondition Check @0-165: BPI_CheckTeleportDunkAvailable alone; no BPI_C016: false. The Condition
/// of Dunk1's AnimNotify_ApplyBuff(Buff_ArmorPack_C): armor for the dunk at a thrown ball only.
fn teleport_dunked(w: &mut World, f: FighterId) -> bool {
    lebron_of(w, f).is_some_and(|s| s.teleport_dunk_ready)
}

/// C016EndTeleportDunk (a notify state on C016_Special_Dunk1_Montage, t 0, 1.385 s): Received_NotifyEnd @0-170 alone,
/// the owner's BPI_SetIsTeleportDunking(false) when it has BPI_C016. No begin, no tick.
fn end_teleport_dunk(w: &mut World, f: FighterId, _n: NotifyRef, _key: i32) {
    set_is_teleport_dunking(w, f, false);
}

// ---- the buffs

/// Buff_C016_Ball_Conditional (Buff_c016_Ball_Conditional) ReceiveBeginPlay @388, after the parent's: with authority,
/// an owner with BPI_C016 gets BPI_SetHasBall(true) @331; then PickUpVisuals @369 whoever the owner is (@418):
/// PS_BasketballPickup at the owner's location (scale 1.6, its Team parameter), PS_C016Basketball_Recall on the
/// owner's Mesh at fml_un_C_rotator, sfx_cha_C016_ball_recover @1146 and vo_C016_eff_attack_small_alwaysplay @1211 on
/// the buff actor, and PF_Post_VO_Event(the buff actor, ability_catch, no context, 0.15) @1276, which posts nothing:
/// with no context fighter it asks the VO actor for BPI_Character_Viewer (Lib_Audio PF_Post_VO_Event @441), and a buff
/// actor has none. Its RandomFloat is not drawn here (the port's stream stands for no stream of the build).
/// The two events are posted at the owner's place (the buff actor's). The two emitters are the view's: it reads the
/// buff's status update of this spawn.
///
/// Class defaults: BuffDuration 0 (no life timer), MaxStacks 1, CanOverride (a spawn on a pawn that has one destroys
/// the old actor and begins a new one), PreventStacksWhenReapplied, DestroyBuffWhenOwnerDefeated, no mods. Given by
/// the passive row of his Comp_Pawn_PassiveBuffManager (BuffSelf, Duration 0, StartingStacks 0: at the pawn's spawn
/// and at every ResetFighterLogic, unverified), by the two recall montages' AnimNotify_ApplyBuff, by OnHitBuffs rows
/// of his no-ball attacks and of C016_SassBubble, and by BPI_GetBallBack (`get_ball_back`).
fn ball_begin(w: &mut World, f: FighterId, _b: BuffRef, _from: Option<FighterId>, _dur: Option<crate::Real>) {
    request_has_ball(w, f);
    let (fighter, x, y) = (w.fighter(f).id, w.fighter(f).body.x, actor_y(w, f));
    for ev in [name!("sfx_cha_C016_ball_recover"), name!("vo_C016_eff_attack_small_alwaysplay")] {
        w.fighter_mut(f).events.push(Event::Sound {
            fighter,
            ev,
            x: Some(x),
            y: Some(y),
            hit: None,
            pooled: false,
            key: None,
        });
    }
}

/// ReceiveEndPlay @10 (after the parent's) and ReceiveDestroyed @403, both -> @22: with authority, an owner with
/// BPI_C016 gets BPI_SetHasBall(false) @155. Two calls at a destroy, one Delay: the second finds the action and does
/// nothing.
fn ball_end(w: &mut World, f: FighterId, _b: BuffRef) {
    request_has_ball(w, f);
}

/// Buff_C016_RemoveBall (BuffDuration 0.1 in the class, 1 in the row): the OnHitBuffs row of
/// C016_Special_Dunk1_Montage's Hitframe_Conditional, ApplyBuffSelf without RequireHitToApplyBuffToSelf: on LeBron
/// himself, hit or not. ReceiveBeginPlay @29, after the parent's: the owner's
/// Comp_State_Manager.RemoveBuff(Buff_C016_Ball_Conditional_C, 0) @224 (every actor of the class, each one's EndPlay
/// inside the call), BPI_SetHasBall(false) @472 and BPI_SetTeleportDunkAvailable(false) @609 for an owner with
/// BPI_C016, then K2_DestroyActor @10: the actor never outlives its BeginPlay. The row is given at the entry of every
/// pfgDoDamage call, ahead of IgnoreActor: an overlap of the hit box with any component, LeBron's own hurt boxes
/// included, so a dunk that meets nobody loses the ball too.
fn remove_ball_begin(w: &mut World, f: FighterId, b: BuffRef, _from: Option<FighterId>, _dur: Option<crate::Real>) {
    let serials: Vec<u32> = w.fighter(f).buffs.buffs.iter().map(|x| x.serial).collect();
    for serial in serials {
        let r = BuffRef { fighter: f, serial };
        if buff(w, r).is_some_and(|x| x.cls == BALL && x.frames > 0.0) {
            end_buff(w, f, r);
        }
    }
    request_has_ball(w, f);
    set_teleport_dunk_available(w, f, false);
    end_buff(w, f, b);
}

/// This file's registry entries (the conditions, the notify, the two buffs).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.conditions.enter("C016BallEnabledCondition_C", Condition::Fn(ball_enabled))?;
    r.conditions.enter("C016BallNotEnabledCondition_C", Condition::Fn(ball_not_enabled))?;
    r.conditions.enter("C016IgniteDunkEnabledCondition_C", Condition::Fn(ignite_dunk_enabled))?;
    r.conditions.enter("C016TeleportDunkActiveCondition_C", Condition::Fn(teleport_dunk_active))?;
    r.conditions.enter("C016TeleportDunkEnabledCondition_C", Condition::Fn(teleport_dunk_enabled))?;
    r.conditions.enter("C016TeleportDunkedCondition_C", Condition::Fn(teleport_dunked))?;
    r.notifies
        .enter("C016EndTeleportDunk_C", NotifyHandler { end: Some(end_teleport_dunk), ..NotifyHandler::default() })?;
    let list = [
        BuffEntry::new(
            BuffClass::BuffC016BallConditional,
            BuffHooks { begin: Some(ball_begin), end: Some(ball_end), ..BuffHooks::default() },
        ),
        BuffEntry::new(
            BuffClass::BuffC016RemoveBall,
            BuffHooks { begin: Some(remove_ball_begin), ..BuffHooks::default() },
        ),
    ];
    for e in list {
        let n = e.class.registered_name().expect("a registered class");
        r.buffs.enter(n, e)?;
    }
    Ok(())
}
