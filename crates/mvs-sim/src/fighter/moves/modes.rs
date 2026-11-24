//! The movement component's mode and values a fighter's Blueprints write: the movement mode's reads
//!, the friction modifiers and the braking values,
//! MaxAcceleration, the input override and its disable counters
//! and Comp_Pawn_Fly's state.

use super::*;
use crate::consts::{
    AIR_CONTROL, BRAKING_DECEL, BRAKING_FRICTION, BRAKING_FRICTION_FACTOR, GROUND_FRICTION, MAX_ACCEL, MAX_ACCEL_AIR,
    MOVE_COMMAND_BRAKING,
};
use crate::fighter::body::set_grounded;

// ---- movement-mode

/// `isFalling`: UCharacterMovementComponent::IsFalling (MOVE_Falling); false in the
/// knockback mode (Custom 2).
pub fn is_falling(w: &World, f: FighterId) -> bool {
    let fi = w.fighter(f);
    !fi.body.on_floor && !fi.hit.kb_mode && !is_flying(w, f) && !fi.moves.move_none && fi.life.dead <= 0.0
}

/// `isFlying`: UCharacterMovementComponent::IsFlying (MOVE_Flying): a ControlSpeed
/// command or Comp_Pawn_Fly, a buff stun's suspension or an item's weld.
pub fn is_flying(w: &World, f: FighterId) -> bool {
    let fi = w.fighter(f);
    fi.moves.flying || fi.buffs.suspended || fi.shapes.carried.is_some()
}

/// `disableMovement`: UCharacterMovementComponent::DisableMovement (MOVE_None).
// Unverified: a write of the floor flag under the hold does not end None here.
pub fn disable_movement(w: &mut World, f: FighterId) {
    w.fighter_mut(f).moves.move_none = true;
}

/// `enableFalling`: SetMovementMode(Falling), or RemoveGrapple's direct write of
/// Falling (1410dd000).
pub fn enable_falling(w: &mut World, f: FighterId) {
    w.fighter_mut(f).moves.move_none = false;
    set_grounded(w, f, false);
}

// ---- friction

/// `FrictionState`: BaseFighter's FrictionModifiers and the movement component's
/// friction values they write.
#[derive(State, Debug, Default)]
pub struct FrictionState {
    /// BaseFighter FrictionModifiers (TMap<FName, float>), in insertion order.
    pub mods: Vec<(Name, Real)>,
    /// FMC GroundFriction.
    pub gf: Real,
    /// CMC BrakingFriction: written, not read (bUseSeparateBrakingFriction is false).
    pub bf: Real,
    /// CMC BrakingDecelerationWalking.
    pub bdw: Real,
    /// CMC BrakingDecelerationFalling.
    pub bdf: Real,
    /// FighterMovementComponent MoveCommandBraking (+0xb0c).
    pub mcb: Real,
    /// CMC BrakingFrictionFactor, `None` at the template's 5.
    pub bff: Option<Real>,
}

/// Stock UCharacterMovementComponent BrakingFriction; no template sets it.
const BASE_BRAKING_FRICTION: Real = 0.0;
/// The FMC's BeginPlay BrakingDecelerationFalling.
const BASE_BRAKING_DECEL_FALLING: Real = 0.0;

/// `state`: the friction state, made with the values a move command's NetFriction left.
fn friction_state(w: &mut World, f: FighterId) -> &mut FrictionState {
    let m = &mut w.fighter_mut(f).moves;
    let z = m.friction_zero;
    m.friction.get_or_insert_with(|| FrictionState {
        mods: Vec::new(),
        gf: if z { 0.0 } else { GROUND_FRICTION },
        bf: BASE_BRAKING_FRICTION,
        bdw: if z { MOVE_COMMAND_BRAKING } else { BRAKING_DECEL },
        bdf: if z { MOVE_COMMAND_BRAKING } else { BASE_BRAKING_DECEL_FALLING },
        mcb: MOVE_COMMAND_BRAKING,
        bff: None,
    })
}

/// `groundFriction`: GroundFriction, the walking CalcVelocity's Friction.
pub fn ground_friction(w: &World, f: FighterId) -> Real {
    let m = &w.fighter(f).moves;
    let gf = match &m.friction {
        Some(s) => s.gf,
        None if m.friction_zero => 0.0,
        None => GROUND_FRICTION,
    };
    gf * crate::world::own_rules::ice(w) // (the port's own Ice Rink)
}

