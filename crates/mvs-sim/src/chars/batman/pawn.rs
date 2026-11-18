//! Pawn_BatmanV2's own logic (offsets of its ubergraph): BeginPlay's writes, the glide, the occlusion passive and
//! BPI_Cleanup; the pawn's own hooks of `mod.rs` (death, matchReset) and its notifies AnimNotify_CancelMoveBuffOnEnd
//! and AnimNotify_Darkness.

use super::state::{BatmanState, batman, batman_w};
use crate::Real;
use crate::chars::CharState;
use crate::combat::BuffRef;
use crate::combat::buffs::{buff, end_buff, spawn_buff};
use crate::consts::AIR_CONTROL;
use crate::events::{Event, NotifyRef};
use crate::fighter::FighterId;
use crate::fighter::anim::montages::instance_data;
use crate::fighter::anim::{nav_of, stop_dash_montage};
use crate::fighter::common::{data_of, default_terminal, latent_frames};
use crate::fighter::match_side::end_move_buff;
use crate::fighter::moves::Impulse;
use crate::fighter::moves::{is_falling, jump_velocity, set_terminal, update_air_control};
use crate::fighter::{actor_y, in_hitstun, update_hurt_boxes};
use crate::name;
use crate::registry::{CharHooks, NotifyHandler, Registry, RegistryError};
use crate::world::World;

/// NetGravity(1.125) @3633.
const GLIDE_GRAVITY: Real = 1.125;
/// SetTerminalVelocity(false, 1000, false) @3670, the select @4654.
const GLIDE_TERMINAL: Real = 1000.0;
/// AssignGlideValues @4417 / @4577.
const GLIDE_CLAMP: Real = -1000.0;

/// The hooks of this file in Batman's CharHooks.
pub fn hooks(h: &mut CharHooks) {
    h.death = Some(death);
    h.match_reset = Some(match_reset);
    h.tick = Some(tick);
    h.end_jump = Some(end_jump);
    h.occluded_event = Some(occluded_event);
    h.not_occluded_event = Some(not_occluded_event);
    h.is_occluded = Some(is_occluded);
    h.get_speed = Some(get_speed);
    h.reset = Some(cleanup);
}

/// The notifies `mod.rs` enters.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.notifies.enter(
        "AnimNotify_CancelMoveBuffOnEnd_C",
        NotifyHandler { end: Some(cancel_move_buff_end), ..NotifyHandler::default() },
    )?;
    r.notifies.enter(
        "AnimNotify_Darkness_C",
        NotifyHandler { begin: Some(darkness_begin), end: Some(darkness_end), ..NotifyHandler::default() },
    )
}

// ---- the notifies

/// AnimNotify_CancelMoveBuffOnEnd Received_NotifyEnd, on authority: the owner's Comp_State_Manager
/// CheckHasBuff(Buff_MoveToPosition_C) @327 and, when its Instigator is the owner @535, its K2_DestroyActor @609. No
/// Begin. The port's Buff_MoveToPosition is the force move of the shared ally code (Comp_Pawn_TeleportToAlly's and
/// Arya's knife's, both spawned with the owner as Instigator): ended while it lasts.
fn cancel_move_buff_end(w: &mut World, f: FighterId, _n: NotifyRef, _key: i32) {
    if w.fighter(f).team.ally.buff > 0.0 {
        end_move_buff(w, f);
    }
}

/// Whether the fighter is a Batman (`f.ch.key === 'batman'`).
fn is_batman(w: &World, f: FighterId) -> bool {
    w.names.str(w.fighter(f).ch) == super::KEY
}

/// AnimNotify_Darkness: Received_NotifyBegin @121 BPI_IdleFade(true), Received_NotifyEnd @121 BPI_IdleFade(false), on
/// a BPI_BaseFighter owner. BaseFighter's own BPI_IdleFade is empty; Pawn_BatmanV2's @7832 plays or reverses
/// IdleFadeTimeline (`BatmanState.idle_fade`, the view's). On another fighter (a Disguise's copy of his montage) it
/// does nothing.
fn darkness_begin(w: &mut World, f: FighterId, _n: NotifyRef, _key: i32) {
    if is_batman(w, f) {
        batman_w(w, f).idle_fade = true;
    }
}

fn darkness_end(w: &mut World, f: FighterId, _n: NotifyRef, _key: i32) {
    if is_batman(w, f) {
        batman_w(w, f).idle_fade = false;
    }
}

// ---- the pawn's hooks of `mod.rs`

