//! The fixed screen HUD: PFGPlayerController's UI_Game holds UI_HUD_Scoreboard_Container, which
//! creates UI_IGv3_Scoreboard (anchors 0..1, margins 50 / 50 / 50 / 80) and Inits it with the broker. The Scoreboard
//! lays its UI_IGv3_Fixed slots out for the UI context mode and fades in after Enter's delay. Graphs ported from
//! UI_IGv3_Scoreboard, Fixed, CharacterPortrait, Clock, MatchInfo, ScorePips and PauseHold.

use crate::bundle::{Bundle, Rgba};
use crate::element::ElementList;
use crate::slate::{Geometry, HAlign, Margin, Stretch, Tree, VAlign, Vis, WidgetId, layout, paint};
use crate::text::Fonts;

use super::attached::{Cooldowns, DodgeMeter, Flame, HudAbility, HudAbilityOp, Percentage};
use super::common::{Anim, BrokerPlayer, LOCATION_FIXED, PlayerNumbers, Prefs, vis_if};
use super::{MODE_WAVES, Res};

/// (the stock rule) the opacity of a slot whose player is out of lives
const OUT_OPACITY: f64 = 0.4;
const LIGHT: Rgba = [0.89627, 0.973446, 0.871367, 1.0];

/// UI_Orientation: 0 centre, 1 left, 2 right.
type Orientation = usize;

/// EHorizontalAlignment by orientation in UI_IGv3_Fixed RespondToOrientation: 0 Center, 1 Left, 2 Right.
const H_ALIGN: [HAlign; 3] = [HAlign::Center, HAlign::Left, HAlign::Right];

/// A Fixed slot whose Player Index has no fighter: UI_Broker Reset fills PlayerData with MaxPlayers (4) default
/// UI_Broker_PlayerData structs and only a fighter writes to its own (BaseFighter SetupPlayerUI, GetColor), so the
/// slot's SetupInitialValues reads the struct defaults: both colours (0, 0, 0, 0), no character, an empty username,
/// every locality flag false.
fn no_player(index: usize) -> BrokerPlayer {
    BrokerPlayer { index, ..BrokerPlayer::default() }
}

/// UPanelWidget::AddChild gives the child a fresh slot: padding 0, Fill / Fill, Automatic size.
fn readd(t: &mut Tree, parent: WidgetId, c: WidgetId) {
    t.remove(c);
    let w = t.widget_mut(c);
    w.pad = Margin::default();
    w.h_align = HAlign::Fill;
    w.v_align = VAlign::Fill;
    w.fill = 0.0;
    t.add(parent, c);
}

fn translate(t: &mut Tree, w: WidgetId, xy: [f64; 2]) {
    t.widget_mut(w).transform.translation = xy;
}

fn shear_x(t: &mut Tree, w: WidgetId, x: f64) {
    t.widget_mut(w).transform.shear[0] = x;
}

// ---------- UI_IGv3_PlayerScore (the Free For All score badge inside the portrait) ----------

#[derive(Debug)]
struct PlayerScore {
    w: WidgetId,
}

impl PlayerScore {
    fn new(t: &mut Tree, w: WidgetId, b: &BrokerPlayer, o: Orientation) -> PlayerScore {
        let crown = t.find_in(w, "Crown");
        t.dynamic_material(crown).set_vector("FillColor", super::common::c4(LIGHT));
        let s = PlayerScore { w };
        s.on_color(t, b);
        // RespondToOrientation, on the widget itself.
        shear_x(t, w, [0.0, -8.0, 8.0][o]);
        translate(t, w, [[0.0, 0.0], [0.0, 0.0], [1.0, 0.0]][o]);
        s.score(t, 0, false, false);
        s
    }

    /// OnColor
    fn on_color(&self, t: &mut Tree, b: &BrokerPlayer) {
        let c = if b.is_me { b.focus } else { b.team };
        for n in ["colorBG", "quadBG"] {
            let x = t.find_in(self.w, n);
            t.widget_mut(x).color = c;
        }
        let l = t.find_in(self.w, "scoreLabel");
        t.widget_mut(l).color = if b.is_me { [0.006049, 0.009721, 0.021219, 1.0] } else { LIGHT };
        let sh = t.find_in(self.w, "Crown_Shimmer");
        t.dynamic_material(sh).set_vector("ShimmerColor", super::common::c4(c));
    }

    /// OnScore
    fn score(&self, t: &mut Tree, score: i64, is_leader: bool, tied: bool) {
        let lead = is_leader && !tied;
        let v = vis_if(lead, Vis::HitTestInvisible, Vis::Hidden);
        for n in ["CrownContainer", "colorBG"] {
            let x = t.find_in(self.w, n);
            t.widget_mut(x).vis = v;
        }
        let l = t.find_in(self.w, "scoreLabel");
        let label = t.widget_mut(l);
        label.text = score.to_string();
        label.font_size = if lead { 40.0 } else { 28.0 };
        label.transform.translation = if lead { [0.0, 5.0] } else { [0.0, 0.0] };
    }
}

// ---------- UI_IGv3_CharacterPortrait ----------

/// TopHUD, BotHUD.
const SIDES: [usize; 2] = [0, 1];

#[derive(Debug)]
struct CharacterPortrait {
    w: WidgetId,
    o: Orientation,
    ffa: bool,
    last_score: i64,
    /// Real time OnScoreChanged started (-1: not playing) and its last position.
    score_start: f64,
    score_at: f64,
    /// LeaderAnim_TopHUD / _BotHUD while they play (start, reverse), and the real time LeaderLoop_TopHUD / _BotHUD
    /// started: two animations each, so a pref change between two scores can leave one of each side playing.
    lead: [Option<(f64, bool)>; 2],
    lead_loop: [Option<f64>; 2],
    /// Hud-position Bottom, stored by RespondToPreferences (@2983) and read by the next OnScore (@5652).
    position_bottom: bool,
}

