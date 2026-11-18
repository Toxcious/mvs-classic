//! The tutorial's demo controllers: Blueprint AIControllers (TutorialController_*) that
//! drive the step's player pawn behind the concept card from their own Delays and looping timers, by calling the
//! pawn's input events (`DemoPad`). A controller torn down with its pawn (TutorialActor ControlTutorialPawn /
//! UncontrolTutorialPawn: SetLifeSpan(0) destroys nothing, AActor::SetLifeSpan 14280b290) lives on with Pawn = None:
//! its timers and Delays keep firing, and each of its calls on a pawn reaches None (K2_GetPawn) or the destroyed pawn
//! it stored (pending kill: skipped by ProcessContextOpcode 1415bf640), so they do nothing. The port keeps its entries
//! running the same way.
//!
//! Each controller is a [`Demo`] in `TutorialRun::demos`, possessing its pawn with a `PawnCtl::Demo` whose `id` is the
//! demo's; its own state is its class's [`DemoState`] variant. The beginner tutorial's classes are ported here, the
//! advanced ones in `advanced/*`. What a closure would capture is a field of the class's state.

use crate::Real;
use crate::State;
use crate::actors::ActorRef;
use crate::fighter::FighterId;
use crate::fighter::common::{actor_is_a, lookup_owner, warn_text};
use crate::input::{AttackBtn, Btn, Dir, InputState, NO_INPUT, Order};
use crate::name;
use crate::world::World;
use crate::world::pawns::{DemoCtl, PawnCtl};

use super::TutorialObj;
use super::advanced::{batman, elements, fighters, geometry, meeseeks};
use super::classes::{Call, Fired, demo_mut, demo_of};
use super::latent::{self, MoveTarget};

/// `DemoPad`: the pawn's input events a demo controller calls, gathered into the pawn's input of its next frame
/// (`input`). A press sets the button held (BaseFighter ButtonPress: ButtonStates[button] = true); no beginner
/// controller calls a release, and nothing native releases a button (a held demo press stays held for the pawn's
/// life), so the charge runs to its maximum and a jump is a full one. The advanced ones call NormalAttackReleased,
/// SpecialAttackReleased, JumpReleased / BPI_JumpReleasedLocal and DownPressed / DownReleased (`release`,
/// `down_pressed`, `down_released`).
/// Unverified: the calls a controller makes in the timer or latent pass reach the pawn in its next frame, as a bot's
/// think does.
/// Deviation: a press of a button already held is a press in the build (the event runs whatever ButtonStates says).
/// An attack or special press is one in the port too (force_press); a jump or evade press of a held button reads
/// the button up for that frame and pressed in the next, one frame late.
/// Deviation: BPI_SetMoveAxisX writes MoveXAxisValue without the MoveX event (BaseFighter @99177); the port's input
/// takes a changed x for a MoveX event. Within one call chain the value the frame's
/// press saw is the frame's x (Evade's SetMoveAxisX(1), EvadeButtonPressed, SetMoveAxisX(0) in one tick dodges
/// right), the value written after it is the next frame's.
/// The class fields, in their order; a button list holds Attack, Special, Jump or Dodge.
#[derive(State, Debug, Default)]
pub struct DemoPad {
    /// `held`: attack, special, jump, dodge.
    pub held: [bool; 4],
    pub x: Real,
    /// MoveYAxisValue (BPI_SetMoveAxisY; no Intro class writes it).
    pub y: Real,
    pub press_x: Option<Real>,
    pub call_x: bool,
    pub gap: Vec<Btn>,
    pub order: Vec<Btn>,
    pub force_press: Option<AttackBtn>,
    pub atk_dir: Option<Dir>,
    pub down: bool,
    pub release_next: Vec<Btn>,
}

fn held_ix(b: Btn) -> usize {
    match b {
        Btn::Attack => 0,
        Btn::Special => 1,
        Btn::Jump => 2,
        Btn::Dodge => 3,
        Btn::Item => panic!("DemoPad: no item button"),
    }
}

impl DemoPad {
    /// NormalAttackPressed(dir, false) / SpecialAttackPressed(dir, false) / JumpPressed / EvadeButtonPressed(5).
    pub fn press(&mut self, b: Btn, dir: Option<Dir>) {
        if self.press_x.is_none() {
            self.press_x = Some(self.x);
        }
        let i = held_ix(b);
        if self.held[i] {
            match b {
                Btn::Attack => self.force_press = Some(AttackBtn::Attack),
                Btn::Special => self.force_press = Some(AttackBtn::Special),
                _ => {
                    if !self.gap.contains(&b) {
                        self.gap.push(b);
                    }
                }
            }
        }
        self.held[i] = true;
        if dir.is_some() {
            self.atk_dir = dir;
        }
        if !self.order.contains(&b) {
            self.order.push(b);
        }
    }

