//! The buff stun, struggling, the holds and grabs and the rescue pull.

#![allow(clippy::neg_cmp_op_on_partial_ord, reason = "the negated tests are kept for their NaN result")]

use crate::Real;
use crate::State;
use crate::actors::ActorRef;
use crate::actors::item::Item;
use crate::actors::projectile::Projectile;
use crate::actors::world_actor::{actor_dead as spawned_dead, attached_to, destroy_on_consume, mark_spawned_dead};
use crate::combat::buffs::{BuffRef, Struggle, buff_collision_off, buff_signal, end_buff, tick_buffs};
use crate::combat::hit::{
    def_of, fighter_data, hitstun_ended, set_grounded, set_most_recent_attacker, stun_changed, timer_ticks,
};
use crate::combat::hits::direct_hit;
use crate::combat::{BuffClass, DataVec};
use crate::consts::{DELTA_SECONDS, DT};
use crate::events::Event;
use crate::fighter::anim::sampled_boxes;
use crate::fighter::anim::{
    clear_queued_attack, start_montage, stop_anim_montage, stop_dash_montage, stop_grouped, stop_montage,
};
use crate::fighter::body::floor_list;
use crate::fighter::common::{buff_def, char_data, peers};
use crate::fighter::match_side::pawn_gone;
use crate::fighter::moves::override_gravity;
use crate::fighter::moves::{
    Impulse, Pull, clear_gravity_override, disable_movement, enable_falling, lateral_friction_event, recall_move,
    settle, update_air_control,
};
use crate::fighter::tick::controller_rumble;
use crate::fighter::{
    Fighter, FighterId, actor_y, cap_half, cap_radius, capsule_half, grapple_off, in_air, mesh_wx, mesh_wy,
};
use crate::fighter::{grapple_on, hold_def, latent, warn};
use crate::input::AttackBtn;
use crate::math::{max, min, ue};
use crate::name;
use crate::names::Name;
use crate::registry::signals::{BuffSignal, BuffSignalArg};
use crate::world::World;
use crate::world::spawning::destroy_spawned;
use mvs_collision::capsule::capsule_gap;
use mvs_collision::solids::capsule_sweep_all;
use mvs_data::value::{Props, Value};

/// `Hold`: one live hold buff, kept on the fighter it holds.
#[derive(State, Debug)]
pub struct Hold {
    /// `cls`: the buff's class.
    pub cls: BuffClass,
    /// `by`: InstigatorRef.
    pub by: FighterId,
    /// Latent ticks left of BeginPlay's Delay(0.033), 0 once it has run.
    pub delay: i32,
    /// Latent ticks left of the Delay(0.06) before the second K2_SetActorLocation, 0 none.
    pub snap: i32,
    /// InitiateGrapple has run and nothing has detached the owner since.
    pub attached: bool,
    /// BindOwnerKnockedBack and the two OnExitedArena bindings have run.
    pub bound: bool,
    /// A child's own flag (Buff_HeldBy_C017 SuccessfullyApplied).
    pub applied: bool,
    /// AttachOffset as a child's code wrote it ahead of the parent's ReceiveBeginPlay; `None`: the class data's.
    pub offset: Option<DataVec>,
    /// AttachSocket as a child's code wrote it; `None`: the class data's.
    pub socket: Option<Name>,
    /// The socket InitiateGrapple took for an AttachSocket with L_ / R_ in its name.
    pub grip: Option<Name>,
    /// A DestroyAttachedConsumableActors call waiting for the World.
    pub consume: HoldConsume,
    /// A child class's own variables (its HoldKind's).
    pub own: HoldOwn,
}

/// `Hold.consume`: `undefined`, `true`, `'all'`.
#[derive(State, Copy, PartialEq, Eq, Debug, Default)]
pub enum HoldConsume {
    #[default]
    None,
    Attached,
    All,
}

/// `Hold.own`: a variant per child class with variables of its own, set by its kind's begin.
#[derive(State, Copy, PartialEq, Eq, Debug, Default)]
pub enum HoldOwn {
    #[default]
    None,
    /// Buff_JakEat_C's (`chars::jake::eat_buffs`).
    JakEat(crate::chars::jake::eat_buffs::JakEatOwn),
}

/// The fighter's holds.
#[derive(State, Debug, Default)]
pub struct Holds {
    pub hold: Option<Hold>,
    pub hold_victims: Vec<FighterId>,
    pub holding_enemy: bool,
    pub hold_buff: Option<FighterId>,
    /// `holdHits`: a fighter's own pfgDoDamage on a held fighter, queued by a notify for the World.
    pub hold_hits: Vec<HoldHit>,
}

/// A `holdHits` entry: the victim and the hit's packet (a Struct_HitBoxDamage as the notify built it).
#[derive(Debug)]
pub struct HoldHit {
    pub vic: FighterId,
    pub p: Props,
}

impl Clone for HoldHit {
    fn clone(&self) -> Self {
        HoldHit { vic: self.vic, p: self.p.clone() }
    }

    /// A save over a save keeps the packet's storage.
    fn clone_from(&mut self, s: &Self) {
        self.vic = s.vic;
        self.p.clone_from(&s.p);
    }
}

impl crate::state::StateHash for HoldHit {
    fn hash_state(&self, h: &mut crate::state::StateHasher) {
        self.vic.hash_state(h);
        crate::combat::buffs::hash_props(&self.p, h);
    }
}

// ---- the holds

/// A buff class's name as the match interned it.
fn class_name(w: &World, c: BuffClass) -> Name {
    crate::fighter::common::lit(w, c.name(&w.names))
}

/// A bool of a buff class's data row (`buffDef(f, cls)?.[k]`).
fn def_flag(w: &World, f: FighterId, cls: Name, k: &str) -> bool {
    buff_def(w, ActorRef::Fighter(f), cls).and_then(|d| d.get(k)).and_then(Value::as_bool) == Some(true)
}

/// The Stun struct of a live buff (`instDef(f, b)?.Stun`): the entry's own values first (Buff_Ice writes its
/// FreezeStun there), else its class's.
fn stun_of(w: &World, f: FighterId, serial: u32) -> Option<&Props> {
    crate::combat::buffs::inst_view(w, BuffRef { fighter: f, serial }).get("Stun").and_then(Value::as_obj)
}

/// A number of a Stun struct.
fn stun_num(s: Option<&Props>, k: &str) -> Option<Real> {
    s.and_then(|s| s.get(k)).and_then(Value::as_f64).map(crate::math::narrow)
}

/// A flag of a Stun struct (`!!stun?.[k]`).
fn stun_flag(s: Option<&Props>, k: &str) -> bool {
    s.and_then(|s| s.get(k)).is_some_and(|v| match v {
        Value::Bool(b) => *b,
        Value::Num(n) => *n != 0.0 && !n.is_nan(),
        Value::Null => false,
        Value::Str(s) => !s.is_empty(),
        _ => true,
    })
}

/// `Fighter.buffStunRemaining`: the BuffstunTimer's remaining time.
pub fn buff_stun_remaining(w: &World, f: FighterId) -> Real {
    let b = &w.fighter(f).buffs;
    b.buff_stun_len - (b.buff_stun_age * DELTA_SECONDS)
}