impl CharacterPortrait {
    fn new(
        t: &mut Tree,
        w: WidgetId,
        b: &BrokerPlayer,
        o: Orientation,
        mode: u8,
        ffa: bool,
        ui: &Prefs,
    ) -> CharacterPortrait {
        let quad = t.find_in(w, "WBP_Quadrilateral");
        let portrait = t.find_in(w, "Portrait");
        let username = t.find_in(w, "usernameLabel");
        // PreConstruct (not design time): Portrait Hidden until the character's material loads.
        t.widget_mut(portrait).vis = Vis::Hidden;
        let mut p = CharacterPortrait {
            w,
            o,
            ffa,
            last_score: 0,
            score_start: -1.0,
            score_at: 0.0,
            lead: [None; 2],
            lead_loop: [None; 2],
            position_bottom: false,
        };
        p.on_color(t, b);
        // OnCharacter (@845, the fighter's HydraCharacterData from BaseFighter @154343): a valid character ->
        // LoadAsset(CharacterSelectPortraitMaterial) -> SetBrushFromMaterial, SelfHitTestInvisible.
        if let Some(m) = &b.portrait {
            let x = t.widget_mut(portrait);
            x.tex = Some(m.clone());
            x.vis = Vis::SelfHitTestInvisible;
        }
        // OnUsername
        t.widget_mut(username).text = b.username.clone();
        // RespondToOrientation
        let sc = t.find_in(w, "shearContainer");
        shear_x(t, sc, [0.0, 8.0, -8.0][o]);
        shear_x(t, portrait, [0.0, -8.0, 8.0][o]);
        translate(t, username, [[0.0, 0.0], [5.0, 45.0], [-15.0, 45.0]][o]);
        let leader = t.find_in(w, "LeaderLabel");
        translate(t, leader, [[0.0, 0.0], [-15.0, 0.0], [5.0, 0.0]][o]);
        t.widget_mut(quad).transform.scale = [[1.0, -1.0, 1.0][o], 1.0];
        p.respond_to_preferences(t, ui);
        // UI_IGv3_Fixed OnUIContextMode: the PlayerScore inset shows (HitTestInvisible) only in context 3.
        let ps = t.find_in(w, "UI_IGv3_PlayerScore");
        t.widget_mut(ps).vis = if mode == 3 { Vis::HitTestInvisible } else { Vis::Hidden };
        // OnScore from BroadcastPlayerValues (not leading, not tied).
        p.on_score(t, 0, false, false, 0.0);
        p
    }

    /// OnColor: colour bar and leader quad take the focus colour for Me, else the team colour.
    fn on_color(&self, t: &mut Tree, b: &BrokerPlayer) {
        let c = if b.is_me { b.focus } else { b.team };
        for n in ["colorBar", "WBP_Quadrilateral"] {
            let x = t.find_in(self.w, n);
            t.widget_mut(x).color = c;
        }
    }

    /// RespondToPreferences (@2983): Hud-position Bottom stored; usernameLabel by hud-showUsername (Location Fixed)
    /// -> HitTestInvisible, else Hidden (@3102).
    fn respond_to_preferences(&mut self, t: &mut Tree, ui: &Prefs) {
        self.position_bottom = ui.hud_position_bottom;
        let u = t.find_in(self.w, "usernameLabel");
        t.widget_mut(u).vis = vis_if(ui.hud_show_username, Vis::HitTestInvisible, Vis::Hidden);
    }

    /// OnScore (ubergraph @3511) for this player's index.
    fn on_score(&mut self, t: &mut Tree, score: i64, is_leader: bool, tied: bool, real: f64) {
        let w = self.w;
        // @7105: a changed score plays OnScoreChanged at rate 1.75.
        if score != self.last_score {
            self.last_score = score;
            self.score_start = real;
            self.score_at = 0.0;
        }
        // @4278: the leader quad shows for a sole leader; the username is larger and lifted for any leader, with a
        // 2 px half-black outline either way.
        let q = t.find_in(w, "WBP_Quadrilateral");
        t.widget_mut(q).vis = vis_if(is_leader && !tied, Vis::SelfHitTestInvisible, Vis::Collapsed);
        let u = t.find_in(w, "usernameLabel");
        let username = t.widget_mut(u);
        username.font_size = if is_leader { 24.0 } else { 20.0 };
        username.pad = Margin { l: 0.0, t: 0.0, r: 0.0, b: if is_leader { -15.0 } else { 0.0 } };
        username.outline = 2.0;
        username.outline_color = [0.0, 0.0, 0.0, 0.5];
        username.outline_shadows = false;
        let leader = t.find_in(w, "LeaderLabel");
        // Free For All only (team style 2): LeaderAnim forward for a leader, in reverse otherwise, on every call;
        // LeaderLoop started (endless) or stopped; a bottom HUD also shifts TranslateContainer.
        if !self.ffa {
            // BroadcastPlayerValues' reverse play has long ended when the scoreboard enters: its first frame.
            let o = t.find_in(w, "Overlay_0");
            t.widget_mut(o).transform.translation[1] = 0.0;
            t.widget_mut(leader).vis = Vis::Collapsed;
            return;
        }
        // @5652: the side by the stored Hud-position Bottom; PlayAnimation(LeaderAnim_<side>) and LeaderLoop_<side>
        // played or StopAnimation'd (@5989 / @6427, @7029 / @7081): the other side's are left as they are.
        let side = usize::from(self.position_bottom);
        self.lead[side] = Some((real, !is_leader));
        self.lead_loop[side] = if is_leader { Some(real) } else { None };
        if self.position_bottom {
            let tc = t.find_in(w, "TranslateContainer");
            let xy = if is_leader {
                [[0.0, 0.0], [-25.0, 0.0], [15.0, 0.0]]
            } else {
                [[0.0, 0.0], [5.0, 0.0], [-15.0, 0.0]]
            };
            translate(t, tc, xy[self.o]);
        }
        t.widget_mut(leader).vis = vis_if(is_leader, Vis::SelfHitTestInvisible, Vis::Collapsed);
    }

    /// Widget animations on real time.
    fn tick(&mut self, t: &mut Tree, a: &Res, real: f64) {
        if self.score_start >= 0.0 {
            let an = &a.portrait_score;
            let s = ((real - self.score_start) * 1.75).min(an.end());
            an.apply(t, self.w, s, self.score_at);
            self.score_at = s;
            if s >= an.end() {
                self.score_start = -1.0;
            }
        }
        // LeaderAnim and LeaderLoop both drive Overlay_0's translation; while both play the float blender averages
        // them (absolute sections of weight 1 in one linker, exe 142536370). A reversed play starts from the last
        // valid frame (UUMGSequencePlayer::PlayInternal, exe 1425e7110), so a fighter who is not leading drops to the
        // leader offset and returns on every score update. A finished player keeps its last state.
        let overlay = t.find_in(self.w, "Overlay_0");
        let (mut sum, mut n) = (0.0, 0);
        for side in SIDES {
            let Some((start, reverse)) = self.lead[side] else { continue };
            let an = &a.portrait_lead[side];
            let s = (real - start).min(an.end());
            an.at(t, self.w, if reverse { an.end() - s } else { s });
            sum += t.widget(overlay).transform.translation[1];
            n += 1;
            if s >= an.end() {
                self.lead[side] = None;
            }
        }
        for side in SIDES {
            let Some(start) = self.lead_loop[side] else { continue };
            let an = &a.portrait_loop[side];
            an.at(t, self.w, (real - start) % an.end());
            sum += t.widget(overlay).transform.translation[1];
            n += 1;
        }
        if n > 0 {
            t.widget_mut(overlay).transform.translation[1] = sum / f64::from(n);
        }
    }
}

// ---------- UI_IGv3_Fixed ----------