    /// NormalAttackReleased / SpecialAttackReleased / JumpReleased (BPI_JumpReleasedLocal alike: the release has no
    /// CancelInvuln to skip): ButtonStates[button] = false.
    /// Deviation: a release in the frame of the button's press is the next frame's (the press reads for one frame),
    /// as the bot's pad turns a press and release in one frame into a one-frame pulse (`ai/pad.rs`).
    pub fn release(&mut self, b: Btn) {
        if self.order.contains(&b) {
            if !self.release_next.contains(&b) {
                self.release_next.push(b);
            }
        } else {
            self.held[held_ix(b)] = false;
        }
    }

    /// DownPressed called directly (InputState.down_btn: isPushingDown and the fast-fall check).
    pub fn down_pressed(&mut self) {
        self.down = true;
    }

    /// DownReleased called directly.
    pub fn down_released(&mut self) {
        self.down = false;
    }

    /// BPI_SetMoveAxisX(v): MoveXAxisValue alone.
    pub fn set_move_axis_x(&mut self, v: Real) {
        self.x = v;
    }

    /// BPI_SetMoveAxisY(v): MoveYAxisValue alone.
    pub fn set_move_axis_y(&mut self, v: Real) {
        self.y = v;
    }

    /// BPI_MoveX(v): the pawn's MoveX event.
    pub fn move_x(&mut self, v: Real) {
        self.x = v;
        self.call_x = true;
    }

    /// The frame's input; the frame's presses are spent.
    pub fn input(&mut self) -> InputState {
        let h = self.held;
        let gap = &self.gap;
        let mut i = InputState {
            x: self.press_x.unwrap_or(self.x),
            y: self.y,
            attack: h[0],
            special: h[1],
            jump: h[2] && !gap.contains(&Btn::Jump),
            dodge: h[3] && !gap.contains(&Btn::Dodge),
            ..NO_INPUT
        };
        if !self.order.is_empty() {
            let mut o = Order::default();
            for &b in self.order.iter().filter(|b| !gap.contains(b)).take(5) {
                o.btns[usize::from(o.len)] = Some(b);
                o.len += 1;
            }
            i.order = Some(o);
        }
        if self.atk_dir.is_some() {
            i.atk_dir = self.atk_dir;
        }
        if self.force_press.is_some() {
            i.force_press = self.force_press;
        }
        if self.call_x {
            i.call_x = true;
        }
        if self.down {
            i.down_btn = true;
        }
        for b in std::mem::take(&mut self.release_next) {
            self.held[held_ix(b)] = false;
        }
        self.press_x = None;
        self.call_x = false;
        self.gap.clear();
        self.order.clear();
        self.force_press = None;
        self.atk_dir = None;
        i
    }
}

/// A demo controller (`Demo`): its id (`TutorialObj::Demo`), the pawn it last possessed (its ReceiveUnPossess finds
/// the controller by it; K2_GetPawn is [`get_pawn`]), its pad and its class's state.
#[derive(State, Debug)]
pub struct Demo {
    pub id: u32,
    /// The controller class (TutorialController_Evade ...).
    pub cls: &'static str,
    pub pawn: Option<FighterId>,
    pub pad: DemoPad,
    pub state: DemoState,
}

/// A demo controller class's own state, a variant per class (`demo.rs`, `advanced/*/demos.rs`); the payload is
/// the class's fields, defined by the track that ports it.
#[derive(State, Debug)]
pub enum DemoState {
    // the beginner tutorial's
    /// TutorialController_AerialAttack (evade false), TutorialController_AerialEvade (true).
    Aerial(Aerial),
    Heavy(Heavy),
    ShaggyKick(ShaggyKick),
    /// TutorialController_Special (Neutral), TutorialController_ShaggySandwich (Down).
    Special(Special),
    Evade(Evade),
    ChargeUp(ChargeUp),
    /// TutorialController_Combat (0), _Damage (200), _IntroFinale (100).
    Combat(Combat),
    /// TutorialController_AttackDecay_Basics.
    AttackDecay(AttackDecay),
    // advanced/fighters/demos.rs
    DodgeBasics(fighters::DodgeBasicsDemo),
    DodgeMeterDeplete(fighters::DodgeMeterDeplete),
    DodgeMeterReplenish(fighters::DodgeMeterReplenish),
    CooldownsBasics(fighters::CooldownsBasics),
    ArmorBasics(fighters::ArmorBasics),
    ArmorBreak(fighters::ArmorBreakDemo),
    DodgeJump(fighters::DodgeJump),
    AttackDecayVariety(fighters::AttackDecayVariety),
    // advanced/geometry/demos.rs
    WallSlide(geometry::WallSlide),
    FastFall(geometry::FastFall),
    FastFallPlatform(geometry::FastFallPlatform),
    KbiSide(geometry::KbiSideDemo),
    KbiUp(geometry::KbiUpDemo),
    // advanced/meeseeks/demos.rs
    AerialCombatBasics(meeseeks::AerialCombatBasics),
    AerialCombatHighUp(meeseeks::AerialCombatHighUp),
    EvadeToAttack(meeseeks::EvadeToAttack),
    // advanced/elements/{downspike,fire}.rs
    KillBotOffPlatform(elements::KillBotOffPlatform),
    ExtinguishFire(elements::ExtinguishFireDemo),
    Ignite(elements::IgniteDemo),
    // advanced/batman.rs (no class of the family)
    BatmanBatarang(batman::BatarangDemo),
    BatmanRetrieve(batman::RetrieveDemo),
    BatmanDestroy(batman::DestroyDemo),
}

