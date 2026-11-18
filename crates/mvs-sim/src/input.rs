//! One fighter's input on one tick (`InputState`), in the form `canonInput`
//! gives it: the sticks rounded to float32, every flag a plain bool, the
//! values a tick may lack an `Option`. It is `Copy`: a tick's inputs are a slice the step reads.

use crate::Real;
use crate::State;
use crate::names::Name;

/// EActionDirection, the attack tables' RequiredDirectionInput (`Dir`).
#[derive(State, Copy, PartialEq, Eq, Debug, Default)]
#[repr(u8)]
pub enum Dir {
    Up = 0,
    Down = 1,
    Fwd = 2,
    Back = 3,
    #[default]
    Neutral = 4,
    Default = 5,
    FwdUp = 6,
    FwdDown = 7,
    BackUp = 8,
    BackDown = 9,
}

/// A button of `InputState.order` (attack, special, jump, dodge or item), and a `Press`.
#[derive(State, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Btn {
    Attack,
    Special,
    Jump,
    Dodge,
    Item,
}

/// `forcePress` / `forceRelease` / a charge's button: `'attack' | 'special'`.
#[derive(State, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum AttackBtn {
    Attack,
    Special,
}

/// This tick's presses in key event order (`order`, at most five: `canonInput` keeps `slice(0, 5)`).
#[derive(State, Copy, PartialEq, Eq, Debug, Default)]
pub struct Order {
    pub len: u8,
    pub btns: [Option<Btn>; 5],
}

impl Order {
    pub fn iter(&self) -> impl Iterator<Item = Btn> + '_ {
        self.btns[..self.len as usize].iter().map(|b| b.expect("a button below len"))
    }
}

/// `InputState`.
#[derive(State, Copy, PartialEq, Debug, Default)]
pub struct InputState {
    /// MoveXAxisValue (float32): below 0 left, above 0 right.
    pub x: Real,
    /// MoveYAxisValue (float32): below 0 down, above 0 up.
    pub y: Real,
    pub attack: bool,
    pub special: bool,
    pub jump: bool,
    pub dodge: bool,
    /// This tick's jump press is BPI_JumpPressedLocal (no CancelInvuln).
    pub jump_local: bool,
    /// "Pickup Item" action.
    pub item: bool,
    /// This tick's presses in key event order; `None` where no list was sent (an empty list is not `None`).
    pub order: Option<Order>,
    /// DownPressed .. DownReleased called directly (a bot's crouch tasks).
    pub down_btn: bool,
    /// The direction passed with an attack or item press.
    pub atk_dir: Option<Dir>,
    /// The absolute InputDirection byte of a directional action's press (0 up, 1 down, 2 right, 3 left, 4 neutral).
    pub force_dir: Option<u8>,
    pub force_press: Option<AttackBtn>,
    pub force_release: Option<AttackBtn>,
    pub force: bool,
    /// Neutral Evade action pressed this tick: EvadeButtonPressed(4).
    pub neutral_evade: bool,
    /// TauntN action pressed this tick: the slot, 0..3.
    pub taunt: Option<u8>,
    /// A taunt action is held (BaseFighter ButtonStates[4]).
    pub taunt_held: bool,
    /// StickAttackX (float32).
    pub rsx: Option<Real>,
    /// StickAttackY (float32), up positive.
    pub rsy: Option<Real>,
    /// The stick comes from the gamepad axis (PFGPlayerController GamePadInputEnabled).
    pub axis: bool,
    /// A bot task called the pawn's MoveX / MoveY this tick.
    pub call_x: bool,
    pub call_y: bool,
}

/// `NO_INPUT`.
pub const NO_INPUT: InputState = InputState {
    x: 0.0,
    y: 0.0,
    attack: false,
    special: false,
    jump: false,
    dodge: false,
    jump_local: false,
    item: false,
    order: None,
    down_btn: false,
    atk_dir: None,
    force_dir: None,
    force_press: None,
    force_release: None,
    force: false,
    neutral_evade: false,
    taunt: None,
    taunt_held: false,
    rsx: None,
    rsy: None,
    axis: false,
    call_x: false,
    call_y: false,
};

/// `canonInput`: the sticks to float32 (already float32) and the rest of what `canonInput` does (`!!` of each flag,
/// `order.slice(0, 5)`): what the Rust type already holds.
pub fn canon_input(i: &InputState) -> InputState {
    *i
}