/// A scoreboard slot. Its broker-aware children are bound to the slot's Player Index (UI_IGv3_Base Init), so the
/// broker's percentage, grey percentage and evade calls for that player reach them as they reach the player's
/// attached widget (the HUD feeds both); whether they show is the hud-* preferences'.
#[derive(Debug)]
pub struct Fixed {
    pub w: WidgetId,
    score: PlayerScore,
    portrait: CharacterPortrait,
    percentage: Percentage,
    flame: Flame,
    meter: DodgeMeter,
    cooldowns: Cooldowns,
    o: Orientation,
    /// Hud-position Bottom, stored by RespondToPreferences (@4010 -> @4056) and read by the next OnScore.
    position_bottom: bool,
}

impl Fixed {
    /// abilities: the player's Comp_Pawn_AbilityManager Abilities (none for a slot without a player).
    #[allow(clippy::too_many_arguments)]
    fn new(
        t: &mut Tree,
        w: WidgetId,
        b: &BrokerPlayer,
        o: Orientation,
        mode: u8,
        ui: &Prefs,
        abilities: &[HudAbility],
        names: &PlayerNumbers,
    ) -> Fixed {
        // Location Fixed, copied to every child by UnifyChildren; Orient Children passes the orientation down.
        let pw = t.find_in(w, "UI_IGv3_Percentage");
        let mut percentage = Percentage::new(t, pw, LOCATION_FIXED, ui, mode == 5);
        let fw = t.find_in(w, "UI_IGv3_Flame");
        let flame = Flame::new(t, fw, b, LOCATION_FIXED, ui);
        let mw = t.find_in(w, "UI_IGv3_DodgeMeter");
        let mut meter = DodgeMeter::new(t, mw, 10.0, 120.0, false, true, LOCATION_FIXED, ui);
        let cw = t.find_in(w, "UI_IGv3_Cooldowns");
        let cooldowns = Cooldowns::new(t, cw, b, abilities, LOCATION_FIXED, ui, mode == 5);
        let portrait_w = t.find_in(w, "UI_IGv3_CharacterPortrait");
        let portrait = CharacterPortrait::new(t, portrait_w, b, o, mode, mode == 3, ui);
        let sw = t.find_in(portrait_w, "UI_IGv3_PlayerScore");
        let score = PlayerScore::new(t, sw, b, o);
        orient(t, w, o);
        // OnColor of the percentage and the meter (the flame's is in its constructor), then SetupInitialValues:
        // OnPercent(0), OnEvadeDecay(1).
        percentage.on_color(t, b.team, b.focus, b.is_me);
        meter.on_color(t, b.team);
        meter.set_player(t, &names.local_play_username(b.index));
        percentage.set_percentage(t, 0, 0.0, 0.0);
        flame.on_percentage(t, 0);
        meter.on_evade_decay(t, 1.0, -1.0);
        Fixed { w, score, portrait, percentage, flame, meter, cooldowns, o, position_bottom: ui.hud_position_bottom }
    }

    /// (the port's own) The stock rule: the portrait's score badge shown in any mode, with the player's lives.
    fn show_lives(&self, t: &mut Tree, lives: i64) {
        t.widget_mut(self.score.w).vis = Vis::HitTestInvisible;
        self.score.score(t, lives, false, false);
    }

    /// OnScore (@3107; the broker's score for this Player Index: Free For All): Location Fixed, by orientation, a
    /// sole leader's infoContainer at (30, 50) on the left and (-30, 50) on the right with a top HUD, else (0, 0)
    /// (@3364 / @3687); an unoriented slot keeps it.
    fn on_score(&self, t: &mut Tree, is_leader: bool, tied: bool) {
        if self.o == 0 {
            return;
        }
        let up = is_leader && !tied && !self.position_bottom;
        let info = t.find_in(self.w, "infoContainer");
        translate(t, info, if up { [if self.o == 1 { 30.0 } else { -30.0 }, 50.0] } else { [0.0, 0.0] });
    }

    /// The broker's calls for the slot's player (UI_Broker SetPercentageForPlayer, SetGreyPercentageForPlayer,
    /// SetEvadeDecayForPlayer), as AttachedBottom takes them.
    fn set_percentage(&mut self, t: &mut Tree, p: i32, now: f64, real: f64) {
        self.percentage.set_percentage(t, p, now, real);
        self.flame.on_percentage(t, p);
    }

    fn tick(&mut self, t: &mut Tree, a: &Res, now: f64, real: f64) {
        self.portrait.tick(t, a, real);
        self.cooldowns.tick(t, a, now, real);
        self.percentage.tick(t, a, now, real);
        self.meter.tick(t, now);
    }
}

/// UI_IGv3_Fixed RespondToOrientation.
fn orient(t: &mut Tree, w: WidgetId, o: Orientation) {
    let main = t.find_in(w, "mainContainer");
    let portrait = t.find_in(w, "portraitContainer");
    let info = t.find_in(w, "infoContainer");
    t.remove(portrait);
    t.remove(info);
    if o == 1 {
        readd(t, main, portrait);
        readd(t, main, info);
    } else if o == 2 {
        readd(t, main, info);
        readd(t, main, portrait);
    }
    t.widget_mut(portrait).pad = Margin { l: 35.0, t: 0.0, r: 25.0, b: 0.0 };
    let pct = t.find_in(w, "UI_IGv3_Percentage");
    let cds = t.find_in(w, "UI_IGv3_Cooldowns");
    let flame = t.find_in(w, "UI_IGv3_Flame");
    let meter = t.find_in(w, "UI_IGv3_DodgeMeter");
    for c in [pct, cds, flame, meter] {
        t.widget_mut(c).h_align = H_ALIGN[o];
    }
    translate(t, pct, [0.0, -25.0]);
    translate(t, flame, [[0.0, 0.0], [-35.0, -55.0], [20.0, -55.0]][o]);
    translate(t, cds, [[0.0, 0.0], [5.0, 0.0], [5.0, 0.0]][o]);
    // UI_IGv3_Cooldowns has Orient Children: each UI_IGv3_Cooldown's RespondToOrientation shears its frame (@5094:
    // 0 -> (0, 0), 1 -> (10, 0), 2 -> (-10, 0)).
    for i in 0..6 {
        let c = t.find_in(cds, &format!("UI_IGv3_Cooldown{i}"));
        let f = t.find_in(c, "frameContainer");
        shear_x(t, f, [0.0, 10.0, -10.0][o]);
    }
    translate(t, meter, [[0.0, 10.0], [20.0, 10.0], [-10.0, 10.0]][o]);
    shear_x(t, meter, [0.0, 5.0, -5.0][o]);
}

// ---------- UI_IGv3_ScorePip ----------

#[derive(Debug)]
struct ScorePip {
    w: WidgetId,
    is_on: bool,
    /// Real time ScoreAnim started (-1: not playing) and its last position.
    anim_start: f64,
    anim_at: f64,
}

impl ScorePip {
    fn new(t: &mut Tree, b: &Bundle, parent: WidgetId, name: &str, color: Rgba) -> ScorePip {
        let w = t.instantiate_in(b, parent, "UI_IGv3_ScorePip", name);
        let p = ScorePip { w, is_on: false, anim_start: -1.0, anim_at: 0.0 };
        p.set_color(t, color);
        p
    }