// ---- what every demo class shares

/// `Place`: a pawn's actor place (`placeOf`: x, actorY).
#[derive(State, Copy, PartialEq, Debug)]
pub struct Place {
    pub x: Real,
    pub y: Real,
}

/// `placeOf`.
pub fn place_of(w: &World, f: FighterId) -> Place {
    Place { x: w.fighter(f).body.x, y: crate::fighter::actor_y(w, f) }
}

/// `callable`: a pawn a graph can still call: in the match and not destroyed.
pub fn callable(w: &World, f: Option<FighterId>) -> bool {
    f.is_some_and(|f| w.fighter(f).life.dead != Real::INFINITY && w.pawns.as_ref().is_some_and(|ps| ps.contains(&f)))
}

/// `active`: the graphs' "active" gate: a BaseFighter with ActiveInTutorialIntro.
pub fn active(w: &World, f: Option<FighterId>) -> bool {
    callable(w, f) && f.is_some_and(|f| w.fighter(f).tutorial.active_in_tutorial_intro)
}

/// `teleport`: K2_TeleportTo(location, the actor's rotation).
pub fn teleport(w: &mut World, f: Option<FighterId>, at: Option<Place>) {
    if let (true, Some(f), Some(at)) = (callable(w, f), f, at) {
        crate::world::spawn_spot::fighter_teleport_to(w, f, at.x, at.y);
    }
}

/// K2_GetPawn of the demo controller `id`: the pawn it possesses now.
pub fn get_pawn(w: &World, id: u32) -> Option<FighterId> {
    w.pawn_ctl.ctl.iter().find(|(_, c)| matches!(c, PawnCtl::Demo(d) if d.id == id)).map(|(&f, _)| f)
}

/// BPI_SetMoveAxisY on a pawn a demo controller holds: its pad's y.
pub fn set_move_axis_y(w: &mut World, f: FighterId, v: Real) {
    let id = held_by(w, f);
    demo_mut(w, id).pad.set_move_axis_y(v);
}

/// The controller a pawn is possessed with (`Demo.ctl`).
pub fn ctl(id: u32) -> PawnCtl {
    PawnCtl::Demo(DemoCtl { id, input: ctl_input, possessed: Some(ctl_possessed), unpossessed: Some(ctl_unpossessed) })
}

fn held_by(w: &World, f: FighterId) -> u32 {
    match w.pawn_ctl.ctl.get(&f) {
        Some(PawnCtl::Demo(d)) => d.id,
        _ => panic!("tutorial: the pawn is held by no demo controller"),
    }
}

/// `ctl.input`: the pad's frame.
fn ctl_input(w: &mut World, f: FighterId) -> InputState {
    let id = held_by(w, f);
    demo_mut(w, id).pad.input()
}

/// `ctl.possessed`: ReceivePossess.
fn ctl_possessed(w: &mut World, f: FighterId) {
    let id = held_by(w, f);
    demo_mut(w, id).pawn = Some(f);
    super::classes::call(w, TutorialObj::Demo(id), Call::Possessed(f));
}

/// `ctl.unpossessed`: ReceiveUnPossess (the controller's entry is gone: found by the pawn it last possessed).
fn ctl_unpossessed(w: &mut World, f: FighterId, destroyed: bool) {
    let run = w.tutorial.as_ref().expect("a tutorial World");
    let Some(id) = run.demos.iter().rev().find(|d| d.pawn == Some(f)).map(|d| d.id) else { return };
    super::classes::call(w, TutorialObj::Demo(id), Call::Unpossessed { pawn: f, destroyed });
}

// ---- the beginner tutorial's classes

/// `BEGINNER_DEMOS`: the beginner tutorial's demo controller classes (`classes.rs` merges them with the advanced
/// tutorials'). Not ported: TutorialController_AttackDecay_Comparison (the Intro never runs it).
pub fn make_demo(cls: &str) -> Option<(&'static str, DemoState)> {
    use DemoState as D;
    Some(match cls {
        "TutorialController_AerialAttack" => ("TutorialController_AerialAttack", D::Aerial(Aerial::new(false))),
        "TutorialController_AerialEvade" => ("TutorialController_AerialEvade", D::Aerial(Aerial::new(true))),
        "TutorialController_Heavy" => ("TutorialController_Heavy", D::Heavy(Heavy::default())),
        "TutorialController_ShaggyKick" => ("TutorialController_ShaggyKick", D::ShaggyKick(ShaggyKick::default())),
        "TutorialController_Special" => ("TutorialController_Special", D::Special(Special::new(Dir::Neutral))),
        "TutorialController_ShaggySandwich" => {
            ("TutorialController_ShaggySandwich", D::Special(Special::new(Dir::Down)))
        }
        "TutorialController_Evade" => ("TutorialController_Evade", D::Evade(Evade::default())),
        "TutorialController_ChargeUp" => ("TutorialController_ChargeUp", D::ChargeUp(ChargeUp::default())),
        "TutorialController_Combat" => ("TutorialController_Combat", D::Combat(Combat::new(0.0))),
        "TutorialController_Damage" => ("TutorialController_Damage", D::Combat(Combat::new(200.0))),
        "TutorialController_IntroFinale" => ("TutorialController_IntroFinale", D::Combat(Combat::new(100.0))),
        "TutorialController_AttackDecay_Basics" => {
            ("TutorialController_AttackDecay_Basics", D::AttackDecay(AttackDecay::default()))
        }
        _ => return None,
    })
}