/// The port does not update a dead pawn's AnimBP.
fn death(w: &mut World, f: FighterId) {
    batman_w(w, f).slot_mix.slot_mix.clear();
}

/// A match reset spawns a new pawn: a new state (the World's reset calls this behind the fighter's spawn; also the
/// first spawn of a match).
fn match_reset(w: &mut World, f: FighterId) {
    w.fighter_mut(f).cs = CharState::Batman(BatmanState::default());
    begin_play(w, f);
}

// ---- BeginPlay

/// ReceiveBeginPlay @1801 (a new pawn: the World's reset, the match's first spawn included): the parent's, then
/// OriginalGravityScale = CharacterMovement.GravityScale @1826 and OriginalWalkSpeed = MaxWalkSpeed @1875 (read by
/// nothing of his: Pawn_C017's reader is its own variable), and Delay(0.2) @1925, whose resume @181 turns
/// HurtBoxGlide's overlap events off. Until then the shape is on: BaseFighter's construction script turns on every
/// HurtBox-tagged shape but HurtBoxFloorBounce / IgnorExperimental (UserConstructionScript @756-1793), so the new pawn
/// starts with it on whatever the last match's pawn left. The AnimBP cache @252 and the SkinApplied binding @1980
/// have no sim effect.
pub fn begin_play(w: &mut World, f: FighterId) {
    let g = w.fighter(f).moves.gravity_scale;
    batman_w(w, f).original_gravity_scale = g;
    update_hurt_boxes(w, f, &[name!("HurtBoxGlide")], true);
    batman_w(w, f).glide_hurt_delay = latent_frames(0.2);
}

// ---- the glide

/// ReadGlideInput's predicate (@2153-2634, computed again @2701-3137 on a change): no active hitstun, the Jump button
/// pressed (IsButtonPressed(2)), MovementMode Falling (EqualEqual_ByteByte(MovementMode, 3) @2319 / @2822), the
/// gravity not modified (GetIsGravityModified: GravityModifierCounter > 0), not Jumping, not wall sliding, not
/// evading.
fn want_glide(w: &World, f: FighterId) -> bool {
    let fi = w.fighter(f);
    !in_hitstun(w, f)
        && fi.controls.inp.jump
        && is_falling(w, f)
        && fi.moves.gravity_mod_count == 0
        && !fi.moves.cmd_jumped
        && !fi.wall.wall_sliding
        && !fi.evade.evading
}

/// StopAnimMontage(WallJumpAnim): ACharacter::StopAnimMontage with a montage stops its active instance with the
/// montage's BlendOut, when it is not stopped already. The wall jump is the port's plain montage.
pub fn stop_wall_jump_anim(w: &mut World, f: FighterId) {
    let Some(d) = &w.fighter(f).montages.dash_montage else { return };
    let (uid, name, out) = (d.montage.uid, d.montage.name, d.blending_out());
    if out || nav_of(w, f).wall_jump.as_deref() != Some(w.names.str(name)) {
        return;
    }
    let data = w.data.clone();
    let Some(blend) = instance_data(&data, w, f, uid).map(|m| m.blend_out) else { return };
    stop_dash_montage(w, f, blend);
}

/// SetGlideValueMulticast @5972: isGliding, BPI_IdleFade(isGliding) @5991, the AnimBP's BPI_SetIsGliding @6135, and
/// StopAnimMontage(WallJumpAnim) @6191 when gliding.
fn set_glide_value(w: &mut World, f: FighterId, g: bool) {
    let s = batman_w(w, f);
    s.gliding = g;
    s.idle_fade = g;
    s.anim_gliding = g;
    if g {
        stop_wall_jump_anim(w, f);
    }
}

/// CharacterMovement's DefaultTerminalVelocity of the fighter's data.
fn default_terminal_of(w: &World, f: FighterId) -> Real {
    default_terminal(data_of(w, f))
}