// ---- the input's own functions

/// `inputState`: the sim's copy of a tick's input. The Rust type holds every field already, so it is the input.
pub fn input_state(i: InputState) -> InputState {
    i
}

/// BaseFighter.GetDirection(X, Y) @0-582, the direction of an Attack / Special / Item press and of
/// GetInputDirection: both 0 -> Neutral; |Y| >= |X| -> Up / Down (a tie is vertical); else Forward / Back by the
/// actor's yaw against X (@344-537). Four ways and Neutral, never a diagonal, and no magnitude threshold: any
/// value that is not 0 is a direction. `facing` is the fighter's, +1 right.
pub fn get_direction(x: Real, y: Real, facing: Real) -> Dir {
    if x == 0.0 && y == 0.0 {
        return Dir::Neutral;
    }
    if y.abs() >= x.abs() {
        return if y > 0.0 { Dir::Up } else { Dir::Down };
    }
    if (x > 0.0) == (facing > 0.0) { Dir::Fwd } else { Dir::Back }
}

/// Round(Atan2(MoveY, MoveX) x 4 / PI), + 8, % 8: the 45 degree sector of the stick, 0 at +X (the screen's right)
/// and counted towards up. The Atan2 node is FMath::Atan2 (1412c8e50), the Round node RoundToInt (142f817a0).
fn sector(x: Real, y: Real) -> usize {
    let s = crate::math::ue::round_to_int((crate::math::ue::atan2(y, x) * 4.0) / std::f32::consts::PI) + 8;
    // atan2 is within [-PI, PI]: the round is within [-4, 4], so `s` is in [4, 12].
    // Unverified: a NaN stick (no input the sim makes has one; a malformed wire input could) gives sector 0 here,
    // where no direction would be right.
    usize::try_from(s).unwrap_or(0) % 8
}

const WAY_RIGHT: [Dir; 8] =
    [Dir::Fwd, Dir::FwdUp, Dir::Up, Dir::BackUp, Dir::Back, Dir::BackDown, Dir::Down, Dir::FwdDown];
const WAY_LEFT: [Dir; 8] =
    [Dir::Back, Dir::BackUp, Dir::Up, Dir::FwdUp, Dir::Fwd, Dir::FwdDown, Dir::Down, Dir::BackDown];

/// BaseFighter.Get8WayDirection @0-1246 (the evade's direction, automatic branches, AnimNotify_InputSectionJump):
/// both exactly 0 -> Neutral, else the sector against FacingLeft. No magnitude threshold.
pub fn get8_way(x: Real, y: Real, facing: Real) -> Dir {
    if x == 0.0 && y == 0.0 {
        return Dir::Neutral;
    }
    (if facing < 0.0 { WAY_LEFT } else { WAY_RIGHT })[sector(x, y)]
}

/// BaseFighter.Get8WayDirectionNoFacing @0-1156: Neutral while both axes are within +-0.3, else the sector with +X
/// as Forward.
pub fn get8_way_no_facing(x: Real, y: Real) -> Dir {
    if (-0.3..=0.3).contains(&x) && (-0.3..=0.3).contains(&y) {
        return Dir::Neutral;
    }
    WAY_RIGHT[sector(x, y)]
}

/// The EActionDirection names the branch rows carry. A diagonal has a name of its own, which no row of the data
/// has: Comp_Pawn_BranchManager compares the bytes exactly (IsBranchValid @743-877, @1195), so a diagonal answers
/// only a row of any direction. `Default` has none: `AD_Neutral`.
pub fn dir_name(d: Dir) -> Name {
    match d {
        Dir::Up => crate::name!("AD_Up"),
        Dir::Down => crate::name!("AD_Down"),
        Dir::Fwd => crate::name!("AD_Forward"),
        Dir::Back => crate::name!("AD_Back"),
        Dir::Neutral | Dir::Default => crate::name!("AD_Neutral"),
        Dir::FwdUp => crate::name!("AD_ForwardUp"),
        Dir::FwdDown => crate::name!("AD_ForwardDown"),
        Dir::BackUp => crate::name!("AD_BackUp"),
        Dir::BackDown => crate::name!("AD_BackDown"),
    }
}