    /// OnColorForTeam (@810-851)
    fn set_color(&self, t: &mut Tree, color: Rgba) {
        for n in ["pipSlot", "pip"] {
            let x = t.find_in(self.w, n);
            t.widget_mut(x).color = color;
        }
    }

    fn toggle(&mut self, t: &mut Tree, on: bool, real: f64) {
        // ScoreAnim forward at play rate 1.25 when the pip turns on.
        if on && !self.is_on {
            self.anim_start = real;
            self.anim_at = 0.0;
        }
        self.is_on = on;
        let v = if on { Vis::Visible } else { Vis::Hidden };
        for n in ["pip", "pipRing"] {
            let x = t.find_in(self.w, n);
            t.widget_mut(x).vis = v;
        }
        let c = t.find_in(self.w, "contentContainer");
        let s = if on { 1.0 } else { 0.6 };
        t.widget_mut(c).transform.scale = [s, s];
        let ring = t.find_in(self.w, "pipRing");
        t.dynamic_material(ring);
        let empty = t.find_in(self.w, "pipEmpty");
        t.dynamic_material(empty).set_scalar("Thickness", if on { 2.25 } else { 1.75 });
    }

    fn tick(&mut self, t: &mut Tree, a: &Anim, real: f64) {
        if self.anim_start < 0.0 {
            return;
        }
        let s = ((real - self.anim_start) * 1.25).min(a.end());
        a.apply(t, self.w, s, self.anim_at);
        self.anim_at = s;
        if s >= a.end() {
            self.anim_start = -1.0;
        }
    }
}

// ---------- UI_IGv3_ScorePips ----------

#[derive(Debug)]
struct ScorePips {
    w: WidgetId,
    pips: Vec<ScorePip>,
    max_score: i32,
    color: Rgba,
}

impl ScorePips {
    fn new(t: &mut Tree, w: WidgetId, color: Rgba, ui: &Prefs) -> ScorePips {
        // RespondToPreferences (@2137): the warning indicator sits 55 below a top HUD, 55 above a bottom one.
        let wi = t.find_in(w, "warningIndicator");
        translate(t, wi, [0.0, if ui.hud_position_bottom { -55.0 } else { 55.0 }]);
        let sc = t.find_in(w, "scoreContainer");
        for k in t.widget(sc).children.clone() {
            t.remove(k);
        }
        ScorePips { w, pips: Vec::new(), max_score: 0, color }
    }

    /// OnMaxScoreForTeam: one pip per ring-out to win.
    fn set_max(&mut self, t: &mut Tree, b: &Bundle, n: i32) {
        let sc = t.find_in(self.w, "scoreContainer");
        for k in t.widget(sc).children.clone() {
            t.remove(k);
        }
        self.max_score = n;
        self.pips = (0..n).map(|i| ScorePip::new(t, b, sc, &format!("UI_IGv3_ScorePip_{i}"), self.color)).collect();
    }

    /// OnScoreForTeam
    fn set_score(&mut self, t: &mut Tree, score: i32, real: f64) {
        let wi = t.find_in(self.w, "warningIndicator");
        t.widget_mut(wi).vis = vis_if(score == self.max_score - 1, Vis::Visible, Vis::Hidden);
        for (i, p) in self.pips.iter_mut().enumerate() {
            p.toggle(t, score > i as i32, real);
        }
    }

    fn tick(&mut self, t: &mut Tree, a: &Res, real: f64) {
        // Construct: WarningAnim loops for the widget's life (real time from the HUD's creation at 0).
        let wa = &a.pips_warning;
        wa.at(t, self.w, real % wa.end());
        for p in &mut self.pips {
            p.tick(t, &a.pip_score, real);
        }
    }
}

// ---------- UI_IGv3_Clock ----------

/// CDO Warning Time
const WARNING_TIME: f64 = 10.0;

#[derive(Debug)]
struct Clock {
    w: WidgetId,
    label: WidgetId,
    mode: u8,
    threshold: f64,
    /// Real time WarningAnim started (-1: not playing).
    anim_start: f64,
}

impl Clock {
    fn new(t: &mut Tree, w: WidgetId, mode: u8) -> Clock {
        // OnUIContextMode: HitTestInvisible in contexts 1 to 5.
        t.widget_mut(w).vis = Vis::HitTestInvisible;
        let label = t.find_in(w, "timeLabel");
        t.widget_mut(label).text = "\u{221e}".to_string();
        Clock { w, label, mode, threshold: WARNING_TIME, anim_start: -1.0 }
    }

    /// UpdateMatchTime, every widget tick. remaining = the game state's RoundTimeRemaining.
    fn tick(&mut self, t: &mut Tree, a: &Res, remaining: f64, real: f64, sounds: &mut Vec<String>) {
        // Contexts 4 and 5 write the infinity sign (@428 -> @564) and leave the warning alone. (The port's own Waves
        // context writes the run's clock instead: Scoreboard::waves.)
        if self.mode == 4 || self.mode == 5 || self.mode == MODE_WAVES {
            return;
        }
        let r = remaining.max(0.0);
        t.widget_mut(self.label).text = clock_text(r);
        if r > WARNING_TIME {
            // Set Defaults
            self.threshold = WARNING_TIME;
            t.widget_mut(self.label).color = LIGHT;
        } else if r < self.threshold {
            self.threshold = r.ceil() - 1.0;
            self.anim_start = real;
            sounds.push("ui_amount_subtract".to_string());
        }
        if self.anim_start >= 0.0 {
            let an = &a.clock_warning;
            let s = (real - self.anim_start).min(an.end());
            an.at(t, self.w, s);
            if s >= an.end() {
                self.anim_start = -1.0;
            }
        }
    }
}

/// The clock's "m:ss" of a time left, seconds.
fn clock_text(r: f64) -> String {
    let sec = (r % 60.0).floor();
    format!("{}:{:02}", (r / 60.0).floor(), sec)
}

// ---------- UI_IGv3_Scoreboard ----------

/// What the scoreboard needs from the match.
#[derive(Clone, Debug)]
pub struct ScoreboardSetup {
    pub mode: u8,
    /// Each team row's MaxScore (KnockoutsToWin).
    pub max_score: i32,
    /// Broker team data colour, teams 0 and 1 (UI_Game Update Team Colors).
    pub team_colors: [Rgba; 2],
    /// The experimental stock rule: max_score lives per player, on each player's score badge; no pips.
    pub stocks: bool,
    /// GetLocalPlayUsername's texts.
    pub names: PlayerNumbers,
    /// UI_IGv3_MatchInfo OnUIContextMode's modeLabel text for the mode.
    pub mode_label: String,
    /// An arcade round's modeLabel text ("Stage N of M"), shown in contexts 1 to 3.
    pub stage_label: Option<String>,
    /// Each player's team, by player index.
    pub teams: Vec<u8>,
}