/// AssignGlideValues (server, reliable; @4164): UpdateAirControl, UpdateHurtBoxes([HurtBoxGlide], isGliding) @4211,
/// SetGlideValueMulticast @4265; not wall sliding, SetTerminalVelocity(false, isGliding ? 1000 :
/// DefaultTerminalVelocity, false) @4705; then (both paths, @4302) StopAnimMontage(WallJumpAnim) when gliding, and
/// gliding with Velocity.Z < -1000, LaunchCharacter((Velocity.X, Velocity.Y, -1000), XY and Z override) @4628 (the
/// next CMC tick takes it: HandlePendingLaunch). It writes no NetGravity.
fn assign_glide_values(w: &mut World, f: FighterId, g: bool, air_control: Real) {
    update_air_control(w, f, air_control);
    update_hurt_boxes(w, f, &[name!("HurtBoxGlide")], g);
    set_glide_value(w, f, g);
    if !w.fighter(f).wall.wall_sliding {
        let v = if g { GLIDE_TERMINAL } else { default_terminal_of(w, f) };
        set_terminal(w, f, false, v, false);
    }
    if !g {
        return;
    }
    stop_wall_jump_anim(w, f);
    let fi = w.fighter_mut(f);
    if fi.body.vy < GLIDE_CLAMP {
        let vx = fi.body.vx;
        fi.moves.impulse = Some(Impulse { vx, vy: GLIDE_CLAMP, xy: true, z: true, kind: 0, d: None });
    }
}

/// ReadGlideInput (owning client, reliable; from ReceiveTick @1722 when locally controlled: every pawn here, bots
/// included). Nothing while the predicate equals isGliding (@2686 / @3872). On a change: isGliding = it @3175, then the
/// flows pushed @3194 / @3199 run in this order: the AnimBP's BPI_SetIsGliding @3325, UpdateAirControl(isGliding ? 1 :
/// BaseAirControl) @3422, AssignGlideValues @3496 (offline and on the host it runs at once); the local gravity @3529
/// (nothing while wall sliding: on, NetGravity(1.125) and SetTerminalVelocity(false, 1000, false); off, only with the
/// gravity not modified, NetGravity(OriginalGravityScale) and SetTerminalVelocity(false, DefaultTerminalVelocity,
/// false)); the once gate @3887 (entering closes it, and with IsFalling plays sfx_cha_batman_foley_glide_flap on the
/// pawn and stops WallJumpAnim; leaving opens it). IsValidSlowfallGravity (the pawn's Gravity field against 1.125) is
/// called by BaseFighter's ValidateNetGravity alone, which nothing calls: no port.
pub fn read_glide_input(w: &mut World, f: FighterId) {
    let g = want_glide(w, f);
    let s = batman_w(w, f);
    if g == s.gliding {
        return;
    }
    s.gliding = g;
    s.anim_gliding = g;
    let air_control = if g { 1.0 } else { AIR_CONTROL };
    update_air_control(w, f, air_control);
    assign_glide_values(w, f, g, air_control);
    if !w.fighter(f).wall.wall_sliding {
        if g {
            w.fighter_mut(f).moves.net_gravity = Some(GLIDE_GRAVITY);
            set_terminal(w, f, false, GLIDE_TERMINAL, false);
        } else if w.fighter(f).moves.gravity_mod_count == 0 {
            let o = batman(w, f).original_gravity_scale;
            w.fighter_mut(f).moves.net_gravity = Some(o);
            let v = default_terminal_of(w, f);
            set_terminal(w, f, false, v, false);
        }
    }
    let s = batman_w(w, f);
    if !g {
        s.glide_once_closed = false;
    } else if !s.glide_once_closed {
        s.glide_once_closed = true;
        if is_falling(w, f) {
            let id = w.fighter(f).id;
            let ev = name!("sfx_cha_batman_foley_glide_flap");
            w.fighter_mut(f).events.push(sound(id, ev, None));
            stop_wall_jump_anim(w, f);
        }
    }
}

fn sound(fighter: i32, ev: crate::names::Name, at: Option<(Real, Real)>) -> Event {
    Event::Sound { fighter, ev, x: at.map(|a| a.0), y: at.map(|a| a.1), hit: None, pooled: false, key: None }
}

/// EndJump @5931: the parent's EndJump(DelaySeconds, Quickfall, false). The parent's body writes FastfallMult = the
/// argument first (BaseFighter @131698), and the jump's cut reads ComputeJumpHeight(FastfallMult) later (@17997 /
/// @18183): after each jump's own EndJump(T, true, FastfallMult) the cut is the unboosted JumpVelocity x
/// JumpStopVelocityMultiplier. The jump's impulse, its stop time and LastJumpVelocity were taken before the call with
/// the boost.
pub fn end_jump(w: &mut World, f: FighterId) {
    w.fighter_mut(f).fast_fall.ff_mult = false;
    let v = jump_velocity(w, f);
    w.fighter_mut(f).jump.jump_v = v;
}

// ---- the occlusion

/// A stored buff reference that still names a running buff of the fighter.
fn live(w: &World, b: Option<BuffRef>) -> Option<BuffRef> {
    b.filter(|&b| buff(w, b).is_some_and(|x| x.frames > 0.0))
}