/// Comp_State_Manager ApplyStunBuffs for a buff with a stun (@291-680: Montage_Stop(0), SendStunMulticast,
/// IsBuffStunned, SetMostRecentAttacker) -> HitstunHandler AddBuffHitStun: BuffstunTimer / EscapeAvailableTimer /
/// HitstunTimer cleared (@82-166), BuffstunTimer = FClamp(StunDuration, MinHitStunClamp 0.3, 99999) ->
/// RemoveHitStunFromBuff (@186-317), HitstunBuffActive, AddBuffstunSource (@384), RemoveHitstunFromKnockBack
/// (@398), SetIgnoreMoveInput(true), BPI_UpdateAirControl(0), SetCanImpulse(false), BPI_ClearQueuedAttack
/// (@745-1156), BuffStunVisuals (@2286, `buff_stun_visuals`), HitStunStart (@2372). Called by `apply_stun_buffs`
/// alone, as AddBuffHitStun by the state manager alone. SuspendPositionOnGround / InAir (@2587-2945) are at the
/// function's end; of the classes read, Buff_Ice's FreezeStun and C023A_ForceDance_Debuff set them.
/// CanOverideLastHit has no port: AddBuffHitStun sets it @1193 behind its clear of the HitstunTimer @166, the one
/// read is AddDecayedHitStun's @406 with a HitstunTimer running, and every start of that timer (@957, @2427) writes
/// it false first (@1387; @3692 for a fighter owner), so no hit on a fighter reads it true.
/// A knockback under way: the function clears the HitstunTimer (@166) and, through RemoveHitstunFromKnockBack (@398
/// -> ubergraph 7427), the knockback's source; with the buff's source added first (@384) CheckHitstunActive is true
/// there and it returns @7478, so the hitstun end sequence (@1132-) waits for the stun's end (`buff_stun_end`).
/// Nothing in it writes the movement mode, Velocity, the True Hitstun Timer, AdditiveKnockbackVel, the raised
/// terminal velocity or the DamageManager's KnockbackLaunchVelocity: the flight goes on under the stun, and a
/// launch still waiting for its hitpause is applied when the pause ends, to a fighter with no knockback source
/// (`free_launch`). Only a suspending stun ends the knockback.
/// The struggle (`stun`: the buff's Stun struct), AddBuffHitStun @1204-2049 with CanStruggle: StruggleIncrement =
/// StunDuration x MaxStruggleMult / StruggleAmount, StruggleMaxCount = StruggleAmount, StruggleCount and
/// StruggleInputsAvailable 0, 'Bind Input Struggle' (@5195 -> 15) and the looping IncreaseStruggleMaxInputTimer,
/// period (StunDuration - StunDuration x MaxStruggleMult) / StruggleMaxCount, or StunDuration / StruggleMaxCount
/// when that is 0 (@1683-1992). Then, with the owner's authority, SetStruggleUiServer(true) (@2076-2271): the
/// struggle widget's first two calls, ahead of BuffStunVisuals and HitStunStart.
// Deviation: HitReactInt keeps its last value.
// Deviation: StruggleAudio (null on the plasma hold's class) and the StruggleSuccess broadcast (no listener
// decoded) are not ported; the Struggling broadcast and the rumble are (`struggle_input`).
pub fn buff_stun_start(w: &mut World, f: FighterId, sec: Real, stun: Option<&Props>) {
    if !(sec > 0.0) {
        return;
    }
    let can_struggle = stun_flag(stun, "CanStruggle");
    if can_struggle {
        let full = stun_num(stun, "StunDuration").unwrap_or(0.0);
        let amount = stun_num(stun, "StruggleAmount").unwrap_or(0.0);
        let cut = full * stun_num(stun, "MaxStruggleMult").unwrap_or(0.0);
        let rest = (full - cut) / amount;
        let period = if rest != 0.0 { rest } else { full / amount };
        let p = timer_ticks(Some(period));
        w.fighter_mut(f).buffs.struggle =
            Some(Struggle { inc: (cut / amount), max: amount, count: 0.0, avail: 0.0, period: p, left: p + 1.0 });
    }
    let suspend_ground = stun_flag(stun, "SuspendPositionOnGround");
    let suspend_air = stun_flag(stun, "SuspendPositionInAir");
    let visuals = stun_num(stun, "StunnedHitReaction").unwrap_or(6.0) == 6.0 && stun_flag(stun, "PlayDanceMontage");
    holder_interrupted(w, f); // HitStunStart reaches the holds this fighter is the instigator of
    // ApplyStunBuffs @291 and SendStunMulticast @2107: Montage_Stop(0.0, null), every not stopped instance of every
    // group with blend 0; the second finds none.
    stop_montage(w, f, Some(0.0), None);
    stop_dash_montage(w, f, 0.0);
    stop_grouped(w, f, None, 0.0);
    // @166: the HitstunTimer alone (see above).
    let x = w.fighter_mut(f);
    x.hit.hitstun = 0.0;
    x.hit.hitstun_paused = false;
    x.hit.hitstun_pending = false;
    if x.hit.launch.as_ref().is_some_and(|l| !l.di) {
        x.hit.free_launch = true;
    }
    x.attackers.stun_source = false;
    stun_changed(w, f, true);
    // @881 BPI_UpdateAirControl(0.0), whatever the fighter was doing. The hitstun end's BPI_ResetAirControl gives it
    // back (`buff_stun_end`).
    update_air_control(w, f, 0.0);
    clear_queued_attack(w, f);
    let x = w.fighter_mut(f);
    x.buffs.buff_stunned = true;
    x.buffs.buff_stun = timer_ticks(Some(max(sec, 0.3))) + 1.0; // pending in the frame it is set in
    x.buffs.buff_stun_len = max(sec, 0.3);
    x.buffs.buff_stun_age = -1.0;
    // @2271 SetStruggleUiServer(true) -> @7226 SetStruggleUi(true) @6912-7065: SetStruggleForPlayer(HitStunDuration,
    // HitStunDuration, 2, false), then SetStruggleTimerOnOwner(HitStunDuration, true) -> @10254-10179 the same call
    // with Hide Input true: two calls in one chain, so the prompt is hidden from the start.
    if can_struggle {
        let len = x.buffs.buff_stun_len;
        struggle_ui(w, f, len, false);
        struggle_ui(w, f, len, true);
    }
    if visuals {
        buff_stun_visuals(w, f);
    }
    // @2372 HitStunStart, BaseFighter's handler (ubergraph 176159-176181): AirControlGate and DashFallGate false,
    // CheckLateralFriction (0 with the stun's source on). Nothing in the stun's end sets the gates back.
    lateral_friction_event(w, f, false, false);
    if let Some(hk) = w.fighter(f).hooks.char.hit_stun_start {
        hk(w, f); // HitStunStart's other bindings (CharHooks.hitStunStart)
    }
    // After HitStunStart (@2372), an owner that is a BP_Fighter_Character (@2476-2945): with (IsMovingOnGround &&
    // Stun.SuspendPositionOnGround) || (!IsMovingOnGround && Stun.SuspendPositionInAir), CurrentlySuspended = true
    // @2733, CharacterMovement.SetMovementMode(5 Flying) @2744 and StopMovementImmediately @2806; else a fighter
    // still suspended by an earlier stun goes to SetMovementMode(3 Falling) @2883 and CurrentlySuspended = false @2945.
    let x = w.fighter_mut(f);
    let grounded = x.body.on_floor;
    if (grounded && suspend_ground) || (!grounded && suspend_air) {
        x.buffs.suspended = true;
        suspend_hold(x);
    } else if x.buffs.suspended {
        // SetMovementMode(Falling) ends any Flying, the one a Blueprint set before the stun too (Buff_Charm @6864)
        x.buffs.suspended = false;
        x.moves.flying = false;
    }
}

/// BuffStunVisuals (HitstunHandler ubergraph 6253, the owner cast to BP_FighterCharacter @6338), reaction 6 with
/// PlayDanceMontage (the caller's test; any other reaction is PlayAnimMontage(null, 0.0001) @6422: nothing):
/// BPI_GetSleepMontage @6595 (BaseFighter: the TauntManager's GetDanceMontage, the DanceTaunt's montage) and, with a
/// montage (@6641-6709), PlayAnimMontage(it, 1.0, None) @6719. A pawn without a DanceTaunt (Pawn_C018, Pawn_C021)
/// gets no montage and stands in the stun's pose. No dance montage has root motion. The stun's end stops it
/// (`buff_stun_end`: StopStunMontage @861). A plain PlayAnimMontage outside the branch manager: no BeginAttack.
// Unverified: startMontage's other ride-alongs (attack count, charge resets, the hit flag reset) are taken as
// harmless here.
fn buff_stun_visuals(w: &mut World, f: FighterId) {
    let ch = w.fighter(f).ch;
    let Some(Some(m)) = &fighter_data(w, ch).dance_montage else { return };
    let m = crate::fighter::common::lit(w, m);
    start_montage(w, f, m, &0, None);
    w.fighter_mut(f).charge.begin_attack = false;
}

/// SetMovementMode(Flying) and StopMovementImmediately: off the floor and off its base, out of the knockback mode,
/// no velocity. The CMC tick of a suspended fighter moves nothing. StopMovementImmediately (140fb90c0) clears
/// neither PendingLaunchVelocity nor the stored NetImpulse.
// Deviation: the one movement tick a launch pending at the suspension (or made under it) takes in the build is not
// made: the velocity is zeroed without a move, and any other pending impulse waits for the suspension's end.
fn suspend_hold(x: &mut Fighter) {
    x.body.vx = 0.0;
    x.body.vy = 0.0;
    set_grounded(x, false);
    x.hit.kb_mode = false;
}

/// HitstunHandler ReceiveTick @8299 -> @9448-9595: while CurrentlySuspended, every tick SetMovementMode(5) @9533 and
/// StopMovementImmediately @9595. The component's tick is taken at the frame's start, as `struggle_tick`.
pub fn suspend_tick(w: &mut World, f: FighterId) {
    let x = w.fighter_mut(f);
    if x.buffs.suspended {
        suspend_hold(x);
    }
}

/// Comp_State_Manager UpdatePlayerStatus's stun part: CalculateBuffValues' pick (@2642-2969) and ApplyStunBuffs.
/// The pick, over the new buff first and then the live buffs: the largest Stun.StunDuration above 0, the first of
/// equals. With a pick that is not CurrentStunActor (@14-95) it becomes CurrentStunActor (@109) and its stun starts
/// (@291-400: `buff_stun_start`), then SetMostRecentAttacker(its Instigator, false) when that is a fighter
/// (@443-680). The same pick again starts nothing. Without a pick, IsBuffStunned -> RemoveHitStunFromBuff
/// (@733-841): `ended` (the call of a stun buff's EndPlay) stands for IsBuffStunned. `nb`: the call's NewBuff.
/// `pawn_gone`: inside the fighter's spawn's buff ends.
pub fn apply_stun_buffs(w: &mut World, f: FighterId, nb: Option<BuffRef>, ended: bool) {
    w.fighter_mut(f).buffs.stun_picks += 1;
    if pawn_gone(w) {
        buff_stun_end(w, f, false);
        return;
    }
    let mut best = 0.0;
    let mut pick: Option<u32> = None;
    let order: Vec<u32> =
        nb.map(|b| b.serial).into_iter().chain(w.fighter(f).buffs.buffs.iter().map(|b| b.serial)).collect();
    for s in order {
        let x = w.fighter(f);
        let Some(b) = x.buffs.buffs.iter().find(|b| b.serial == s) else { continue };
        if b.frames <= 0.0 || pick == Some(s) {
            continue;
        }
        let d = stun_num(stun_of(w, f, s), "StunDuration").unwrap_or(0.0);
        if !(d > 0.0 && d > best) {
            continue;
        }
        best = d;
        pick = Some(s);
    }
    let Some(pick) = pick else {
        if ended {
            buff_stun_end(w, f, true);
        }
        return;
    };
    let x = w.fighter_mut(f);
    let i = x.buffs.buffs.iter().position(|b| b.serial == pick).unwrap_or_default();
    if x.buffs.buffs[i].stun {
        return;
    }
    for b in &mut x.buffs.buffs {
        b.stun = false;
    }
    x.buffs.buffs[i].stun = true;
    let by = x.buffs.buffs[i].stun_by;
    let stun = stun_of(w, f, pick).cloned();
    buff_stun_start(w, f, best, stun.as_ref());
    if let Some(by) = by {
        set_most_recent_attacker(w, f, by);
    }
}

/// The frame's timers and the HitstunHandler's ReceiveTick for a running buff stun (beginFrame).
/// IncreaseStruggleMaxServer (@10419): below StruggleMaxCount, StruggleInputsAvailable + 1, then
/// IncreaseStruggleMaxOwner (@10970), a Client RPC that adds 1 again where it runs. ReceiveTick (@8376): while the
/// BuffstunTimer is active, CheckStruggleTime (@8123). After both additions, still under the test @10494:
/// SetStruggleTimerOnOwner (@10614-10937, `struggle_timer_ui`).
// Unverified: offline (NM_Standalone) every RPC runs locally (stock UE 4.26 GetFunctionCallspace, not read in the
// exe), so each period adds 2 here; a live read of StruggleInputsAvailable over a plasma hold settles it.
// Unverified: the component's tick is taken at the frame's start, after the timers.
pub fn struggle_tick(w: &mut World, f: FighterId) {
    let x = w.fighter_mut(f);
    let Some(s) = &mut x.buffs.struggle else { return };
    s.left -= 1.0;
    if s.left <= 0.0 {
        s.left = s.period;
        if s.avail < s.max {
            s.avail += 2.0;
            struggle_timer_ui(w, f);
        }
    }
    check_struggle_time(w, f);
}