/// `walkBrakingFriction`: GroundFriction x BrakingFrictionFactor, float32.
pub fn walk_braking_friction(w: &World, f: FighterId) -> Real {
    let m = &w.fighter(f).moves;
    let bf = match &m.friction {
        Some(s) => s.gf * s.bff.unwrap_or(BRAKING_FRICTION_FACTOR),
        None if m.friction_zero => 0.0,
        None => BRAKING_FRICTION,
    };
    bf * crate::world::own_rules::ice(w) // (the port's own Ice Rink)
}

/// `brakingFrictionFactor`: the factor of the CMC's every braking friction.
pub fn braking_friction_factor(w: &World, f: FighterId) -> Real {
    w.fighter(f).moves.friction.as_ref().and_then(|s| s.bff).unwrap_or(BRAKING_FRICTION_FACTOR)
}

/// `setCmcFriction`: a graph's direct writes of GroundFriction, BrakingFriction and BrakingFrictionFactor.
pub fn set_cmc_friction(w: &mut World, f: FighterId, gf: Real, bf: Real, bff: Real) {
    let s = friction_state(w, f);
    s.gf = gf;
    s.bf = bf;
    s.bff = if bff == BRAKING_FRICTION_FACTOR { None } else { Some(bff) };
}

/// `brakingDecelWalking`: GetMaxBrakingDeceleration while walking.
pub fn braking_decel_walking(w: &World, f: FighterId) -> Real {
    let m = &w.fighter(f).moves;
    let b = match &m.friction {
        Some(s) => s.bdw,
        None if m.friction_zero => MOVE_COMMAND_BRAKING,
        None => BRAKING_DECEL,
    };
    b * crate::world::own_rules::ice(w) // (the port's own Ice Rink)
}

/// `brakingDecelFalling`: GetMaxBrakingDeceleration while falling.
pub fn braking_decel_falling(w: &World, f: FighterId) -> Real {
    let m = &w.fighter(f).moves;
    match &m.friction {
        Some(s) => s.bdf,
        None if m.friction_zero => MOVE_COMMAND_BRAKING,
        None => BASE_BRAKING_DECEL_FALLING,
    }
}

/// `setNetFriction`: a NetFriction the CMC takes in OnMovementUpdated: true = NetFriction(0.0), false =
/// NetFriction(BPI_GetBaseGroundFriction).
pub fn set_net_friction(w: &mut World, f: FighterId, zero: bool) {
    let m = &mut w.fighter_mut(f).moves;
    m.friction_zero = zero;
    if let Some(s) = &mut m.friction {
        s.gf = if zero { 0.0 } else { GROUND_FRICTION };
    }
}

/// `nativeBraking`: OnMovementUpdated's (FUN_1410d5550) braking decelerations:
/// MoveCommandBraking while GroundFriction differs from its BeginPlay value, the BeginPlay values otherwise.
pub fn native_braking(w: &mut World, f: FighterId) {
    let Some(s) = &mut w.fighter_mut(f).moves.friction else { return };
    if s.gf != GROUND_FRICTION {
        s.bdw = s.mcb;
        s.bdf = s.mcb;
    } else {
        s.bdw = BRAKING_DECEL;
        s.bdf = BASE_BRAKING_DECEL_FALLING;
    }
}

/// `setIsFrictionModified`: BaseFighter BPI_SetIsFrictionModified (@108405-108553):
/// Map_Add or Map_Remove, then UpdateFrictionModifier.
pub fn set_is_friction_modified(w: &mut World, f: FighterId, mult: Real, name: Name, add: bool) {
    let s = friction_state(w, f);
    if add {
        let v = mult;
        match s.mods.iter_mut().find(|m| m.0 == name) {
            Some(m) => m.1 = v,
            None => s.mods.push((name, v)),
        }
    } else {
        s.mods.retain(|m| m.0 != name);
    }
    update_friction_modifier(s);
}