/// The state of the beginner class of demo `id`, by its variant.
macro_rules! st {
    ($w:expr, $id:expr, $v:ident) => {
        match &mut demo_mut($w, $id).state {
            DemoState::$v(s) => s,
            _ => unreachable!(),
        }
    };
}

fn pad(w: &mut World, id: u32) -> &mut DemoPad {
    &mut demo_mut(w, id).pad
}

fn delay(w: &mut World, id: u32, node: &'static str, s: Real) {
    latent::delay(w, TutorialObj::Demo(id), node, s);
}

fn set_timer(w: &mut World, id: u32, name: &'static str, s: Real) {
    latent::set_timer(w, TutorialObj::Demo(id), name, s, true);
}

fn clear_timer(w: &mut World, id: u32, name: &'static str) {
    latent::clear_timer(w, TutorialObj::Demo(id), name);
}

/// ReceivePossess's pawn for the Delay it starts: the first while that Delay is pending (a second call of a pending
/// Delay starts nothing, so its link runs with the first call's pawn).
fn first_possess(w: &World, id: u32, f: FighterId, cur: Option<FighterId>) -> Option<FighterId> {
    if latent::delay_pending(w, TutorialObj::Demo(id), "possess") { cur } else { Some(f) }
}

/// A call on a beginner demo controller.
pub fn call(w: &mut World, id: u32, c: Call<'_>) {
    use DemoState as D;
    let kind = match &demo_of(w, id).state {
        D::Aerial(_) => 0,
        D::Heavy(_) => 1,
        D::ShaggyKick(_) => 2,
        D::Special(_) => 3,
        D::Evade(_) => 4,
        D::ChargeUp(_) => 5,
        D::Combat(_) => 6,
        D::AttackDecay(_) => 7,
        _ => unreachable!("an advanced demo controller"),
    };
    match kind {
        0 => aerial(w, id, c),
        1 => heavy(w, id, c),
        2 => shaggy_kick(w, id, c),
        3 => special(w, id, c),
        4 => evade(w, id, c),
        5 => charge_up(w, id, c),
        6 => combat(w, id, c),
        _ => attack_decay(w, id, c),
    }
}

/// TutorialController_AerialAttack / _AerialEvade: ReceivePossess Delay 0.5, InitialLocation of the possessed pawn,
/// a looping 3.0 s timer to Attack Loop and one call now. Attack Loop: K2_GetPawn teleported back; active:
/// JumpPressed, Delay 0.2 (AerialEvade 0.22), then NormalAttackPressed(4 Neutral) (AerialEvade EvadeButtonPressed) on
/// the cast. ReceiveUnPossess clears the timer.
#[derive(State, Debug, Default)]
pub struct Aerial {
    pub evade: bool,
    pub at: Option<Place>,
    pub cast: Option<FighterId>,
    /// ReceivePossess's pawn (the Delay's capture).
    pub possessed: Option<FighterId>,
}

impl Aerial {
    fn new(evade: bool) -> Self {
        Aerial { evade, ..Aerial::default() }
    }
}

fn aerial(w: &mut World, id: u32, c: Call<'_>) {
    match c {
        Call::Possessed(f) => {
            let p = {
                let cur = st!(w, id, Aerial).possessed;
                first_possess(w, id, f, cur)
            };
            st!(w, id, Aerial).possessed = p;
            delay(w, id, "possess", 0.5);
        }
        Call::Fired(Fired::Delay("possess")) => {
            let f = st!(w, id, Aerial).possessed;
            if callable(w, f) {
                let at = place_of(w, f.expect("callable"));
                st!(w, id, Aerial).at = Some(at);
            }
            set_timer(w, id, "Attack Loop", 3.0);
            aerial_loop(w, id);
        }
        Call::Fired(Fired::Timer("Attack Loop")) => aerial_loop(w, id),
        Call::Fired(Fired::Delay("jump")) => {
            let s = st!(w, id, Aerial);
            let (cast, evade) = (s.cast, s.evade);
            if !callable(w, cast) {
                return;
            }
            if evade {
                pad(w, id).press(Btn::Dodge, None);
            } else {
                pad(w, id).press(Btn::Attack, Some(Dir::Neutral));
            }
        }
        Call::Unpossessed { .. } => clear_timer(w, id, "Attack Loop"),
        _ => unhandled(w, id, c),
    }
}