/// Button values (RequiredButtonInput, Enum_InputButton: 0 NormalAttack, 1 SpecialAttack, 2 Jump, 3 Evade, 4 Taunt,
/// 5 Item, 6 None; the index of BaseFighter.ButtonStates).
pub const BTN_NORMAL: u8 = 0;
pub const BTN_SPECIAL: u8 = 1;
pub const BTN_EVADE: u8 = 3;

/// The EActionButton name of a button value (`BUTTON_NAMES`).
pub fn button_name(b: u8) -> Option<&'static str> {
    match b {
        BTN_NORMAL => Some("AB_HeavyAttack"),
        BTN_SPECIAL => Some("AB_SpecialAttack"),
        _ => None,
    }
}

// ---- the wire form of one input

/// One input on the wire (`INPUT_BYTES`; the last 4 bytes, after the first 18, hold `rsy`).
pub const INPUT_BYTES: usize = 22;

const BTNS: [Btn; 5] = [Btn::Attack, Btn::Special, Btn::Jump, Btn::Dodge, Btn::Item];

/// `held`: an input held over a tick that has none of its own, without its one-tick parts (a bot's MoveX / MoveY
/// calls are the tick's own events, so they go too).
pub fn held(i: &InputState) -> InputState {
    let one_tick = i.taunt.is_some()
        || i.order.is_some()
        || i.force_press.is_some()
        || i.force_release.is_some()
        || i.call_x
        || i.call_y;
    if !one_tick {
        return *i;
    }
    InputState { taunt: None, order: None, force_press: None, force_release: None, call_x: false, call_y: false, ..*i }
}

fn press_index(p: Option<AttackBtn>) -> u8 {
    match p {
        None => 0,
        Some(AttackBtn::Attack) => 1,
        Some(AttackBtn::Special) => 2,
    }
}

fn press_of(v: u8) -> Option<AttackBtn> {
    match v {
        0 => None,
        1 => Some(AttackBtn::Attack),
        _ => Some(AttackBtn::Special),
    }
}

/// `encodeInput`: `i` into `out[..INPUT_BYTES]`, little-endian.
pub fn encode_input(i: &InputState, out: &mut [u8]) {
    let b = |v: bool, s: u8| u8::from(v) << s;
    let o = b;
    out[0] = o(i.call_x, 0)
        | o(i.call_y, 1)
        | o(i.taunt_held, 2)
        | b(i.rsy.is_some(), 3)
        | b(i.attack, 4)
        | b(i.special, 5)
        | b(i.jump, 6)
        | b(i.dodge, 7);
    out[1] = o(i.jump_local, 0)
        | o(i.item, 1)
        | o(i.down_btn, 2)
        | o(i.force, 3)
        | o(i.neutral_evade, 4)
        | o(i.axis, 5)
        | (press_index(i.force_press) << 6);
    let len = i.order.map_or(0, |o| o.len.min(5));
    out[2] = press_index(i.force_release) | (i.taunt.map_or(0, |t| t + 1) << 2) | (len << 5);
    let mut ob: u16 = if i.order.is_some() { 0x8000 } else { 0 };
    if let Some(order) = &i.order {
        for (n, k) in order.iter().take(5).enumerate() {
            let at = BTNS.iter().position(|&x| x == k).expect("a button of BTNS");
            ob |= u16::try_from(at).expect("a button index") << (3 * n);
        }
    }
    out[3..5].copy_from_slice(&ob.to_le_bytes());
    out[5] = i.atk_dir.map_or(0, |d| d as u8 + 1) | (i.force_dir.map_or(0, |d| d + 1) << 4) | b(i.rsx.is_some(), 7);
    // The sticks are float32 on the wire (DataView setFloat32 rounds to nearest, as `as f32`).
    #[allow(clippy::cast_possible_truncation)]
    let f = |v: Real| v.to_le_bytes();
    out[6..10].copy_from_slice(&f(i.x));
    out[10..14].copy_from_slice(&f(i.y));
    out[14..18].copy_from_slice(&f(i.rsx.unwrap_or(0.0)));
    out[18..22].copy_from_slice(&f(i.rsy.unwrap_or(0.0)));
}

const DIRS: [Dir; 10] = [
    Dir::Up,
    Dir::Down,
    Dir::Fwd,
    Dir::Back,
    Dir::Neutral,
    Dir::Default,
    Dir::FwdUp,
    Dir::FwdDown,
    Dir::BackUp,
    Dir::BackDown,
];