/// (the port's own) Whether a match takes the big matches' scoreboard (`big_sides`): more than four players, or a
/// Versus match whose teams are not the build's alternation (team = player index % 2: an uneven 3v1).
fn big_layout(mode: u8, teams: &[u8]) -> bool {
    (mode == 2 || mode == 3)
        && (teams.len() > 4 || (mode == 2 && teams.iter().enumerate().any(|(i, &t)| usize::from(t) != i % 2)))
}

/// (the port's own) The big matches' scoreboard sides, player indexes left to right: Versus team 0 on the left and
/// team 1 on the right, Free For All the even indexes on the left and the odd on the right (the build's 2v2 and Free
/// For All put players 0 and 2 left, 1 and 3 right).
fn big_sides(mode: u8, teams: &[u8]) -> [Vec<usize>; 2] {
    let side = |i: usize| if mode == 3 { i % 2 } else { usize::from(teams[i] != 0) };
    [0, 1].map(|k| (0..teams.len()).filter(|&i| side(i) == k).collect())
}

/// (the port's own) The scale of every big-match slot: a side of `per_side` slots as wide as the build's side of two.
/// `portrait_only`: the slots show no percentage and no cooldowns (the default preferences), so their infoContainer
/// is collapsed and a slot is its portrait alone: a side of four at the build's size of a portrait.
fn big_scale(per_side: usize, portrait_only: bool) -> f64 {
    let fit = if portrait_only { 4.0 } else { 2.0 };
    (fit / per_side.max(1) as f64).min(1.0)
}

/// UI_IGv3_Scoreboard.
#[derive(Debug)]
pub struct Scoreboard {
    pub t: Tree,
    mode: u8,
    main: WidgetId,
    /// Game time of AnimateIn.
    enter_at: f64,
    /// Real time EnterAnim / ExitAnim started (-1: not yet).
    anim_start: f64,
    exit_start: f64,
    pips: [ScorePips; 2],
    clock: Clock,
    /// UI_IGv3_MatchInfo's modeLabel.
    mode_label: WidgetId,
    /// By Player Index (at least four: the build's slots without a player).
    slots: Vec<Option<Fixed>>,
    /// UI_IGv3_PauseHold: its widget, Meter and its Pause Hold Value.
    pause_hold_w: WidgetId,
    pause_hold_meter: WidgetId,
    pause_hold_value: f64,
}

