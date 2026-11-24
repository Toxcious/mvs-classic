//! The fighter's part of the World's step: the phases of a frame in the engine's tick order (the
//! frame boundary, the controller, the CMC, the pawn, the mesh, TG_DuringPhysics), the frame's flags, and the small
//! files the step calls beside them: the carried fighter, Comp_Pawn_Fly's tick
//! and the controller rumble.
//!
//! Offline the game's rollback driver is off (cvar rollback.ToggleRollbackSystem defaults to 0), so the stock tick
//! order holds. TG_PrePhysics: the PlayerController (input events: attacks, jumps, MoveX), the CMC (after the
//! controller, AddPawnTickDependency; before its owner, UMovementComponent bTickBeforeOwner, FUN_142976540), the
//! pawn's ReceiveTick (HandleMovementInput), then the mesh (after the CMC and, by BaseFighter BeginPlay's
//! Mesh.AddTickPrerequisiteActor(self), after the pawn), whose montage advance fires the notifies and whose pose is
//! final when its tick returns (the bone buffers change inside it). TG_DuringPhysics (component default, the manager
//! CDO keeps it): MoveCommandManager ReceiveTick. Notify and manager NetImpulses are pending, so they move the
//! fighter at the next frame's CMC tick, and so does the pawn tick's AddMovementInput (moveIn).
//!
//! Hitpause is CustomTimeDilation 0.005 on the actor: the CMC still ticks on 0.5 % of the frame, so a pending launch
//! is applied, the fighter creeps and brakes (pausedMove) and OnMovementUpdated stages the stored NetImpulse
//! (dropped in hitstun); the pawn's latent actions run on the world's DeltaSeconds (AActor::Tick 14294e9e0 calls
//! ProcessLatentActions(f, World->DeltaTimeSeconds) after ReceiveTick), so its Delays keep counting.

use crate::Real;
use crate::State;
use crate::actors::ActorRef;
use crate::actors::ItemId;
use crate::chars::CharState;
use crate::combat::hit::ApplyKnockbackL;
use crate::combat::{buffs, defense, hit, holds, phases};
use crate::consts as C;
use crate::events::Event;
use crate::fighter::match_side::TutorialEvent;
use crate::fighter::moves::FlyProps;
use crate::fighter::{self as fighter, FighterId, anim, body, control, moves, traces};
use crate::input::{Btn, Dir, InputState, NO_INPUT};
use crate::math;
use crate::names::Name;
use crate::world::{World, pawns};

/// `frameMode`.
#[derive(State, Copy, PartialEq, Eq, Debug, Default)]
pub enum FrameMode {
    Dead,
    Paused,
    Hitstun,
    #[default]
    Normal,
}

/// What `Fighter.onTurn` holds during the step (set to a call during the step and cleared after): the call the fighter
/// makes when it turns.
#[derive(State, Copy, PartialEq, Eq, Debug, Default)]
pub enum OnTurn {
    #[default]
    None,
    /// `onTurn = () => Overlap.overlaps(this, f)` (`World.step`, around the controller and CMC ticks).
    Overlaps,
}

/// `jumpListeners`: the actors told of the fighter's jump press (`{ a, w }`: the World is the caller's).
#[derive(State, Copy, PartialEq, Eq, Debug)]
pub struct JumpListener {
    pub actor: ActorRef,
}

/// `surfaceTraces`: a footstep or landing sound owed to a surface trace.
#[derive(State, Debug, Default)]
pub struct SurfaceTrace {
    pub ev: Name,
    pub sock: Name,
    pub queued: bool,
}

/// The frame's flags.
#[derive(State, Debug, Default)]
pub struct FrameFlags {
    pub frame_mode: FrameMode,
    pub moved: bool,
    pub hit_this_frame: bool,
    pub on_turn: OnTurn,
    pub jump_listeners: Vec<JumpListener>,
    pub surface_traces: Vec<SurfaceTrace>,
}

/// UCharacterMovementComponent::UnCrouch expanding in place on a floor (4.26 source; exe 14294f140 is the stock one):
/// the standing capsule is put SweepInflation (KINDA_SMALL_NUMBER x 10) + MIN_FLOOR_DIST (1.9) / 2 over the contact
/// the crouched capsule's sweep down found.
const UNCROUCH_IN_PLACE_GAP: Real = 0.001 + 1.9 / 2.0;

/// BaseFighter BounceWithoutHitStunMinSpeed.
// The same constant as `combat::hit`'s once its port is in; one of the two goes at the merge.
const BOUNCE_NO_HITSTUN_MIN_SPEED: Real = 4000.0;

/// The dilation of a fighter's tick in `mode` (CustomTimeDilation under its hit pause).
fn dilation_of(m: FrameMode) -> Real {
    if m == FrameMode::Paused { C::HITPAUSE_DILATION } else { 1.0 }
}

// ---- the step's phases (`World.step`)

/// `World.step`: `for (const f of fs) f.platforms = platforms`, the frame's platforms (`Scratch::platforms`, `world::spawning::platforms_of_frame`).
pub fn platforms_of_frame(w: &mut World, f: FighterId) {
    let World { fighters, scratch, .. } = w;
    fighters.get_mut(f).expect("a live fighter id").platforms.platforms.clone_from(&scratch.platforms);
}

/// `World.step`: `f.beginFrame(i < np ? inputs[i] : Pawns.pawnInput(this, f, closed, inputs))`, `i` the fighter's
/// place in `fs`. `fs` starts with the players in their order, so a player's place in `fs` is its place in
/// `players`; every other pawn takes its controller's input.
pub fn begin_frame(w: &mut World, f: FighterId) {
    let inp = match w.players.iter().position(|&x| x == f) {
        // (the port's own Chaos Wheel's Body Swap drives a player with its teammate's input)
        Some(i) => w.scratch.inputs.get(crate::world::wheel_events::input_slot(w, i)).copied().unwrap_or(NO_INPUT),
        None => {
            // (the inputs taken out for the call: a demo controller's input writes the World)
            let inputs = std::mem::take(&mut w.scratch.inputs);
            let i = pawns::pawn_input(w, f, w.scratch.closed, &inputs);
            w.scratch.inputs = inputs;
            i
        }
    };
    begin_frame_input(w, f, inp);
}

/// `World.step`: `f.onTurn = () => Overlap.overlaps(w, f, false)` ahead of the controller ticks. A facing change is
/// a rotation-only root move and updates overlaps too (SetRotationLogic), inside the call: with the position and
/// the pose the fighter holds at that point. A turn in the mesh tick is left to the finalize update that follows it.
pub fn set_on_turn(w: &mut World, f: FighterId) {
    w.fighter_mut(f).frame.on_turn = OnTurn::Overlaps;
}

/// `World.step`'s CMC pass for `f`: `Fighter.cmcTick`; when the fighter moved, its overlaps (`Overlap.overlaps`), its
/// item reach (`Items.updateItemReach`, not a minion's: a minion's item reach is not kept) and the fighter its held
/// item carries (`followHolder`).
pub fn cmc_tick(w: &mut World, f: FighterId) {
    cmc_tick_own(w, f);
    let fm = w.fighter(f);
    if !fm.frame.moved {
        return;
    }
    let minion = fm.life.is_minion;
    phases::overlaps(w, f, false);
    if !minion {
        crate::world::items::update_item_reach(w, Some(f));
    }
    // Read after the overlaps and the item reach, which can pick up or drop the item.
    if w.fighter(f).items.held_item.is_some() {
        follow_holder(w, f);
    }
}

/// `World.step`'s pawn pass: `f.pawnTick(this)`, after the stage's and the run-time solids' moves.
pub fn pawn_tick(w: &mut World, f: FighterId) {
    pawn_tick_own(w, f, true);
}

/// `World.step`: `f.onTurn = null` after the pawn ticks.
pub fn clear_on_turn(w: &mut World, f: FighterId) {
    w.fighter_mut(f).frame.on_turn = OnTurn::None;
}

/// `World.step`'s mesh pass: `f.meshTick()`, then `Overlap.overlaps(w, f, true)`.
pub fn mesh_tick(w: &mut World, f: FighterId) {
    mesh_tick_own(w, f);
    phases::overlaps(w, f, true);
}

/// `World.step`'s `holdFollow(fs)` (a held fighter at its holder's socket in the new pose) and
/// `followAll` (a fighter carried by a held item, at its holder's socket in the new pose), after the mesh ticks.
pub fn follow(w: &mut World) {
    let fs = std::mem::take(&mut w.scratch.fs);
    holds::hold_follow(w, &fs);
    w.scratch.fs = fs;
    follow_all(w);
}

/// `World.step`'s TG_DuringPhysics pass: `f.duringPhysicsTick()`, Comp_Pawn_Fly's ReceiveTick (`flyTick`; its place
/// among the components unverified), then a pawn's BTComponent past the players, behind its components: the input of
/// its next frame (`Pawns.controllerThink`).
pub fn during_physics_tick(w: &mut World, f: FighterId) {
    during_physics_tick_own(w, f);
    fly_tick(w, f);
    if !w.pawn_ctl.ctl.is_empty() {
        pawns::controller_think(w, f);
    }
}

/// `fireWallTick`: Comp_Pawn_FireWall's Delay, in the leftover latent pass of the pawns'
/// components, ahead of the timer pass.
pub fn fire_wall_tick(w: &mut World, f: FighterId) {
    crate::actors::firewall::fire_wall_tick(w, f);
}

/// `Fighter.resetAimTimer` (Comp_Spawner's ResetAimTimer).
pub fn reset_aim_timer(w: &mut World, f: FighterId) {
    fighter::match_side::reset_aim_timer(w, f);
}

/// `holdChargeTimer` (Comp_HoldCharge's timer).
pub fn hold_charge_timer(w: &mut World, f: FighterId) {
    crate::combat::hits::hold_charge_timer(w, f);
}