/// UI_Broker SetStruggleForPlayer(the fighter's player index, Time Left, HitStunDuration, 2, Hide Input) as the
/// HitstunHandler calls it. HitStunDuration is the buff stun's clamped duration (AddBuffHitStun @412-463). The
/// calls: 1, 2 the stun's start under CanStruggle; 3 IncreaseStruggleMaxServer, 4 Struggle (`struggle_timer_ui`);
/// 5 RemoveHitStunFromBuff @608-832 SetStruggleUiServer(false) -> @7090-7163: (0, false), at every end of a buff
/// stun; 6 AddDecayedHitStun @570 EndEscapeServer(null, null) -> UpdateEscapeUI(false) @3462-3555: (0, false), where
/// a knockback hit replaces a running HitstunTimer. UpdateEscapeUI(true) (@3154-3395) is not ported: its one caller
/// is the EscapeAvailableTimer's, set only under CanUseEscape, which nothing writes. An event of the tick, not state.
pub fn struggle_ui(w: &mut World, f: FighterId, time_left: Real, hide_input: bool) {
    let x = w.fighter_mut(f);
    let ev = Event::StruggleUi { fighter: x.id, time_left, total: x.buffs.buff_stun_len, hide_input };
    x.events.push(ev);
}

/// SetStruggleTimerOnOwner(FMax(K2_GetTimerRemainingTimeHandle(BuffstunTimer) - StruggleIncrement x StruggleCount,
/// 0), StruggleCount >= StruggleMaxCount OR StruggleCount >= StruggleInputsAvailable) (@5608-5931, @10614-10937).
/// After a struggle break: StruggleIncrement 0 (@7383), StruggleCount and StruggleInputsAvailable 0, so Hide Input
/// is true and Time Left the timer's remaining time (0 then: `buff_stun_end`'s clear at every end is the build's).
fn struggle_timer_ui(w: &mut World, f: FighterId) {
    let left = if w.fighter(f).buffs.buff_stunned { buff_stun_remaining(w, f) } else { 0.0 };
    match &w.fighter(f).buffs.struggle {
        None => struggle_ui(w, f, max(left, 0.0), true),
        Some(s) => {
            let t = max(left - (s.inc * s.count), 0.0);
            let hide = s.count >= s.max || s.count >= s.avail;
            struggle_ui(w, f, t, hide);
        }
    }
}

/// CheckStruggleTime (@8123): StruggleIncrement x StruggleCount >= the BuffstunTimer's remaining time ->
/// StruggleBreak (@7383: StruggleIncrement 0, RemoveHitStunFromBuff).
pub fn check_struggle_time(w: &mut World, f: FighterId) {
    let x = w.fighter(f);
    let Some(s) = &x.buffs.struggle else { return };
    if !x.buffs.buff_stunned {
        return;
    }
    if (s.inc * s.count) >= buff_stun_remaining(w, f) {
        buff_stun_end(w, f, false);
    }
}

/// `onStruggling`: the Struggling broadcast of fighter `v` (`struggle_input`), asked of every
/// fighter of the match (`peers`, in their order) whose character enters the hook (`CharHooks::struggling`): each
/// answers for the bindings it holds in its own state (Comp_JakeEat BindOrUnbindStruggle @10042-10130 ->
/// StruggleServer, `chars::jake::eat`).
// Deviation: the listeners run in the order of the fighters that hold them, not in the order of their binds (one
// listener class in this build).
fn struggling(w: &mut World, v: FighterId) {
    let mut i = 0;
    while let Some(&j) = peers(w).get(i) {
        if let Some(h) = w.fighter(j).hooks.char.struggling {
            h(w, j, v);
        }
        i += 1;
    }
}

/// StruggleInput (@4691), bound to BP_FighterCharacter.InputReleased: ButtonRelease (BaseFighter @131535-131632)
/// broadcasts it for every released button with no gate, so under a hitpause too. Struggle (@5318): with
/// StruggleCount below StruggleMaxCount and below StruggleInputsAvailable, StruggleCount + 1, CheckStruggleTime
/// (@5594) and SetStruggleTimerOnOwner (@5608-5931). The Button argument is not tested: every release is one
/// Struggle call; Enum_InputButton 0-5 (Taunt 4 from `taunt_held`, Item 5 ButtonRelease(5) @169712) are the six inputs here, one
/// release a button a tick, in this order. Struggle @5318 broadcasts Struggling first, ahead of its count test: the handlers
/// bound to this fighter's HitstunHandler ([`struggling`]), which may end the stun inside the call.
pub fn struggle_input(w: &mut World, f: FighterId) {
    if w.fighter(f).buffs.struggle.is_none() {
        return;
    }
    for k in 0..6 {
        let c = &w.fighter(f).controls;
        let (now, was, force) = match k {
            0 => (c.inp.attack, c.prev.attack, c.inp.force_release == Some(AttackBtn::Attack)),
            1 => (c.inp.special, c.prev.special, c.inp.force_release == Some(AttackBtn::Special)),
            2 => (c.inp.jump, c.prev.jump, false),
            3 => (c.inp.dodge, c.prev.dodge, false),
            4 => (c.inp.taunt_held, c.prev.taunt_held, false),
            _ => (c.inp.item, c.prev.item, false),
        };
        if !((was && !now) || force) {
            continue;
        }
        struggling(w, f);
        let counted = w.fighter_mut(f).buffs.struggle.as_mut().is_some_and(|s| {
            let ok = s.count < s.max && s.count < s.avail;
            if ok {
                s.count += 1.0;
            }
            ok
        });
        if counted {
            check_struggle_time(w, f);
            struggle_timer_ui(w, f);
        }
        // StruggleInput @4988-5148, after Struggle() and the StruggleSuccess broadcast with no test of its own: the
        // owner's BPI_ControllerRumble(Small), whether Struggle counted the release or not.
        let r = name!("SmallForceFeedbackRumbleEffect");
        controller_rumble(w, f, Some(r));
        if w.fighter(f).buffs.struggle.is_none() {
            return;
        }
    }
}

/// RemoveHitStunFromBuff (HitstunHandler ubergraph 7411 -> 608): from the BuffstunTimer, a struggle break, a
/// knockback hit with hitstun on a buff-stunned fighter (AddDecayedHitStun @1038 / @2488) and the state manager when
/// no stun buff is left (ApplyStunBuffs @818). HitstunBuffActive off, 'Unbind Input Struggle', StopStunMontage
/// (StopAnimMontage(null), @861), RemoveBuffstunSource (@875), BuffStunEnded (@889; a hold's Escaped,
/// Buff_HeldByEnemy @7270 -> 8026: K2_DestroyActor); then, unless CheckHitstunActive (@1094: a source counter,
/// never a timer), the hitstun end (@1132-). At a hit, @2488 is ahead of the hit's AddHitstunSource (@2556), so the
/// whole end runs there too, before the new hitstun starts. `force`: the state manager's call, which does not test
/// HitstunBuffActive.
pub fn buff_stun_end(w: &mut World, f: FighterId, force: bool) {
    let was = w.fighter(f).buffs.buff_stunned;
    if !was && !force {
        return;
    }
    let x = w.fighter_mut(f);
    x.buffs.buff_stunned = false;
    x.buffs.buff_stun = 0.0;
    struggle_ui(w, f, 0.0, false); // @608-832 SetStruggleUiServer(false), ahead of the unbind: the widget hides
    w.fighter_mut(f).buffs.struggle = None; // 'Unbind Input Struggle' (@847 -> 5964)
    stop_anim_montage(w, f);
    if was {
        stun_changed(w, f, false); // RemoveBuffstunSource: only with the source on
    }
    if w.fighter(f).holds.hold.as_ref().is_some_and(|h| h.bound) {
        end_hold_buff(w, f);
    }
    buff_signal(w, f, BuffSignal::BuffStunEnded, BuffSignalArg::default());
    // @908-1032, after the broadcast: CurrentlySuspended off and SetMovementMode(3 Falling). Not suspended: no mode
    // is written, so a Flying set beside the stun stays.
    let x = w.fighter_mut(f);
    if x.buffs.suspended {
        x.buffs.suspended = false;
        x.moves.flying = false;
    }
    // The marker of a launch applied before the stun, where no frame boundary has dropped it yet: its hitstun end
    // is this one.
    if x.hit.launch.as_ref().is_some_and(|l| l.di) && x.hit.hitstun <= 0.0 {
        x.hit.launch = None;
    }
    if !x.attackers.stun_source && !(x.moves.pull.is_some() && !was) {
        hitstun_ended(w, f);
    }
}

/// K2_DestroyActor on this fighter's hold buff: its EndPlay runs in the buff sweep (`hold_end`).
pub fn end_hold_buff(w: &mut World, f: FighterId) {
    let x = w.fighter_mut(f);
    let Some(h) = &x.holds.hold else { return };
    let cls = h.cls;
    let Some(b) = x.buffs.buffs.iter_mut().find(|b| b.class == cls && b.frames > 0.0) else {
        hold_end(w, f, true);
        return;
    };
    b.frames = 0.0;
    tick_buffs(w, f, true);
}

/// Buff_HeldByEnemy KnockedBack / OnInstigatorHitStunStart (ubergraph 7848), for every hold this fighter is the
/// instigator of: the held fighter's SetMovementMode(Falling), RemoveHitstunFromKnockBack if its HitstunBuffActive,
/// K2_DestroyActor. BuffParentActor's own bindings come first: ReceiveBeginPlay (authority, @1607-1612) with
/// GetInstigator a BP_FighterCharacter binds its DamageManager.KnockedBack (@2443 -> @5766), its
/// HitstunHandler.HitStunStart (@2558 -> @5855) and its IsPickedUp (@2622 -> @5899): with
/// DestroyOnInstigatorInterrupt in the class's defaults each is K2_DestroyActor and nothing else. Buff_HeldByPlasmaBall
/// and Buff_HeldBy_C017 set it, of the exported classes. IsPickedUp is broadcast by LassoProjectileFastV3 (@14591)
/// and TetherProjectile (@11298) alone ([`picked_up`]).
pub fn holder_interrupted(w: &mut World, f: FighterId) {
    picked_up(w, f);
    let vics = w.fighter(f).holds.hold_victims.clone();
    for v in vics {
        end_hold_buff(w, v);
    }
}