/// `updateFrictionModifier`: BaseFighter UpdateFrictionModifier: the bases x the float32 product of the map's
/// values (the bases for an empty map).
// Unverified: a TMap's value order after removals follows its sparse array, the port's list its insertion order.
fn update_friction_modifier(s: &mut FrictionState) {
    if s.mods.is_empty() {
        s.mcb = MOVE_COMMAND_BRAKING;
        s.bdw = BRAKING_DECEL;
        s.bf = BASE_BRAKING_FRICTION;
        s.gf = GROUND_FRICTION;
        return;
    }
    let mut m: Real = 1.0;
    for &(_, v) in &s.mods {
        m *= v;
    }
    s.mcb = MOVE_COMMAND_BRAKING * m;
    s.bdw = BRAKING_DECEL * m;
    s.bf = BASE_BRAKING_FRICTION * m;
    s.gf = GROUND_FRICTION * m;
}

/// `frictionReset`: a new pawn's empty FrictionModifiers and template values.
pub fn friction_reset(w: &mut World, f: FighterId) {
    w.fighter_mut(f).moves.friction = None;
}

// ---- max-accel

/// `maxAccel`: CharacterMovement MaxAcceleration (+0x1a0).
pub fn max_accel(w: &World, f: FighterId) -> Real {
    w.fighter(f).moves.max_accel.unwrap_or(MAX_ACCEL)
}

/// `setMaxAccel`.
pub fn set_max_accel(w: &mut World, f: FighterId, v: Real) {
    w.fighter_mut(f).moves.max_accel = if v == MAX_ACCEL { None } else { Some(v) };
}

/// `maxAccelReset`.
pub fn max_accel_reset(w: &mut World, f: FighterId) {
    w.fighter_mut(f).moves.max_accel = None;
}

// ---- input-override

/// `InputOverrideState`: BaseFighter's input override and disable counters.
#[derive(State, Debug, Default)]
pub struct InputOverrideState {
    /// OverrideXInput.
    pub ox: bool,
    /// XAxisInputOverride (float32).
    pub xv: Real,
    /// OverrideYInput.
    pub oy: bool,
    /// YAxisInputOverride.
    pub yv: Real,
    /// MoveXAxisValue as last written.
    pub mx: Real,
    /// MoveYAxisValue.
    pub my: Real,
    /// The frame input's x of the last frame (an axis event on keys is a change of it).
    pub rx: Real,
    pub ry: Real,
    /// JumpDisabledCounter.
    pub jd: i32,
    /// EvadeDisabledCounter.
    pub ed: i32,
    /// AttackDisabledCounter.
    pub ad: i32,
    /// InputDisabledCounter (`None`: 0).
    pub id: Option<i32>,
}

/// `state`: the override's state, made with the defaults from the frame's input.
fn ov_state(w: &mut World, f: FighterId) -> &mut InputOverrideState {
    let fi = w.fighter_mut(f);
    let (x, y) = (fi.controls.inp.x, fi.controls.inp.y);
    fi.moves.input_ov.get_or_insert_with(|| InputOverrideState { mx: x, my: y, rx: x, ry: y, ..Default::default() })
}

/// `settle`: back to none once nothing differs from the defaults.
fn ov_settle(w: &mut World, f: FighterId) {
    let fi = w.fighter_mut(f);
    let (x, y) = (fi.controls.inp.x, fi.controls.inp.y);
    let Some(s) = &fi.moves.input_ov else { return };
    if s.ox || s.oy || s.jd != 0 || s.ed != 0 || s.ad != 0 || s.id.is_some_and(|i| i != 0) {
        return;
    }
    if s.mx != x || s.my != y || s.rx != x || s.ry != y {
        return;
    }
    fi.moves.input_ov = None;
}

/// `setInputOverride`: BPI_SetInputOverride (@100614-100733): MoveXAxisValue / MoveYAxisValue written at once.
pub fn set_input_override(w: &mut World, f: FighterId, override_x: bool, x: Real, override_y: bool, y: Real) {
    let s = ov_state(w, f);
    s.ox = override_x;
    s.xv = x + 0.0;
    s.oy = override_y;
    s.yv = y + 0.0;
    s.mx = s.xv;
    s.my = s.yv;
    let (mx, my) = (s.mx, s.my);
    let inp = &mut w.fighter_mut(f).controls.inp;
    // (a new input is written only when it differs: a -0 against a +0 keeps the old one)
    if inp.x != mx || inp.y != my {
        inp.x = mx;
        inp.y = my;
    }
    ov_settle(w, f);
}