/// `chargeTimerTick` (Comp_Pawn_ChargeAttack's ChargeTimer).
pub fn charge_timer_tick(w: &mut World, f: FighterId) {
    control::charge_timer_tick(w, f);
}

/// `jumpHoldTimerTick` (BaseFighter's JumpHoldTimer, a tutorial's only).
pub fn jump_hold_timer_tick(w: &mut World, f: FighterId) {
    moves::jump_hold_timer_tick(w, f);
}

// ---- tick

/// `Fighter.beginFrame(inp)`: the timers and latent work of the frame boundary, and the
/// branch the frame takes (`frameMode`).
pub fn begin_frame_input(w: &mut World, f: FighterId, inp: InputState) {
    hit::hitstun_timer_tick(w, f);
    {
        let fm = w.fighter_mut(f);
        fm.frame.hit_this_frame = false;
        fm.frame.moved = false;
        fm.frame.frame_mode = FrameMode::Normal;
        fm.clock.tick = fm.clock.tick.wrapping_add(1);
    }
    anim::seq_frame_start(w, f);
    let enabled = moves::input_enabled(w, f);
    let (prev_x, prev_y) = {
        let fm = w.fighter_mut(f);
        fm.clock.prev_time = fm.clock.time;
        fm.clock.time += C::DELTA_SECONDS;
        fm.body.prev_x = fm.body.x;
        fm.body.prev_y = fm.body.y;
        fm.controls.prev = fm.controls.inp;
        (fm.controls.prev.x, fm.controls.prev.y)
    };
    // MoveXAxisValue / MoveYAxisValue: the frame's input, or the input override where one is on
    //. InputDisabledCounter above 0: no input (`disableInput`).
    let given = if enabled { crate::input::input_state(inp) } else { NO_INPUT };
    let (read, x_event, y_event) = moves::read_move_input(w, f, given, prev_x, prev_y);
    let (cancel_bounce, inp_x, inp_y) = {
        let fm = w.fighter_mut(f);
        fm.controls.inp = read;
        fm.root.anim_updated = false;
        // The last pawn tick's BaseFighterVelocity = GetVelocity() (ReceiveTick, after its CMC).
        fm.hit.base_vx = fm.body.vx;
        fm.hit.base_vy = fm.body.vy;
        // The same tick, right after (@35625-35839): CanBounceWithoutHitStun goes off when
        // VSize(BaseFighterVelocity) < BounceWithoutHitStunMinSpeed and no HitstunTimer exists (the timer outlives
        // a ring-out). Walking, dead or paused, the tick reaches this.
        let cancel = fm.hit.can_bounce_no_hitstun
            && fm.hit.hitstun <= 0.0
            && fm.life.ko_hitstun <= 0.0
            && math::ue::size2d([fm.hit.base_vx, fm.hit.base_vy]) < BOUNCE_NO_HITSTUN_MIN_SPEED;
        (cancel, read.x, read.y)
    };
    if cancel_bounce {
        w.fighter_mut(f).hit.can_bounce_no_hitstun = false;
    }
    // BaseFighter MoveX (sets MoveXAxisValue, CheckLateralFriction), called by PFGPlayerController: the Left / Right
    // action keys call it on press and release; the MoveX axis event (gamepad stick only) calls it every frame while
    // the stick is off centre or GamePadInputEnabled is set (`axis`); a bot task at each of its calls (`callX`).
    if x_event {
        moves::check_lateral_friction(w, f);
    }
    // The same MoveX call: MoveLeft for MoveXAxisValue < 0 (@127368), MoveRight for > 0 (@127548); then MoveY's (the
    // same events, `callY` a bot's): Up for MoveYAxisValue > 0 (@128789).
    // Unverified: a bot's MoveX / MoveY calls of one frame are one call here (`callX` / `callY`).
    if (x_event || y_event) && fighter::match_side::tutorial_world(w, f) {
        if x_event && inp_x != 0.0 {
            let ev = if inp_x < 0.0 { TutorialEvent::MoveLeft } else { TutorialEvent::MoveRight };
            fighter::match_side::tutorial_event_of(w, f, ev);
        }
        if y_event && inp_y > 0.0 {
            fighter::match_side::tutorial_event_of(w, f, TutorialEvent::Up);
        }
    }
    let fm = w.fighter_mut(f);
    fm.air.time_in_air = if fm.body.on_floor { 0.0 } else { fm.air.time_in_air + C::DT };
    control::tick_evade_regen(w, f);
    {
        let fm = w.fighter_mut(f);
        if fm.air.air_attack_delay > 0.0 {
            fm.air.air_attack_delay -= 1.0;
            if fm.air.air_attack_delay == 0.0 {
                fm.air.air_attack_available = true;
            }
        }
    }
    defense::tick_rage(w, f);
    // HitstunHandler BuffstunTimer (AddBuffHitStun @260): a world timer, which no graph pauses for a hitpause.
    let stun_end = {
        let fm = w.fighter_mut(f);
        if fm.buffs.buff_stunned {
            fm.buffs.buff_stun_age += 1.0;
            fm.buffs.buff_stun -= 1.0;
            fm.buffs.buff_stun <= 0.0
        } else {
            false
        }
    };
    if stun_end {
        holds::buff_stun_end(w, f, false);
    }
    // Comp_Armor TempHealthTimer (K2_SetTimer on the component: a world timer). Its GetGameTimeInSeconds is the clock
    // of the frame whose timer tick this is, the one before this frame's.
    let temp_fire = {
        let fm = w.fighter_mut(f);
        match fm.stats.temp_health.first() {
            Some(t) if t.fire != Real::INFINITY => {
                let left = t.fire - 1.0;
                for pk in &mut fm.stats.temp_health {
                    pk.fire = if left > 0.0 { left } else { Real::INFINITY };
                }
                (left <= 0.0).then_some(fm.clock.prev_time)
            }
            _ => None,
        }
    };
    if let Some(now) = temp_fire {
        defense::temp_health_timer(w, f, now, true);
    }
    defense::tick_timed_armor(w, f); // Comp_Armor ArmorTimer (AddTimedArmorPack)
    holds::struggle_tick(w, f);
    holds::suspend_tick(w, f);
    buffs::buff_decay_tick(w, f);
    buffs::tick_buffs(w, f, false);
    control::tick_abilities(w, f);
    let pre_spawn = {
        let fm = w.fighter_mut(f);
        if fm.life.pre_spawn_delay > 0 {
            fm.life.pre_spawn_delay -= 1;
            fm.life.pre_spawn_delay == 0
        } else {
            false
        }
    };
    if pre_spawn {
        // CreateAbilities' Delay(1.0).
        // The list stays in place: the cooldown's ability UI reads the fighter's slots.
        let n = w.fighter(f).abilities.abilities.len();
        for k in 0..n {
            control::pre_spawn_cooldown(w, f, i32::try_from(k).expect("an ability index"));
        }
    }
    fighter::match_side::tick_vo(w, f);
    moves::tick_wall_timers(w, f);
    control::tick_charge_ramp(w, f);
    // SpammedAttackDelayTimer: Trigger Current Attack does nothing when dead or in hitstun.
    let spam_due = {
        let fm = w.fighter(f);
        fm.controls.spam_delayed.as_ref().is_some_and(|s| f64::from(fm.clock.tick) >= f64::from(s.at))
    };
    if spam_due && (w.fighter(f).life.dead > 0.0 || fighter::in_hitstun(w, f)) {
        w.fighter_mut(f).controls.spam_delayed = None;
    }
    // The HitstunTimer outlives a ring-out: its RemoveHitstunSource broadcasts HitStunEnded, which stops the
    // DamageManager's smoke trail (StopSmokeTrailMulticast), and with the knockback hit's binding still on its
    // StopAnimMontage(null) (@12054, as at hitstunEnded).
    let ko_end = {
        let fm = w.fighter_mut(f);
        if fm.life.ko_hitstun > 0.0 {
            fm.life.ko_hitstun -= 1.0;
            fm.life.ko_hitstun == 0.0
        } else {
            false
        }
    };
    if ko_end {
        let fm = w.fighter_mut(f);
        let id = fm.id;
        fm.events.push(Event::SmokeTrail { fighter: id, on: false });
        if fm.attackers.hit_stun_ended_bound {
            fm.attackers.hit_stun_ended_bound = false;
            anim::stop_anim_montage(w, f);
        }
    }
    if w.fighter(f).life.dead > 0.0 {
        w.fighter_mut(f).air.move_in = 0.0;
        if w.fighter(f).wall.wall_sliding {
            moves::wall_slide_off(w, f);
        }
        let fm = w.fighter_mut(f);
        fm.life.dead -= 1.0;
        if fm.life.dead == 0.0 {
            let id = fm.id;
            fm.events.push(Event::Respawn { fighter: id });
        }
        // No GravityScale from a stored NetGravity while dead: the defeat tail's DisableMovement (BaseFighter
        // @19170) is MOVE_None, where PerformMovement (1429385a0) takes its early exit and OnMovementUpdated
        // (FUN_1410d5550, the one place the stored scale goes to GravityScale) is not run. So the pawn respawns
        // with the scale it died with: ResetFighterLogic stores the default again (@378) and sets Falling (@392),
        // and the first move after it still runs at the old scale. Rung out inside a SetGravity(0) window, that
        // move has no gravity and no terminal clamp (NewFallVelocity: none under a zero scale): one tick at the
        // respawn's -7500, then the default scale.
        // Deviation: a pawn that dies with a True Hitstun Timer running is set Falling by the timer's delegate
        // while dead; its moves would then run and take the stored scale. Not modelled.
        fm.frame.frame_mode = FrameMode::Dead;
        return;
    }
    fighter::match_side::wind_rush(w, f);
    let in_hitstun = fighter::in_hitstun(w, f);
    {
        let fm = w.fighter_mut(f);
        if fm.attackers.combo_reset_tick >= 0 && fm.clock.tick >= fm.attackers.combo_reset_tick {
            fm.attackers.combo_reset_tick = -1;
            if !in_hitstun {
                fm.attackers.di_combo = 0.0;
            }
        }
    }
    let invuln_at = {
        let fm = w.fighter_mut(f);
        if fm.invuln.invuln > 0.0 {
            fm.invuln.invuln -= 1.0;
            Some(fm.invuln.invuln)
        } else {
            None
        }
    };
    if let Some(left) = invuln_at {
        // The 0.03 s Delay resumes (11960): with lives and SpawnInvulnerabilityTime > 0,
        // EnableInvulnerabilityFlashLocal(true) (DodgedSuccessfully false), HasSpawnInvulnerability, then
        // Delay(SpawnInvulnerabilityTime) resumes at CancelInvuln(false) (11944).
        if left == math::round(C::SPAWN_INVULN * 60.0) {
            let fm = w.fighter_mut(f);
            fm.invuln.has_spawn_invuln = true;
            fm.evade.dodged_successfully = false;
            // EvadeFresnel 4 and UseEvadeSine 1 are set first and overwritten by this update in the same frame.
            control::update_evade_blink(w, f, true);
        } else if left == 0.0 {
            control::cancel_invuln(w, f, false);
        }
    }
    // AfterEvadeInvulnerabilityTimer: EnableInvulnerability(false).
    let evade_end = {
        let fm = w.fighter_mut(f);
        if fm.evade.evade_invuln > 0.0 {
            fm.evade.evade_invuln -= 1.0;
            fm.evade.evade_invuln == 0.0
        } else {
            false
        }
    };
    if evade_end {
        control::enable_invulnerability(w, f, false, false);
    }
    if w.fighter(f).hit.hitpause > 0.0 {
        // The timer manager ignores the actor's CustomTimeDilation, so the spam timer fires on time in our own
        // hitpause (in the victim's it is dropped above); its attack starts under the pause.
        control::fire_spam_delayed(w, f);
        w.fighter_mut(f).hit.hitpause -= 1.0;
        tick_true_hitstun(w, f);
        // The HitPauseTimer fires in the timer manager's tick, after the last frame's ReceiveTick: on the tick it
        // ends, HandleHitShake finds no time left and does nothing.
        if w.fighter(f).hit.hitpause == 0.0 {
            hit::hit_pause_end(w, f);
        } else {
            hit::handle_hit_shake(w, f);
        }
        w.fighter_mut(f).frame.frame_mode = FrameMode::Paused;
        return;
    }
    let fm = w.fighter(f);
    if let Some(l) = &fm.hit.launch
        && fm.hit.free_launch
        && fm.hit.hitstun <= 0.0
    {
        // The pause of a bounce taken out of hitstun has ended (DMHitPauseEnd -> ApplyKnockback, on the timer after
        // the last frame's ticks). The hit added no hitstun and its hitstun end already ran (tryBounce), so the
        // launch, AdditiveKnockbackVel and the raised terminal velocity stay with a fighter in control; the True
        // Hitstun Timer the hit set puts it in the knockback mode (@4045-4307 SetMovementMode(6, 2)).
        // The same for a launch whose HitstunTimer a buff stun cleared under the pause (at
        // buffStunStart). A zero launch returns at ApplyKnockback's head (@299-370) and sets no mode.
        let mut kl = ApplyKnockbackL { vx: l.vx, vy: l.vy, di: l.di, wall: l.wall, kbi: l.kbi };
        let applied = hit::apply_knockback(w, f, &mut kl, true);
        // The call is handed the fighter's own launch object and writes it.
        if let Some(l) = &mut w.fighter_mut(f).hit.launch {
            (l.vx, l.vy, l.di, l.wall, l.kbi) = (kl.vx, kl.vy, kl.di, kl.wall, kl.kbi);
        }
        if applied {
            let vy = w.fighter(f).hit.launch.as_ref().map_or(kl.vy, |l| l.vy);
            hit::free_launch_mode(w, f, vy);
        }
        w.fighter_mut(f).hit.launch = None;
    }
    // A buff stun cleared the HitstunTimer of a launch already applied: the
    // fighter is in the stun alone, still in the knockback mode with his velocity (the normal step's PhysKnockback)
    // until the True Hitstun Timer ends it.
    let fm = w.fighter_mut(f);
    if fm.hit.launch.as_ref().is_some_and(|l| l.di) && fm.hit.hitstun <= 0.0 && fm.buffs.buff_stunned {
        fm.hit.launch = None;
    }
    if fm.hit.hitstun > 0.0 || fm.hit.launch.is_some() {
        fm.frame.frame_mode = FrameMode::Hitstun;
    }
}