/// A knockback hit on this fighter (`receive_hit`). AddDecayedHitStun with HitstunBuffActive and a computed hitstun
/// above 0 (`stun`): RemoveHitStunFromBuff (@1038 / @2488), the whole of it. A hit without hitstun (@3723:
/// RemoveHitstunFromKnockBack alone, which stops at the buff's source) leaves the buff stun on. The DamageManager's
/// KnockedBack (@10471): the hold this fighter is in, once bound and with CanGetKnockedOut (@7140-7184), and the
/// holds it is the instigator of -> @7848.
// Unverified: both are taken ahead of the hit's own hitstun start; @7848's RemoveHitstunFromKnockBack would clear the
// hit's hitstun if the broadcast came after it.
pub fn hold_knocked_back(w: &mut World, f: FighterId, stun: bool, arg: Option<BuffSignalArg>) {
    if stun {
        buff_stun_end(w, f, false);
    }
    if let Some(h) = &w.fighter(f).holds.hold
        && h.bound
    {
        let cls = class_name(w, h.cls);
        if def_flag(w, f, cls, "CanGetKnockedOut") {
            end_hold_buff(w, f);
        }
    }
    buff_signal(w, f, BuffSignal::KnockedBack, arg.unwrap_or_default());
    holder_interrupted(w, f);
}

/// What a child class of Buff_HeldByEnemy adds to the parent's graph (`HoldKind`). A child's kind comes with its
/// character's module.
#[derive(Default)]
pub struct HoldKind {
    /// The child's ReceiveBeginPlay ahead of its call of the parent's (Buff_EatenByC015 @0-248, the parent's @275),
    /// run once the hold is made, before any of the parent's BeginPlay; what it writes there that the parent reads
    /// (AttachOffset, AttachSocket) is kept on the hold, which is not on the fighter yet. True: its K2_DestroyActor
    /// ahead of the parent's ReceiveBeginPlay (Buff_HeldByEnemyGarnet @513): no hold is made.
    pub before_begin: Option<fn(&mut World, FighterId, &mut Hold) -> bool>,
    /// After the parent's ReceiveBeginPlay.
    pub begin: Option<fn(&mut World, FighterId)>,
    /// ReceiveTick; true: K2_DestroyActor.
    pub tick: Option<fn(&mut World, FighterId) -> bool>,
    /// After the parent's ReceiveEndPlay (the hold is off the fighter).
    pub end: Option<fn(&mut World, FighterId, &Hold)>,
    /// The box track of AttachSocket on the holder (the argument), where it is not `socket:` + AttachSocket.
    pub track: Option<fn(&World, FighterId) -> Name>,
    /// The handlers it binds to OnExitArenaWarning and OnExitedArena of the held fighter or of its instigator
    /// (`from`). True: K2_DestroyActor.
    pub warning: Option<HoldHandler>,
    pub exited_arena: Option<HoldHandler>,
    /// The child's own latent actions (a Delay of its BeginPlay: Buff_JakEat @699 / @4274), run after the parent's
    /// (`hold_tick`) in the same pass. True: K2_DestroyActor.
    pub latent: Option<fn(&mut World, FighterId) -> bool>,
}

/// A child's handler: (the held fighter, the broadcaster `from`) -> K2_DestroyActor.
pub type HoldHandler = fn(&mut World, FighterId, FighterId) -> bool;

/// Buff_HeldByEnemy_C's own kind: nothing over the parent.
const HELD_BY_ENEMY: HoldKind = HoldKind {
    before_begin: None,
    begin: None,
    tick: None,
    end: None,
    track: None,
    warning: None,
    exited_arena: None,
    latent: None,
};

/// `holdKind`: the kind of a hold class.
pub fn hold_kind(cls: BuffClass) -> Option<&'static HoldKind> {
    if matches!(cls, BuffClass::Data(c) if c == crate::chars::iron_giant::hold::CLASS) {
        return Some(&crate::chars::iron_giant::hold::KIND);
    }
    match cls {
        BuffClass::BuffHeldByEnemyGarnet => return Some(&crate::chars::garnet::hold::KIND),
        BuffClass::BuffEatenByC015 => return Some(&crate::chars::taz::eat::KIND),
        BuffClass::BuffJakEat => return Some(&crate::chars::jake::eat_buffs::KIND),
        _ => {}
    }
    Some(&HELD_BY_ENEMY)
}

/// BP_FighterCharacter OnExitArenaWarning of fighter f, broadcast by BlastBoxPlayerExit (the warning box's end @432
/// -> @3009, the projectile-stop box's end @423 -> @3438): the handlers hold buffs bound to it (`HoldKind.warning`):
/// the hold f is in (bound to its owner's delegate: Buff_EatenByC015 @1393) and the holds f is the instigator of
/// (@1556), each in turn; a true answer is K2_DestroyActor on that buff. SetActorEnableCollision(false) ends every
/// overlap of the actor inside the call, with GetActorEnableCollision already false: the warning box does not
/// broadcast for that end (its @2585 gate), the projectile-stop box does (no gate), so a fighter whose actor
/// collision is off broadcasts nothing (`at_disable`: the call is the projectile-stop box's end inside the disable).
pub fn hold_arena_warning(w: &mut World, f: FighterId, at_disable: bool) {
    if (w.fighter(f).moves.pull.is_some() || buff_collision_off(w, f)) && !at_disable {
        return;
    }
    hold_handlers(w, f, |k| k.warning);
}

/// The hold f is in and the holds f is the instigator of, each asked through `pick` of its kind.
fn hold_handlers(w: &mut World, f: FighterId, pick: fn(&HoldKind) -> Option<HoldHandler>) {
    if let Some(h) = &w.fighter(f).holds.hold
        && let Some(hk) = hold_kind(h.cls).and_then(pick)
        && hk(w, f, f)
    {
        end_hold_buff(w, f);
    }
    let vics = w.fighter(f).holds.hold_victims.clone();
    for v in vics {
        if let Some(h) = &w.fighter(v).holds.hold
            && let Some(hk) = hold_kind(h.cls).and_then(pick)
            && hk(w, v, f)
        {
            end_hold_buff(w, v);
        }
    }
}

/// BP_FighterCharacter OnExitedArena of fighter f (the ring-out): the handlers hold buffs bound to it in their own
/// BeginPlay (`HoldKind.exited_arena`: Buff_JakEat's JakeDeath @2378 / @2442), for the hold f is in and for the holds
/// f is the instigator of; a true answer is K2_DestroyActor on that buff. The parent's Ringed Out is bound later (at
/// the attach) and is run at the respawn.
pub fn hold_exited_arena(w: &mut World, f: FighterId) {
    hold_handlers(w, f, |k| k.exited_arena);
}

/// Buff_OnExitedArenaTeleportToAlly_C, the rescue's caller class.
const RESCUE: &str = "Buff_OnExitedArenaTeleportToAlly_C";

/// Buff_StunnedMoveToPositionThroughGeo_C, one mover for its callers: Buff_OnExitedArenaTeleportToAlly's rescue
/// (`rescue_start`) and TetherProjectile's ally pull (`pull_toward`). The buff takes the time T as its caller gives
/// it (the caps are the callers'; the buff's own graph has only FMax(T, 0.01) on the timeline's rate, @770), the
/// destination and the end: each caller's own Delay(T) after StartMoveServer, then its StopAnimMontage(null), its
/// LaunchCharacter((0, 0, 200), true, true) and its K2_DestroyActor (`pull_end`).
/// StartMoveToAlly (ubergraph 3677-3126), run once by a sensor sphere that left the blast box:
/// sfx_cha_C019_teleport_ally_pull at the owner, SpawnBuffActor(Buff_StunnedMoveToPositionThroughGeo, owner,
/// instigator, 0, T) with T = FMin(GetDistanceTo(instigator) x 0.0004, 0.5), its StartMoveServer(the instigator's
/// location, T) and Delay(T). StartMoveServer (@53-544): SetCanSurviveBlastboxJumpOutArea(true), Stun.StunDuration =
/// T @286, the state manager's UpdatePlayerStatus(null) @507 (`pull_start`), then StartMoveMulticast:
/// SetActorEnableCollision(false), MoveTimeline (linear 0 -> 1 over 1 s) at rate 1 / FMax(T, 0.01).
/// ApplyStunBuffs' SetMostRecentAttacker(the buff's Instigator, false) @680 refuses a team mate, and the instigator
/// is one.
// Deviation: the stun buff is not in the buff list (its StateInfo is not applied) and its class defaults are not in
// the data: its Stun struct is taken without a struggle and without a suspension; HitReactInt keeps its last value.
pub fn rescue_start(w: &mut World, f: FighterId, dest_x: Real, dest_actor_y: Real) {
    let x = w.fighter_mut(f);
    if x.moves.pull.is_some() {
        return;
    }
    let Some(b) = x.buffs.buffs.iter_mut().find(|b| b.rescue.is_some() && b.frames > 0.0 && b.pulled != Some(true))
    else {
        return;
    };
    b.pulled = Some(true);
    let (id, px) = (x.id, x.body.x);
    let py = actor_y(w, f);
    let ev = name!("sfx_cha_C019_teleport_ally_pull");
    w.fighter_mut(f).events.push(Event::Sound {
        fighter: id,
        ev,
        x: Some(px),
        y: Some(py),
        hit: None,
        pooled: false,
        key: None,
    });
    let t = pull_time(ue::size2d([dest_x - px, dest_actor_y - py]), 0.5, 1.0);
    let end = name!("Buff_OnExitedArenaTeleportToAlly_C");
    pull_start(w, f, dest_x, dest_actor_y, t, end, -1);
}

/// T = FMin(GetDistanceTo x 0.0004, cap) x mult in float32, as the callers' Multiply_FloatFloat / FMin / SelectFloat
/// (TetherProjectile @3705-3835, Buff_OnExitedArenaTeleportToAlly @2789).
pub fn pull_time(dist: Real, cap: Real, mult: Real) -> Real {
    let t = min(dist * 0.0004, cap);
    if mult == 1.0 { t } else { t * mult }
}

/// The end both callers run: StopAnimMontage(null), LaunchCharacter((0, 0, 200), true, true) on `f`.
pub fn pull_launch(w: &mut World, f: FighterId) {
    stop_anim_montage(w, f);
    w.fighter_mut(f).moves.impulse = Some(Impulse { vx: 0.0, vy: 200.0, xy: true, z: true, kind: 0, d: None });
}