/// `readMoveInput`: the frame's input as the pawn's MoveXAxisValue /
/// MoveYAxisValue, with whether the frame had a MoveX and a MoveY event (`moveXEvent` / `moveYEvent`).
pub fn read_move_input(
    w: &mut World,
    f: FighterId,
    inp: InputState,
    prev_x: Real,
    prev_y: Real,
) -> (InputState, bool, bool) {
    let fi = w.fighter_mut(f);
    let Some(s) = &mut fi.moves.input_ov else {
        let xe = inp.axis || inp.call_x || inp.x != prev_x;
        let ye = inp.axis || inp.call_y || inp.y != prev_y;
        return (inp, xe, ye);
    };
    let xe = inp.axis || inp.call_x || inp.x != s.rx;
    let ye = inp.axis || inp.call_y || inp.y != s.ry;
    s.rx = inp.x;
    s.ry = inp.y;
    if xe {
        s.mx = if s.ox { s.xv } else { inp.x };
    }
    if ye {
        s.my = if s.oy { s.yv } else { inp.y };
    }
    // (the input itself where equal: a -0 against a +0 keeps the input's)
    let same = s.mx == inp.x && s.my == inp.y;
    let mut out = inp;
    if !same {
        out.x = s.mx;
        out.y = s.my;
    }
    if !s.ox && !s.oy && s.jd == 0 && s.ed == 0 && s.ad == 0 && s.id.is_none_or(|i| i == 0) && same {
        fi.moves.input_ov = None;
    }
    (out, xe, ye)
}

/// `disableJump`: BPI_DisableJump.
pub fn disable_jump(w: &mut World, f: FighterId) {
    ov_state(w, f).jd += 1;
}

/// `enableJump`: BPI_EnableJump.
pub fn enable_jump(w: &mut World, f: FighterId) {
    if let Some(s) = &mut w.fighter_mut(f).moves.input_ov
        && s.jd > 0
    {
        s.jd -= 1;
    }
    ov_settle(w, f);
}

/// `disableEvade`: BPI_DisableEvade.
pub fn disable_evade(w: &mut World, f: FighterId) {
    ov_state(w, f).ed += 1;
}

/// `enableEvade`: BPI_EnableEvade.
pub fn enable_evade(w: &mut World, f: FighterId) {
    if let Some(s) = &mut w.fighter_mut(f).moves.input_ov
        && s.ed > 0
    {
        s.ed -= 1;
    }
    ov_settle(w, f);
}

/// `disableAttack`: BPI_DisableAttack.
pub fn disable_attack(w: &mut World, f: FighterId) {
    ov_state(w, f).ad += 1;
}

/// `enableAttack`: BPI_EnableAttack.
pub fn enable_attack(w: &mut World, f: FighterId) {
    if let Some(s) = &mut w.fighter_mut(f).moves.input_ov
        && s.ad > 0
    {
        s.ad -= 1;
    }
    ov_settle(w, f);
}

/// `disableInput`: BPI_DisableInput.
pub fn disable_input(w: &mut World, f: FighterId) {
    let s = ov_state(w, f);
    s.id = Some(s.id.unwrap_or(0) + 1);
}

/// `enableInput`: BPI_EnableInput.
pub fn enable_input(w: &mut World, f: FighterId) {
    if let Some(s) = &mut w.fighter_mut(f).moves.input_ov
        && let Some(i) = s.id
        && i != 0
    {
        s.id = if i > 1 { Some(i - 1) } else { None };
    }
    ov_settle(w, f);
}

/// `inputEnabled`: GetInputEnabled.
pub fn input_enabled(w: &World, f: FighterId) -> bool {
    w.fighter(f).moves.input_ov.as_ref().and_then(|s| s.id).is_none_or(|i| i == 0)
}

/// `jumpEnabled`: GetJumpEnabled.
pub fn jump_enabled(w: &World, f: FighterId) -> bool {
    w.fighter(f).moves.input_ov.as_ref().is_none_or(|s| s.jd == 0)
}

/// `evadeEnabled`: GetEvadeEnabled.
pub fn evade_enabled(w: &World, f: FighterId) -> bool {
    w.fighter(f).moves.input_ov.as_ref().is_none_or(|s| s.ed == 0)
}

/// `attackEnabled`: GetAttackEnabled.
pub fn attack_enabled(w: &World, f: FighterId) -> bool {
    w.fighter(f).moves.input_ov.as_ref().is_none_or(|s| s.ad == 0)
}