/// `Fighter.controllerTick`: the input's handling, the PlayerController's tick.
pub fn controller_tick(w: &mut World, f: FighterId) {
    let m = fighter::mode(w, f);
    holds::struggle_input(w, f);
    // InputPreTriggered of the presses the loops below do not reach: ahead of
    // them.
    if m != FrameMode::Normal {
        control::pre_trigger_end(w, f);
    }
    // Not under a buff stun (CheckHitstunActive counts its source): a launch waiting there has no knockback source
    // either (a hit without hitstun, or the HitstunTimer the stun cleared).
    let fm = w.fighter(f);
    let free_pause = m == FrameMode::Paused && fm.hit.launch.is_some() && fm.hit.free_launch && !fm.buffs.buff_stunned;
    match m {
        FrameMode::Paused => {
            control::charge_release_input(w, f);
            defense::rage_release_input(w, f);
            if free_pause {
                control::free_pause_actions(w, f);
            } else {
                control::buffer_during_hitpause(w, f);
            }
        }
        FrameMode::Normal => control::actions(w, f),
        // The presses that still reach ButtonPress; a dead pawn's too: PFGPlayerController's evade action
        // (@9670-10008) finds the pawn through Get Keyboard Fighter, whose fallback K2_GetPawn (@677) tests nothing,
        // and the pawn is kept through the respawn (PandaGameMode RespawnCharacter @1766, isDead false @1214).
        FrameMode::Hitstun | FrameMode::Dead => control::save_presses_only(w, f),
    }
    if m != FrameMode::Normal {
        control::pawn_presses(w, f);
    } else {
        control::pre_trigger_end(w, f); // a press `actions` dropped ahead of its press loop
    }
    // JumpPressed (the controller's Jump action, the bots' JumpPressed) runs JumpPressedLocal, then
    // CancelInvuln(true) whatever the state; BPI_JumpPressedLocal skips it.
    if control::pressed(w, f, Btn::Jump) && !w.fighter(f).controls.inp.jump_local {
        control::cancel_invuln(w, f, true);
    }
    // JumpPressedLocal's two broadcasts (@122712, @122732) on a frame whose press no loop above reached (hitstun,
    // dead, a pause without a montage): they are ahead of every test of the event.
    control::jump_dispatch_end(w, f);
    // The stop-anim NotifyTicks fire in the mesh tick, after every input event
    // of the controller: the parent's press handling and the pawn's own override in the same event (Pawn_C017
    // NormalAttackPressed @17650-17670, CharHooks.pressed / pawnPresses), so a press that branches or starts an
    // attack this frame replaces the montage before its MovementStopAnim tick reads the stick
    // (Finn_Combo_01_Attack_Montage: BranchWindow_C_4 is Immediate over MovementStopAnim from 1.306 s;
    // InputTriggered_Event -> SetActiveAttackFromBranch plays Combo_02). On the frames `actions` runs and under a
    // free launch's pause (freePauseActions).
    // Deviation: they run at the end of the controller's turn, ahead of the CMC, the pawn and the mesh tick, on the
    // notify states the previous mesh tick left on (one frame late at a state's start, one frame more at its end),
    // and a stop's dash NetImpulse moves the fighter a frame earlier than the build's. A press stored in a branch
    // window such a stop ends runs at this frame's dispatch (`stop_montage`'s late branch), the build's at the next.
    if m == FrameMode::Normal || free_pause {
        control::stop_anim_ticks(w, f);
    }
}

