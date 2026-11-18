//! (the port's own) The Lab Tools: a panel over the running Lab, at the top right of
//! the match, that the Lab menu's Lab Tools switch turns on (screens/lab.rs LabPanel; the panel's widgets:
//! screens/lab_tools.rs). The build has no such tools. Three pages:
//!   Settings  the Lab menu's own settings but Hazards Enabled (the Lab menu shows only that one and the Lab Tools
//!             switch), and the match's clock slower or faster
//!   Display   the sim's shapes over the match (shapes.rs), the HUD and the effects hidden
//!   Fighters  one fighter at a time: its damage, a locked damage, invulnerability, air moves that never run out, a
//!             held place
//! The panel is closed (its header alone) at each Lab start. The Lab Tools key (a binding of the game's keybind
//! settings, bindings.rs: F1 and the right stick's button) or the header's button opens and closes it. While it is
//! open the game's cursor shows on it and the pad is the panel's: it moves the cursor and the focus as in the game's
//! menus, B or the Lab Tools key closes it, and the game reads the pads as let go until every button is up.
//! The sim is not edited: what the tools change is written between ticks (run.rs step).
// Deviation: the tools' state (the fighters' switches, the clock, the chosen fighter) is not kept over
// a Lab left and entered again, nor are the Display screen's switches saved; all of it lives with one visit of the Lab
// (a restart or a map change keeps it, a Lab entered from the menus starts at the defaults). The
// Lab Tools switch and the panel's page are held with the settings (mvs-settings LabSettings) for the visit, not saved.

use mvs_sim::World;
use mvs_sim::fighter::FighterId;
use mvs_ui::bindings::{Bindings, LAB_TOOLS_ACTION, PAD_BUTTONS};
use mvs_ui::bundle::Bundle;
use mvs_ui::element::ElementList;
use mvs_ui::menu::controller::MenuRun;
use mvs_ui::menu::input::{MenuPad, mouse_button_key};
use mvs_ui::menu::screens::lab_tools::{
    FighterInfo, LabToolsPanel, ToolCmd, ToolLabel, ToolPage, ToolRow, ToolsState, find_node,
};
use mvs_ui::menu::training_config::LabSession;
use mvs_ui::menu::{Manager, MenuInput, MenuScreen, MenuView};
use mvs_ui::slate::WidgetId;

use crate::app::Shell;
use crate::match_lab::LabState;
use crate::menu_audio::MenuAudio;
use crate::shapes::{LabShow, ShapesState, ShowKey};

/// The clock's rates (the Game Speed row's steps).
const SPEEDS: [f64; 7] = [0.1, 0.25, 0.5, 0.75, 1.0, 1.5, 2.0];
/// SPEEDS' 1.
const SPEED_ONE: usize = 4;
/// The rows' state is read again this often while the panel shows (a fighter's damage changes in play), s.
const REFRESH: f64 = 0.1;
/// The standard mapping's B.
const PAD_B: u32 = 1 << 1;

// The panel's controls (ToolRow ids).
const T_HITSTUN: u32 = 1;
const T_JUGGLE: u32 = 2;
const T_COOLDOWNS: u32 = 3;
const T_SAME_ATTACK: u32 = 4;
/// The Display screen's shape switches: T_SHOW + their ShowKey index.
const T_SHOW: u32 = 10;
const T_HIDE_HUD: u32 = 20;
const T_HIDE_FX: u32 = 21;
const T_LOCK: u32 = 30;
const T_INVULN: u32 = 31;
const T_AIR: u32 = 32;
const T_HOLD: u32 = 33;
const S_SPEED: u32 = 40;
const S_DAMAGE: u32 = 41;
const B_RESET: u32 = 50;
const B_SPAWN: u32 = 51;
const B_TURN: u32 = 52;

/// A fighter's tools, written between ticks.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Pins {
    lock_damage: Option<f64>,
    invuln: bool,
    air_moves: bool,
    hold: Option<(f64, f64)>,
}