impl Scoreboard {
    /// players: the broker's player data by player index; abilities: each player's abilities.
    pub fn new(
        b: &Bundle,
        players: &[BrokerPlayer],
        now: f64,
        setup: &ScoreboardSetup,
        ui: &Prefs,
        abilities: &[Vec<HudAbility>],
    ) -> Scoreboard {
        let mode = setup.mode;
        let mut t = super::attached::proto(b, "UI_IGv3_Scoreboard", "Scoreboard");
        let t_ = &mut t;
        let main = t_.find("mainContainer");
        // Construct: mainContainer Hidden until AnimateIn.
        t_.widget_mut(main).vis = Vis::Hidden;
        // LayoutUIForMode. Mode 1: FixedA left, FixedC right. Modes 2 and 3: FixedA, FixedB left and FixedC, FixedD
        // right; Free For All pads FixedA 40 on the right and FixedD 40 on the left and hides both pip rows. Mode 4
        // goes by the Lab's occupied slots S (@5217): up to two, S[0] on the left and S[1] on the right with the
        // slot's widget from {0: A, 1: C, 2: B, 3: D} (@7872); more than two, S[0] and S[1] on the left and S[2] and
        // S[3] on the right from {0: A, 1: B, 2: C, 3: D} (@5490-7871). Pip rows Hidden. The pips and the info
        // overlay go to centerContainer. Assign Player Indexes: FixedA = player 0, FixedB = 2, FixedC = 1,
        // FixedD = 3. A Lab player's index is its config slot (TrainGameMode.PopulatePlayerDataManager @514).
        // Unverified: the native match has no Lab config slots: the occupied slots are the players' indexes.
        let left = t_.find("leftContainer");
        let center = t_.find("centerContainer");
        let right = t_.find("rightContainer");
        let [fa, fb, fc, fd] =
            ["UI_IGv3_FixedA", "UI_IGv3_FixedB", "UI_IGv3_FixedC", "UI_IGv3_FixedD"].map(|n| t_.find(n));
        let pips_l = t_.find("UI_IGv3_ScorePipsLeft");
        let pips_r = t_.find("UI_IGv3_ScorePipsRight");
        let info = t_.find("infoContainer");
        // (found before the containers are cleared: context 5 leaves infoContainer out of the tree)
        let clock_w = t_.find("UI_IGv3_Clock");
        let mi = t_.find("UI_IGv3_MatchInfo");
        let pause_hold_w = t_.find("UI_IGv3_PauseHold");
        for c in [left, center, right] {
            for k in t_.widget(c).children.clone() {
                t_.remove(k);
            }
            t_.widget_mut(c).vis = Vis::HitTestInvisible;
        }
        let four = mode == 2 || mode == 3;
        let occupied: Vec<usize> = (0..players.len()).collect();
        let index_of = |w: WidgetId| {
            if w == fa {
                0
            } else if w == fb {
                2
            } else if w == fc {
                1
            } else {
                3
            }
        };
        let mut place: Vec<(WidgetId, usize, Orientation)> = Vec::new();
        let mut put = |t: &mut Tree, c: WidgetId, w: Option<WidgetId>, o: Orientation| {
            if let Some(w) = w {
                readd(t, c, w);
                place.push((w, index_of(w), o));
            }
        };
        let many = occupied.len() > 2;
        let by_slot = if many { [fa, fb, fc, fd] } else { [fa, fc, fb, fd] };
        let slot_w = |k: usize| occupied.get(k).and_then(|&s| by_slot.get(s).copied());
        // Deviation: a 2v2 match of three (the port's Versus lobby plays 2 against 1 as a 2v2) leaves out the slot of
        // the player index no player has: the side of one shows its one slot, as a 1v1's side. The build's 2v2
        // always adds all four (below).
        let held = |i: usize| mode != 2 || i < players.len();
        // Deviation: a match of more than four (the port's big matches) or an uneven Versus split has no layout in
        // the build: each side's slots in one row, every slot the build's widget in a ScaleBox of `big_scale`, so a
        // side of three or four is as wide as the build's side of two. Free For All keeps the build's gap of 40
        // between a side's slots (FixedA's and FixedD's padding), scaled. A slot that shows no percentage and no
        // cooldowns there collapses its infoContainer (the build keeps its width empty), so the portraits stay large.
        let big = big_layout(mode, &setup.teams);
        let mut big_place = Vec::new();
        if big {
            let sides = big_sides(mode, &setup.teams);
            let portrait_only = !ui.hud_show_percentage && !ui.hud_show_cooldowns;
            let k = big_scale(sides[0].len().max(sides[1].len()), portrait_only);
            for (side, list) in sides.iter().enumerate() {
                let (c, o) = if side == 0 { (left, 1) } else { (right, 2) };
                for (j, &i) in list.iter().enumerate() {
                    let fixed = match i {
                        0 => fa,
                        1 => fc,
                        2 => fb,
                        3 => fd,
                        _ => t_.instantiate_in(b, c, "UI_IGv3_Fixed", &format!("UI_IGv3_Fixed{i}")),
                    };
                    if portrait_only {
                        let info = t_.find_in(fixed, "infoContainer");
                        t_.widget_mut(info).vis = Vis::Collapsed;
                    }
                    let sb = t_.create_plain(b, "ScaleBox", &format!("BigSlot{i}"));
                    readd(t_, c, sb);
                    readd(t_, sb, fixed);
                    let w = t_.widget_mut(sb);
                    w.stretch = Stretch::UserSpecified;
                    w.user_scale = k;
                    // (Automatic: each slot at its scaled width and the gap exact; the side is never wider than its
                    // container)
                    w.v_align = VAlign::Center;
                    let gap = 40.0 * k;
                    if mode == 3 && side == 0 && j + 1 < list.len() {
                        w.pad.r = gap;
                    } else if mode == 3 && side == 1 && j > 0 {
                        w.pad.l = gap;
                    }
                    big_place.push((fixed, i, o));
                }
            }
        } else if mode == 4 {
            put(t_, left, slot_w(0), 1);
            if many {
                put(t_, left, slot_w(1), 1);
            }
        } else if mode != 5 {
            // (the port's own Waves context: the player's slot alone, FixedA on the left, as the Lab's one player)
            put(t_, left, Some(fa), 1);
            if four && held(2) {
                put(t_, left, Some(fb), 1);
            }
        }
        // (context 5 has no case in LayoutUIForMode @2793-3687: the cleared containers stay empty)
        if mode != 5 {
            readd(t_, center, pips_l);
            readd(t_, center, info);
            readd(t_, center, pips_r);
        }
        if big {
            // (placed above)
        } else if mode == 4 {
            if many {
                put(t_, right, slot_w(2), 2);
                put(t_, right, slot_w(3), 2);
            } else {
                put(t_, right, slot_w(1), 2);
            }
        } else if mode != 5 && mode != MODE_WAVES {
            put(t_, right, Some(fc), 2);
            if four && held(3) {
                put(t_, right, Some(fd), 2);
            }
        }
        if mode == 3 && !big {
            t_.widget_mut(fa).pad = Margin { l: 0.0, t: 0.0, r: 40.0, b: 0.0 };
            t_.widget_mut(fd).pad = Margin { l: 40.0, t: 0.0, r: 0.0, b: 0.0 };
        }
        if mode >= 3 {
            t_.widget_mut(pips_l).vis = Vis::Hidden;
            t_.widget_mut(pips_r).vis = Vis::Hidden;
        }
        // The centre / fill loop runs after the mode's block (@2873 pushes the block first, then @1418), so the
        // block's own HAlign Right / Left on FixedA, FixedC and FixedD does not survive it.
        let all = [fa, fb, fc, fd, pips_l, pips_r, info];
        for &c in &all[if big { 4 } else { 0 }..] {
            let w = t_.widget_mut(c);
            w.h_align = HAlign::Center;
            w.v_align = VAlign::Center;
            w.fill = 1.0;
        }
        place.append(&mut big_place);
        let mut slots: Vec<Option<Fixed>> = (0..players.len().max(4)).map(|_| None).collect();
        for (w, i, o) in place {
            // A slot without a player (Free For All with fewer than four) keeps its place in the row and is
            // initialised like any other: LayoutUIForMode adds all four, Init(Broker, Player Index) runs on each
            // (Scoreboard @57-285) and no widget of the slot tests IsActive or the player count.
            let p = players.get(i).cloned().unwrap_or_else(|| no_player(i));
            let f = Fixed::new(t_, w, &p, o, mode, ui, abilities.get(i).map_or(&[][..], Vec::as_slice), &setup.names);
            // (the port's own) the stock rule: the Free For All score badge in every mode, with the player's lives
            if setup.stocks && i < players.len() {
                f.show_lives(t_, i64::from(setup.max_score));
            }
            slots[i] = Some(f);
        }
        // ScorePips (Team Index 0 left, 1 right). The match start sends Set Max Score for Team(KnockoutsToWin) and
        // SetScoreForTeam(0) (PFGPlayerController @141097); the Lab never sets a max score (KnockoutsToWin -1), so
        // no pips there. The Hidden warning indicator keeps its 50 x 50.
        let mut pips = [
            ScorePips::new(t_, pips_l, setup.team_colors[0], ui),
            ScorePips::new(t_, pips_r, setup.team_colors[1], ui),
        ];
        for p in &mut pips {
            p.set_max(t_, b, if mode == 4 || setup.stocks { 0 } else { setup.max_score.max(0) });
            if mode != 4 {
                p.set_score(t_, 0, 0.0);
            }
        }
        if mode == 4 || setup.stocks {
            for p in [pips_l, pips_r] {
                let wi = t_.find_in(p, "warningIndicator");
                t_.widget_mut(wi).vis = Vis::Hidden;
            }
        }
        // UI_IGv3_Clock: UpdateMatchTime on every tick.
        let clock = Clock::new(t_, clock_w, mode);
        // UI_IGv3_MatchInfo OnUIContextMode: the mode's name goes into modeLabel. Context 4 leaves modeLabel Hidden
        // and makes the widget SelfHitTestInvisible; contexts 1 to 3 collapse modeLabel outside the arcade.
        // (context 5 has no case @85: the label empty, the widget as the tree has it)
        // Deviation: the native match has no queue-set rematch yet: its MatchInfo form (rematchContainer's set
        // score) is not ported; rematchContainer stays Hidden as outside one (and as in an arcade round, @1966).
        let label = t_.find_in(mi, "modeLabel");
        t_.widget_mut(label).text = setup.mode_label.clone();
        if mode == 4 {
            t_.widget_mut(mi).vis = Vis::SelfHitTestInvisible;
        } else if mode == MODE_WAVES {
            // (the port's own) the run's counter, written each frame by `waves`, where an arcade round has its
            // stage line
            t_.widget_mut(label).vis = Vis::SelfHitTestInvisible;
        } else if let (true, Some(s)) = ((1..4).contains(&mode), &setup.stage_label) {
            // In arcade (IsArcadeModeActive @387): modeLabel = "Stage {Number} of {NumRounds}" (@978, @1084),
            // SelfHitTestInvisible (@1129).
            // Unverified: an arcade match is read as UI context 1 (its 1v1 team style); the native that turns the
            // arcade's match config into the context is not read.
            let w = t_.widget_mut(label);
            w.text.clone_from(s);
            w.vis = Vis::SelfHitTestInvisible;
        } else if mode != 5 {
            t_.widget_mut(label).vis = Vis::Collapsed;
        }
        let rc = t_.find_in(mi, "rematchContainer");
        t_.widget_mut(rc).vis = Vis::Hidden;
        // UI_IGv3_Scoreboard RespondToPreferences (@2221): positionContainer's vertical alignment Top, Bottom with
        // hud-positionBottom. UI_IGv3_MatchInfo's (@2556): modeLabel and rematchContainer at Y -50, 50 with it.
        let pc = t_.find("positionContainer");
        t_.widget_mut(pc).v_align = if ui.hud_position_bottom { VAlign::Bottom } else { VAlign::Top };
        for x in [label, rc] {
            translate(t_, x, [0.0, if ui.hud_position_bottom { 50.0 } else { -50.0 }]);
        }
        // UI_IGv3_PauseHold OnInitialized (@10-71): Material = Meter.GetDynamicMaterial(), SetRenderOpacity(0).
        let meter = t_.find_in(pause_hold_w, "Meter");
        t_.dynamic_material(meter);
        t_.widget_mut(pause_hold_w).opacity = 0.0;
        // Enter: contexts 1 to 3 wait 8.0 s, context 4 1.0 s, context 5 2.0 s (the delay table @1258) on a world
        // timer, then AnimateIn. (The port's own Waves context has no versus card: the Lab's delay.)
        let enter_at = now
            + if mode == 4 || mode == MODE_WAVES {
                1.0
            } else if mode == 5 {
                2.0
            } else {
                8.0
            };
        Scoreboard {
            t,
            mode,
            main,
            enter_at,
            anim_start: -1.0,
            exit_start: -1.0,
            pips,
            clock,
            mode_label: label,
            slots,
            pause_hold_w,
            pause_hold_meter: meter,
            pause_hold_value: 0.0,
        }
    }