/// `Fighter.cmcTick`: the CharacterMovementComponent's tick.
pub fn cmc_tick_own(w: &mut World, f: FighterId) {
    // A pawn no controller holds (a mode pawn: Fighter.uncontrolled): UCharacterMovementComponent's tick runs
    // ControlledCharacterMove only for a locally controlled pawn, or with no controller under
    // bRunPhysicsWithNoController (off: CMC ctor 1429164d0) or root motion, and a never-possessed pawn's mode is
    // None, where PerformMovement returns at once.
    // Unverified: the whole port CMC tick is skipped (its crouch follow and base update too), and a pawn left by its
    // controller with a root motion montage playing is not moved by it (4.26 source for the tick's gate, not read
    // in the exe).
    if w.fighter(f).life.uncontrolled {
        return;
    }
    let m = fighter::mode(w, f);
    let (x, y, cmd_in) = {
        let fm = w.fighter_mut(f);
        fm.platforms.pawn_vy = fm.body.vy;
        (fm.body.x, fm.body.y, fm.moves.fly_input.map_or(0.0, |i| i[0]))
    };
    body::base_update(w, f, m == FrameMode::Normal);
    moves::force_move_clean_up(w, f);
    // UpdateCharacterStateBeforeMovement: bIsCrouched follows bWantsToCrouch (CanCrouchInCurrentState: walking or
    // falling), so the AnimBP sees the crouch a frame after the pawn tick set it. PerformMovement also runs on the
    // dilated ticks of a hitpause, so a fighter hit while crouched stands up on the CMC tick after the pawn tick
    // whose MoveX ran SetCrouching(false). The knockback mode is a custom movement mode, where
    // CanCrouchInCurrentState fails and the same step uncrouches (from 4.26 source).
    let def = &crate::fighter::common::data_of(w, f).def;
    let (rest_gap, crouch_rate) = (def.rest_gap, def.crouch_evade_recharge.is_some());
    let fm = w.fighter_mut(f);
    let was_crouched = fm.capsule.cmc_crouched;
    fm.capsule.cmc_crouched = fm.crouch.crouching && m != FrameMode::Hitstun;
    // A crouch that ends in the knockback mode: DMHitPauseEnd's ApplyKnockback ran in the timer tick before this CMC
    // tick (LaunchCharacter pending, SetMovementMode(6, 2) at once with a True Hitstun Timer, @3947-4111),
    // OnMovementModeChanged 142936f60 left bCrouchMaintainsBaseLocation off outside Walking, and PerformMovement
    // uncrouches (UpdateCharacterStateBeforeMovement) before it takes the launch (HandlePendingLaunch; 4.26 source).
    // So UnCrouch expands in place: encroaching the floor, it sweeps the crouched capsule down and stands the full
    // one 0.951 over the contact, under the walking rest height, and the launch's move starts from there. Only a
    // pause of at most one frame gets here: a longer one has a dilated CMC tick after the pawn tick whose MoveX ran
    // SetCrouching(false), still Walking (the stand above).
    // Unverified: not in a capture; the FMC's PerformMovement order is taken as stock.
    if was_crouched
        && !fm.capsule.cmc_crouched
        && m == FrameMode::Hitstun
        && fm.body.on_floor
        && fm.hit.launch.as_ref().is_some_and(|l| !l.di && (l.vx != 0.0 || l.vy != 0.0))
        && fm.hit.true_hitstun > 0.0
    {
        fm.body.y += UNCROUCH_IN_PLACE_GAP - crate::math::narrow(rest_gap);
    }
    let crouched = fm.capsule.cmc_crouched;
    // K2_OnStartCrouch / K2_OnEndCrouch of a pawn graph that has them (Pawn_C019's dodge meter rate).
    if crouch_rate && crouched != fm.evade.evade_crouch_rate {
        control::crouch_evade_rate(w, f, crouched);
    }
    if was_crouched != crouched
        && let Some(h) = w.fighter(f).hooks.char.crouch_changed
    {
        h(w, f, crouched);
    }
    let fm = w.fighter(f);
    if fm.buffs.grappled || fm.buffs.suspended || fm.shapes.carried.is_some() {
        // `carried`: DisableMovement, then Flying with MaxFlySpeed 0 and gravity overridden
        // to 0: PerformMovement runs and PhysFlying moves nothing while Velocity is 0.
        // Unverified: what writes Velocity while carried (a launch, a MoveCommand of a montage the BranchWindow lets
        // the player start) is taken as stopped here, as for the grapple; not read against the build.
        // `suspended`: MOVE_Flying with the velocity stopped in every handler tick
        // and no move input (AddBuffHitStun's SetIgnoreMoveInput), so PhysFlying has nothing to move.
        let grappled = fm.buffs.grappled;
        let fm = w.fighter_mut(f);
        fm.air.move_in = 0.0;
        fm.body.vx = 0.0;
        fm.body.vy = 0.0;
        fighter::set_grounded(w, f, false);
        // A hit's launch whose pause ends under the grapple: DMHitPauseEnd's ApplyKnockback runs as for a free fighter
        // (DI, the additive knockback, the terminal velocity, LaunchCharacter, SetMovementMode(6, 2) with the True
        // Hitstun Timer), and the grapple's MOVE_None exit drops what it launched (below); the mode it set leaves
        // None (`held_mode_tick`). What is left is a zero launch, so the release's Falling (RemoveGrapple) finds
        // no knockback to apply and the stop of the grapple's velocity.
        // Unverified: suspended or carried, the launch stays for the hitstun step after (not read against the build).
        if grappled
            && w.fighter(f).frame.frame_mode != FrameMode::Paused
            && let Some(l) = w.fighter(f).hit.launch.as_ref().filter(|l| !l.di)
        {
            let mut kl = ApplyKnockbackL { vx: l.vx, vy: l.vy, di: l.di, wall: l.wall, kbi: l.kbi };
            if hit::apply_knockback(w, f, &mut kl, true) {
                hit::free_launch_mode(w, f, kl.vy);
            }
            w.fighter_mut(f).hit.launch = Some(ApplyKnockbackL { vx: 0.0, vy: 0.0, ..kl });
        }
        // Under the grapple (CMC +0x4c0) PerformMovement takes the MOVE_None early exit: no HandlePendingLaunch, and
        // ClearAccumulatedForces 142924c40 clears PendingLaunchVelocity (+0x3c4) every tick, so a launch pending
        // there (a free launch's from the frame's start, a NetImpulse staged before the grapple) is dropped. The
        // stored NetImpulse (`net`) survives it (ClearAccumulatedForces clears +0x274, +0x280 and +0x3c4 only).
        // Suspended, the CMC takes a pending free launch (HandlePendingLaunch 1410c8e70 has no suspension test) and
        // the handler's Flying + StopMovementImmediately (HitstunHandler @9448-9595) cancels it: the launch is lost
        // too.
        // Unverified: carried, the launch is taken as lost the same way.
        let fm = w.fighter_mut(f);
        if grappled || fm.moves.hit_launch {
            fm.moves.impulse = None;
            fm.moves.hit_launch = false;
        }
        // The True Hitstun Timer of a knockback the stun found under way is a world timer AddBuffHitStun leaves it
        // runs on (the frame's start ticks it under a pause). Its Falling is undone by the handler's next tick under
        // a suspension. Under the grapple the modes ApplyKnockback and the timer set end None and RemoveGrapple's
        // Falling ends the knockback mode (heldModeTick, removeGrapple).
        holds::held_mode_tick(w, f, false);
        let fm = w.fighter(f);
        if fm.frame.frame_mode != FrameMode::Paused && fm.hit.true_hitstun_set_tick != fm.clock.tick {
            tick_true_hitstun(w, f);
        }
        // Carried or suspended (Flying) PerformMovement runs on to OnMovementUpdated (FUN_1410d5550): a stored
        // NetGravity is taken, else a standing gravity override writes 1.0, then the MoveCommandBraking rule
        // (nativeBraking: a SetGroundFriction window that ends while carried gives the braking decelerations back
        // their bases here). Under the grapple (the MOVE_None exit) it does not run.
        // Deviation: of that OnMovementUpdated the stored NetImpulse and NetFriction wait for the first move after.
        if !w.fighter(f).buffs.grappled {
            moves::apply_net_gravity(w, f);
            moves::native_braking(w, f);
        }
        // The pawn tick's HandlePlatformCollision is not the movement component's: it runs for a held fighter too,
        // so the platforms its capsule ignores are those of where it is held, not of where it was grabbed.
        body::handle_platform_collision(w, f, 0.0);
    } else if m == FrameMode::Paused {
        // In hitstun only CancelJump's direct launch is pending.
        if !fighter::in_hitstun(w, f) || w.fighter(f).moves.hit_launch {
            moves::apply_impulse(w, f);
        }
        if !fighter::no_collision(w, f) {
            moves::paused_move(w, f, C::DT * C::HITPAUSE_DILATION);
        }
        let fm = w.fighter(f);
        if !fighter::no_collision(w, f)
            && !fm.body.on_floor
            && fm.life.dead <= 0.0
            && !fm.platforms.platforms.is_empty()
        {
            body::lift_over_platforms(w, f);
        }
        let attacking = fighter::is_attacking(w, f);
        moves::jump_timers(w, f, attacking);
        moves::on_movement_updated(w, f);
        let attacking = fighter::is_attacking(w, f);
        moves::fast_fall_tick(w, f, attacking, false);
        let pawn_vy = w.fighter(f).platforms.pawn_vy;
        body::handle_platform_collision(w, f, pawn_vy);
        // A pause that began in this frame, before this CMC tick (a hit found at an earlier fighter's move or turn):
        // the frame's start did not tick a True Hitstun Timer kept from an earlier hit, and the branch this tick
        // would have taken does not run. One set by this frame's hit is pending.
        let fm = w.fighter(f);
        if fm.frame.frame_mode != FrameMode::Paused && fm.hit.true_hitstun_set_tick != fm.clock.tick {
            hit::tick_true_hitstun(w, f);
        }
    } else if m == FrameMode::Hitstun {
        if w.fighter(f).wall.wall_sliding {
            moves::wall_slide_off(w, f); // HandleWallSlideCheck: hitstun
        }
        moves::fast_fall_tick(w, f, false, false);
        let pawn_vy = w.fighter(f).platforms.pawn_vy;
        body::handle_platform_collision(w, f, pawn_vy);
        moves::jump_timers(w, f, false); // the jump's Delays keep running; their bodies skip the launches in hitstun
        w.fighter_mut(f).air.move_in = 0.0; // HandleMovementInput: hitstun adds no input
        hit::step_hitstun(w, f);
        moves::apply_net_gravity(w, f);
        moves::native_braking(w, f); // OnMovementUpdated's MoveCommandBraking rule
    } else if m == FrameMode::Normal {
        moves::physics(w, f);
        hit::tick_true_hitstun(w, f);
        moves::on_movement_updated(w, f);
    }
    // Internal_ConsumeMovementInputVector (142c76590), the first call of every CMC tick: ControlInputVector goes to
    // LastControlInputVector and is zeroed, whatever the movement mode or the dilation. It holds the pawn tick's
    // AddMovementInput and, added after it in the same frame (the manager ticks in DuringPhysics), a ControlSpeed
    // command's AddMovementInput(direction x |MoveVector|, 1.0, bForce) (Command_Movement.TickMoveTime @5683;
    // APawn::AddMovementInput 142c6c950: float32 Dir x Scale, bForce passes IsMoveInputIgnored).
    let fm = w.fighter_mut(f);
    fm.air.last_move_in = fm.air.move_in + cmd_in;
    fm.moves.fly_input = None;
    fm.platforms.walk_drop = false;
    fm.platforms.walk_base = None;
    fm.frame.moved = fm.body.x != x || fm.body.y != y;
}