/// The frame's input as the Lab's screens take it (the window's, or the script's).
#[derive(Clone, Debug, Default)]
pub struct FrameIn {
    pub menu: Vec<MenuInput>,
    /// The first pad's state (the first standard pad), the menus' axes (Y down positive).
    pub pad: Option<MenuPad>,
}

/// The Lab Tools of a Lab match: the panel's screen and what the tools hold.
pub struct LabTools {
    screen: MenuScreen,
    panel: WidgetId,
    /// The panel's pages show; the cursor and the pad are the panel's.
    open: bool,
    /// The pads are still kept from the game after a close, until all the first pad's buttons are up (the B or the
    /// key that closed it is no press of the game's).
    pad_held: bool,
    /// The first pad's buttons down at the last frame.
    pad_prev: u32,
    pins: Vec<Option<Pins>>,
    speed_at: usize,
    /// The Display screen's switches: the shapes drawn, the HUD and the effects hidden (read while the switch is on).
    pub show: LabShow,
    pub hide_hud: bool,
    pub hide_fx: bool,
    sel: usize,
    /// The panel showed at the last frame.
    shown: bool,
    /// The clock of the last refresh, s.
    last_refresh: f64,
    /// The Lab menu's switch as the last frame saw it.
    on_seen: bool,
    pub shapes: ShapesState,
}

impl std::fmt::Debug for LabTools {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LabTools")
            .field("open", &self.open)
            .field("speed", &SPEEDS[self.speed_at])
            .finish_non_exhaustive()
    }
}

/// `f` on a MenuRun of the shell's UI (no session: the panel's rows write through the tools) and its element list.
fn with_run<R>(
    sh: &mut Shell,
    bindings: &mut Bindings,
    f: impl FnOnce(&mut MenuRun<'_>, &mut ElementList) -> R,
) -> Option<R> {
    let Shell { ui, settings, audio, music, window, .. } = sh;
    let u = ui.as_mut()?;
    let mut sink = MenuAudio { audio, tick: music.tick, window: Some(crate::apply::window_info(window)), equip: None };
    let mut run = MenuRun {
        fonts: &mut u.menu_fonts,
        prefs: settings,
        bindings,
        sink: &mut sink,
        pointer: &mut u.pointer,
        lab: None,
        pause: None,
        tutorial: None,
    };
    Some(f(&mut run, &mut u.list))
}

/// A text of UI_LAB_Panel: the label of its widget `name` (the Lab menu's toggles), else the name.
fn lab_text(b: &Bundle, name: &str) -> ToolLabel {
    b.widgets
        .get("UI_LAB_Panel")
        .and_then(|c| find_node(&c.root, name))
        .and_then(|n| n.p.get("LabelText"))
        .map_or_else(|| ToolLabel::Typed(name.to_owned()), |v| ToolLabel::Build(v.clone()))
}

fn speed_text(v: f64) -> String {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let s = SPEEDS[(v.max(0.0) as usize).min(SPEEDS.len() - 1)];
    format!("{}%", (s * 100.0).round())
}

fn damage_text(v: f64) -> String {
    format!("{v}%")
}

/// The panel's pages.
fn pages(b: &Bundle) -> Vec<ToolPage> {
    let heading = |t: &str| ToolRow::Heading(ToolLabel::Typed(t.to_owned()));
    let toggle = |id: u32, label: ToolLabel| ToolRow::Toggle { id, label, swatch: None };
    let typed = |t: &str| ToolLabel::Typed(t.to_owned());
    let show = |k: ShowKey, label: ToolLabel| ToolRow::Toggle {
        id: T_SHOW + ShowKey::ALL.iter().position(|x| *x == k).unwrap_or(0) as u32,
        label,
        swatch: Some(k.color()),
    };
    vec![
        ToolPage {
            // (the Lab menu's "Settings" header) the Lab menu's settings and the clock, written as its toggles write
            // them (screens/lab.rs LabPanel)
            name: "Settings".into(),
            rows: vec![
                heading("Rules"),
                toggle(T_HITSTUN, lab_text(b, "toggleHitstunCombo")),
                toggle(T_JUGGLE, lab_text(b, "toggleJuggleCombo")),
                toggle(T_COOLDOWNS, lab_text(b, "toggleNoCooldowns")),
                toggle(T_SAME_ATTACK, lab_text(b, "toggleSameAttack")),
                heading("Time"),
                ToolRow::Stepper {
                    id: S_SPEED,
                    label: "Game Speed".into(),
                    min: 0.0,
                    max: (SPEEDS.len() - 1) as f64,
                    step: 1.0,
                    text: speed_text,
                },
            ],
        },
        ToolPage {
            name: "Display".into(),
            rows: vec![
                heading("Hitboxes"),
                show(ShowKey::Hit, lab_text(b, "toggleHitboxes")),
                show(ShowKey::Hurt, lab_text(b, "toggleHurtboxes")),
                show(ShowKey::Capsule, typed("Show Collision Capsules")),
                show(ShowKey::Actors, typed("Show Projectiles")),
                show(ShowKey::Stage, typed("Show Stage Bounds")),
                show(ShowKey::Labels, typed("Show Fighter States")),
                heading("Screen"),
                toggle(T_HIDE_HUD, typed("Hide HUD")),
                toggle(T_HIDE_FX, typed("Hide Effects")),
            ],
        },
        ToolPage {
            name: "Fighters".into(),
            rows: vec![
                ToolRow::Fighter,
                heading("Damage"),
                ToolRow::Stepper {
                    id: S_DAMAGE,
                    label: "Damage".into(),
                    min: 0.0,
                    max: 300.0,
                    step: 10.0,
                    text: damage_text,
                },
                toggle(T_LOCK, typed("Lock Damage")),
                toggle(T_INVULN, typed("Invulnerable")),
                heading("Movement"),
                toggle(T_AIR, typed("Infinite Air Moves")),
                toggle(T_HOLD, typed("Hold In Place")),
                heading("Actions"),
                ToolRow::Buttons(vec![
                    (B_RESET, "Reset Lab".into()),
                    (B_SPAWN, "To Spawn".into()),
                    (B_TURN, "Turn".into()),
                ]),
            ],
        },
    ]
}

/// The Lab Tools key's action is on `key` (binding set 0's).
fn is_lab_key(bindings: &Bindings, key: &str) -> bool {
    bindings.actions_of(key, 0).iter().any(|a| a == LAB_TOOLS_ACTION)
}

/// What the tools' frame asks of the match.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ToolsOut {
    /// Reset Lab: the World reset as the Lab's Spawn All Players does, on the fighters' starting damage.
    pub reset: bool,
}