/// The tether's call (TetherProjectile Activate, normal branch @3330-4611): T = FMin(GetDistanceTo(owner, ally) x
/// 0.0004, cap) x mult; the destination the instigator's location, raised by (allyHalf - instigatorHalf) when
/// !BPI_GetIsInAir(instigator) and allyHalf > instigatorHalf (`raw_half`). `raise` false is the plain location (the
/// BigBrother branch @2103). Returns T: the caller's Delay(T).
#[allow(clippy::too_many_arguments, reason = "the call's inputs")]
pub fn pull_toward(
    w: &mut World,
    f: FighterId,
    o: FighterId,
    cap: Real,
    raise: bool,
    mult: Real,
    end: Name,
    by: i32,
) -> Real {
    let (fx, fy, ox, oy) = (w.fighter(f).body.x, actor_y(w, f), w.fighter(o).body.x, actor_y(w, o));
    let t = pull_time(ue::size2d([ox - fx, oy - fy]), cap, mult); // DistanceTo
    let (hf, ho) = (raw_half(w, f), raw_half(w, o));
    let lift = if raise && !in_air(w, o) && hf > ho { hf - ho } else { 0.0 };
    pull_start(w, f, ox, oy + lift, t, end, by);
    t
}

/// CapsuleComponent.CapsuleHalfHeight, the unscaled field: Crouch writes it (Max(CrouchedHalfHeight, the unscaled
/// radius)), so a crouched fighter has the CMC's CrouchedHalfHeight there (the template's: baseCh), the standing one
/// capHalfU.
pub fn raw_half(w: &World, f: FighterId) -> Real {
    let x = w.fighter(f);
    if x.capsule.cmc_crouched {
        max(crate::math::narrow(def_of(w, x.base_ch).crouched_half), x.capsule.cap_radius_u)
    } else {
        x.capsule.cap_half_u
    }
}

/// StartMoveServer(destination, T) of a buff just spawned on f; `end` names the caller (`pull_end`). CalculateBuffValues'
/// pick: a live buff with a stun as long or longer stays the pick and nothing starts.
// Deviation: a second buff on a fighter already in a move (its Move group breaks the first, whose caller's Delay
// still runs) is not ported: nothing starts.
pub fn pull_start(w: &mut World, f: FighterId, dest_x: Real, dest_actor_y: Real, t: Real, end: Name, by: i32) {
    if w.fighter(f).moves.pull.is_some() {
        return;
    }
    let mut other: Real = 0.0;
    for v in &w.fighter(f).buffs.buffs {
        if v.frames > 0.0 {
            other = max(other, stun_num(stun_of(w, f, v.serial), "StunDuration").unwrap_or(0.0));
        }
    }
    if t > 0.0 && t > other {
        for v in &mut w.fighter_mut(f).buffs.buffs {
            v.stun = false;
        }
        buff_stun_start(w, f, t, None);
    }
    let rate = 1.0 / max(t, 0.01);
    let (sx, sy) = (w.fighter(f).body.x, actor_y(w, f));
    let n = max((1.0 / (rate * DT) - 1e-6).ceil(), 1.0);
    w.fighter_mut(f).moves.pull = Some(Pull { sx, sy, dx: dest_x, dy: dest_actor_y, k: 0.0, n, rate, end, by });
}

/// MoveTimeline's update: K2_SetActorLocation(VLerp(StartingLocation, Destination, alpha), sweep: nothing to hit).
/// Finished: the stun buff is destroyed (its EndPlay's UpdatePlayerStatus: with no other stun buff
/// RemoveHitStunFromBuff -> the hitstun end, `apply_stun_buffs` with `ended`), and the caller's Delay(T) is over:
/// its end. A knockback's True Hitstun Timer still running keeps the fighter in Custom 2 through the end's launch.
// Unverified: the timeline and the Delay(T) are taken to end on the same frame, the launch being the pending launch
// of the next CMC tick.
pub fn pull_tick(w: &mut World, f: FighterId) {
    let Some(p) = &mut w.fighter_mut(f).moves.pull else { return };
    p.k += 1.0;
    let a = if p.k >= p.n { 1.0 } else { min(p.k * DT * p.rate, 1.0) };
    let (nx, ny, end, by) = (p.sx + (p.dx - p.sx) * a, p.sy + (p.dy - p.sy) * a, p.end, p.by);
    recall_move(w, f, nx, ny);
    if a < 1.0 {
        return;
    }
    w.fighter_mut(f).moves.pull = None;
    if w.fighter(f).buffs.buff_stunned {
        apply_stun_buffs(w, f, None, true);
    }
    // The destination is the centre of the instigator's capsule: a taller capsule ends inside the floor, and its next
    // move starts in penetration, resolved along the floor's normal (stock).
    // Deviation: taken as a lift to the rest height here (the tether's raised destination ends at or above it).
    // Every walking box whose span holds him (R past its edges) and that the capsule is in: the highest one's rest.
    let r = cap_radius(w, f);
    let ch = cap_half(w, f);
    let gap = def_of(w, w.fighter(f).ch).rest_gap;
    let mut lift = Real::NEG_INFINITY;
    let (fx, fy) = (w.fighter(f).body.x, w.fighter(f).body.y);
    // The box edges and the rest height in float32, as the build's component bounds.
    let gap = crate::math::narrow(gap);
    for g in floor_list(w, f) {
        let n = crate::math::narrow;
        let (left, right, top, bottom) = (n(g.left), n(g.right), n(g.top), n(g.bottom));
        let sy = (top + ch) + gap;
        if fx >= left - r && fx <= right + r && fy < sy && fy + ch > bottom && sy > lift {
            lift = sy;
        }
    }
    if lift > Real::NEG_INFINITY {
        w.fighter_mut(f).body.y = lift;
    }
    pull_end(w, f, end, by);
}

/// A caller's end of the move (`PULL_ENDS`): its Delay(T), taken to end on the move's last frame. The rescue's
/// (@3068): StopAnimMontage, LaunchCharacter, K2_DestroyActor (its end effect and sound); a rescue buff that ended
/// during the move (its 4 s, the instigator's ring-out) launches nothing. The tether's end is its character's
/// (`chars::creature::tether::pull_end`).
// Deviation: for the rescue the Delay is the move's end: a move cut short (a reset clears `pull`) runs no end; the
// rescue's buff ends at that reset too, so nothing is lost there.
fn pull_end(w: &mut World, f: FighterId, end: Name, by: i32) {
    if end == crate::chars::creature::tether::TETHER {
        crate::chars::creature::tether::pull_end(w, f, by);
        return;
    }
    if w.names.get(RESCUE) != Some(end) {
        return;
    }
    let Some(b) = w.fighter(f).buffs.buffs.iter().position(|v| v.pulled == Some(true) && v.frames > 0.0) else {
        return;
    };
    pull_launch(w, f);
    let e = &mut w.fighter_mut(f).buffs.buffs[b];
    e.rescue = Some(0.0);
    e.frames = 0.0;
    tick_buffs(w, f, true);
}

/// `pickedUp`: the instigator's IsPickedUp delegate, broadcast on the fighter a
/// lasso or the Creature's tether pulls. Its one binder is BuffParentActor (InstigatorPickedUp @5899): each peer's live
/// buff with DestroyOnInstigatorInterrupt whose Instigator is `f` (its `src`, else the holder of the hold of that
/// class) ends. `holder_interrupted` runs the same rule for KnockedBack and HitStunStart.
pub fn picked_up(w: &mut World, f: FighterId) {
    for i in 0..peers(w).len() {
        let v = peers(w)[i];
        let snapshot: Vec<(u32, Name, Option<FighterId>, BuffClass)> =
            w.fighter(v).buffs.buffs.iter().map(|b| (b.serial, b.cls, b.src, b.class)).collect();
        for (serial, cls, src, class) in snapshot {
            let live = w.fighter(v).buffs.buffs.iter().any(|b| b.serial == serial && b.frames > 0.0);
            if !live || !def_flag(w, f, cls, "DestroyOnInstigatorInterrupt") {
                continue;
            }
            let held_by = w.fighter(v).holds.hold.as_ref().filter(|h| h.cls == class).map(|h| h.by);
            if src.or(held_by) == Some(f) {
                end_buff(w, v, BuffRef { fighter: v, serial });
            }
        }
    }
}

/// InstigatorDeath: the instigator left the arena.
pub fn end_rescue_by(w: &mut World, f: FighterId, id: i32) {
    let x = w.fighter_mut(f);
    for b in &mut x.buffs.buffs {
        if b.rescue.is_some() && b.by == Some(crate::real(id)) && b.frames > 0.0 {
            b.rescue = Some(0.0);
            b.frames = 0.0;
        }
    }
    if x.buffs.buffs.iter().any(|b| b.frames <= 0.0) {
        tick_buffs(w, f, true);
    }
}

// ---- the grapple

/// Buff_HeldByEnemy ReceiveEndPlay (ubergraph 9568 -> 8676), run when the buff entry ends: OnExitedArena unbound;
/// BFResetGravityMulticast @9313, FMC.RemoveGrapple @9446, ClearGravityOverride @9482; the release position check
/// @4930-6965 (`release_check`); K2_SetActorRotation(zero) @7027. Then the child's. The buff stun's end is the parent
/// actor's (UpdateCurrentStates -> UpdatePlayerStatus(null) -> ApplyStunBuffs). `release` off: the owner's pawn is
/// gone (its defeat), where the check is left out.
pub fn hold_end(w: &mut World, vic: FighterId, release: bool) {
    let Some(mut h) = w.fighter_mut(vic).holds.hold.take() else { return };
    w.fighter_mut(h.by).holds.hold_victims.retain(|v| *v != vic);
    if h.attached {
        grapple_off(w, vic);
    }
    clear_gravity_override(w, vic); // @9482, attached or not
    h.attached = false;
    if release {
        release_check(w, vic, h.by);
    }
    apply_stun_buffs(w, vic, None, true); // the entry is ended already (frames 0): not a stun pick
    if let Some(e) = hold_kind(h.cls).and_then(|k| k.end) {
        e(w, vic, &h);
    }
}

/// Under the sweep's own rounding at a hit location.
const TOUCH: f64 = 1e-3;