/// `Fighter.pawnTick`: the pawn's ReceiveTick and its latent actions. `in_world`: the caller
/// passes the World (the step's pawn pass), which StandingOnEdge needs; a fighter's step alone has none.
pub fn pawn_tick_own(w: &mut World, f: FighterId, in_world: bool) {
    let m = fighter::mode(w, f);
    // ReceiveTick's PF_Velocity_Ak(root AkComponent, 500, 2500, DT, PreviousLocation): the root's move since the
    // ReceiveTick before over this tick's DeltaSeconds, MapRangeClamped to 0..1. DeltaSeconds carries the actor's
    // CustomTimeDilation as it stands at this tick (a hit earlier in the frame already dilates it). Sound posts read
    // the value until the next ReceiveTick.
    let tick_dt = C::DELTA_SECONDS * dilation_of(m);
    // The root is the CMC capsule: a crouch's 116 drop of the actor location (and the rise back) counts as a move.
    let actor_y = fighter::actor_y(w, f);
    let fm = w.fighter_mut(f);
    let dist = math::ue::size2d([fm.body.x - fm.body.ak_x, actor_y - fm.body.ak_y]) / tick_dt;
    fm.body.vel_clamped = math::min(math::max((dist - 500.0) / 2000.0, 0.0), 1.0);
    fm.body.ak_x = fm.body.x;
    fm.body.ak_y = actor_y;
    if m == FrameMode::Paused {
        // ReceiveTick runs every frame under the dilation: HandleNewJumpMovement (a ground jump from a branch
        // pressed in the pause), then HandleMovementInput in full.
        if !fighter::in_hitstun(w, f) {
            moves::issue_ground_jump(w, f);
        }
        moves::pawn_move_input(w, f);
    } else if m == FrameMode::Normal {
        moves::issue_ground_jump(w, f); // ReceiveTick: HandleNewJumpMovement, then HandleMovementInput
        moves::pawn_move_input(w, f);
    }
    // ReceiveTick's sequence, after HandleMovementInput, HandleEvadeUse and HandleHitShake: StandingOnEdge, in every
    // state.
    if in_world {
        traces::standing_on_edge(w, f);
    }
    // Pawn_C017 ReceiveTick (@7485): the parent's tick, then ReadHoverInput, whatever the state.
    if let Some(h) = w.fighter(f).hooks.char.tick {
        h(w, f);
    }
    // The actor's latent actions, after ReceiveTick whatever the state (AActor::Tick, on the world's delta).
    if w.fighter(f).jump.end_jump_latent {
        moves::end_jump_rerun(w, f);
    }
    // Unverified: the order of the two latent actions in the pawn's list is not read; taken as the rerun first (its
    // jump stop skips a fighter already fast-falling, and the fast fall's NetImpulse is the later store).
    // Deviation: the build fires the Delay whatever the state; here one due while dead or held waits.
    let fm = w.fighter(f);
    if m != FrameMode::Dead && !fm.buffs.grappled && !fm.buffs.suspended && fm.shapes.carried.is_none() {
        moves::fast_fall_delay_fire(w, f);
    }
    body::drop_tutorial_tick(w, f); // the platform drop's tutorial Delay, whatever the state
    // BaseFighter's ArmorShake timeline component (TG_PrePhysics; its place after ReceiveTick is unverified there).
    defense::armor_shake_tick(w, f, dilation_of(m));
}

/// `cmcPoseTick`: ACharacter / UCharacterMovementComponent::TickCharacterPose (14294e8b0)
/// from PerformMovement, while the mesh IsPlayingRootMotion: the mesh's TickPose with the
/// frame's time, i.e. the AnimInstance's update and the montages' Advance with their branching-point notifies, before
/// the pawn tick. The queued notifies wait for the mesh's own tick, which does not advance again that frame
/// (LastPoseTickFrame). `dilation`: the CMC's tick in the fighter's own hit pause (pausedMove) is dilated, and
/// TickCharacterPose with it.
// Deviation: a branching-point Ak_SurfaceEffect of such a frame traces from this frame's pose before the update as on
// any frame (`flushSurfaceTraces` takes the pose at the mesh tick's start, which here is the updated one: none in the
// root-motion montages of the data).
pub fn cmc_pose_tick(w: &mut World, f: FighterId, dilation: Real) {
    let paused = dilation != 1.0;
    let fm = w.fighter_mut(f);
    fm.root.pose_ticked = true;
    fm.root.dispatch_owed = None;
    fm.anim.seq_delta = 0.0;
    let hook = fm.hooks.char.anim_update;
    if let Some(h) = hook {
        h(w, f, dilation, true);
    }
    anim::anim_direction_tick(w, f, paused);
    let fm = w.fighter_mut(f);
    let grounded = fm.body.on_floor;
    if let Some(m) = &mut fm.montages.montage {
        m.walking = grounded; // read before the move here
    }
    anim_weights_tick(w, f, dilation);
    // Montage_Advance sets bQueueMontageEvents and only the mesh tick's TriggerQueuedMontageEvents clears it: the
    // proxies' events of this Advance, and of any stop made before that dispatch, wait for meshTick's flush.
    anim::open_proxy_events(w, f);
    anim::advance_montage(w, f, 1, dilation);
    anim::slot_graph_tick(w, f, dilation);
    if paused {
        paused_pose(w, f);
    } else {
        anim::update_pose(w, f);
    }
    if let Some(h) = hook {
        h(w, f, dilation, false);
    }
}

/// `pausedPose`: the pose of a dilated update, the montage's pose at its new position (a section jump made under the
/// pause shows at once).
// Deviation: the AnimBP's state machines, the locomotion players and the slot mixes built from the pose (Morty's
// blendSlots, graphSlots) stand still (`anim::update_pose` steps them by whole frames); they are next updated by the
// first frame after the pause. The montage layers' blend weights (anim_weights_tick) step on the dilated delta.
fn paused_pose(w: &mut World, f: FighterId) {
    if let Some((a, t)) = anim::shown_pose(w, f) {
        let fm = w.fighter_mut(f);
        fm.anim.anim = a;
        fm.anim.anim_time = t;
    }
}

/// `Fighter.meshTick`: the mesh's anim tick (montages, notifies, root motion).
pub fn mesh_tick_own(w: &mut World, f: FighterId) {
    let m0 = fighter::mode(w, f);
    let fm = w.fighter_mut(f);
    let hp0 = fm.hit.hitpause;
    let prev_anim = fm.anim.anim;
    let prev_time = fm.anim.anim_time;
    if m0 != FrameMode::Paused {
        for e in &mut fm.montages.montage_meshes {
            e.age += 1.0;
        }
    }
    // The montage events of the update and its dispatch (the proxies' handlers) wait for the dispatch's end.
    anim::open_proxy_events(w, f);
    if w.fighter(f).root.pose_ticked {
        // The pose was ticked by the movement component: the queued dispatch alone, whatever the mode is by now.
        w.fighter_mut(f).root.pose_ticked = false;
        let dil = dilation_of(fighter::mode(w, f));
        anim::advance_montage(w, f, 2, dil);
        // This frame's graph update is behind us (cmc_pose_tick): a write of this dispatch is in effect at the next.
        let fm = w.fighter_mut(f);
        if fm.charge.anim_attack_off == 1 {
            fm.charge.anim_attack_off = 2;
        }
        anim::complete_late_branch(w, f);
    } else {
        w.fighter_mut(f).anim.seq_delta = 0.0;
        mesh_tick_inner(w, f);
    }
    anim::flush_proxy_events(w, f);
    anim::seq_notify_tick(w, f);
    if !w.fighter(f).frame.surface_traces.is_empty() {
        fighter::match_side::flush_surface_traces(w, f, prev_anim, prev_time);
    }
    // A pause the dispatch started (AnimNotify_HitPausePoseFrames: a queued attacker pause, StartPause's
    // CustomTimeDilation): the tick functions after the mesh's run dilated this frame (Fighter.mode).
    let fm = w.fighter_mut(f);
    if hp0 == 0.0 && fm.hit.hitpause > 0.0 {
        fm.frame.hit_this_frame = true;
    }
    // A Disguise's second mesh: its own anim instance, updated behind the pawn's Mesh.
    if w.fighter(f).shapes.disguise.is_some() {
        let m = fighter::mode(w, f);
        crate::combat::disguise::copy_tick(w, f, m);
    }
    // A Polymorph's mesh: its own anim instance, the same.
    if w.fighter(f).shapes.polymorph.is_some() {
        let m = fighter::mode(w, f);
        crate::combat::polymorph::poly_tick(w, f, m);
    }
}

/// `meshTickInner`: the AnimBP's update around the montages' tick (not a dead pawn's).
fn mesh_tick_inner(w: &mut World, f: FighterId) {
    let m = fighter::mode(w, f);
    let hook = if m == FrameMode::Dead { None } else { w.fighter(f).hooks.char.anim_update };
    let dil = dilation_of(m);
    if let Some(h) = hook {
        h(w, f, dil, true);
    }
    mesh_tick_montages(w, f, m);
    if let Some(h) = hook {
        h(w, f, dil, false);
    }
}