/// The parts of the Lab the tools read and write.
pub struct ToolsCx<'a> {
    pub bindings: &'a mut Bindings,
    pub session: &'a mut LabState,
    pub world: &'a mut World,
}

impl LabTools {
    /// The tools of a new Lab: the panel built on a screen of its own, closed (None without the UI).
    pub fn new(sh: &mut Shell, bindings: &mut Bindings) -> Option<LabTools> {
        let size = sh.swap.size;
        let (bundle, text) = sh.ui.as_ref().map(|u| (u.menu.clone(), u.text.clone()))?;
        let page = sh.settings.lab.page as usize;
        let on = sh.settings.lab.tools_on;
        let view = MenuView::new(bundle.clone(), text, size);
        let (screen, panel) = with_run(sh, bindings, |run, _| {
            let mut screen = MenuScreen::new(view, run);
            let state = ToolsState { page, folded: true, ..ToolsState::default() };
            let panel = screen.with_cx(run, |cx| LabToolsPanel::add(cx, pages(&bundle), state));
            // (the closed panel shows its header alone, over the match)
            screen.alone.push(panel);
            (screen, panel)
        })?;
        let mut t = LabTools {
            screen,
            panel,
            open: true,
            pad_held: false,
            pad_prev: 0,
            pins: Vec::new(),
            speed_at: SPEED_ONE,
            show: LabShow::default(),
            hide_hud: false,
            hide_fx: false,
            sel: 0,
            shown: false,
            last_refresh: f64::NEG_INFINITY,
            on_seen: on,
            shapes: ShapesState::default(),
        };
        with_run(sh, bindings, |run, _| t.set_open_in(run, false));
        Some(t)
    }