/// BPI_DisableInputType(type) (BaseFighter @110136): DisabledInputTypes[type] = 1023, every direction.
pub fn disable_input_type(w: &mut World, f: FighterId, t: u8) {
    w.fighter_mut(f).tutorial.disabled_input_types[usize::from(t)] = 1023;
}

/// BPI_EnableInputTypeDirection(type, dir) (@109268-110076): with an entry, its bit Round(2 ** dir) cleared; the entry
/// removed once no bit is left. No entry: nothing.
pub fn enable_input_type_direction(w: &mut World, f: FighterId, t: u8, dir: i32) {
    let m = &mut w.fighter_mut(f).tutorial.disabled_input_types[usize::from(t)];
    if *m != 0 {
        *m &= (1 << dir) ^ 1023;
    }
}

/// GetInputTypeDirectionEnabled(type, dir) (BaseFighter function @0-420): with an entry, whether its bit of `dir` is
/// clear; true without one.
pub fn input_type_direction_enabled(w: &World, f: FighterId, t: u8, dir: i32) -> bool {
    let m = w.fighter(f).tutorial.disabled_input_types[usize::from(t)];
    m == 0 || m & (1 << dir) == 0
}

/// Whether DisabledInputTypes has an entry for `t` (the readers compute the direction only then: the answer is true
/// otherwise).
pub fn input_type_disabled_any(w: &World, f: FighterId, t: u8) -> bool {
    w.fighter(f).tutorial.disabled_input_types[usize::from(t)] != 0
}

/// BPI_DisableMovement (@100781): MoveDisabledCounter + 1.
pub fn disable_movement_input(w: &mut World, f: FighterId) {
    w.fighter_mut(f).tutorial.move_disabled += 1;
}

/// GetMovementEnabled: MoveDisabledCounter == 0. Its readers: the in-air wall slide test (@79428, `wall_slide_check`),
/// OnLanded's dash (@111246, `land_loco`), DownPressed (@117331, `down_pressed`), WallSlideOnServer (@126571, behind
/// `wall_slide_check`), HandleMovementInput (@139577, `pawn_move_input`) and the new movement system's
/// GetMovementImpulsesForCurrentFrameCmd (@178152, off in this build).
pub fn movement_enabled(w: &World, f: FighterId) -> bool {
    w.fighter(f).tutorial.move_disabled == 0
}

/// BPI_SetMoveAxisY(v) (@99205): MoveYAxisValue = v, no MoveY event. It goes to the pawn's controller, whose frame
/// input is the pawn's axis pair for every reader (the batarang's steer, DI, crouch, fast fall): a demo controller's
/// pad or a bot's pad keeps it until written again. A player's pawn: nothing (the player's axis event writes the
/// value every frame).
/// Unverified: as the other calls on a held pawn, the value reaches the pawn in its next frame.
pub fn set_move_axis_y(w: &mut World, f: FighterId, v: Real) {
    match w.pawn_ctl.ctl.get_mut(&f) {
        Some(crate::world::pawns::PawnCtl::Bot(b)) => b.bot.pad.set_axis_y(v),
        Some(crate::world::pawns::PawnCtl::Demo(_)) => crate::modes::tutorial::demo::set_move_axis_y(w, f, v),
        _ => {}
    }
}

/// `setCanWalkOffLedges`: `CharacterMovement.bCanWalkOffLedges = v`.
pub fn set_can_walk_off_ledges(w: &mut World, f: FighterId, v: bool) {
    w.fighter_mut(f).platforms.no_walk_off_ledges = !v;
}

/// `canWalkOffLedges`: CanWalkOffLedges (CMC +0x838, stock): bCanWalkOffLedges.
pub fn can_walk_off_ledges(w: &World, f: FighterId) -> bool {
    !w.fighter(f).platforms.no_walk_off_ledges
}

/// `inputOverrideReset`: a new pawn has no override and the template's
/// bCanWalkOffLedges (true).
pub fn input_override_reset(w: &mut World, f: FighterId) {
    let fi = w.fighter_mut(f);
    fi.moves.input_ov = None;
    fi.platforms.no_walk_off_ledges = false;
}

// ---- fly