fn aerial_loop(w: &mut World, id: u32) {
    let p = get_pawn(w, id);
    let s = st!(w, id, Aerial);
    let (at, evade) = (s.at, s.evade);
    teleport(w, p, at);
    if !active(w, p) {
        return;
    }
    st!(w, id, Aerial).cast = p;
    pad(w, id).press(Btn::Jump, None);
    delay(w, id, "jump", if evade { 0.22 } else { 0.2 });
}

/// TutorialController_Heavy: ReceivePossess Delay 0.5, InitialLocation, a looping 1.0 s timer and one call now. The
/// loop: teleport back; active: NormalAttackPressed(SwappedAttackStyleGround(0) ? 4 Neutral : 2 Forward) (@677), the
/// player's attack-style preference. ReceiveUnPossess clears the timer.
#[derive(State, Debug, Default)]
pub struct Heavy {
    pub at: Option<Place>,
    pub possessed: Option<FighterId>,
}

fn heavy(w: &mut World, id: u32, c: Call<'_>) {
    match c {
        Call::Possessed(f) => {
            let p = {
                let cur = st!(w, id, Heavy).possessed;
                first_possess(w, id, f, cur)
            };
            st!(w, id, Heavy).possessed = p;
            delay(w, id, "possess", 0.5);
        }
        Call::Fired(Fired::Delay("possess")) => {
            let f = st!(w, id, Heavy).possessed;
            if callable(w, f) {
                let at = place_of(w, f.expect("callable"));
                st!(w, id, Heavy).at = Some(at);
            }
            set_timer(w, id, "Attack Loop", 1.0);
            heavy_loop(w, id);
        }
        Call::Fired(Fired::Timer("Attack Loop")) => heavy_loop(w, id),
        Call::Unpossessed { .. } => clear_timer(w, id, "Attack Loop"),
        _ => unhandled(w, id, c),
    }
}

fn heavy_loop(w: &mut World, id: u32) {
    let p = get_pawn(w, id);
    let at = st!(w, id, Heavy).at;
    teleport(w, p, at);
    if !active(w, p) {
        return;
    }
    let swapped = w.setup.players.first().is_some_and(|p| p.input.swapped_ground);
    pad(w, id).press(Btn::Attack, Some(if swapped { Dir::Neutral } else { Dir::Fwd }));
}

/// TutorialController_ShaggyKick: ReceivePossess stores DemoFighter, Delay 0.1, InitialLocation, a looping 3.5 s
/// timer to @168 and one call now (no pop after the K2_SetTimerDelegate @115): BPI_SetFacingLogic(IsLeft),
/// SpecialAttackPressed(2 Forward), IsLeft = !IsLeft; with IsLeft now false, Delay 0.85 then MoveComponentTo(
/// InitialLocation, 0.3 s, ease out, ease in) with nothing after. No active gate. ReceiveUnPossess clears the timer.
#[derive(State, Debug, Default)]
pub struct ShaggyKick {
    pub at: Option<Place>,
    pub fighter: Option<FighterId>,
    pub is_left: bool,
    pub possessed: Option<FighterId>,
}

fn shaggy_kick(w: &mut World, id: u32, c: Call<'_>) {
    match c {
        Call::Possessed(f) => {
            let p = {
                let cur = st!(w, id, ShaggyKick).possessed;
                first_possess(w, id, f, cur)
            };
            let s = st!(w, id, ShaggyKick);
            s.fighter = Some(f);
            s.possessed = p;
            delay(w, id, "possess", 0.1);
        }
        Call::Fired(Fired::Delay("possess")) => {
            let f = st!(w, id, ShaggyKick).possessed;
            if callable(w, f) {
                let at = place_of(w, f.expect("callable"));
                st!(w, id, ShaggyKick).at = Some(at);
            }
            set_timer(w, id, "Kick", 3.5);
            kick(w, id);
        }
        Call::Fired(Fired::Timer("Kick")) => kick(w, id),
        Call::Fired(Fired::Delay("back")) => {
            let s = st!(w, id, ShaggyKick);
            let (g, at) = (s.fighter, s.at);
            if let (true, Some(g), Some(at)) = (callable(w, g), g, at) {
                let me = TutorialObj::Demo(id);
                latent::move_to(w, me, "back", MoveTarget::Fighter(g), at.x, at.y, 0.3, true, true);
            }
        }
        Call::Fired(Fired::Move("back")) => {}
        Call::Unpossessed { .. } => clear_timer(w, id, "Kick"),
        _ => unhandled(w, id, c),
    }
}

fn kick(w: &mut World, id: u32) {
    let s = st!(w, id, ShaggyKick);
    let (f, left) = (s.fighter, s.is_left);
    if let (true, Some(f)) = (callable(w, f), f) {
        crate::fighter::set_facing(w, f, if left { -1.0 } else { 1.0 });
        pad(w, id).press(Btn::Special, Some(Dir::Fwd));
    }
    let s = st!(w, id, ShaggyKick);
    s.is_left = !s.is_left;
    if s.is_left {
        return;
    }
    delay(w, id, "back", 0.85);
}