/// The AnimBP's IsWalking, read after the CMC moved (mesh tick).
fn set_montage_walking(w: &mut World, f: FighterId) {
    let fm = w.fighter_mut(f);
    let grounded = fm.body.on_floor;
    if let Some(m) = &mut fm.montages.montage {
        m.walking = grounded;
    }
}

/// `meshTickMontages`.
fn mesh_tick_montages(w: &mut World, f: FighterId, m: FrameMode) {
    match m {
        FrameMode::Paused => {
            // The montage update keeps running under the hitpause dilation (CustomTimeDilation 0.005): the current
            // instance advances by that share of the frame with its notifies and NotifyTicks (an on-hit
            // ConditionSectionJump jumps in the first frame of the pause), and a late window end fires here too:
            // CompleteBranchNotify -> Trigger Current Attack Client has no hitpause gate (only hitstun / dead,
            // ubergraph 5754) and plays the montage at once; its notifies at 0 begin in the next frame's dilated
            // Advance.
            set_montage_walking(w, f);
            anim_weights_tick(w, f, C::HITPAUSE_DILATION);
            anim::advance_montage(w, f, 0, C::HITPAUSE_DILATION);
            anim::complete_late_branch(w, f);
            anim::anim_direction_tick(w, f, true);
            // The slot graph's timed lists run on under the dilation (the pose is read again at the next updatePose).
            anim::slot_graph_tick(w, f, C::HITPAUSE_DILATION);
            paused_pose(w, f);
        }
        FrameMode::Hitstun => {
            // The anim instance keeps updating in hitstun: every montage playing there advances and dispatches as in
            // a normal frame. The hit stopped every instance (HitReactionMulticast Montage_Stop,
            // Comp_Pawn_DamageManager @4306; AddDecayedHitStun's StopStunMontage, Comp_Pawn_HitstunHandler @2542 /
            // @1052 -> @7345), so what plays here began after the hit: a pawn's own (Pawn_Reindog's bounce montages,
            // played from its tick after a capsule hit) or a free attack whose DoPreAttackRotation turn (BaseFighter
            // @90730) took the hit, which still plays its montage (@84992 -> @56635 -> @59159
            // CreateProxyObjectForPlayMontage, @57282) with no second hitstun test. Those are stopped again by a
            // later hit or bounce (the same Montage_Stop, blend 0; tryBounce), a buff stun's
            // StopStunMontage, and at the hitstun end by the hit's HitStunEnded binding (DamageManager @3912 ->
            // @12054 StopAnimMontage(null): the current one with its own BlendOut, at
            // hitstunEnded).
            set_montage_walking(w, f);
            // BlueprintUpdateAnimation has no hitstun gate (Shaggy_AnimBP @14294, C019_AnimBP @1238-1621): the
            // layers' weights step on.
            anim_weights_tick(w, f, 1.0);
            anim::advance_montage(w, f, 0, 1.0);
            anim::complete_late_branch(w, f);
            anim::anim_direction_tick(w, f, false);
            anim::slot_graph_tick(w, f, 1.0);
            anim::update_pose(w, f);
        }
        FrameMode::Normal => {
            anim::anim_direction_tick(w, f, false);
            set_montage_walking(w, f);
            anim_weights_tick(w, f, 1.0);
            anim::advance_montage(w, f, 0, 1.0);
            // The late window end of the dispatch; the branch it triggers first advances next frame.
            anim::complete_late_branch(w, f);
            anim::slot_graph_tick(w, f, 1.0);
            anim::update_pose(w, f);
        }
        FrameMode::Dead => {
            // A defeated pawn is kept (hidden, input and movement off: BaseFighter's defeat tail @19027-19264) and
            // its mesh still updates its AnimInstance: nothing in the tail stops the component's tick, and the
            // fighters' meshes have VisibilityBasedAnimTickOption AlwaysTickPoseAndRefreshBones. The tail's
            // StopAnimMontage(null) @19155 is an interrupt (Montage_Stop FUN_1428412a0 -> FAnimMontageInstance::Stop
            // FUN_142865260), so the stopped instance lists nothing at the next TriggerAnimNotifies (FUN_142845f30):
            // its queued states get their NotifyEnd there (AnimNotify_Spawner's DestroySpawnedActors, PlayVFX's
            // kill, the move commands' removal), and its branching-point states at Terminate, when its blend-out is
            // complete. "One frame after the defeat" is the port's order: the World's blast check runs after the
            // mesh tick.
            // Deviation: the build's kill is inside the CMC's move, ahead of that frame's dispatch, so there the ends
            // come in the defeat's own frame.
            // No late branch: the tail's ClearQueuedAttack @19206 emptied the manager, and Trigger Current Attack
            // does nothing on a dead fighter.
            // Deviation: instances the stop did not reach (another slot group's, C017) play on through the death in
            // the build; here they stand still until the respawn clears them, only their owed queued ends run.
            // The AnimBP's update runs on (AlwaysTickPoseAndRefreshBones): its layers' weights step.
            anim_weights_tick(w, f, 1.0);
            anim::terminate_fading(w, f, 0, 1.0);
            if w.fighter(f).montages.grouped.is_some() {
                anim::grouped_late_ends(w, f);
            }
        }
    }
}

/// `animWeightsTick`: the AnimBPs' blend weights that the montage layers read, stepped with the anim update (on the
/// dilated delta under a hit pause).
fn anim_weights_tick(w: &mut World, f: FighterId, dilation: Real) {
    let dt = C::DT * dilation;
    let facing = fighter::facing(w, f);
    let fm = w.fighter_mut(f);
    let grounded = fm.body.on_floor;
    // Shaggy_AnimBP's KickGround / KickAir blend.
    if let CharState::Shaggy(sh) = &mut fm.cs {
        let step = if grounded { 1.0 } else { -1.0 };
        let dt = C::DT * dilation;
        sh.kick_w = math::min(math::max(sh.kick_w + step * (dt / 0.05), 0.0), 1.0);
    }
    // C019_AnimBP's IsMoving / IsMovingBackwards blend lists.
    let (vx, vy) = (fm.body.vx, fm.body.vy);
    if let CharState::Morty(mo) = &mut fm.cs {
        morty_slot_weights(&mut mo.slot_move_w, &mut mo.slot_back_w, vx, vy, facing, dt);
    }
}

/// C019_AnimBP's IsMoving / IsMovingBackwards blend lists (0.1 s), the Morty part of `animWeightsTick` (see slotMix,
/// Morty's state): `slot_move_w` and `slot_back_w` (-1: BlendListByBool(IsMovingBackwards) not updated yet)
/// of his state, stepped from the fighter's velocity and facing.
pub fn morty_slot_weights(slot_move_w: &mut Real, slot_back_w: &mut Real, vx: Real, vy: Real, facing: Real, dt: Real) {
    let back = (facing < 0.0) != (vx < 0.0);
    let step = if math::ue::size2d([vx, vy]) >= 100.0 { 1.0 } else { -1.0 };
    *slot_move_w = math::min(math::max(*slot_move_w + step * (dt / 0.1), 0.0), 1.0);
    if *slot_move_w > 0.0 {
        *slot_back_w = if *slot_back_w < 0.0 {
            if back { 1.0 } else { 0.0 }
        } else {
            let step = if back { 1.0 } else { -1.0 };
            math::min(math::max(*slot_back_w + step * (dt / 0.1), 0.0), 1.0)
        };
    }
}

/// `Fighter.duringPhysicsTick`: the TG_DuringPhysics components.
pub fn during_physics_tick_own(w: &mut World, f: FighterId) {
    let m = fighter::mode(w, f);
    // Comp_Actor_MoveCommandManager ticks with the owner's dilated DeltaTime (FActorComponentTickFunction::
    // ExecuteTick 14292aa20 multiplies by CustomTimeDilation), so it also runs in a hitpause, where the timers all
    // but stand still and the end logic of a finished list runs, and in hitstun. A pause the mesh tick's dispatch
    // started (AnimNotify_HitPausePoseFrames) dilates it already (mesh_tick_own, Fighter.mode).
    // Dead too: ReceiveTick (@8594-8947) has no dead gate and the defeat tail (BaseFighter @19027-19264) turns off no
    // component tick, so a listed command runs out in the death and its end logic runs; their NetImpulse and
    // NetFriction stay stored (a dead pawn is MOVE_None: no OnMovementUpdated) for the first move after the respawn.
    // Unverified: a ControlSpeed command's SetMovementMode(Flying) in the death would take the pawn out of MOVE_None
    // and move it in the build; the port moves no dead pawn (its flying flag is reset by the end logic or the
    // respawn).
    let dilation = if m == FrameMode::Paused { C::HITPAUSE_DILATION } else { 1.0 };
    moves::tick_move_cmds(w, f, C::DT * dilation);
    if m != FrameMode::Dead {
        anim::poll_auto_branches(w, f);
    }
    // Comp_C017_Hover_Rage ReceiveTick.
    // Unverified: its place among the owner's component ticks is not read.
    if let Some(h) = w.fighter(f).hooks.char.component_tick {
        h(w, f, dilation_of(m));
    }
    hit::hitstun_handler_tick(w, f);
    fighter::match_side::ally_transfer_tick(w, f);
}

/// `tickTrueHitstun`: the True Hitstun Timer's tick on the frame's start and in the held
/// branch of the CMC tick: its delegate's SetMovementMode(Falling) (HitstunHandler @10319) ends a grapple's None too
///.
fn tick_true_hitstun(w: &mut World, f: FighterId) {
    let t = w.fighter(f).hit.true_hitstun;
    hit::tick_true_hitstun(w, f);
    let ended = t > 0.0 && w.fighter(f).hit.true_hitstun == 0.0;
    holds::held_mode_tick(w, f, ended);
}