/// `decodeInput`: the input `encodeInput` wrote at `d[..INPUT_BYTES]`; equal to `canonInput` of what was encoded.
/// Deviation: an `atkDir` byte past the enum's values (a malformed packet: 11..15) reads as none; the number is not
/// kept.
pub fn decode_input(d: &[u8]) -> InputState {
    let (b0, b1, b2, b5) = (d[0], d[1], d[2], d[5]);
    let o = u16::from_le_bytes([d[3], d[4]]);
    let bit = |v: u8, s: u8| (v >> s) & 1 == 1;
    let f = |at: usize| f64::from(f32::from_le_bytes([d[at], d[at + 1], d[at + 2], d[at + 3]]));
    let mut order = Order::default();
    for n in 0..(b2 >> 5).min(5) {
        let k = usize::from((o >> (3 * u16::from(n))) & 7).min(4);
        order.btns[usize::from(n)] = Some(BTNS[k]);
        order.len = n + 1;
    }
    InputState {
        x: crate::math::narrow(f(6)),
        y: crate::math::narrow(f(10)),
        attack: bit(b0, 4),
        special: bit(b0, 5),
        jump: bit(b0, 6),
        dodge: bit(b0, 7),
        jump_local: bit(b1, 0),
        item: bit(b1, 1),
        order: if o & 0x8000 != 0 { Some(order) } else { None },
        down_btn: bit(b1, 2),
        atk_dir: if b5 & 15 != 0 { DIRS.get(usize::from((b5 & 15) - 1)).copied() } else { None },
        force_dir: if (b5 >> 4) & 7 != 0 { Some(((b5 >> 4) & 7) - 1) } else { None },
        force_press: press_of((b1 >> 6).min(2)),
        force_release: press_of((b2 & 3).min(2)),
        force: bit(b1, 3),
        neutral_evade: bit(b1, 4),
        taunt: if (b2 >> 2) & 7 != 0 { Some(((b2 >> 2) & 7) - 1) } else { None },
        taunt_held: bit(b0, 2),
        rsx: if bit(b5, 7) { Some(crate::math::narrow(f(14))) } else { None },
        rsy: if bit(b0, 3) { Some(crate::math::narrow(f(18))) } else { None },
        axis: bit(b1, 5),
        call_x: bit(b0, 0),
        call_y: bit(b0, 1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directions() {
        assert_eq!(get_direction(0.0, 0.0, 1.0), Dir::Neutral);
        assert_eq!(get_direction(0.5, 0.5, 1.0), Dir::Up);
        assert_eq!(get_direction(-1.0, 0.2, 1.0), Dir::Back);
        assert_eq!(get_direction(-1.0, 0.2, -1.0), Dir::Fwd);
        assert_eq!(get8_way(1.0, 1.0, 1.0), Dir::FwdUp);
        assert_eq!(get8_way(1.0, 1.0, -1.0), Dir::BackUp);
        assert_eq!(get8_way(-1.0, 0.0, 1.0), Dir::Back);
        assert_eq!(get8_way(0.0, -1.0, 1.0), Dir::Down);
        assert_eq!(get8_way_no_facing(0.3, -0.3), Dir::Neutral);
        assert_eq!(get8_way_no_facing(-0.31, -0.31), Dir::BackDown);
    }

    #[test]
    fn wire_round_trip() {
        let mut order = Order { len: 2, ..Order::default() };
        order.btns[0] = Some(Btn::Dodge);
        order.btns[1] = Some(Btn::Attack);
        let i = InputState {
            x: 0.3,
            y: -1.0,
            attack: true,
            order: Some(order),
            atk_dir: Some(Dir::BackDown),
            force_dir: Some(4),
            force_press: Some(AttackBtn::Special),
            taunt: Some(3),
            taunt_held: true,
            rsx: Some(-0.7),
            rsy: Some(0.4),
            call_y: true,
            ..NO_INPUT
        };
        let mut b = [0u8; INPUT_BYTES];
        encode_input(&i, &mut b);
        assert_eq!(decode_input(&b), canon_input(&i));
        encode_input(&NO_INPUT, &mut b);
        assert_eq!(decode_input(&b), canon_input(&NO_INPUT));
    }
}