/// TutorialController_Special / _ShaggySandwich: ReceivePossess Delay 0.1, InitialLocation, a looping 3.0 s timer and
/// one call now. The loop: teleport back, Delay 0.2, K2_GetPawn; active: SpecialAttackPressed(dir) (4 Neutral /
/// 1 Down). ReceiveUnPossess clears the timer.
#[derive(State, Debug, Default)]
pub struct Special {
    pub dir: Dir,
    pub at: Option<Place>,
    pub possessed: Option<FighterId>,
}

impl Special {
    fn new(dir: Dir) -> Self {
        Special { dir, ..Special::default() }
    }
}

fn special(w: &mut World, id: u32, c: Call<'_>) {
    match c {
        Call::Possessed(f) => {
            let p = {
                let cur = st!(w, id, Special).possessed;
                first_possess(w, id, f, cur)
            };
            st!(w, id, Special).possessed = p;
            delay(w, id, "possess", 0.1);
        }
        Call::Fired(Fired::Delay("possess")) => {
            let f = st!(w, id, Special).possessed;
            if callable(w, f) {
                let at = place_of(w, f.expect("callable"));
                st!(w, id, Special).at = Some(at);
            }
            set_timer(w, id, "Attack Loop", 3.0);
            special_loop(w, id);
        }
        Call::Fired(Fired::Timer("Attack Loop")) => special_loop(w, id),
        Call::Fired(Fired::Delay("press")) => {
            let p = get_pawn(w, id);
            if active(w, p) {
                let dir = st!(w, id, Special).dir;
                pad(w, id).press(Btn::Special, Some(dir));
            }
        }
        Call::Unpossessed { .. } => clear_timer(w, id, "Attack Loop"),
        _ => unhandled(w, id, c),
    }
}

fn special_loop(w: &mut World, id: u32) {
    let p = get_pawn(w, id);
    let at = st!(w, id, Special).at;
    teleport(w, p, at);
    delay(w, id, "press", 0.2);
}

/// TutorialController_Evade: ReceivePossess stores DemoFighter, Delay 0.5, InitialLocation, a looping 4.0 s timer to
/// Attack (@1122), then Delay 1.0 (@223): active: SetMoveAxisX(1), EvadeButtonPressed, SetMoveAxisX(0) in one call.
/// Attack: SetMoveAxisX(-1), Delay 0.3, SetMoveAxisX(1), Delay 0.01, SetMoveAxisX(0) and MoveComponentTo(
/// InitialLocation, 0.35 s, ease out, ease in), whose Completed goes back to the Delay 1.0 and the dodge. No
/// ReceiveUnPossess: its timer runs on with the controller.
#[derive(State, Debug, Default)]
pub struct Evade {
    pub at: Option<Place>,
    pub fighter: Option<FighterId>,
    pub possessed: Option<FighterId>,
}

fn evade(w: &mut World, id: u32, c: Call<'_>) {
    match c {
        Call::Possessed(f) => {
            let p = {
                let cur = st!(w, id, Evade).possessed;
                first_possess(w, id, f, cur)
            };
            let s = st!(w, id, Evade);
            s.fighter = Some(f);
            s.possessed = p;
            delay(w, id, "possess", 0.5);
        }
        Call::Fired(Fired::Delay("possess")) => {
            let f = st!(w, id, Evade).possessed;
            if callable(w, f) {
                let at = place_of(w, f.expect("callable"));
                st!(w, id, Evade).at = Some(at);
            }
            set_timer(w, id, "Attack", 4.0);
            delay(w, id, "dodge", 1.0);
        }
        Call::Fired(Fired::Delay("dodge")) => {
            if !{
                let v = st!(w, id, Evade).fighter;
                active(w, v)
            } {
                return;
            }
            let p = pad(w, id);
            p.set_move_axis_x(1.0);
            p.press(Btn::Dodge, None);
            p.set_move_axis_x(0.0);
        }
        Call::Fired(Fired::Timer("Attack")) => {
            let v = st!(w, id, Evade).fighter;
            if callable(w, v) {
                pad(w, id).set_move_axis_x(-1.0);
            }
            delay(w, id, "a1", 0.3);
        }
        Call::Fired(Fired::Delay("a1")) => {
            let v = st!(w, id, Evade).fighter;
            if callable(w, v) {
                pad(w, id).set_move_axis_x(1.0);
            }
            delay(w, id, "a2", 0.01);
        }
        Call::Fired(Fired::Delay("a2")) => {
            let s = st!(w, id, Evade);
            let (f, at) = (s.fighter, s.at);
            let (true, Some(f)) = (callable(w, f), f) else { return };
            pad(w, id).set_move_axis_x(0.0);
            if let Some(at) = at {
                let me = TutorialObj::Demo(id);
                latent::move_to(w, me, "back", MoveTarget::Fighter(f), at.x, at.y, 0.35, true, true);
            }
        }
        Call::Fired(Fired::Move("back")) => delay(w, id, "dodge", 1.0),
        Call::Unpossessed { .. } => {}
        _ => unhandled(w, id, c),
    }
}