    /// Broker SetScoreForTeam (1v1, 2v2) or SetScoreForPlayer(team index used as the player index, Free For All).
    pub fn score(&mut self, team: usize, score: i32, is_leader: bool, tied: bool, real: f64) {
        let t = &mut self.t;
        // (the port's own Waves context shows no score: its counter is the run's)
        if self.mode == MODE_WAVES {
            return;
        }
        if self.mode == 3 {
            if let Some(s) = self.slots.get_mut(team).and_then(Option::as_mut) {
                s.score.score(t, i64::from(score), is_leader, tied);
                s.portrait.on_score(t, i64::from(score), is_leader, tied, real);
                s.on_score(t, is_leader, tied);
            }
        } else if self.mode != 4
            && let Some(p) = self.pips.get_mut(team)
        {
            p.set_score(t, score, real);
        }
    }

    /// A player's slot widget and its number of cooldowns.
    #[cfg(test)]
    pub(crate) fn slot_parts(&self, player: usize) -> Option<(WidgetId, usize)> {
        self.slots.get(player)?.as_ref().map(|s| (s.w, s.cooldowns.len()))
    }

    /// (the port's own) A player's scoreboard portrait as last painted, in device pixels of the HUD canvas: x, y, w, h
    /// (the match chat's bubbles stand at it). None: no slot, not shown, or not painted yet.
    pub fn slot_rect(&self, player: usize) -> Option<[f64; 4]> {
        let s = self.slots.get(player).and_then(Option::as_ref)?;
        if !self.t.widget(s.w).shown() {
            return None;
        }
        let p = self.t.find_in(s.w, "portraitContainer");
        let m = self.t.painted(p)?;
        Some([m.transform[4], m.transform[5], m.size[0] * m.transform[0], m.size[1] * m.transform[3]])
    }

    /// (the port's own) A Waves run's counter in modeLabel and its clock in the clock's label, each frame; the clock's
    /// digits even, so the centred clock does not shake as its hundredths change.
    pub fn waves(&mut self, counter: &str, clock: &str) {
        for (w, s) in [(self.mode_label, counter), (self.clock.label, clock)] {
            let x = self.t.widget_mut(w);
            x.even_digits = w == self.clock.label;
            if x.text != s {
                x.text = s.to_owned();
            }
        }
    }

    /// (the port's own) The clock as last painted, in device pixels of the HUD canvas (x, y, w, h), and the
    /// scoreboard's opacity: the Chaos Wheel's card stands under it. None before AnimateIn, once the board has left,
    /// or before its first paint.
    pub fn clock_place(&self) -> Option<([f64; 4], f64, f64)> {
        let r = self.t.root();
        if self.t.widget(self.main).vis == Vis::Hidden || !self.t.widget(r).shown() {
            return None;
        }
        let m = self.t.painted(self.clock.w)?;
        let rect = [m.transform[4], m.transform[5], m.size[0] * m.transform[0], m.size[1] * m.transform[3]];
        Some((rect, self.t.widget(r).opacity * self.t.widget(self.main).opacity, m.transform[0]))
    }

    /// (the port's own) The clock's label and modeLabel, whose fonts and colours the Chaos Wheel's card takes.
    pub fn text_styles(&self) -> (&crate::slate::Widget, &crate::slate::Widget) {
        (self.t.widget(self.clock.label), self.t.widget(self.mode_label))
    }

    /// (the port's own) A player slot's cooldowns made again for the abilities of the fighter that stands for the
    /// player now: a Waves run's player is a mode pawn, bound after the scoreboard was made.
    pub fn set_abilities(&mut self, player: usize, b: &BrokerPlayer, abilities: &[HudAbility], ui: &Prefs) {
        let mode = self.mode;
        let Some(s) = self.slots.get_mut(player).and_then(Option::as_mut) else { return };
        let cw = self.t.find_in(s.w, "UI_IGv3_Cooldowns");
        s.cooldowns = Cooldowns::new(&mut self.t, cw, b, abilities, LOCATION_FIXED, ui, mode == 5);
    }

    /// The stock rule's lives of a player (its index); a player out of them is faded.
    pub fn stocks(&mut self, player: usize, left: i64) {
        let Some(s) = self.slots.get(player).and_then(Option::as_ref) else { return };
        s.score.score(&mut self.t, left, false, false);
        if left <= 0 {
            self.t.widget_mut(s.w).opacity = OUT_OPACITY;
        } else {
            // (the port's own Chaos Wheel's Body Swap hands a slot lives again)
            self.t.widget_mut(s.w).opacity = 1.0;
        }
    }

    /// (the port's own) The Chaos Wheel's Body Swap: a fighter's slot shows the player driving it (the colours,
    /// username and number; the fighter's portrait, percentage and cooldowns stay).
    pub fn rebind(&mut self, player: usize, b: &BrokerPlayer, names: &PlayerNumbers) {
        let Some(s) = self.slots.get_mut(player).and_then(Option::as_mut) else { return };
        let t = &mut self.t;
        s.portrait.on_color(t, b);
        let u = t.find_in(s.portrait.w, "usernameLabel");
        t.widget_mut(u).text = b.username.clone();
        s.score.on_color(t, b);
        s.percentage.on_color(t, b.team, b.focus, b.is_me);
        s.meter.on_color(t, b.team);
        s.meter.set_player(t, &names.local_play_username(b.index));
    }