/// The release position check (@4930-6965): where the held fighter is let go when the place it is held at is in
/// the stage. half = the owner's BPI_GetCapsuleHalfHeight (unscaled; @4930), r = the scaled radius of the owner's
/// Capsule (@5055: BaseFighter's own Capsule component, CharDef.innerCapRadius). CapsuleTraceSingle (r, half) from
/// InstigatorRef's actor location to the owner's Capsule location, trace type 5 (the stage block alone) ->
/// ReleaseLocationPreCheck = the hit's Location or the owner's Capsule location (@5607). CapsuleOverlapActors (r,
/// half) there (@5836). Neither (@5950): the owner is not moved. Else LineTraceSingle from the instigator's location
/// to it + (0, 0, half + 25) (@6198); candidate = overlap ? the instigator's Capsule location + (0, 0, half) :
/// ReleaseLocationPreCheck (@6716); final = line hit ? the instigator's location + (0, 0, its half height - (half +
/// 25)) : candidate (@6910); K2_SetActorLocation(final) (@6965). The owner's movement then starts from there.
// Deviation: the queries are the port's 2D capsule against the walking boxes and the polygon solids that block the
// held fighter (`fighter_polys`; Unverified: a polygon met by the shape traces' convex sweep and nearest point, not
// PhysX's convex queries). A capsule at its own sweep's hit
// location is taken not to overlap the body it stopped at; a line trace that starts inside the block is taken as a
// hit. Not tested: a third fighter's FullBody sphere while its Hitframe is on and a C019_MortarAim's Hurtbox.
fn release_check(w: &mut World, vic: FighterId, by: FighterId) {
    let k = w.fighter(vic).body.size_scale;
    let half = capsule_half(w, vic) / k;
    let r = def_of(w, w.fighter(vic).ch).inner_cap_radius * f64::from(k);
    let (ax, ay) = (w.fighter(by).body.x, actor_y(w, by));
    let (vx, vy) = (w.fighter(vic).body.x, actor_y(w, vic));
    let gs = floor_list(w, vic);
    let mut polys = Vec::new();
    crate::fighter::traces::solids::fighter_polys(w, vic, &mut polys);
    let rn = crate::math::narrow(r);
    let mut t = capsule_sweep_all(ax, ay, vx, vy, r, half, &gs).map(crate::math::narrow);
    for q in &polys {
        if let Some(tq) = q.capsule_sweep(ax, ay, vx, vy, rn, half)
            && t.is_none_or(|b| tq < b)
        {
            t = Some(tq);
        }
    }
    let (px, py) = match t {
        None => (vx, vy),
        Some(t) => (ax + (vx - ax) * t, ay + (vy - ay) * t),
    };
    let mut overlap = false;
    let mut line_hit = false;
    for g in &gs {
        if capsule_gap(px, py, r, half, g) < r - TOUCH {
            overlap = true;
        }
        if ax >= crate::math::narrow(g.left)
            && ax <= crate::math::narrow(g.right)
            && ay <= crate::math::narrow(g.top)
            && ay + half + 25.0 >= crate::math::narrow(g.bottom)
        {
            line_hit = true;
        }
    }
    // A polygon solid: the capsule's segment within r - TOUCH of it, the line through it or starting in it.
    let s = max(half - rn, 0.0);
    for q in &polys {
        if r > TOUCH && f64::from(q.seg_dist2(px, py - s, px, py + s)) < (r - TOUCH) * (r - TOUCH) {
            overlap = true;
        }
        if q.ray(ax, ay, ax, ay + half + 25.0).is_some() {
            line_hit = true;
        }
    }
    if !overlap && t.is_none() {
        return;
    }
    if line_hit {
        let bh = capsule_half(w, by) / w.fighter(by).body.size_scale;
        recall_move(w, vic, ax, ay + bh - (half + 25.0));
    } else if overlap {
        recall_move(w, vic, ax, ay + half);
    } else {
        recall_move(w, vic, px, py);
    }
    settle(w, vic);
}

/// Buff_HeldByEnemy DestroyAttachedConsumableActors(Target): the hold's child asks for the call; the World makes it
/// in the pass after the fighter's tick that asked (`consume_requests`).
pub fn consume_attached(h: &mut Hold) {
    if h.consume != HoldConsume::All {
        h.consume = HoldConsume::Attached;
    }
}

/// Buff_JakEat ReceiveBeginPlay @3137-3813's loop over every attached spawned actor.
pub fn consume_all_attached(h: &mut Hold) {
    h.consume = HoldConsume::All;
}

/// The native grapple on the held fighter's movement component. InitiateGrapple 1410d0dc0: CMC +0x4c0 = 1,
/// DisableMovement (vtable +0x5d0, 1429297d0): SetMovementMode(None), whose OnMovementModeChanged (142936f60) on
/// entering None runs StopMovementImmediately (Velocity 0), ResetJumpState and ClearAccumulatedForces
/// (PendingLaunchVelocity); then the attach. While +0x4c0 is set PerformMovement takes the MOVE_None early exit
/// every tick, but SetMovementMode has no gate (`held_mode_tick`).
// Unverified: the Blueprint mode-changed event of the switch to None (BaseFighter K2_OnMovementModeChanged @127741)
// is taken to do nothing the stand-in does not.
// Unverified: a Flying fighter (a ControlSpeed command; a buff stun's suspension) keeps the port's flag under the
// grapple and after it.
pub fn initiate_grapple(w: &mut World, f: FighterId) {
    w.fighter_mut(f).buffs.grappled = true;
    disable_movement(w, f);
    let x = w.fighter_mut(f);
    x.hit.kb_mode = false; // None from (Custom, 2)
    x.body.vx = 0.0;
    x.body.vy = 0.0;
    set_grounded(x, false);
    x.moves.impulse = None;
    x.moves.hit_launch = false;
}

/// RemoveGrapple 1410dd000: +0x4c0 = 0, the MovementMode byte written 3 (Falling) directly (no mode-changed event),
/// then the root's detach KeepWorld.
pub fn remove_grapple(w: &mut World, f: FighterId) {
    w.fighter_mut(f).buffs.grappled = false;
    enable_falling(w, f);
    w.fighter_mut(f).hit.kb_mode = false;
}

/// A SetMovementMode made while the grapple holds the fighter, read after the port's own writes of the frame: the
/// knockback mode (ApplyKnockback's SetMovementMode(6, 2)) or a True Hitstun Timer that ended (its
/// SetMovementMode(Falling) @10319) leaves None.
pub fn held_mode_tick(w: &mut World, f: FighterId, timer_ended: bool) {
    let x = w.fighter(f);
    if x.buffs.grappled && (x.hit.kb_mode || timer_ended) {
        enable_falling(w, f);
    }
}

/// The hold classes (`holds({...})`): Buff_HeldByEnemy_C and the children the characters' modules enter.
const HOLDS: [Name; 6] = [
    name!("Buff_HeldByEnemy_C"),
    name!("Buff_Held_By_C021_C"),
    name!("Buff_HeldByEnemyGarnet_C"),
    name!("Buff_HeldBy_C017_C"),
    name!("Buff_JakEat_C"),
    name!("Buff_EatenByC015_C"),
];

/// `isHold`.
pub fn is_hold(cls: Name) -> bool {
    HOLDS.contains(&cls)
}

/// Buff_HeldByEnemy ReceiveBeginPlay (ubergraph 9592 -> 7729 -> 4542): InstigatorRef = GetInstigator as BaseFighter
/// (fail: nothing more), FighterRef = the owner; BindInstigatorKnockedBack (@9640: the instigator's
/// DamageManager.KnockedBack; @9726: its HitstunHandler.HitStunStart; both run @7848: `holder_interrupted`);
/// Delay(0.033) @4747. The buff's Stun is started by SpawnBuffActor's UpdatePlayerStatus, after this BeginPlay and the
/// child's (`spawn_buff` -> `apply_stun_buffs`).
// Unverified: the Delay is taken to count the spawn frame, as the rescue buff's (`rescue_begin`).
pub fn hold_begin(w: &mut World, vic: FighterId, cls: Name, by: Option<FighterId>) {
    let Some(by) = by else { return };
    if w.fighter(vic).holds.hold.is_some() {
        warn(w, vic, name!("a second hold on one fighter is not ported"));
        return;
    }
    let class = BuffClass::of(&w.names, w.names.str(cls)).unwrap_or(BuffClass::Data(cls));
    #[allow(clippy::cast_possible_truncation, reason = "a count of ticks")]
    let delay = max(latent(w, vic, 0.033) - 1.0, 1.0) as i32;
    let mut h = Hold {
        cls: class,
        by,
        delay,
        snap: 0,
        attached: false,
        bound: false,
        applied: false,
        offset: None,
        socket: None,
        grip: None,
        consume: HoldConsume::None,
        own: HoldOwn::None,
    };
    let kind = hold_kind(class);
    if let Some(b) = kind.and_then(|k| k.before_begin)
        && b(w, vic, &mut h)
    {
        return;
    }
    w.fighter_mut(vic).holds.hold = Some(h);
    w.fighter_mut(by).holds.hold_victims.push(vic);
    if let Some(b) = kind.and_then(|k| k.begin) {
        b(w, vic);
    }
}

/// The buff actor's tick, run from the owner's buff tick (`tick_buffs`): the child's ReceiveTick, then the latent
/// Delays of the parent's graph, then the child's own. True: a K2_DestroyActor on the buff (the caller, inside the
/// buff loop, ends the entry).
// Unverified: the buff actor is taken to tick ahead of the fighters, and its ReceiveTick ahead of its latent actions.
// Unverified: two Delays of one buff actor that end in the same frame (the parent's 0.033 attach @4747 and
// Buff_JakEat's 0.033 retry @699, both made in the one BeginPlay) are taken in the order they were made.
pub fn hold_tick(w: &mut World, vic: FighterId) -> bool {
    let Some(h) = &w.fighter(vic).holds.hold else { return false };
    let (cls, by) = (h.cls, h.by);
    let kind = hold_kind(cls);
    if let Some(t) = kind.and_then(|k| k.tick)
        && t(w, vic)
    {
        return true;
    }
    if parent_latent(w, vic) {
        return true;
    }
    // `vic.hold === h`: the hold of this tick is still the fighter's (a hold has no identity of its own here: the
    // class and the instigator stand for it).
    let same = w.fighter(vic).holds.hold.as_ref().is_some_and(|h| h.cls == cls && h.by == by);
    same && kind.and_then(|k| k.latent).is_some_and(|l| l(w, vic))
}