    /// The Lab started again on a changed config: the panel is kept as it was, a held place is taken again (the
    /// fighters stand at their spawns).
    pub fn restarted(&mut self) {
        for p in self.pins.iter_mut().flatten() {
            p.hold = None;
        }
        self.shown = false;
    }

    /// The pads are the panel's (open) or still held from the game after a close.
    pub fn pad_claimed(&self) -> bool {
        self.open || self.pad_held
    }

    /// The match's tick rate against the clock's.
    pub fn time_scale(&self, on: bool) -> f64 {
        if on { SPEEDS[self.speed_at] } else { 1.0 }
    }

    /// The tools' writes on the World, before and after a tick (while the Lab Tools are on).
    pub fn apply_pins(&self, w: &mut World, on: bool) {
        if !on {
            return;
        }
        for (i, p) in self.pins.iter().enumerate() {
            let (Some(p), Some(&f)) = (p, w.players.get(i)) else { continue };
            let fm = w.fighter_mut(f);
            if let Some(d) = p.lock_damage {
                fm.stats.damage = mvs_sim::math::narrow(d);
            }
            // (Comp_Actor_Destructible's InvulnerabilityCount: one of the tools' own while the sim holds none)
            if p.invuln && fm.invuln.invuln_count <= 0 {
                fm.invuln.invuln_count = 1;
            }
            if p.air_moves {
                fm.jump.jumps = 0;
                fm.air.air_evades = 0;
                fm.air.air_specials = 0;
            }
            if let Some((x, y)) = p.hold {
                fm.body.x = mvs_sim::math::narrow(x);
                fm.body.prev_x = mvs_sim::math::narrow(x);
                fm.body.y = mvs_sim::math::narrow(y);
                fm.body.prev_y = mvs_sim::math::narrow(y);
                fm.body.vx = 0.0;
                fm.body.vy = 0.0;
            }
        }
    }

    fn pin(&mut self, i: usize) -> &mut Pins {
        if self.pins.len() <= i {
            self.pins.resize(i + 1, None);
        }
        self.pins[i].get_or_insert_with(Pins::default)
    }

    fn pin_of(&self, i: usize) -> Option<&Pins> {
        self.pins.get(i).and_then(Option::as_ref)
    }

    /// The tools' writes go when the switch goes off: the invulnerability count they hold, the clock; the panel
    /// closes (the HUD and the effects show again by the switch itself, run.rs draw).
    fn release(&mut self, run: &mut MenuRun<'_>, w: &mut World) {
        for (i, &f) in w.players.clone().iter().enumerate() {
            let fm = w.fighter_mut(f);
            if self.pins.get(i).and_then(Option::as_ref).is_some_and(|p| p.invuln) && fm.invuln.invuln_count > 0 {
                fm.invuln.invuln_count -= 1;
            }
        }
        self.pins.clear();
        self.speed_at = SPEED_ONE;
        self.set_open_in(run, false);
    }

    /// The panel opened or closed (its state shown at the next refresh).
    fn set_open_in(&mut self, run: &mut MenuRun<'_>, v: bool) {
        if self.open == v {
            return;
        }
        self.open = v;
        if v {
            self.pad_held = true;
            // the game's cursor at the window's pointer (where the mouse last was), shown until the panel closes
            self.screen.set_active(run, true);
            self.screen.view.show_cursor(true);
            self.screen.view.manager.set_handle_input(true);
        } else {
            // The closed panel takes no input of the view's (a click on it opens it): no cursor, no focus look, no
            // hover.
            self.screen.view.show_cursor(false);
            self.screen.with_cx(run, |cx| {
                if let Some(f) = cx.view.manager.get_currently_focused(0) {
                    Manager::set_unfocused(cx, f, 0);
                }
            });
            self.screen.view.manager.set_handle_input(false);
            self.screen.view.mouse = None;
            self.screen.set_active(run, false);
        }
    }