    /// The broker's percentage, grey percentage and evade calls for a player's slot.
    pub fn set_percentage(&mut self, player: usize, p: i32, now: f64, real: f64) {
        if let Some(s) = self.slots.get_mut(player).and_then(Option::as_mut) {
            s.set_percentage(&mut self.t, p, now, real);
        }
    }

    pub fn set_grey_percentage(&mut self, player: usize, g: i32, real: f64) {
        if let Some(s) = self.slots.get_mut(player).and_then(Option::as_mut) {
            s.percentage.set_grey_percentage(&mut self.t, g, real);
        }
    }

    /// UI_Broker SetCooldown...ForPlayer for a player's slot. row: the slot's row now, for a create.
    #[allow(clippy::too_many_arguments)]
    pub fn ability_op(
        &mut self,
        player: usize,
        n: i32,
        op: &HudAbilityOp,
        row: Option<&HudAbility>,
        now: f64,
        real: f64,
        sounds: &mut Vec<String>,
    ) {
        if let Some(s) = self.slots.get_mut(player).and_then(Option::as_mut) {
            s.cooldowns.ability_op(&mut self.t, n, op, row, now, real, sounds);
        }
    }

    pub fn set_evade_decay(&mut self, player: usize, d: f64, now: f64) {
        if let Some(s) = self.slots.get_mut(player).and_then(Option::as_mut) {
            s.meter.on_evade_decay(&mut self.t, d, now);
        }
    }

    /// UI_IGv3_PauseHold OnPauseHold(Player Index, Pause Hold Value) (@408-532), the broker's
    /// SetPauseHoldForPlayer: kept when the index is the widget's own Player Index (0, UI_HUD_Scoreboard_Container's
    /// Init(broker, 0) @661). -> whether the kept value changed.
    pub fn pause_hold(&mut self, player: i32, value: f64) -> bool {
        if player != 0 || value == self.pause_hold_value {
            return false;
        }
        self.pause_hold_value = value;
        true
    }

    /// Hide: StopAnimation(EnterAnim), PlayAnimation(ExitAnim), the enter timer cleared.
    pub fn hide(&mut self, real: f64) {
        if self.anim_start < 0.0 {
            self.enter_at = f64::INFINITY;
        }
        self.exit_start = real;
    }

    /// remaining: the game state's RoundTimeRemaining.
    pub fn tick(&mut self, a: &Res, now: f64, real: f64, remaining: f64, sounds: &mut Vec<String>) {
        let t = &mut self.t;
        // UI_IGv3_Clock's UpdateMatchTime is its widget Tick, which Slate runs from the widget's paint
        // (SWidget::Paint, exe 1416cab90): nothing under the Hidden mainContainer ticks before AnimateIn.
        if t.widget(self.main).vis != Vis::Hidden {
            self.clock.tick(t, a, remaining, real, sounds);
            // UI_IGv3_PauseHold Tick (@91-191): RenderOpacity 1 while the value is above 0, else 0, and Meter's
            // Percentage = the value.
            t.widget_mut(self.pause_hold_w).opacity = if self.pause_hold_value > 0.0 { 1.0 } else { 0.0 };
            t.dynamic_material(self.pause_hold_meter).set_scalar("Percentage", self.pause_hold_value as f32);
        }
        if self.mode != 4 {
            for p in &mut self.pips {
                p.tick(t, a, real);
            }
        }
        for s in self.slots.iter_mut().flatten() {
            s.tick(t, a, now, real);
        }
        if self.anim_start < 0.0 && now >= self.enter_at {
            // AnimateIn: mainContainer SelfHitTestInvisible, PlayAnimation(EnterAnim).
            t.widget_mut(self.main).vis = Vis::SelfHitTestInvisible;
            self.anim_start = real;
        }
        let r = t.root();
        if self.exit_start >= 0.0 {
            let an = &a.board_exit;
            an.at(t, r, (real - self.exit_start).min(an.end()));
        } else if self.anim_start >= 0.0 {
            let an = &a.board_enter;
            an.at(t, r, (real - self.anim_start).min(an.end()));
        }
    }

    /// UI_HUD_Scoreboard_Container: SafeZone (no padding on PC) -> canvas slot with offsets 50 / 50 / 50 / 80.
    /// -> whether its root was painted.
    pub fn draw(&mut self, fonts: &mut Fonts, out: &mut ElementList, width: f64, height: f64, dpi: f64) -> bool {
        let (w, h) = (width / dpi, height / dpi);
        layout(&mut self.t, fonts, Geometry { pos: [50.0, 50.0], size: [w - 100.0, h - 130.0] }, dpi);
        paint(&self.t, fonts, out, 1.0);
        self.t.widget(self.t.root()).shown()
    }
}

#[cfg(test)]
mod tests {
    use super::{big_layout, big_scale, big_sides, clock_text};

    #[test]
    fn big_scoreboard_sides() {
        // the build's shapes keep its layout: a 1v1, a 2v2, the port's 2v1, a Free For All of four
        assert!(!big_layout(1, &[0, 1]));
        assert!(!big_layout(2, &[0, 1, 0, 1]));
        assert!(!big_layout(2, &[0, 1, 0]));
        assert!(!big_layout(3, &[0, 1, 2, 3]));
        // the lobby's 3v1, 4v2, 4v3 (the larger team 0, the sides alternating while both have players)
        assert!(big_layout(2, &[0, 1, 0, 0]));
        assert_eq!(big_sides(2, &[0, 1, 0, 0]), [vec![0, 2, 3], vec![1]]);
        assert_eq!(big_sides(2, &[0, 1, 0, 1, 0, 0]), [vec![0, 2, 4, 5], vec![1, 3]]);
        assert_eq!(big_sides(2, &[0, 1, 0, 1, 0, 1, 0]), [vec![0, 2, 4, 6], vec![1, 3, 5]]);
        assert!(big_layout(3, &[0, 1, 2, 3, 4]));
        assert_eq!(big_sides(3, &[0, 1, 2, 3, 4]), [vec![0, 2, 4], vec![1, 3]]);
        assert_eq!(big_sides(3, &[0, 1, 2, 3, 4, 5, 6, 7]), [vec![0, 2, 4, 6], vec![1, 3, 5, 7]]);
        assert_eq!([1, 2, 3, 4].map(|n| big_scale(n, false)), [1.0, 1.0, 2.0 / 3.0, 0.5]);
        assert_eq!([1, 4, 5].map(|n| big_scale(n, true)), [1.0, 1.0, 0.8]);
    }

    #[test]
    fn clock() {
        assert_eq!(clock_text(0.0), "0:00");
        assert_eq!(clock_text(9.5), "0:09");
        assert_eq!(clock_text(185.2), "3:05");
    }
}