// ---- carried: a fighter welded to an item's root
//
// The pawn's root attached to the item's root with SnapToTarget, its movement disabled and set to Flying with gravity
// overridden to 0, riding the item in a hand and in flight until the item's End lets it go (Fastball's PickUp resume
// and End). The world moves it with the item and detaches it after EndPlay.

/// `carryAttach`: Fastball PickUp resume after its Delay(0) (@121-665): OverrideGravity(0)
/// @304, DisableMovement @341 (mode None: Velocity zeroed), K2_AttachToComponent(the item's root, SnapToTarget x 2,
/// KeepWorld scale) @397, K2_SetActorLocation(the item's location, teleport) @486, BPI_SetIsBeingHeld(true) @627,
/// SetMovementMode(Flying) @665. While carried the movement does not run (the CMC tick's carried branch, as
/// PhysFlying with MaxFlySpeed 0 and Velocity 0 makes no move); the override's 1.0 is written by its
/// OnMovementUpdated (the gravity step of that branch). `at`: the item root's place now (the holder's hand socket for
/// a held item).
///
/// Entering None: StopMovementImmediately (the velocity), ResetJumpState (ACharacter's own jump state:
/// bPressedJump, JumpCurrentCount, which the port does not hold: the jumps are BaseFighter's own, launches and
/// NetImpulses) and ClearAccumulatedForces (PendingLaunchVelocity: a direct launch not taken yet, `impulse`); a
/// launch while None is dropped, and nothing launches between @341 and @665. The staged NetImpulse (`net`) is
/// BaseFighter's and stays for OnMovementUpdated, which the carried fighter's PerformMovement runs.
// Unverified: ClearAccumulatedForces taken as stock (PendingImpulseToApply, PendingForceToApply,
// PendingLaunchVelocity), not read in the exe.
pub fn carry_attach(w: &mut World, f: FighterId, it: ItemId, at_x: Real, at_y: Real) {
    w.fighter_mut(f).shapes.carried = Some(crate::combat::shapes::Carried { item: it });
    moves::override_gravity(w, f); // @304
    let fm = w.fighter_mut(f);
    fm.body.vx = 0.0;
    fm.body.vy = 0.0;
    fighter::set_grounded(w, f, false);
    let fm = w.fighter_mut(f);
    fm.platforms.base = None;
    fm.hit.kb_mode = false; // None from (Custom, 2)
    fm.moves.impulse = None;
    fm.moves.hit_launch = false;
    moves::recall_move(w, f, at_x, at_y);
    phases::root_moved(w, f);
    set_is_being_held(w, f, true);
    w.fighter_mut(f).moves.flying = true;
}

/// `setIsBeingHeld`: BPI_SetIsBeingHeld on the pawn (the class's cast to its own interface:
/// a pawn without the hook is the failed cast, nothing).
pub fn set_is_being_held(w: &mut World, f: FighterId, held: bool) {
    if let Some(h) = w.fighter(f).hooks.char.set_is_being_held {
        h(w, f, held);
    }
}

/// `carryEnd`: the fighter's part of Fastball's End (@4749-6118), run by the class's end
/// while the item is still its attach parent (UWorld::DestroyActor: EndPlay ahead of the detach of attached
/// children): BPI_SetIsBeingHeld(false) @4953 when it had been picked up (the caller's test), the facing written back
/// (no change), ClearGravityOverride @6001, Falling @6118 while the attach parent is valid.
/// SetMovementMode(Falling) leaves whatever mode the fighter is in, (Custom, 2) included. The destroy that ends a
/// carry on a hit (Fastball's OnInstigatorKnockedBack @1986: DestroySpawnedActor) runs inside the hit, at the
/// KnockedBack broadcast (DamageAndLaunch @10471, receiveHit), ahead of the knockback launch at the
/// end of the hit's pause (ApplyKnockback): the mode is still Flying there and the launch comes after.
pub fn carry_end(w: &mut World, f: FighterId, it: ItemId) {
    set_is_being_held(w, f, false);
    moves::clear_gravity_override(w, f); // @6001, ahead of the attach parent's test
    if w.fighter(f).shapes.carried.as_ref().map(|c| c.item) != Some(it) {
        return;
    }
    let fm = w.fighter_mut(f);
    fm.moves.flying = false; // SetMovementMode(Falling)
    fm.hit.kb_mode = false;
    fighter::set_grounded(w, f, false);
}

/// `detachCarried`: the native detach after the item's EndPlay (attached children detached,
/// KeepWorld): the fighter stays where the item left it. The penetration the weld left is resolved by the next
/// sweeping move (settle), as for the other detaches of the port.
pub fn detach_carried(w: &mut World, fs: &[FighterId], it: ItemId) {
    for &f in fs {
        if w.fighter(f).shapes.carried.as_ref().map(|c| c.item) != Some(it) {
            continue;
        }
        w.fighter_mut(f).shapes.carried = None;
        moves::settle(w, f);
    }
}

/// `matchFighters` into `out`: the pawns but the minions, else the players.
fn match_fighters_into(w: &World, out: &mut Vec<FighterId>) {
    out.clear();
    match &w.pawns {
        Some(ps) => out.extend(ps.iter().copied().filter(|&p| !w.fighter(p).life.is_minion)),
        None => out.extend_from_slice(&w.players),
    }
}

/// Runs `step` for every fighter of `matchFighters(w)` as it is at the call, allocating nothing once the scratch has
/// grown.
fn each_match_fighter(w: &mut World, mut step: impl FnMut(&mut World, FighterId)) {
    let mut order = std::mem::take(&mut w.scratch.order);
    match_fighters_into(w, &mut order);
    for &f in &order {
        step(w, f);
    }
    w.scratch.order = order;
}

/// The item a fighter is carried by, unless it is dead.
// Unverified: an item whose slot is gone is taken as not followed (`follow_holder`, `follow_all` read none).
// The item's end
// (`detachCarried`) clears the carry of every fighter of the match first, so only a
// carry the end does not reach would differ.
fn carried_live(w: &World, f: FighterId) -> Option<ItemId> {
    let fm = w.fighter(f);
    if fm.life.dead != 0.0 {
        return None;
    }
    fm.shapes.carried.as_ref().map(|c| c.item)
}

/// `followItem`: the weld, a carried fighter's root at its item's root whenever the item's
/// root gets a new transform: in the item's own move (the thrown item: its ProjectileMovement's MoveComponent), after
/// the holder's movement and after the holder's animation (FinalizeAnimationUpdate -> UpdateChildTransforms: the item
/// rides the hand socket). The world updates the fighter's overlaps after each.
// Unverified: the attached children's overlaps are taken as updated at each of those moves (stock UpdateOverlaps down
// the attach chain), not read in the exe.
pub fn follow_item(w: &mut World, it: ItemId) {
    each_match_fighter(w, |w, f| {
        if carried_live(w, f) != Some(it) {
            return;
        }
        let Some(item) = w.actors.items.get(it) else { return };
        let (x, y) = (item.x, item.y);
        moves::recall_move(w, f, x, y);
        phases::root_moved(w, f);
    });
}

/// `followHolder`: the fighters carried by the items `holder` holds, at its hand socket
///.
pub fn follow_holder(w: &mut World, holder: FighterId) {
    let holder_id = w.fighter(holder).id;
    each_match_fighter(w, |w, f| {
        let Some(it) = carried_live(w, f) else { return };
        if w.actors.items.get(it).is_none_or(|i| i.holder != holder_id) {
            return;
        }
        let (x, y) = fighter::match_side::hold_point(w, holder);
        moves::recall_move(w, f, x, y);
        phases::root_moved(w, f);
    });
}

/// `followAll`: after every fighter's animation, each carried fighter at its holder's
/// socket in the new pose.
pub fn follow_all(w: &mut World) {
    each_match_fighter(w, |w, f| {
        let Some(it) = carried_live(w, f) else { return };
        let Some(holder) = w.actors.items.get(it).map(|i| i.holder) else { return };
        if holder < 0 {
            return;
        }
        let h = w.by_id(holder).expect("the carrying item's holder");
        let (x, y) = fighter::match_side::hold_point(w, h);
        moves::recall_move(w, f, x, y);
        phases::root_moved(w, f);
    });
}

/// `faceAlong`: Fastball's facing write (ReceiveHit / HitNonPlayer @4131, Thrown @3499):
/// with the item's Velocity.Y not 0, BPI_SetFacingLogic(Velocity.Y > 0), FacingLeft on UE's Y (= -x).
pub fn face_along(w: &mut World, f: FighterId, it: ItemId) {
    let vx = w.actors.items.get(it).expect("a live item").vx;
    if vx != 0.0 {
        fighter::set_facing(w, f, if vx < 0.0 { -1.0 } else { 1.0 });
    }
}

/// `jumpSectionLogic`: Comp_Pawn_MontageNavigation JumpToSectionLogic(section, null): the
/// current active montage.
pub fn jump_section_logic(w: &mut World, f: FighterId, section: Name) {
    if let Some(m) = anim::active_instance(w, f) {
        moves::nav_jump(w, f, m, section);
    }
}

/// `inSection`: GetCurrentActiveMontage == montage and Montage_GetCurrentSection ==
/// section (Fastball End @6449-6695).
pub fn in_section(w: &World, f: FighterId, montage: Name, section: Name) -> bool {
    let Some(m) = anim::active_instance(w, f) else { return false };
    let Some((name, cur)) = anim::instance_section(w, f, m) else { return false };
    name == montage && !cur.is_none() && mvs_anim::montage::same_name(w.names.str(cur), w.names.str(section))
}

// ---- fly tick: Comp_Pawn_Fly ReceiveTick, the flight's input, speed pick and fly trace

/// The MakeArray [0] @1532: WorldStatic.
const FLY_TYPES: [Real; 1] = [0.0];