    /// The panel's state from the tools, the session and the World.
    fn state(&self, settings: &mvs_settings::Settings, session: &LabState, w: &World) -> ToolsState {
        let lab = &settings.lab;
        let mut s = ToolsState { page: lab.page as usize, folded: !self.open, sel: self.sel, ..ToolsState::default() };
        // the Lab's match settings are its leader's (UI_LAB_Panel HandleServerUpdateConfig @488-689)
        let leader = session.config.leader_account_id == session.me();
        let t = &mut s.toggles;
        t.insert(T_HITSTUN, (session.toggles.hitstun_combo, true));
        t.insert(T_JUGGLE, (session.toggles.juggle_combo, true));
        t.insert(T_COOLDOWNS, (session.config.instant_cooldowns, leader));
        t.insert(T_SAME_ATTACK, (session.config.same_attack_penalty, leader));
        for (k, key) in ShowKey::ALL.iter().enumerate() {
            t.insert(T_SHOW + k as u32, (key.get(&self.show), true));
        }
        t.insert(T_HIDE_HUD, (self.hide_hud, true));
        t.insert(T_HIDE_FX, (self.hide_fx, true));
        let p = self.pin_of(self.sel).copied().unwrap_or_default();
        t.insert(T_LOCK, (p.lock_damage.is_some(), true));
        t.insert(T_INVULN, (p.invuln, true));
        t.insert(T_AIR, (p.air_moves, true));
        t.insert(T_HOLD, (p.hold.is_some(), true));
        s.values.insert(S_SPEED, self.speed_at as f64);
        let damage = w.players.get(self.sel).map_or(0.0, |&f| w.fighter(f).stats.damage);
        s.values.insert(S_DAMAGE, f64::from((damage + 0.5).floor()));
        s.fighters = w
            .players
            .iter()
            .enumerate()
            .map(|(i, &f)| {
                let p = w.setup.players.get(i);
                Some(FighterInfo {
                    character: p.and_then(|p| p.character.clone()),
                    skin: p.and_then(|p| p.skin.clone()).unwrap_or_default(),
                    team: w.fighter(f).team.team,
                })
            })
            .collect();
        s
    }

    fn refresh_in(&mut self, run: &mut MenuRun<'_>, session: &LabState, w: &World, now: f64) {
        self.sel = self.sel.min(w.players.len().saturating_sub(1));
        let state = self.state(run.prefs, session, w);
        let panel = self.panel;
        self.screen.with_cx(run, |cx| cx.visit::<LabToolsPanel, _>(panel, |p, cx| p.set_state(cx, state)));
        self.last_refresh = now;
    }

    /// The chosen fighter.
    fn fighter(&self, w: &World) -> Option<FighterId> {
        w.players.get(self.sel).copied()
    }