/// OccludedEvent @5064 (after the parent's Occluded write), on authority: a valid
/// OccludedSpeedBuff returns at once @5133; else SpawnBuffActor(Buff_SpeedUp_C, self, self, 0, 0.0) @5148, stored,
/// SpeedOn = true whatever the spawn gave; then a valid OccludedDecrementEvadeBuff returns @5275, else
/// SpawnBuffActor(Buff_DecrementEvadeCount_C, self, self, 0, 6.0) @5290, stored.
pub fn occluded_event(w: &mut World, f: FighterId) {
    if live(w, batman(w, f).occluded_speed_buff).is_some() {
        return;
    }
    let b = spawn_buff(w, f, name!("Buff_SpeedUp_C"), 0.0, Some(0), Some(f), true, None);
    let s = batman_w(w, f);
    s.occluded_speed_buff = b;
    s.speed_on = true;
    if live(w, batman(w, f).occluded_evade_buff).is_some() {
        return;
    }
    let b = spawn_buff(w, f, name!("Buff_DecrementEvadeCount_C"), 6.0, Some(0), Some(f), true, None);
    batman_w(w, f).occluded_evade_buff = b;
}

/// NotOccludedEvent @4849 (after the parent's): not on a dedicated server, PF_Post_Event_At_Location(
/// sfx_cha_batman_smokebomb_buff, the actor's location) @4951; on authority Delay(1.0) @4794. A Delay reached again
/// while it is pending starts nothing (the latent action of that UUID exists): the first one's end stands.
pub fn not_occluded_event(w: &mut World, f: FighterId) {
    let (id, x, y) = (w.fighter(f).id, w.fighter(f).body.x, actor_y(w, f));
    w.fighter_mut(f).events.push(sound(id, name!("sfx_cha_batman_smokebomb_buff"), Some((x, y))));
    let s = batman_w(w, f);
    if s.not_occluded_delay == 0.0 {
        s.not_occluded_delay = latent_frames(1.0);
    }
}

/// BPI_IsOccluded: BaseFighter.Occluded.
pub fn is_occluded(w: &World, f: FighterId) -> bool {
    crate::actors::smoke_bomb::occluded(w, f)
}

/// BPI_GetSpeed: SpeedOn.
pub fn get_speed(w: &World, f: FighterId) -> bool {
    batman(w, f).speed_on
}

// ---- tick, cleanup

/// ReceiveTick @1664: the parent's, then ReadGlideInput (@1722); the heart skin's opacity @1786 is the view's. Then
/// the actor's latent actions (after ReceiveTick, as the fighter's pawn tick runs EndJump's): BeginPlay's Delay(0.2)
/// @181 and NotOccludedEvent's Delay(1.0) @15 (SpeedOn = false, each stored buff still valid destroyed @69 / @144, the
/// pointers kept).
// Unverified: the Delays count the pawn's ticks from the one they were made in when that comes before the pawn's
// tick of the frame (BeginPlay, a buff's end in the world's pass): not checked against a capture.
pub fn tick(w: &mut World, f: FighterId) {
    read_glide_input(w, f);
    let s = batman_w(w, f);
    if s.glide_hurt_delay > 0.0 {
        s.glide_hurt_delay -= 1.0;
        if s.glide_hurt_delay == 0.0 {
            update_hurt_boxes(w, f, &[name!("HurtBoxGlide")], false);
        }
    }
    let s = batman_w(w, f);
    if s.not_occluded_delay > 0.0 {
        s.not_occluded_delay -= 1.0;
        if s.not_occluded_delay == 0.0 {
            s.speed_on = false;
            let (sb, eb) = (s.occluded_speed_buff, s.occluded_evade_buff);
            if let Some(b) = live(w, sb) {
                end_buff(w, f, b);
            }
            if let Some(b) = live(w, eb) {
                end_buff(w, f, b);
            }
        }
    }
}

/// BPI_Cleanup @8023 (BaseFighter ResetFighterLogic @554): isGliding = false and SpeedOn = false, nothing else (no
/// gravity, terminal, air control or hurtbox write, no AnimBP call, the Delays and buffs left): a Batman that respawns
/// with HurtBoxGlide on keeps it on until his next glide change, and his AnimBP keeps IsGliding.
pub fn cleanup(w: &mut World, f: FighterId) {
    let s = batman_w(w, f);
    s.gliding = false;
    s.speed_on = false;
}