/// `flyTick`: ReceiveTick @1017-1951, a component tick of TG_DuringPhysics (the World's
/// step calls it after the pawn's duringPhysicsTick, so after its MoveCommandManager). Its state and Update Is Flying
/// are `fighter::moves`'.
// Unverified: the order of the Fly tick, the manager's tick, the montage's End notify (mesh tick) and the hit inside
// one frame is not fixed by the code; the port runs the manager first, then this tick. A hit is seen at this tick in
// either order (Falling at the next Fly tick).
pub fn fly_tick(w: &mut World, f: FighterId) {
    // HasAuthority @1017, Is Flying @1069.
    if !w.fighter(f).moves.fly.as_ref().is_some_and(|s| s.is_flying) {
        return;
    }
    let p: FlyProps = moves::fly_props(w, f).expect("a flying fighter's fly props");
    // CheckHitstunActive NOR IsMovingOnGround @1214, else Set Is Flying Server(false) @1956.
    let fm = w.fighter(f);
    if fighter::in_hitstun(w, f) || (fm.body.on_floor && !fm.moves.flying && !fm.hit.kb_mode) {
        moves::update_is_flying(w, f, &p, false);
        return;
    }
    if !w.fighter(f).moves.flying {
        moves::set_flying_mode(w, f, true); // MovementMode != 5 -> SetMovementMode(5) @1266-1355
    }
    // Get Local Input(Last Direction) @1417 -> @2041: Get8WayDirection of this moment's stick and facing, the vector
    // per direction byte by the Facing Left of the flight's start (UE (0, -1, 0) forward when facing right, i.e. +sim
    // x; (0, 1, 0) facing left), up (0, 0, 1), down, back = -forward, the diagonals as sums (not normalised); Neutral
    // and Default (4, 5): zero with Can Hover In Place, else Last Direction.
    let fm = w.fighter(f);
    let d = crate::input::get8_way(fm.controls.inp.x, fm.controls.inp.y, fighter::facing(w, f));
    let s = fm.moves.fly.as_ref().expect("the fly state");
    let fwd = if s.facing_left { -1.0 } else { 1.0 };
    let (ix, iy) = match d {
        Dir::Up => (0.0, 1.0),
        Dir::Down => (0.0, -1.0),
        Dir::Fwd => (fwd, 0.0),
        Dir::Back => (-fwd, 0.0),
        Dir::FwdUp => (fwd, 1.0),
        Dir::FwdDown => (fwd, -1.0),
        Dir::BackUp => (-fwd, 1.0),
        Dir::BackDown => (-fwd, -1.0),
        Dir::Neutral | Dir::Default => {
            if p.hover_in_place {
                (0.0, 0.0)
            } else {
                (s.last_x, s.last_y)
            }
        }
    };
    // AddMovementInput(vector, 1.0, false) @2710: into the ControlInputVector the next CMC tick consumes, beside the
    // pawn's own HandleMovementInput (Fighter.moveIn) and a ControlSpeed command's input of the same DuringPhysics.
    // Then Add Movement Input Server (@4647 / @5692, the same vector) -> @1972 -> Add Movement Input Multicast @1999
    // -> @6020: LocallyControlled is false on every fighter (the fly move's head), so AddMovementInput(vector)
    // @6086 a second time, in the same call. Two inputs a tick: the CMC clamps the sum to length 1 (the physics'
    // flyingVelocity; the 4.26 source's rule, unverified in the exe), so the doubling shows only beside the other
    // inputs of the vector (moveIn, a command's).
    // Deviation: as for a command's input, what a flight that ends below leaves in the vector reaches the next CMC
    // tick in Falling, whose acceleration the port takes from Fighter.moveIn alone (the physics drops flyInput).
    let fm = w.fighter_mut(f);
    if ix != 0.0 || iy != 0.0 {
        match &mut fm.moves.fly_input {
            Some(fi) => {
                fi[0] += ix + ix;
                fi[1] += iy + iy;
            }
            None => fm.moves.fly_input = Some([ix + ix, iy + iy]),
        }
    }
    // The speed pick by the velocity's direction @3059-3402: Normal(Velocity, 1e-4) (zero under the tolerance, so
    // Horizontal), |Y| >= |Z| Horizontal, else Z > 0 Upward, else Downward. Only when it differs from
    // PreviousFlySpeed (@3457), which the tick never writes, is MaxFlySpeed written (@3907; the server event's
    // UpdateSpeed write @6147 is the same value). Either branch then sets Last Direction to the commanded vector
    // (Add Movement Input Server @1972; the multicast @6020 again; its second AddMovementInput @6086 is above).
    let n = math::ue::normal([0.0, fm.body.vx, fm.body.vy], 1e-4);
    let pick = if n[1].abs() >= n[2].abs() {
        p.horizontal
    } else if n[2] > 0.0 {
        p.upward
    } else {
        p.downward
    };
    let s = fm.moves.fly.as_mut().expect("the fly state");
    if pick != s.prev_fly_speed {
        s.max_fly_speed = pick;
    }
    s.last_x = ix;
    s.last_y = iy;
    // The fly trace @1440-1790: CapsuleTraceMultiForObjects from the actor location to location + Normal(Velocity) x
    // 10, the root capsule's scaled size, [WorldStatic], no actors to ignore, bIgnoreSelf (the component's owner).
    // The hits in Time order with the initial overlaps first; the first whose component's profile is not `Platform`
    // ends the flight (@292-344): the stage block (profile Floor) and another fighter's KnockBack sphere while a
    // Hitframe has it on are WorldStatic, the pass-through platforms are not.
    let (x0, y0) = (w.fighter(f).body.x, fighter::actor_y(w, f));
    let (radius, half) = (fighter::cap_radius(w, f), fighter::capsule_half(w, f));
    let mut out = std::mem::take(&mut w.fighter_mut(f).bufs.hits);
    traces::capsule_trace_multi_for_objects(
        w,
        x0,
        y0,
        x0 + n[1] * 10.0,
        y0 + n[2] * 10.0,
        radius,
        half,
        &FLY_TYPES,
        &[],
        Some(ActorRef::Fighter(f)),
        &mut out,
    );
    let end = out.iter().any(|h| h.profile != crate::name!("Platform"));
    out.clear();
    w.fighter_mut(f).bufs.hits = out;
    if end {
        moves::update_is_flying(w, f, &p, false);
    }
}

// ---- rumble: controller rumble (force feedback)
//
// BaseFighter ControllerRumble / RemoveControllerRumble and GetHitRumble. The sim only says who rumbles and with
// which effect asset (`Event::Rumble` / `RumbleStop`); the game plays it on its own pad.

/// `controllerRumble`: ControllerRumble (BaseFighter ubergraph @164492-164823; a client
/// RPC, which offline runs locally: GetFunctionCallspace): IsValid(MatchPlayerData) @164492 (every fighter of the port
/// has one), IsVibrationEnabled @164531 (IsLocallyControlled AND NOT MatchPlayerData.IsBot AND the player's
/// Get-Input-Vibration, on by default and with no toggle in the port), the controller a PFGPlayerController
/// @164584-164659 (a bot's AIController never passes), its isGamePadKey @164734, then
/// K2_ClientPlayForceFeedback(effect, Tag None, bLooping false, bIgnoreTimeDilation false, bPlayWhilePaused false)
/// @164766. What the sim tests: NOT IsBot (`botControlled`, the match's bot flag that MatchPlayerData.IsBot stands
/// for). Left to the game: IsLocallyControlled (the game's own fighter: under rollback every peer runs every fighter),
/// the vibration preference and isGamePadKey (the device of the last Attack press / release or Special release:
/// PFGPlayerController @17438, @50118; the sim's input carries no such bit, and its `axis` flag is the stick's device,
/// another rule).
// Unverified: a null effect posts nothing (GetHitRumble with HitReaction None, a hit packet without ControllerRumble):
// what K2_ClientPlayForceFeedback / ClientStopForceFeedback do with a null effect is native, not read; the one stop
// with a null effect (BPI_RemoveCurrentControllerRumble, the pause hold) is the game's.
pub fn controller_rumble(w: &mut World, f: FighterId, name: Option<Name>) {
    let Some(name) = name.filter(|n| !n.is_none()) else { return };
    if w.fighter(f).controls.bot_controlled {
        return;
    }
    let tag = crate::name!("None");
    let fm = w.fighter_mut(f);
    let id = fm.id;
    fm.events.push(Event::Rumble {
        fighter: id,
        name,
        looping: false,
        tag,
        ignore_time_dilation: false,
        play_while_paused: false,
    });
}

/// `removeControllerRumble`: RemoveControllerRumble (@165054-165386): the same gates, then
/// ClientStopForceFeedback(effect, None) @165328.
pub fn remove_controller_rumble(w: &mut World, f: FighterId, name: Option<Name>) {
    if w.fighter(f).controls.bot_controlled {
        return;
    }
    let tag = crate::name!("None");
    let fm = w.fighter_mut(f);
    let id = fm.id;
    fm.events.push(Event::RumbleStop { fighter: id, name, tag });
}

/// `hitRumble`: Comp_Pawn_DamageManager GetHitRumble (@0-414), by Enum_HitReaction's byte:
/// 0 Default and 5 Air Small; 1 Floored, 2 SpikeBack, 3 SpikeDown and 4 SpikeUp Medium; 6 None (and any other) writes
/// no output: `None`.
pub fn hit_rumble(reaction: Option<Real>) -> Option<Name> {
    let r = reaction.unwrap_or(0.0);
    if r == 0.0 || r == 5.0 {
        Some(crate::name!("SmallForceFeedbackRumbleEffect"))
    } else if (1.0..=4.0).contains(&r) {
        Some(crate::name!("MediumForceFeedbackRumbleEffect"))
    } else {
        None
    }
}