/// TutorialController_ChargeUp: ReceivePossess Delay 0.5, (@188) InitialLocation of the possessed pawn, a looping
/// 6.0 s timer to Attack Loop and one call now. Attack Loop (@952): K2_GetPawn teleported back; the cast of
/// K2_GetPawn, active: NormalAttackPressed(4); Delay 2.0, MoveX(-0.1), Delay 0.05 (@477), MoveX(0),
/// NormalAttackPressed(0 Up); Delay 2.0 (@771), MoveX(0.1), Delay 0.05 (@15), MoveX(0). No ReceiveUnPossess.
#[derive(State, Debug, Default)]
pub struct ChargeUp {
    pub at: Option<Place>,
    pub cast: Option<FighterId>,
    pub possessed: Option<FighterId>,
}

fn charge_up(w: &mut World, id: u32, c: Call<'_>) {
    let mv = |w: &mut World, v: Real| {
        let cast = st!(w, id, ChargeUp).cast;
        if callable(w, cast) {
            pad(w, id).move_x(v);
        }
    };
    match c {
        Call::Possessed(f) => {
            let p = {
                let cur = st!(w, id, ChargeUp).possessed;
                first_possess(w, id, f, cur)
            };
            st!(w, id, ChargeUp).possessed = p;
            delay(w, id, "possess", 0.5);
        }
        Call::Fired(Fired::Delay("possess")) => {
            let f = st!(w, id, ChargeUp).possessed;
            if callable(w, f) {
                let at = place_of(w, f.expect("callable"));
                st!(w, id, ChargeUp).at = Some(at);
            }
            set_timer(w, id, "Attack Loop", 6.0);
            charge_loop(w, id);
        }
        Call::Fired(Fired::Timer("Attack Loop")) => charge_loop(w, id),
        Call::Fired(Fired::Delay("@1327")) => {
            mv(w, -0.1);
            delay(w, id, "@477", 0.05);
        }
        Call::Fired(Fired::Delay("@477")) => {
            mv(w, 0.0);
            let v = st!(w, id, ChargeUp).cast;
            if callable(w, v) {
                pad(w, id).press(Btn::Attack, Some(Dir::Up));
            }
            delay(w, id, "@771", 2.0);
        }
        Call::Fired(Fired::Delay("@771")) => {
            mv(w, 0.1);
            delay(w, id, "@15", 0.05);
        }
        Call::Fired(Fired::Delay("@15")) => mv(w, 0.0),
        Call::Unpossessed { .. } => {}
        _ => unhandled(w, id, c),
    }
}

fn charge_loop(w: &mut World, id: u32) {
    let at = st!(w, id, ChargeUp).at;
    let p = get_pawn(w, id);
    teleport(w, p, at);
    let p = get_pawn(w, id);
    st!(w, id, ChargeUp).cast = p;
    if !active(w, p) {
        return;
    }
    pad(w, id).press(Btn::Attack, Some(Dir::Neutral));
    delay(w, id, "@1327", 2.0);
}

/// TutorialController_Combat / _Damage / _IntroFinale (one graph, the bag's damage 0 / 200 / 100): ReceivePossess
/// Delay 0.2, then a Wonder Woman bag at the first actor tagged PunchingBag (`run::spawn_idle_bag`). The demo pawn
/// presses nothing. ReceiveUnPossess (@1561): K2_DestroyActor on that bag (its Idle controller goes with it,
/// AController::PawnPendingDestroy), then on every TetherProjectile_C.
#[derive(State, Debug, Default)]
pub struct Combat {
    pub damage: Real,
    pub bag: Option<FighterId>,
}

impl Combat {
    fn new(damage: Real) -> Self {
        Combat { damage, bag: None }
    }
}

fn combat(w: &mut World, id: u32, c: Call<'_>) {
    match c {
        Call::Possessed(_) => delay(w, id, "possess", 0.2),
        Call::Fired(Fired::Delay("possess")) => {
            let damage = st!(w, id, Combat).damage;
            let bag = super::run::spawn_idle_bag(w, "PunchingBag", damage, None);
            st!(w, id, Combat).bag = bag;
        }
        Call::Unpossessed { .. } => {
            let bag = st!(w, id, Combat).bag;
            if let (true, Some(bag)) = (callable(w, bag), bag) {
                super::run::despawn(w, bag);
            }
            // Not ported: the tethers' destroy (GetAllActorsOfClass(TetherProjectile_C) @1608); no tutorial pawn
            // throws one.
            let tether = w.actors.projectiles.iter().any(|(pid, p)| {
                let own = ActorRef::Fighter(lookup_owner(w, ActorRef::Projectile(pid)));
                !p.dead && actor_is_a(w, own, p.cls, name!("TetherProjectile_C")) == Some(true)
            });
            if tether {
                warn_text(w, "TutorialController_Combat: a TetherProjectile_C left at the demo's end is not destroyed");
            }
        }
        _ => unhandled(w, id, c),
    }
}