    /// What a control asked: written to the tools, the session, the settings or the World. True: Reset Lab.
    fn apply(&mut self, run: &mut MenuRun<'_>, c: ToolCmd, session: &mut LabState, w: &mut World) -> bool {
        let sel = self.sel;
        let f = self.fighter(w);
        match c {
            ToolCmd::Toggle(id, v) => match id {
                T_HITSTUN => session.toggles.hitstun_combo = v,
                T_JUGGLE => session.toggles.juggle_combo = v,
                T_COOLDOWNS => {
                    let c = mvs_ui::menu::training_config::LabConfig { instant_cooldowns: v, ..session.config.clone() };
                    session.request_config_update(c);
                }
                T_SAME_ATTACK => {
                    let c =
                        mvs_ui::menu::training_config::LabConfig { same_attack_penalty: v, ..session.config.clone() };
                    session.request_config_update(c);
                }
                T_HIDE_HUD => self.hide_hud = v,
                T_HIDE_FX => self.hide_fx = v,
                T_LOCK => {
                    let d = f.map_or(0.0, |f| w.fighter(f).stats.damage);
                    self.pin(sel).lock_damage = v.then_some(f64::from(d));
                }
                T_INVULN => {
                    self.pin(sel).invuln = v;
                    if let Some(f) = f
                        && !v
                        && w.fighter(f).invuln.invuln_count > 0
                    {
                        w.fighter_mut(f).invuln.invuln_count -= 1;
                    }
                }
                T_AIR => self.pin(sel).air_moves = v,
                T_HOLD => {
                    let at = f.map(|f| (f64::from(w.fighter(f).body.x), f64::from(w.fighter(f).body.y)));
                    self.pin(sel).hold = if v { at } else { None };
                }
                id if (T_SHOW..T_SHOW + ShowKey::ALL.len() as u32).contains(&id) => {
                    ShowKey::ALL[(id - T_SHOW) as usize].set(&mut self.show, v);
                }
                _ => {}
            },
            ToolCmd::Value(S_SPEED, v) => {
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let i = v.max(0.0) as usize;
                self.speed_at = i.min(SPEEDS.len() - 1);
            }
            ToolCmd::Value(S_DAMAGE, v) => {
                let Some(f) = f else { return false };
                let d = mvs_sim::math::narrow(v);
                w.fighter_mut(f).stats.damage = d;
                if let Some(p) = self.pins.get_mut(sel).and_then(Option::as_mut)
                    && p.lock_damage.is_some()
                {
                    p.lock_damage = Some(f64::from(d));
                }
            }
            ToolCmd::Value(..) => {}
            ToolCmd::Click(B_RESET) => return true,
            ToolCmd::Click(B_SPAWN) => self.send_to_spawn(w),
            ToolCmd::Click(B_TURN) => {
                // (the facing's setter: the turn's signals and hook; between ticks no overlap update runs)
                if let Some(f) = f {
                    let v = -mvs_sim::fighter::facing(w, f);
                    mvs_sim::fighter::set_facing(w, f, v);
                }
            }
            ToolCmd::Click(_) => {}
            ToolCmd::Page(i) => run.prefs.lab.page = i as u32,
            ToolCmd::Fold(folded) => self.set_open_in(run, !folded),
            ToolCmd::Fighter(i) => self.sel = i,
        }
        false
    }

    fn send_to_spawn(&mut self, w: &mut World) {
        let sel = self.sel;
        let (Some(f), Some(s)) = (self.fighter(w), w.stage.spawns.get(sel).copied()) else { return };
        let b = &mut w.fighter_mut(f).body;
        b.x = s.x;
        b.prev_x = s.x;
        b.y = s.y;
        b.prev_y = s.y;
        b.vx = 0.0;
        b.vy = 0.0;
        mvs_sim::fighter::set_facing(w, f, s.facing);
        if let Some(p) = self.pins.get_mut(sel).and_then(Option::as_mut)
            && p.hold.is_some()
        {
            p.hold = Some((f64::from(s.x), f64::from(s.y)));
        }
    }