/// The parent's latent Delays: the attach (Delay(0.033) @4747) and the second K2_SetActorLocation (Delay(0.06)
/// @2927).
fn parent_latent(w: &mut World, vic: FighterId) -> bool {
    let Some(h) = &mut w.fighter_mut(vic).holds.hold else { return false };
    if h.delay > 0 {
        h.delay -= 1;
        if h.delay > 0 {
            return false;
        }
        let (by, cls, socket) = (h.by, h.cls, h.socket);
        // @15: the instigator's HitstunManager.HitstunBuffActive -> K2_DestroyActor.
        if w.fighter(by).buffs.buff_stunned {
            return true;
        }
        // @88, first pin -> @335: FMC.OverrideGravity(0.0) @432, the socket choice @469-1198,
        // InitiateGrapple(InstigatorRef.Mesh, socket) @1229, K2_SetActorLocation @2865, Delay(0.06) @2927. Its place:
        // InitiateGrapple attaches its root to the holder's mesh at the socket (AttachToComponent 1427f1200,
        // SnapToTarget).
        // Unverified: the attach chain is stood in for by `hold_follow`, the actor put at the socket plus AttachOffset
        // after the holder's mesh tick of every frame. The Blueprint itself sets that location only here and 0.06 s
        // later.
        let sock = match socket {
            Some(n) => w.names.str(n).to_owned(),
            None => attach_socket(w, vic, cls),
        };
        let mut grip = None;
        if has_lr(&sock) {
            // Kept only where it differs from the plain read (a twin on the mesh, or another name): "VB
            // Fighter_Hold_Bone" has "r_" (Contains ignores case) and no twin, so the parent's choice is the name.
            let g = grip_socket(w, by, &sock);
            if g != sock || has_socket(w, by, &mirror_twin(&g)) {
                grip = Some(w.names.get(&g).unwrap_or_else(|| panic!("the socket {g} is not interned")));
            }
        }
        if let Some(h) = &mut w.fighter_mut(vic).holds.hold {
            h.attached = true;
            if grip.is_some() {
                h.grip = grip;
            }
        }
        override_gravity(w, vic); // @432
        grapple_on(w, vic);
        hold_snap(w, vic);
        #[allow(clippy::cast_possible_truncation, reason = "a count of ticks")]
        let snap = latent(w, vic, 0.06) as i32;
        // Second pin @158: BindOwnerKnockedBack (@7140: CanGetKnockedOut -> the owner's DamageManager.KnockedBack ->
        // @7848; the owner's HitstunManager.BuffStunEnded -> Escaped), then both fighters' OnExitedArena -> Ringed
        // Out. The owner's IsPickedUp binding (@7356) is the item pick-up of a fighter, which nothing here does.
        if let Some(h) = &mut w.fighter_mut(vic).holds.hold {
            h.snap = snap;
            h.bound = true;
        }
        return false;
    }
    // @2982: not being destroyed, both fighters valid -> the same K2_SetActorLocation @4479.
    if h.snap > 0 {
        h.snap -= 1;
        if h.snap == 0 {
            hold_snap(w, vic);
        }
    }
    false
}

/// The class data's AttachSocket (`holdDef(cls).AttachSocket ?? 'None'`).
fn attach_socket(w: &World, vic: FighterId, cls: BuffClass) -> String {
    let d = hold_def(w, vic, class_name(w, cls));
    match d.get("AttachSocket") {
        Some(Value::Str(s)) => s.clone(),
        _ => "None".to_owned(),
    }
}

/// K2_SetActorLocation(FighterRef, (0, socket.Y + AttachOffset.Y (negated when the instigator faces right), socket.Z +
/// AttachOffset.Z), no sweep, teleport) (@1500-2865). AttachOffset.X goes into X, which is then 0. The socket:
/// AttachSocket (the child's where it wrote one); a name with L_ / R_ is mirrored for an instigator facing left when
/// that socket exists (@469-1198: `grip_socket`, kept as the hold's grip at the attach: the bone the grapple follows).
/// AttachSocket None: DoesSocketExist(None) is false, so the name is None (@1198 / @2430) for GetSocketLocation @2461
/// and InitiateGrapple @1229; the mesh's GetSocketLocation(None) is the component's own location
/// (USkinnedMeshComponent::GetSocketTransform finds neither a socket nor a bone): the mesh's RelativeLocation.Z (the
/// box index's meshZ) above the actor; a socket track has it baked in.
// Unverified: the sampled pose is the right-facing one, reflected with the facing: facing left, the left pose's socket
// G is where the right pose has G's mirror twin, so the track read is the twin of the grip facing left (where the twin
// exists) and the grip facing right.
fn hold_snap(w: &mut World, vic: FighterId) {
    let Some(h) = &w.fighter(vic).holds.hold else { return };
    let (cls, by, grip, socket, offset) = (h.cls, h.by, h.grip, h.socket, h.offset);
    let facing = w.fighter(by).body.facing_dir;
    let mut sock = match grip.or(socket) {
        Some(n) => w.names.str(n).to_owned(),
        None => attach_socket(w, vic, cls),
    };
    if let Some(g) = grip {
        let t = mirror_twin(w.names.str(g));
        if facing < 0.0 && has_socket(w, by, &t) {
            sock = t;
        }
    }
    let own = hold_kind(cls).and_then(|k| k.track).map(|t| t(w, by));
    let ch = w.fighter(by).ch;
    let mut px = 0.0;
    let mut py = char_data(w, ch).boxes.mesh_z;
    if own.is_some() || sock != "None" {
        let name = match own {
            Some(n) => w.names.str(n).to_owned(),
            None => format!("socket:{sock}"),
        };
        let fx = w.fixed.clone();
        let b = fx.samplers.get(&ch).unwrap_or_else(|| panic!("no box sampler for {}", w.names.str(ch)));
        let i = w.names.get(&name).and_then(|n| b.by_name.get(&n)).copied();
        let pose = i.and_then(|_| sampled_boxes(w, by));
        let (Some(i), Some(pose)) = (i, pose) else {
            warn(w, vic, name!("a hold's track is not on its holder"));
            return;
        };
        let o = b.offsets[i as usize] as usize;
        px = crate::real(pose[o]);
        py = f64::from(crate::real(pose[o + 1]));
        crate::fighter::anim::put_pose(w, pose);
    }
    let (oy, oz) = match offset {
        Some(o) => (o.y.unwrap_or(0.0), o.z.unwrap_or(0.0)),
        None => {
            let d = hold_def(w, vic, class_name(w, cls));
            {
                let (a, b) = match d.get("AttachOffset").and_then(Value::as_obj) {
                    Some(p) => (
                        p.get("Y").and_then(Value::as_f64).unwrap_or(0.0),
                        p.get("Z").and_then(Value::as_f64).unwrap_or(0.0),
                    ),
                    None => (0.0, 0.0),
                };
                (crate::math::narrow(a), crate::math::narrow(b))
            }
        }
    };
    // SelectFloat(Y, -Y, FacingLeft) (@1683 / @3297): +Y facing left, -Y facing right. FacingLeft's forward is +UE Y
    // and sim x is -UE Y, so a positive AttachOffset.Y is on the holder's facing side either way: +Y * facing in x.
    let nx = mesh_wx(w, by) + px * facing + oy * facing;
    let ny = mesh_wy(w, by) + crate::math::narrow(py) + oz;
    recall_move(w, vic, nx, ny);
}

/// `/[LR]_/i`: the name holds L_ or R_, either case.
fn has_lr(s: &str) -> bool {
    s.as_bytes().windows(2).any(|p| matches!(p[0], b'L' | b'l' | b'R' | b'r') && p[1] == b'_')
}

/// Every `/X_/gi` of `s` replaced by `to`.
fn replace_ci(s: &str, x: u8, to: &str) -> String {
    let b = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < b.len() {
        if i + 1 < b.len() && b[i].eq_ignore_ascii_case(&x) && b[i + 1] == b'_' {
            out.push_str(to);
            i += 2;
        } else {
            let c = s[i..].chars().next().expect("inside the string");
            out.push(c);
            i += c.len_utf8();
        }
    }
    out
}

/// The parent's socket choice (@469-1198, Contains and Replace ignoring case): the name with L_ made R_, or (without
/// an L_) R_ made L_.
fn mirror_twin(s: &str) -> String {
    if s.as_bytes().windows(2).any(|p| p[0].eq_ignore_ascii_case(&b'L') && p[1] == b'_') {
        replace_ci(s, b'L', "R_")
    } else {
        replace_ci(s, b'R', "L_")
    }
}

/// DoesSocketExist on the instigator's mesh: a socket, a bone of the mirror table or a virtual bone.
fn has_socket(w: &World, by: FighterId, s: &str) -> bool {
    let sk = &char_data(w, w.fighter(by).ch).fighter.skeleton;
    sk.sockets.contains_key(s)
        || sk.mirror.contains_key(s)
        || sk.virtual_bones.as_ref().is_some_and(|v| v.contains_key(s))
}

/// cand = the instigator's FacingLeft ? the twin : the name; None when neither cand nor the name exists on the
/// instigator's mesh, cand when the name has L_ or R_ and cand exists, else the name.
fn grip_socket(w: &World, by: FighterId, name: &str) -> String {
    let cand = if w.fighter(by).body.facing_dir < 0.0 { mirror_twin(name) } else { name.to_owned() };
    let cand_ok = has_socket(w, by, &cand);
    if !cand_ok && !has_socket(w, by, name) {
        return "None".to_owned();
    }
    if has_lr(name) && cand_ok { cand } else { name.to_owned() }
}

/// The stand-in for the native grapple: after the fighters' mesh ticks of a frame, every attached fighter is put at
/// its holder's socket in the new pose.
// Unverified: the attach chain itself (see `parent_latent`).
pub fn hold_follow(w: &mut World, fs: &[FighterId]) {
    for &f in fs {
        let attached = w.fighter(f).holds.hold.as_ref().is_some_and(|h| h.attached);
        if attached && !truthy(w.fighter(f).life.dead) {
            hold_snap(w, f);
        }
    }
}

/// JavaScript's truthiness of a number.
fn truthy(x: Real) -> bool {
    x != 0.0 && !x.is_nan()
}

/// A fighter's own pfgDoDamage on a held fighter (Comp_Pawn_HitBoxHandler.pfgDoDamage(null, victim, packet, ...)
/// called by a notify), queued by the notify in `hold_hits` and dealt by the World after the frame's ticks. The packet
/// is a Struct_HitBoxDamage; the World's hit reads a Hitframe's names for the direction and the sound.
// Deviation: the build deals it inside the notify.
pub fn deal_hold_hits(w: &mut World, f: FighterId) {
    let q = std::mem::take(&mut w.fighter_mut(f).holds.hold_hits);
    for r in q {
        if truthy(w.fighter(r.vic).life.dead) || truthy(w.fighter(f).life.dead) {
            continue;
        }
        let mut p = r.p;
        let nn = |v: Option<&Value>| v.filter(|v| !matches!(v, Value::Null)).cloned();
        let dir = nn(p.get("KnockBackDirectionVector")).or_else(|| p.get("KnockBackDirection").cloned());
        let snd = nn(p.get("OnHitAudio")).or_else(|| p.get("OnHitSound").cloned());
        set_or_clear(&mut p, "KnockBackDirection", dir);
        set_or_clear(&mut p, "OnHitSound", snd);
        let hp = crate::combat::HitPacket::from_props(&w.names, &p, None);
        direct_hit(w, f, r.vic, &hp);
    }
}