/// TutorialController_AttackDecay_Basics: ReceivePossess (@2390) DemoComplete = false, DemoFighter,
/// InitialDemoFighterLocation, Delay 0.6, (@15) its SpawnPunchingBag(PunchingBagClose, 1, Pawn_WonderWomanV3), the
/// bag moved by (0, 50, 0) (K2_SetActorLocation, no sweep) and that place stored; Delay 1.6; a looping 4.0 s timer to
/// AttackDecayDemo; BPI_InsertAttackDecay("combo", i) on DemoFighter for i 0..7; AttackDecayDemo now.
/// AttackDecayDemo (@2500): NormalAttackPressed(2 Forward) (no active gate); Delay 2.5; not DemoComplete: Delay 0.0,
/// not DemoComplete: both teleported back, the bag's CurrentDamage 0, AddInvisibility on both; Delay 0.15, not
/// DemoComplete: RemoveInvisibility on both. ReceiveUnPossess (@2151): with the bag valid, the bag destroyed, the
/// timer cleared, DemoComplete = true (the possession's own second Possess finds no bag yet and changes nothing).
/// Not ported: PS_WonderWoman_TutorialSpawn at both (@794, @967) and PS_Invulnerability on both meshes (@1860,
/// @2032).
#[derive(State, Debug, Default)]
pub struct AttackDecay {
    pub fighter: Option<FighterId>,
    pub bag: Option<FighterId>,
    pub at: Option<Place>,
    pub bag_at: Option<Place>,
    pub complete: bool,
}

fn attack_decay(w: &mut World, id: u32, c: Call<'_>) {
    match c {
        Call::Possessed(f) => {
            let at = place_of(w, f);
            let s = st!(w, id, AttackDecay);
            s.complete = false;
            s.fighter = Some(f);
            s.at = Some(at);
            delay(w, id, "possess", 0.6);
        }
        Call::Fired(Fired::Delay("possess")) => {
            let bag = super::run::spawn_idle_bag(w, "PunchingBagClose", 0.0, Some(0.0)); // (SpawnPunchingBag @363)
            st!(w, id, AttackDecay).bag = bag;
            if let Some(bag) = bag {
                let (x, y) = (w.fighter(bag).body.x, crate::fighter::actor_y(w, bag));
                crate::fighter::moves::recall_move(w, bag, x - 50.0, y);
                crate::combat::phases::root_moved(w, bag);
                let p = place_of(w, bag);
                st!(w, id, AttackDecay).bag_at = Some(p);
            }
            delay(w, id, "bag", 1.6);
        }
        Call::Fired(Fired::Delay("bag")) => {
            set_timer(w, id, "AttackDecayDemo", 4.0);
            let f = st!(w, id, AttackDecay).fighter;
            if let (true, Some(f)) = (callable(w, f), f) {
                let d = &mut w.fighter_mut(f).decay.attack_decay;
                for i in 0..=7 {
                    // (splice past the end appends)
                    d.insert(i.min(d.len()), name!("combo"));
                }
            }
            decay_demo(w, id);
        }
        Call::Fired(Fired::Timer("AttackDecayDemo")) => decay_demo(w, id),
        Call::Fired(Fired::Delay("show")) => {
            if !st!(w, id, AttackDecay).complete {
                delay(w, id, "reset", 0.0);
            }
        }
        Call::Fired(Fired::Delay("reset")) => {
            let s = st!(w, id, AttackDecay);
            if s.complete {
                return;
            }
            let (f, bag, at, bag_at) = (s.fighter, s.bag, s.at, s.bag_at);
            teleport(w, f, at);
            teleport(w, bag, bag_at);
            if let (true, Some(b)) = (callable(w, bag), bag) {
                w.fighter_mut(b).stats.damage = 0.0;
            }
            if let (true, Some(f)) = (callable(w, f), f) {
                crate::fighter::anim::add_invisibility(w, f);
            }
            if let (true, Some(b)) = (callable(w, bag), bag) {
                crate::fighter::anim::add_invisibility(w, b);
            }
            delay(w, id, "appear", 0.15);
        }
        Call::Fired(Fired::Delay("appear")) => {
            let s = st!(w, id, AttackDecay);
            if s.complete {
                return;
            }
            let (f, bag) = (s.fighter, s.bag);
            if let (true, Some(f)) = (callable(w, f), f) {
                crate::fighter::anim::remove_invisibility(w, f);
            }
            if let (true, Some(b)) = (callable(w, bag), bag) {
                crate::fighter::anim::remove_invisibility(w, b);
            }
        }
        Call::Unpossessed { .. } => {
            let bag = st!(w, id, AttackDecay).bag;
            let (true, Some(bag)) = (callable(w, bag), bag) else { return };
            super::run::despawn(w, bag);
            clear_timer(w, id, "AttackDecayDemo");
            st!(w, id, AttackDecay).complete = true;
        }
        _ => unhandled(w, id, c),
    }
}

fn decay_demo(w: &mut World, id: u32) {
    let v = st!(w, id, AttackDecay).fighter;
    if callable(w, v) {
        pad(w, id).press(Btn::Attack, Some(Dir::Fwd));
    }
    delay(w, id, "show", 2.5);
}

/// A call the class has no handler for: a node of another class's, which no entry of this one makes.
fn unhandled(w: &World, id: u32, c: Call<'_>) {
    panic!("tutorial: {} has no handler for {c:?}", demo_of(w, id).cls);
}