    /// One frame of the panel, once the Lab's menu has had its own (its paint added to the shell's element list):
    /// `visible` the panel shows (the switch on, the Lab not paused, its menu not up), `input` the frame's, `now`
    /// the match's real clock.
    pub fn frame(&mut self, sh: &mut Shell, t: &mut ToolsCx<'_>, input: &FrameIn, visible: bool, now: f64) -> ToolsOut {
        let mut out = ToolsOut::default();
        let on = sh.settings.lab.tools_on;
        let ToolsCx { bindings, session, world } = t;
        if on != self.on_seen {
            self.on_seen = on;
            if !on {
                with_run(sh, bindings, |run, _| self.release(run, world));
            }
        }
        let down = input.pad.map_or(0, |p| p.buttons);
        // (after a close the game has the pads back once the first pad's buttons are up, whether the panel shows
        // or not)
        // Deviation: the hold is cleared whenever the panel is closed, not only while it shows: a close by the switch
        // going off (the
        // Lab menu up) would keep the pads from the game for the rest of the Lab.
        if !self.open && self.pad_held && down == 0 {
            self.pad_held = false;
        }
        if visible != self.shown {
            self.shown = visible;
            // (a panel shown again reads the pad from a held start: the pause button that closed the Lab menu)
            self.pad_prev = down;
            if visible {
                let panel = self.panel;
                with_run(sh, bindings, |run, _| {
                    if self.open {
                        // (the screen's next frame is a first: what is down is held, not pressed)
                        self.screen.set_active(run, false);
                        self.screen.set_active(run, true);
                    }
                    self.screen.with_cx(run, |cx| cx.visit::<LabToolsPanel, _>(panel, |p, cx| p.refresh_keys(cx)));
                    self.refresh_in(run, session, world, now);
                });
            }
        }
        if !self.shown {
            return out;
        }
        let panel = self.panel;
        // (the clicks over the panel: a capture listener would stop them before the game's; here the window gave them
        // to the match's devices already, which give them back)
        let mut taken = Vec::new();
        let fed = with_run(sh, bindings, |run, list| {
            let mut fwd = Vec::new();
            let mut at = run.pointer.at;
            let mut toggled = false;
            for ev in &input.menu {
                match *ev {
                    // (the fold button's key, the device the player last touched, is the window's: the panel's
                    // manager starts each frame from it, Pointer::interaction)
                    MenuInput::Key { key, down: true, repeat, .. } => {
                        if is_lab_key(run.bindings, key) && !repeat {
                            let v = !self.open;
                            self.set_open_in(run, v);
                            toggled = true;
                        }
                    }
                    MenuInput::MouseMove { x, y } => {
                        at = Some([x, y]);
                        fwd.push(*ev);
                    }
                    MenuInput::MouseButton { button, down: true } => {
                        let Some([x, y]) = at else { continue };
                        let over = self
                            .screen
                            .with_cx(run, |cx| cx.visit::<LabToolsPanel, _>(panel, |p, cx| p.covers(cx, x, y)))
                            .unwrap_or(false);
                        if !over {
                            continue;
                        }
                        taken.push(mouse_button_key(button));
                        if self.open {
                            fwd.push(*ev);
                        } else {
                            self.set_open_in(run, true);
                            toggled = true;
                        }
                    }
                    MenuInput::MouseButton { down: false, .. } => fwd.push(*ev),
                    _ => {}
                }
            }
            // The pad: closed, a press of the Lab Tools key opens it; open, the panel's (B or the Lab Tools key
            // closes it).
            let pressed = down & !self.pad_prev;
            let lab_pressed =
                PAD_BUTTONS.iter().enumerate().any(|(k, b)| pressed & (1 << k) != 0 && is_lab_key(run.bindings, b));
            if self.open {
                if pressed & PAD_B != 0 || lab_pressed {
                    self.set_open_in(run, false);
                    toggled = true;
                } else if let Some(p) = input.pad {
                    fwd.push(MenuInput::Pad { pad: 0, player: 0, state: p });
                }
            } else if lab_pressed {
                self.set_open_in(run, true);
                toggled = true;
            }
            self.pad_prev = down;
            if toggled {
                self.refresh_in(run, session, world, now);
                if self.open {
                    self.screen.with_cx(run, |cx| cx.visit::<LabToolsPanel, _>(panel, |p, cx| p.focus_page(cx)));
                }
            }
            // (the rows a few times a second: a fighter's damage changes in play)
            if now - self.last_refresh > REFRESH {
                self.refresh_in(run, session, world, now);
            }
            self.screen.frame(run, &fwd, now, list);
            self.screen.with_cx(run, |cx| cx.visit::<LabToolsPanel, _>(panel, |p, _| p.take_cmds())).unwrap_or_default()
        });
        for name in taken {
            sh.devices.take_back_mouse(name);
        }
        let cmds = fed.unwrap_or_default();
        if cmds.is_empty() {
            return out;
        }
        with_run(sh, bindings, |run, _| {
            let was = self.open;
            for c in cmds {
                out.reset |= self.apply(run, c, session, world);
            }
            // (a click's change shows at once)
            self.refresh_in(run, session, world, now);
            if self.open && !was {
                self.screen.with_cx(run, |cx| cx.visit::<LabToolsPanel, _>(panel, |p, cx| p.focus_page(cx)));
            }
        });
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_steppers_show_their_values() {
        assert_eq!(speed_text(0.0), "10%");
        assert_eq!(speed_text(4.0), "100%");
        assert_eq!(speed_text(6.0), "200%");
        assert_eq!(damage_text(50.0), "50%");
        assert_eq!(SPEEDS[SPEED_ONE], 1.0);
    }
}