/// `FlyState`: Comp_Pawn_Fly's variables and the CharacterMovement values it writes.
#[derive(State, Debug, Default)]
pub struct FlyState {
    /// Is Flying.
    pub is_flying: bool,
    /// PreviousFlySpeed.
    pub prev_fly_speed: Real,
    /// Facing Left: BPI_IsFacingLeft at the last change of Is Flying (@1078).
    pub facing_left: bool,
    /// Last Direction, sim x.
    pub last_x: Real,
    /// Last Direction, sim y.
    pub last_y: Real,
    /// CharacterMovement.MaxFlySpeed.
    pub max_fly_speed: Real,
    /// CharacterMovement.BrakingDecelerationFlying.
    pub braking_decel_flying: Real,
}

/// The component class GetComponentByClass looks for.
const FLY_CLASS: &str = "Comp_Pawn_Fly_C";

/// `flyProps`: the pawn's Comp_Pawn_Fly_C template values, else none.
pub fn fly_props(w: &World, f: FighterId) -> Option<FlyProps> {
    let ch = w.names.str(w.fighter(f).ch);
    let d = &w.data.chars[ch].fighter;
    let c = d.components.pawn.values().find(|c| c.ty == FLY_CLASS)?;
    let q = c.p.as_obj();
    let num = |k: &str| -> Real {
        crate::math::narrow(
            q.and_then(|q| q.get(k)).and_then(|v| v.as_f64()).unwrap_or_else(|| panic!("{ch}: {FLY_CLASS} has no {k}")),
        )
    };
    Some(FlyProps {
        max_accel: num("Flying Max Acceleration"),
        brake: num("Flying Brake Deceleration"),
        air_control: num("Flying Air Control"),
        horizontal: num("Horizontal Flying Max Speed"),
        upward: num("Upward Flying Max Speed"),
        downward: num("Downward Flying Max Speed"),
        hover_in_place: q.and_then(|q| q.get("Can Hover In Place")).and_then(|v| v.as_bool()) == Some(true),
    })
}

/// `hasFly`.
pub fn has_fly(w: &World, f: FighterId) -> bool {
    fly_props(w, f).is_some()
}

/// `isFlying`: Get Is Flying.
pub fn fly_is_flying(w: &World, f: FighterId) -> bool {
    w.fighter(f).moves.fly.as_ref().is_some_and(|s| s.is_flying)
}

/// `flyReset`.
pub fn fly_reset(w: &mut World, f: FighterId) {
    w.fighter_mut(f).moves.fly = None;
}

/// `setFlyingMode`: SetMovementMode(Flying) / SetMovementMode(Falling) from any mode.
pub fn set_flying_mode(w: &mut World, f: FighterId, on: bool) {
    let fi = w.fighter_mut(f);
    fi.moves.flying = on;
    fi.hit.kb_mode = false;
    set_grounded(w, f, false);
}

/// `setIsFlying`: the notifies' Set Is Flying (@893 -> @993 -> @969 -> Update Is Flying).
pub fn set_is_flying(w: &mut World, f: FighterId, on: bool) {
    if let Some(p) = fly_props(w, f) {
        update_is_flying(w, f, &p, on);
    }
}

/// `updateIsFlying`: Update Is Flying @0-1360, only on a change.
pub fn update_is_flying(w: &mut World, f: FighterId, p: &FlyProps, on: bool) {
    if on == fly_is_flying(w, f) {
        return;
    }
    let mfs = crate::fighter::body::def(w, f).max_fly_speed;
    let s = w
        .fighter_mut(f)
        .moves
        .fly
        .get_or_insert_with(|| FlyState { max_fly_speed: crate::math::narrow(mfs), ..Default::default() });
    s.is_flying = on; // @52
    s.max_fly_speed = if on { p.upward } else { 0.0 }; // @247
    s.prev_fly_speed = s.max_fly_speed; // @318
    s.braking_decel_flying = if on { p.brake } else { 0.0 }; // @440
    set_max_accel(w, f, if on { p.max_accel } else { MAX_ACCEL_AIR }); // @588
    update_air_control(w, f, if on { p.air_control } else { AIR_CONTROL }); // @736
    set_flying_mode(w, f, on); // @840
    let fi = w.fighter_mut(f);
    let left = fi.body.facing_dir < 0.0;
    let (vx, vy) = (fi.body.vx, fi.body.vy);
    let s = fi.moves.fly.as_mut().expect("made above");
    s.facing_left = left; // @1032-1078
    if on {
        s.last_x = vx; // Last Direction = GetVelocity @1312
        s.last_y = vy;
    }
}