/// `{ ...p, k: v }`: `v` undefined leaves the key undefined (as absent).
fn set_or_clear(p: &mut Props, k: &str, v: Option<Value>) {
    match v {
        Some(v) => {
            p.insert(k.to_owned(), v);
        }
        None => {
            p.shift_remove(k);
        }
    }
}

/// The World's pass over the holds' DestroyAttachedConsumableActors requests (`consume_attached`,
/// `consume_all_attached`), in the pass after the fighter's tick that asked.
pub fn consume_requests(w: &mut World) {
    for i in 0..peers(w).len() {
        let vic = peers(w)[i];
        let Some(h) = &mut w.fighter_mut(vic).holds.hold else { continue };
        if h.consume == HoldConsume::None {
            continue;
        }
        let all = h.consume == HoldConsume::All;
        h.consume = HoldConsume::None;
        destroy_attached_consumable_actors(w, vic, all);
    }
}

/// Buff_HeldByEnemy DestroyAttachedConsumableActors(Target) (ubergraph @8042-8671; called by Buff_EatenByC015
/// BeginPlay @1897 and by Buff_JakEat): a counted loop over Target.GetAttachedActors(out, true), the array fetched again
/// for the length test (@8484-8634) and for each read (@8058, @8277); an entry that is a SpawnedActorParent
/// (@8159-8224) with DestroyOnConsume (@8234) gets K2_DestroyActor (@8378). The index goes on by one after a destroy,
/// so the actor after a destroyed one is skipped. DestroyOnConsume is false in every class's data;
/// MarkTeleportProjectile alone writes it, true at its attach to an enemy (@1508). `all`: Buff_JakEat ReceiveBeginPlay
/// @3137-3813's loop, every attached spawned actor destroyed @3707, the held item first (Comp_ItemManager attaches
/// HeldItem to the mesh, @439 / @583).
// Deviation: the build destroys inside the BeginPlay. The order of GetAttachedActors (the root's AttachChildren,
// attach order) is taken as the fighters in World order, each one's spawned actors in spawn order.
// Unverified: the held item is taken first in GetAttachedActors' order (the mesh is the root's first attach child,
// and the held item hangs on it).
pub fn destroy_attached_consumable_actors(w: &mut World, vic: FighterId, all: bool) {
    let mut on: Vec<ActorRef> = Vec::new();
    let held = if all { w.fighter(vic).items.held_item.map(ActorRef::Item) } else { None };
    if let Some(h) = held
        && !spawned_dead(w, h)
    {
        on.push(h);
    }
    for i in 0..peers(w).len() {
        let o = peers(w)[i];
        for s in w.fighter(o).items.spawned_actors.clone() {
            if !spawned_dead(w, s) && attached_to(w, s) == Some(vic) && Some(s) != held {
                on.push(s);
            }
        }
    }
    let mut i = 0;
    while i < on.len() {
        let s = on[i];
        if !all && !destroy_on_consume(w, s) {
            i += 1;
            continue;
        }
        on.remove(i);
        i += 1; // the loop's own step after the splice: the next actor is passed over
        match s {
            ActorRef::Projectile(_) | ActorRef::Item(_) => destroy_spawned(w, s),
            _ => mark_spawned_dead(w, s),
        }
    }
}

/// Where a grapple to an actor's root puts the held fighter: the root's world location (the sim's x and the actor y).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct GrappleRoot {
    pub x: Real,
    pub y: Real,
}

/// FighterMovementComponent.InitiateGrapple(root, None) (InitiateGrapple 1410d0dc0: AttachToComponent with location
/// SnapToTarget, CMC +0x4c0 set): the snap puts the held fighter's actor location at the root's, and from then on it
/// moves only with that root. A root that moves is snapped to by its caller's own pass.
pub fn grapple_to_root(w: &mut World, f: FighterId, root: GrappleRoot) {
    if w.fighter(f).holds.hold.as_ref().is_some_and(|h| h.attached) {
        warn(w, f, name!("a grapple to an actor's root over a hold is not ported"));
    }
    grapple_on(w, f);
    recall_move(w, f, root.x, root.y);
}

/// RemoveGrapple 1410dd000 on f's FighterMovementComponent (after `grapple_to_root`): Falling with no mode-changed
/// event, the root detached KeepWorld.
pub fn remove_root_grapple(w: &mut World, f: FighterId) {
    grapple_off(w, f);
}

/// Where a buff actor's root (its DefaultSceneRoot) sits: Comp_State_Manager SpawnBuffActor spawns the buff at the
/// owner's transform (GetTransform @660, BeginDeferredActorSpawnFromClass @710, FinishSpawningActor @933) and nothing
/// moves it after, so the root is the owner's actor location at the spawn. `owner`: a fighter or a projectile.
pub fn buff_root(w: &World, owner: ActorRef) -> GrappleRoot {
    match owner {
        ActorRef::Fighter(f) => GrappleRoot { x: w.fighter(f).body.x, y: actor_y(w, f) },
        ActorRef::Projectile(p) => {
            let pr = w.actors.projectiles.get(p).expect("a live projectile");
            GrappleRoot { x: pr.x, y: pr.y }
        }
        _ => panic!("buffRoot of an actor that is neither a fighter nor a projectile"),
    }
}

/// A non-fighter actor's root with a Mobility (`MobileRoot`): a projectile's, an item's, a World.actors actor's.
pub trait MobileRoot {
    /// `mobility`: 0 Static, 1 Stationary; `None` Movable.
    fn mobility(&mut self) -> &mut Option<u8>;
    /// `stationaryCount`: `None` at 0.
    fn stationary_count(&mut self) -> &mut Option<i32>;
}

impl MobileRoot for Projectile {
    fn mobility(&mut self) -> &mut Option<u8> {
        &mut self.mobility
    }
    fn stationary_count(&mut self) -> &mut Option<i32> {
        &mut self.stationary_count
    }
}

impl MobileRoot for crate::actors::world_actor::WorldActor {
    fn mobility(&mut self) -> &mut Option<u8> {
        &mut self.mobility
    }
    fn stationary_count(&mut self) -> &mut Option<i32> {
        &mut self.stationary_count
    }
}

impl MobileRoot for Item {
    fn mobility(&mut self) -> &mut Option<u8> {
        &mut self.mobility
    }
    fn stationary_count(&mut self) -> &mut Option<i32> {
        &mut self.stationary_count
    }
}

/// USceneComponent::SetMobility(m) on a non-fighter actor's root (EComponentMobility: 0 Static, 1 Stationary, 2
/// Movable). While the root is not Movable its ProjectileMovement's tick returns early and keeps the Velocity
/// (SetMobility 14299d210, ShouldSkipUpdate 14297c390). A Movable root holds no own value.
// Unverified: SetMobility re-registers the component; which overlaps begin again there is not read. Unverified: the
// actor's other movers are left as they are under a Stationary root (CheckStaticMobilityAndWarn 14298bc80).
pub fn set_root_mobility(a: &mut impl MobileRoot, m: u8) {
    *a.mobility() = if m == 2 { None } else { Some(m) };
}

/// SpawnedActorParent BPI_SetMobilityToStationary(bool) @6724-8048: true counts StationaryMobilityCount up and at 1
/// sets the root Stationary (@6878); false counts it down while above 0 (@7408) and at 0 sets the root Movable
/// (@7592). No spawned actor attaches to a projectile or an item, so the attached actors' AssertMobility pass has
/// nothing to reach.
pub fn root_stationary(a: &mut impl MobileRoot, on: bool) {
    let n = a.stationary_count().unwrap_or(0);
    if on {
        *a.stationary_count() = Some(n + 1);
        if n + 1 == 1 {
            set_root_mobility(a, 1);
        }
        return;
    }
    if n <= 0 {
        return;
    }
    if n - 1 == 0 {
        *a.stationary_count() = None;
        set_root_mobility(a, 2);
    } else {
        *a.stationary_count() = Some(n - 1);
    }
}

/// `setMobilityToStationary` on a spawned actor's root (`root_stationary`): a projectile's, an item's, a World.actors
/// actor's (of those only Jerry_SpawnedActor_C reads it: its step).
pub fn set_mobility_to_stationary(w: &mut World, a: crate::actors::ActorRef, on: bool) {
    use crate::actors::ActorRef;
    match a {
        ActorRef::Projectile(id) => {
            if let Some(p) = w.actors.projectiles.get_mut(id) {
                root_stationary(p, on);
            }
        }
        ActorRef::Item(id) => {
            if let Some(p) = w.actors.items.get_mut(id) {
                root_stationary(p, on);
            }
        }
        ActorRef::Actor(id) => {
            if let Some(p) = w.actors.actors.get_mut(id) {
                root_stationary(p, on);
            }
        }
        _ => crate::unported!("BPI_SetMobilityToStationary on a root that is no spawned actor's", w, a, on),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_twin_of_a_socket_name() {
        assert_eq!(mirror_twin("VB L_Hand"), "VB R_Hand");
        assert_eq!(mirror_twin("VB r_hand"), "VB L_hand");
        // An L_ anywhere wins: every l_ / L_ becomes R_, the r_ stays.
        assert_eq!(mirror_twin("l_arm_r_x"), "R_arm_r_x");
        assert_eq!(mirror_twin("VB Fighter_Hold_Bone"), "VB FighteL_Hold_Bone");
        assert!(has_lr("VB Fighter_Hold_Bone"));
        assert!(!has_lr("Head"));
    }

    struct Root(Option<u8>, Option<i32>);

    impl MobileRoot for Root {
        fn mobility(&mut self) -> &mut Option<u8> {
            &mut self.0
        }
        fn stationary_count(&mut self) -> &mut Option<i32> {
            &mut self.1
        }
    }

    #[test]
    fn stationary_by_count() {
        let mut r = Root(None, None);
        root_stationary(&mut r, true);
        root_stationary(&mut r, true);
        assert_eq!((r.0, r.1), (Some(1), Some(2)));
        root_stationary(&mut r, false);
        assert_eq!((r.0, r.1), (Some(1), Some(1)));
        root_stationary(&mut r, false);
        assert_eq!((r.0, r.1), (None, None));
        root_stationary(&mut r, false);
        assert_eq!((r.0, r.1), (None, None));
    }
}
