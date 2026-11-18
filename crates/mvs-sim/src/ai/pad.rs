//! A bot's pad: the fighter input calls a bot task makes (BPI_MoveX / MoveY, ...Pressed /
//! ...Released), turned into the InputState the sim reads. MoveX(+1) is toward world -Y, which is sim +x.
//!
//! The stick goes out as the task wrote it. Deviation: a press and release in one frame shows as a one-frame pulse;
//! a press while the button still reads down shows as a one-frame gap first.
//! DownPressed / DownReleased (BaseFighter, old movement system: DownPressedLogic + DownPressedServer) set
//! isPushingDown and run the fast-fall check; the stick (MoveY) stays at rest, so GetInputDirection is neutral.
//! MoveY itself reaches DownPressed through the tick's stick handler (BotFastFall).
//! A write of `x` / `y` is one BPI_MoveX / BPI_MoveY call: BaseFighter MoveX (@127097-127210) writes MoveXAxisValue
//! (the input override's value while one is on) and runs CheckLateralFriction at every call, an unchanged value
//! included, and MoveY (@128536-128555) writes MoveYAxisValue the same way. The frame's output says which were
//! called (InputState `call_x` / `call_y`).

use crate::Real;
use crate::input::{Dir, InputState, NO_INPUT, get_direction};

/// `Btn`: a button a bot presses.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Btn {
    Attack = 0,
    Special = 1,
    Jump = 2,
    Dodge = 3,
    Item = 4,
}

/// `BTNS`, in the order `out` walks them.
const BTNS: [Btn; 5] = [Btn::Attack, Btn::Special, Btn::Jump, Btn::Dodge, Btn::Item];

#[derive(Clone, Debug, Default)]
pub struct Pad {
    sx: Real,
    sy: Real,
    /// MoveX called since the last `out`.
    cx: bool,
    cy: bool,
    /// DownPressed .. DownReleased.
    pub down: bool,
    held: [bool; 5],
    edge: [bool; 5],
    last: [bool; 5],
    dir: Option<Dir>,
    /// The jump press is BPI_JumpPressedLocal.
    jump_local: bool,
    /// A forced press's direction byte (ForceAttackDirection / ForceSpecialDirection true), sent with the press.
    force: Option<u8>,
}

impl Pad {
    /// The stick's x as last written.
    pub fn x(&self) -> Real {
        self.sx
    }

    /// `pad.x = v`: one BPI_MoveX call.
    pub fn set_x(&mut self, v: Real) {
        self.sx = v;
        self.cx = true;
    }

    pub fn y(&self) -> Real {
        self.sy
    }

    /// BPI_SetMoveAxisY(v): MoveYAxisValue alone (no MoveY event).
    pub fn set_axis_y(&mut self, v: Real) {
        self.sy = v;
    }

    /// `pad.y = v`: one BPI_MoveY call.
    pub fn set_y(&mut self, v: Real) {
        self.sy = v;
        self.cy = true;
    }

    /// `press(b, dir, local)`. `dir`: the direction passed with NormalAttackPressed / SpecialAttackPressed /
    /// ItemButtonPressed (BaseFighter ubergraph 168937: ButtonPress(5, Direction), BFGetAttack(.., 5, Direction,
    /// ..), as the attack buttons); `local`: BPI_JumpPressedLocal instead of JumpPressed (no CancelInvuln).
    pub fn press(&mut self, b: Btn, dir: Option<Dir>, local: bool) {
        let i = b as usize;
        self.held[i] = true;
        self.edge[i] = true;
        if b == Btn::Jump {
            self.jump_local = local;
        }
        if matches!(b, Btn::Attack | Btn::Special | Btn::Item) {
            self.dir = dir;
        }
    }

    /// NormalAttackPressed / SpecialAttackPressed(Direction, Force true): the press with its direction byte forced
    /// (`InputState::force` and `force_dir`: ForcedPress, DoPreAttackRotation's locked turn). `byte` is the
    /// call's Direction as passed (2 / 3 read as Right / Left, `pre_attack_rotation`).
    pub fn press_forced(&mut self, b: Btn, byte: u8) {
        self.press(b, Some(crate::fighter::control::dir_of(i32::from(byte)).expect("a direction byte")), false);
        self.force = Some(byte);
    }

    pub fn release(&mut self, b: Btn) {
        self.held[b as usize] = false;
    }

    /// `isHeld(b)`.
    pub fn is_held(&self, b: Btn) -> bool {
        self.held[b as usize]
    }

    /// `inputDir(facing)`: GetInputDirection: GetDirection of the pawn's pair as set so far this frame (0..4,
    /// |Y| >= |X| is vertical).
    pub fn input_dir(&self, facing: Real) -> Dir {
        let (x, y) = self.stick();
        get_direction(x, y, facing)
    }

    /// The floats a task wrote, as a float32 pair (the pawn's MoveXAxisValue / MoveYAxisValue): no dead zone and no
    /// rounding, a bot's MoveX goes to the pawn and not through the controller's axis event. (`+ 0.0`: no negative
    /// zero.)
    fn stick(&self) -> (Real, Real) {
        (self.sx + 0.0, self.sy + 0.0)
    }

    /// (the port's own: a Lab reset or the dev pause, no MoveX / MoveY call)
    /// (`jump_local` is left as it was: the next jump press writes it)
    pub fn reset(&mut self) {
        *self = Pad { jump_local: self.jump_local, ..Pad::default() };
    }

    /// `out()`: the frame's input; the press edges and the call flags are used up.
    pub fn out(&mut self) -> InputState {
        let (x, y) = self.stick();
        let mut o = InputState { x, y, down_btn: self.down, ..NO_INPUT };
        o.call_x = self.cx;
        o.call_y = self.cy;
        self.cx = false;
        self.cy = false;
        let mut rise = false;
        for b in BTNS {
            let i = b as usize;
            let mut v = self.held[i];
            if self.edge[i] {
                if self.last[i] {
                    v = false;
                } else {
                    v = true;
                    self.edge[i] = false;
                }
            }
            if v && !self.last[i] && matches!(b, Btn::Attack | Btn::Special | Btn::Item) {
                rise = true;
            }
            if v && !self.last[i] && b == Btn::Jump && self.jump_local {
                o.jump_local = true;
            }
            self.last[i] = v;
            match b {
                Btn::Attack => o.attack = v,
                Btn::Special => o.special = v,
                Btn::Jump => o.jump = v,
                Btn::Dodge => o.dodge = v,
                Btn::Item => o.item = v,
            }
        }
        if rise && let Some(d) = self.dir {
            o.atk_dir = Some(d);
        }
        if rise && let Some(byte) = self.force.take() {
            o.force = true;
            o.force_dir = Some(byte);
        }
        o
    }
}
