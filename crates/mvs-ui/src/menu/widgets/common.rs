//! What the widgets' controllers share: the property readers, WBP_Quadrilateral,
//! the page fit of a full-screen menu, UI_ModalBase, WBP_MenuHeader and the UI_Functions colour helpers.
//!
//! The `Controller` trait and `TextEdit` are controller.rs's.

use serde_json::Value;

use crate::bundle::Rgba;
use crate::menu::controller::{Controller, Cx};
use crate::menu::manager::{Manager, Modal, NULL_GUID};
use mvs_view::skins::{ffa_team_palette, team_index_palette};

use crate::menu::palette::{palette_colour, palette_for_index};
use crate::slate::{Margin, Mid, Vis, WidgetId};

/// A LinearColor property ({R, G, B, A}); missing channels 0.
pub fn rgba(c: &Value) -> Rgba {
    let ch = |k: &str| c.get(k).and_then(Value::as_f64).unwrap_or(0.0);
    [ch("R"), ch("G"), ch("B"), ch("A")]
}

/// A sound property: the event's name, None for none ('' or "None").
pub fn sound(s: Option<&Value>) -> Option<String> {
    s.and_then(Value::as_str).filter(|s| !s.is_empty() && *s != "None").map(str::to_string)
}

/// A Text property's string (its culture-invariant string, else its source string; '' for none).
pub fn text(t: Option<&Value>) -> String {
    let Some(t) = t else { return String::new() };
    t.get("CultureInvariantString")
        .or_else(|| t.get("SourceString"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

/// UI_Functions "Rarity to Color" @1247, the palettes' base colours by ERewardRarity: None 0, Common 1, Rare 2,
/// Epic 3, Legendary 4 (the native enum's enumerator table, exe 143d686b0).
pub const RARITY: [(&str, Rgba); 5] = [
    ("None", [0.194618, 0.651406, 0.630757, 1.0]),
    ("Common", [0.068478, 0.651406, 0.016807, 1.0]),
    ("Rare", [0.035911, 0.394246, 0.9, 1.0]),
    ("Epic", [0.530391, 0.060502, 0.9, 1.0]),
    ("Legendary", [1.0, 0.223228, 0.002428, 1.0]),
];

/// A byte enum's value, for the enums read here, whose values equal the number in the NewEnumerator<n> name
/// (E_ButtonTypes, InputType 0 / 1, UI_NotificationPosition, UI_ExitButtonStyle). That does not hold in general:
/// the enum asset's Names map has the value.
pub fn enum_index(v: Option<&Value>) -> i32 {
    match v {
        Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0) as i32,
        Some(Value::String(s)) => {
            let digits: String =
                s.chars().rev().take_while(char::is_ascii_digit).collect::<Vec<_>>().into_iter().rev().collect();
            digits.parse().unwrap_or(0)
        }
        _ => 0,
    }
}

/// A Margin property ({Left, Top, Right, Bottom}); a missing side takes `d`'s.
pub fn margin(m: Option<&Value>, d: Margin) -> Margin {
    let side = |k: &str, d: f64| m.and_then(|m| m.get(k)).and_then(Value::as_f64).unwrap_or(d);
    Margin { l: side("Left", d.l), t: side("Top", d.t), r: side("Right", d.r), b: side("Bottom", d.b) }
}

fn num(p: &Value, k: &str) -> f64 {
    p.get(k).and_then(Value::as_f64).unwrap_or(0.0)
}

fn flag(p: &Value, k: &str) -> bool {
    p.get(k).and_then(Value::as_bool).unwrap_or(false)
}

fn v2(v: &Value) -> [f64; 2] {
    [num(v, "X"), num(v, "Y")]
}

fn v2s(p: &Value, k: &str) -> Vec<[f64; 2]> {
    p.get(k).and_then(Value::as_array).map_or_else(Vec::new, |a| a.iter().map(v2).collect())
}

// ---------- WBP_Quadrilateral ----------

/// The single quad the tree makes of an instance (its Image with the corner material). An outline instance
/// (IsOutline) is the Image retained under the RetainerBox's effect material OutlineMat (slate `retained`).
#[derive(Debug)]
pub struct Quadrilateral {
    should_wobble: bool,
    wobble_data_set: bool,
    /// Time to the wobble timer's next run; < 0: not set.
    timer: f64,
    corner_index: Vec<usize>,
    index_to_use: usize,
    starting: Vec<[f64; 2]>,
    current: Vec<[f64; 2]>,
    desired: Vec<[f64; 2]>,
    amounts_x: Vec<[f64; 2]>,
    amounts_y: Vec<[f64; 2]>,
    /// The DoOnce in front of SizeInitialized, reset by Construct (Constructed @670).
    size_done: bool,
}

impl Quadrilateral {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(Quadrilateral {
            should_wobble: false,
            wobble_data_set: false,
            timer: -1.0,
            corner_index: vec![0, 2, 4, 6],
            index_to_use: 0,
            starting: Vec::new(),
            current: Vec::new(),
            desired: Vec::new(),
            amounts_x: Vec::new(),
            amounts_y: Vec::new(),
            size_done: true,
        })
    }

    /// An owner's write of IsOutline and OutlineThickness in its OnInitialized, ahead of this widget's PreConstruct.
    pub fn set_outline(&mut self, cx: &mut Cx<'_>, thickness: f64) {
        cx.view.tree_mut().set_outline(cx.w, thickness);
    }

    /// ApplyWobble writes a corner to DynamicMat and to OutlineMat.
    fn set_scalar(cx: &mut Cx<'_>, name: &str, x: f64) {
        let w = cx.view.tree_mut().widget_mut(cx.w);
        let x = x as f32;
        if let Some(q) = &mut w.quad {
            match q.iter_mut().find(|e| e.0 == name) {
                Some(e) => e.1 = x,
                None => q.push((name.to_string(), x)),
            }
        }
        if let Some(m) = &mut w.mid {
            m.set_scalar(name, x);
        }
        if let Some(ol) = &mut w.retained
            && name != "EdgeFalloff"
        {
            match ol.quad.iter_mut().find(|e| e.0 == name) {
                Some(e) => e.1 = x,
                None => ol.quad.push((name.to_string(), x)),
            }
        }
    }

    /// UpdateColor @1668: with IsOutline or IsColorOverride the colour goes to the material's ColorA / ColorB
    /// (OutlineMat when there is one), else to the Image.
    pub fn update_color(&mut self, cx: &mut Cx<'_>, c: Rgba) {
        let over = flag(cx.view.tree().props(cx.w), "IsColorOverride");
        let t = cx.view.tree_mut();
        if let Some(ol) = &mut t.widget_mut(cx.w).retained {
            ol.color = c;
        } else if over {
            // @422-524: GetMaterial (DynamicMat, else the Image's dynamic material) ColorA and ColorB
            let c = c.map(|v| v as f32);
            let m = t.dynamic_material(cx.w);
            m.set_vector("ColorA", c);
            m.set_vector("ColorB", c);
        } else {
            t.widget_mut(cx.w).color = c;
        }
    }

    pub fn update_sharpness(&mut self, cx: &mut Cx<'_>, s: f64) {
        Self::set_scalar(cx, "EdgeFalloff", s);
    }

    /// UpdateMaterial @1542: Image.SetBrushFromMaterial, then DynamicMat = a new dynamic instance of it; the corner
    /// parameters are not written to it again.
    /// `name` is the bundle's material name (ui.createMaterial's key); the instance is of its object path.
    pub fn update_material(&mut self, cx: &mut Cx<'_>, name: &str) {
        let path = cx.view.bundle.material_path(name).unwrap_or(name).to_string();
        let w = cx.view.tree_mut().widget_mut(cx.w);
        w.mid = Some(Mid::of(&path));
        w.tex = Some(path);
    }

    pub fn wobble(&mut self, cx: &mut Cx<'_>, on: bool) {
        if on {
            self.timer = num(cx.view.tree().props(cx.w), "WobbleUpdateDelay");
            cx.view.tree_mut().widget_mut(cx.w).opacity = 1.0;
            self.determine_wobble(cx);
            self.should_wobble = true;
        } else {
            self.should_wobble = false;
            self.timer = -1.0;
            cx.view.tree_mut().widget_mut(cx.w).opacity = 0.0;
        }
    }

    /// MagnitudeOverride > 0: every corner moves by half of it (unit space) on both axes. Without it the amounts
    /// come from the padding of an Overlay sibling; no instance of this bundle takes that path.
    fn set_wobble_data(&mut self, p: &Value) {
        let mag = num(p, "MagnitudeOverride");
        if mag.is_nan() || mag <= 0.0 {
            return;
        }
        let m = mag * 0.5;
        self.amounts_x = vec![[0.0, m], [0.0, -m], [0.0, m], [0.0, -m]];
        self.amounts_y = vec![[0.0, m], [0.0, m], [0.0, -m], [0.0, -m]];
        self.wobble_data_set = true;
    }

    /// The next corner in turn gets a new target: its starting position, or that plus the corner's amount on
    /// both axes (Round(RandomFloat) picks one of the two for both).
    fn determine_wobble(&mut self, cx: &mut Cx<'_>) {
        if self.corner_index.is_empty() {
            self.corner_index = vec![0, 2, 4, 6];
        }
        self.index_to_use = self.corner_index.remove(0);
        let i = 3 - self.corner_index.len();
        // Math.round of [0, 1): 1 from 0.5 up
        let r = if cx.view.manager.random() >= 0.5 { 1.0 } else { 0.0 };
        let (Some(ax), Some(ay), Some(s)) = (self.amounts_x.get(i), self.amounts_y.get(i), self.starting.get(i)) else {
            return;
        };
        self.desired[i] = [s[0] + ax[0] + (ax[1] - ax[0]) * r, s[1] + ay[0] + (ay[1] - ay[0]) * r];
    }

    /// Every corner moves towards its target at WobbleInterpSpeed units per second (Vector2DInterpTo_Constant);
    /// only the corner in turn (Parameters[IndexToUse], [IndexToUse + 1]) is written to the material.
    fn apply_wobble(&mut self, cx: &mut Cx<'_>, dt: f64) {
        let p = cx.view.tree().props(cx.w);
        let step = dt * num(p, "WobbleInterpSpeed");
        let names: Vec<String> = p
            .get("Parameters")
            .and_then(Value::as_array)
            .map_or_else(Vec::new, |a| a.iter().map(|v| v.as_str().unwrap_or_default().to_string()).collect());
        for (c, d) in self.current.iter_mut().zip(&self.desired) {
            let dx = d[0] - c[0];
            let dy = d[1] - c[1];
            let d2 = dx * dx + dy * dy;
            if d2 < 1e-8 {
                *c = *d;
                continue;
            }
            let len = d2.sqrt();
            *c = if len > step { [c[0] + dx / len * step, c[1] + dy / len * step] } else { *d };
        }
        let flat: Vec<f64> = self.current.iter().flatten().copied().collect();
        for k in self.index_to_use..=self.index_to_use + 1 {
            if let (Some(n), Some(x)) = (names.get(k), flat.get(k)) {
                Self::set_scalar(cx, n, *x);
            }
        }
    }
}

impl Controller for Quadrilateral {
    fn construct(&mut self, cx: &mut Cx<'_>) {
        let p = cx.view.tree().props(cx.w).clone();
        // @1244: an outline stays at render opacity 0 until its size is known
        if flag(&p, "IsOutline") {
            cx.view.tree_mut().widget_mut(cx.w).opacity = 0.0;
            self.size_done = false;
        }
        self.amounts_x = v2s(&p, "WobbleAmountsX");
        self.amounts_y = v2s(&p, "WobbleAmountsY");
        self.starting = ["TopLeft", "TopRight", "BottomLeft", "BottomRight"]
            .iter()
            .map(|k| p.get(*k).map_or([0.0; 2], v2))
            .collect();
        self.current = self.starting.clone();
        self.desired = self.current.clone();
    }

    fn timers(&mut self, cx: &mut Cx<'_>, dt: f64) {
        if self.timer < 0.0 {
            return;
        }
        let delay = num(cx.view.tree().props(cx.w), "WobbleUpdateDelay");
        self.timer -= dt;
        while self.timer <= 0.0 {
            self.determine_wobble(cx);
            self.timer += delay;
            // (a delay of 0 or less would never end the loop: one run a frame)
            if delay <= 0.0 {
                break;
            }
        }
    }

    fn tick(&mut self, cx: &mut Cx<'_>, dt: f64) {
        // Tick @701: an outline with a size runs SizeInitialized once (@1794): the Image's scale becomes
        // (1 - height / width x thickness, 1 - thickness), a border of the same width on both axes, and the widget
        // shows unless MaskInnerDuringWobble.
        // (every tile of a list has a few of these: a quad with neither step left reads nothing)
        if self.size_done && !self.should_wobble {
            return;
        }
        let [w, h] = cx.view.tree().arranged_size(cx.w);
        let has_ol = cx.view.tree().widget(cx.w).retained.is_some();
        if has_ol && !self.size_done && w > 0.0 {
            self.size_done = true;
            let p = cx.view.tree().props(cx.w);
            let t = num(p, "OutlineThickness");
            let show = !flag(p, "MaskInnerDuringWobble");
            let x = cx.view.tree_mut().widget_mut(cx.w);
            if let Some(ol) = &mut x.retained {
                ol.scale = [1.0 - h / w * t, 1.0 - t];
            }
            if show {
                x.opacity = 1.0;
            }
        }
        if !self.should_wobble {
            return;
        }
        if !self.wobble_data_set {
            if w > 0.0 {
                let p = cx.view.tree().props(cx.w).clone();
                self.set_wobble_data(&p);
            }
        } else {
            self.apply_wobble(cx, dt);
        }
    }
}

// ---------- the page fit ----------

/// How much wider or taller than 16:9 the viewport is (each at least 1, one of them 1).
fn page_stretch(cx: &Cx<'_>) -> Option<[f64; 2]> {
    let [w, h] = cx.view.viewport_size();
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    let a = w / h;
    Some([(a / (16.0 / 9.0)).max(1.0), (16.0 / 9.0 / a).max(1.0)])
}

/// Deviation: the build fits a full-screen menu into the window as a 16:9 box (a ScaleBox over a SizeBox with its
/// desired size clamped to 2560x1440 .. 3840x2160) and leaves bars beside it at other aspect ratios. The port
/// widens or heightens that box to the viewport's aspect ratio, so the scale stays the build's and the anchored
/// children reach the page's edges. Content that outgrows the minimum (UI_LAB_SkinSelector with more than 8 skins)
/// grows the box on its other side too, so it keeps the page's aspect and still fills the page.
pub fn fill_page(cx: &mut Cx<'_>, sbox: WidgetId) {
    let Some([wide, tall]) = page_stretch(cx) else { return };
    let b = cx.view.tree_mut().widget_mut(sbox);
    // (a box with a fixed size, UI_CharacterMoveList's WidthOverride 2560 x HeightOverride 1440: the size itself)
    if !b.w_override.is_nan() && !b.h_override.is_nan() {
        b.w_override = 2560.0 * wide;
        b.h_override = 1440.0 * tall;
        return;
    }
    b.min_w = 2560.0 * wide;
    b.max_w = 3840.0 * wide;
    b.min_h = 1440.0 * tall;
    b.max_h = 2160.0 * tall;
    b.keep_aspect = (2560.0 * wide) / (1440.0 * tall);
}

/// Where the build's 16:9 page (2560 x 1440 units) sits inside a box `fill_page` widened: centred, so a position
/// the build computes in the 16:9 band's units (UI_Functions WorldLocationToScreenPosition: Visible Game Resolution
/// @175 / @971) moves by half the added width and height.
pub fn page_frame_offset(cx: &Cx<'_>) -> [f64; 2] {
    page_stretch(cx).map_or([0.0; 2], |[wide, tall]| [1280.0 * (wide - 1.0), 720.0 * (tall - 1.0)])
}

/// Deviation: a full-screen picture that the build fits into the window as a 16:9 box (a ScaleBox, ScaleToFit,
/// around an Overlay that takes its size from the picture's brush) with bars beside it. The picture's brush takes
/// the viewport's aspect ratio, so the ScaleBox's scale stays the build's and the picture's siblings in the
/// Overlay reach the page's edges; the picture itself is scaled evenly to cover that box, by its render transform
/// about its centre. `w`, `h`: the brush's ImageSize in the widget tree.
pub fn cover_page(cx: &mut Cx<'_>, picture: WidgetId, w: f64, h: f64) {
    let Some([wide, tall]) = page_stretch(cx) else { return };
    let s = wide.max(tall);
    let p = cx.view.tree_mut().widget_mut(picture);
    p.brush_w = w * wide;
    p.brush_h = h * tall;
    p.transform.scale = [s / wide, s / tall];
}

// ---------- UI_ModalBase ----------

/// UI_ModalBase: a modal that dismisses itself when asked.
#[derive(Debug)]
pub struct ModalBase {
    pub modal_id: String,
    pub context: String,
}

impl Default for ModalBase {
    fn default() -> ModalBase {
        ModalBase { modal_id: String::new(), context: NULL_GUID.to_string() }
    }
}

impl ModalBase {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(ModalBase::default())
    }
}

impl Modal for ModalBase {
    fn pre_show(&mut self, _cx: &mut Cx<'_>, id: &str) {
        self.modal_id = id.to_string();
    }
    fn post_show(&mut self, _cx: &mut Cx<'_>, _instructions: &str, _extra_data: &str) {}
    fn pre_dismiss(&mut self, _cx: &mut Cx<'_>) {}
    fn post_dismiss(&mut self, _cx: &mut Cx<'_>) {}
    fn request_dismiss(&mut self, cx: &mut Cx<'_>, _reason: &str, _from_cancel: bool) {
        let id = self.modal_id.clone();
        Manager::dismiss_modal(cx, &id);
    }
    fn get_modal_context(&self) -> String {
        self.context.clone()
    }
}

impl Controller for ModalBase {
    fn as_modal(&mut self) -> Option<&mut dyn Modal> {
        Some(self)
    }
}

// ---------- WBP_MenuHeader ----------

/// 0 is the Automatic size rule in slate.
fn fill_of(v: f64) -> f64 {
    v.max(1e-6)
}

/// WBP_MenuHeader: its PreConstruct. ShowStrip is not in the class defaults (false).
#[derive(Debug, Default)]
pub struct MenuHeader;

impl MenuHeader {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(MenuHeader)
    }
}

impl Controller for MenuHeader {
    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        let p = cx.view.tree().props(cx.w).clone();
        let me = cx.w;
        // (a bundle name: the batches draw an entry)
        let page_icon = p
            .get("PageIconTexture")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(|n| crate::slate::resolve(&cx.view.bundle, n));
        let t = cx.view.tree_mut();
        let find = |t: &crate::slate::Tree, n: &str| t.find_in(me, n);
        let strip = find(t, "StripVerticalBox");
        t.widget_mut(strip).vis = if flag(&p, "ShowStrip") { Vis::SelfHitTestInvisible } else { Vis::Collapsed };
        if let Some(tex) = page_icon {
            let icon = find(t, "PageIcon");
            t.widget_mut(icon).tex = Some(tex);
        }
        let size = num(&p, "IconSize");
        let sb = find(t, "IconSizeBox");
        let b = t.widget_mut(sb);
        b.w_override = size;
        b.h_override = size;
        let l = if size == 72.0 {
            -4.0
        } else if size >= 80.0 {
            -8.0
        } else {
            0.0
        };
        b.pad = Margin { l, t: 0.0, r: 0.0, b: 0.0 };
        // @2177 and @1475: both set always (a title the instance leaves out is the empty text: the tree's "BATTLE
        // PASS" placeholder does not show)
        let bg = find(t, "txt_BackgroundTitle");
        t.widget_mut(bg).text = text(p.get("BackgroundTitle"));
        let pt = find(t, "txt_PageTitle");
        t.widget_mut(pt).text = text(p.get("PageTitle"));
        let hh = num(&p, "HeaderHeight");
        let sv = find(t, "Spacer_StripVert");
        let x = t.widget_mut(sv);
        x.brush_w = 0.0;
        x.brush_h = hh;
        let fv = find(t, "SpacerFillVert");
        let x = t.widget_mut(fv);
        x.brush_w = 0.0;
        x.brush_h = hh + num(&p, "StripWidth");
        let fl = num(&p, "Fill_LeftSidePosition");
        let fr = num(&p, "Fill_RightSidePosition");
        let sl = num(&p, "Strip_LeftSidePosition");
        let sr = num(&p, "Strip_RightSidePosition");
        for (n, v) in [
            ("FillQuadLeft", fl),
            ("QuadLeft_RSpacer", 1.0 - fl),
            ("FillQuadRight", 1.0 - fr),
            ("QuadRight_LSpacer", fr),
            ("StripQuadLeft", sl),
            ("StripQuadLeft_RSpacer", 1.0 - sl),
            ("StripQuadRight", 1.0 - sr),
            ("StripQuadRight_LSpacer", sr),
        ] {
            let w = find(t, n);
            t.widget_mut(w).fill = fill_of(v);
        }
    }
}

// ---------- the UI_Functions colours ----------

/// GetPaletteEntry(Type).UI: PaletteEntryType 0 Ally, 1 Enemy, 2 Me, 3 Team3, 4 Team4 through the colour bindings
/// of the UI preferences (as they are now), 5..7 the UI-* rows (palette.rs).
pub fn palette_entry(cx: &Cx<'_>, kind: u8) -> Rgba {
    palette_colour(cx.view.palette(), &cx.prefs.ui, kind)
}

/// UI_Functions ColorForIndex(index).linearColor = PaletteForIndex(index).base: another index than 0..4 the
/// select's default (zero).
pub fn color_for_index(cx: &Cx<'_>, index: i32) -> Rgba {
    palette_for_index(index).map_or([0.0; 4], |k| palette_entry(cx, k))
}

/// What UI_Functions "Color for Team" reads of the match: the active match config's context is 4 (Local Play), the
/// game state type is 3 (Training), the config's team style (ETeamStyle: 2 free for all), and the local player's
/// team index (GetMyPlayerData; None when it fails).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TeamColorMatch {
    pub local_play: bool,
    pub training: bool,
    pub team_style: i32,
    pub my_team: Option<i32>,
}

/// UI_Functions "Color for Team"(teamIndex).linearColor (@0-3356): `color_for_index` of `team_color_index`, the
/// select's default (zero) for none.
pub fn color_for_team(cx: &Cx<'_>, team_index: i32, m: Option<&TeamColorMatch>) -> Rgba {
    team_color_index(team_index, m).map_or([0.0; 4], |i| color_for_index(cx, i))
}

/// The ColorForIndex index "Color for Team" takes (None: the select's default).
/// Deviation: a team past the build's four (the port's big matches) takes the opponent palettes in turn (mvs-view
/// skins.rs `team_index_palette`, `ffa_team_palette`), as the HUD does; in free for all whenever the team or the
/// local player's is past 3 (the build's place list [0, 1, 2, 3] would give a team the "Me" index 4 or the own
/// colour). The build has no such team.
pub fn team_color_index(team_index: i32, m: Option<&TeamColorMatch>) -> Option<i32> {
    let mine = m.and_then(|m| m.my_team);
    let by_index = match u8::try_from(team_index) {
        Ok(t) if t > 3 => team_index_palette(t) as i32,
        _ => team_index,
    };
    // @20-677: Local Play, or the local player a spectator (`SPECTATOR_TEAM`)
    let spectator = Some(crate::menu::profile_rules::SPECTATOR_TEAM);
    let Some(m) = m.filter(|m| !m.local_play && mine != spectator) else {
        return Some(by_index);
    };
    // @682-886: Training
    if m.training {
        return Some(by_index);
    }
    // @3229: no player data
    let Some(mine) = mine else { return Some(by_index) };
    // @891-2152: free for all: the own team 0, the others 1 + their place in [0, 1, 2, 3] without the own team
    // (Array_Find of a team not in it: -1, so 0)
    if m.team_style == 2 {
        if team_index == mine {
            return Some(0);
        }
        if let (Ok(t), Ok(me)) = (u8::try_from(team_index), u8::try_from(mine))
            && (t > 3 || me > 3)
        {
            return Some(ffa_team_palette(t, me) as i32);
        }
        let place = [0, 1, 2, 3].iter().filter(|&&t| t != mine).position(|&t| t == team_index);
        return Some(place.map_or(0, |p| p as i32 + 1));
    }
    // @2157-3224: team 0 and team 1, each 0 when it is the own team, else 1; another team the select's default
    match team_index {
        0 | 1 => Some(if mine == team_index { 0 } else { 1 }),
        _ => None,
    }
}

const FOUNDER_SHIMMER: &str = "MI_FoundersPackShimmer";
const FOUNDER_DARK: Rgba = [0.009721, 0.011612, 0.033105, 1.0];

/// UI_Functions Apply Founder Pack Name Treatment(Text Widget, User) @79-1458, for a founder: white, an outline of
/// Size / 10 (integer) in the dark navy, MI_FoundersPackShimmer as the font material, a shadow (-k, k) in the same
/// navy and a render translation (-2k, -k / 4), k = Size / 10. Deviation: every player is treated as a founder (the
/// build asks User.IsFounder, a Founder's Pack owner). The font size is not changed, so a second call sets the same
/// values.
pub fn apply_founder_name_treatment(cx: &mut Cx<'_>, w: WidgetId) {
    // (the material's store entry by its name in the bundle)
    let shimmer = crate::slate::resolve(&cx.view.bundle, FOUNDER_SHIMMER);
    let x = cx.view.tree_mut().widget_mut(w);
    let k = (x.font_size / 10.0).trunc();
    x.color = [1.0; 4];
    x.outline = k;
    x.outline_color = FOUNDER_DARK;
    x.outline_shadows = false;
    x.separate_fill = false;
    if x.font_mat.as_deref() != Some(shimmer.as_str()) {
        x.font_mid = Some(Mid::of(&shimmer));
        x.font_mat = Some(shimmer);
    }
    x.shadow_offset = [-k, k];
    x.shadow_color = FOUNDER_DARK;
    x.transform.translation = [-2.0 * k, -k / 4.0];
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Free for all past the build's four teams: the opponent palettes in turn whenever either team is past 3, never
    /// the "Me" index 4; the build's four teams keep its place list; Teams' other team the default.
    #[test]
    fn team_colours_past_four_teams() {
        let ffa = |mine: i32| TeamColorMatch { local_play: false, training: false, team_style: 2, my_team: Some(mine) };
        // the local player on team 5, an opponent on team 3: not 4 (the build's list would give 3 + 1)
        assert_eq!(team_color_index(3, Some(&ffa(5))), Some(ffa_team_palette(3, 5) as i32));
        assert_ne!(team_color_index(3, Some(&ffa(5))), Some(4));
        assert_eq!(team_color_index(5, Some(&ffa(5))), Some(0));
        assert_eq!(team_color_index(6, Some(&ffa(0))), Some(ffa_team_palette(6, 0) as i32));
        for t in 0..8 {
            assert_ne!(team_color_index(t, Some(&ffa(7))), Some(4), "team {t}");
        }
        // the build's four teams: 1 + the place among the others
        assert_eq!([1, 2, 3].map(|t| team_color_index(t, Some(&ffa(0)))), [Some(1), Some(2), Some(3)]);
        // Local Play: the team index, past 3 the opponent palettes
        assert_eq!(team_color_index(2, None), Some(2));
        assert_eq!(team_color_index(4, None), Some(team_index_palette(4) as i32));
        let teams = TeamColorMatch { local_play: false, training: false, team_style: 1, my_team: Some(1) };
        assert_eq!([0, 1, 2].map(|t| team_color_index(t, Some(&teams))), [Some(1), Some(0), None]);
    }

    #[test]
    fn readers() {
        assert_eq!(enum_index(Some(&json!("E_ButtonTypes::NewEnumerator12"))), 12);
        assert_eq!(enum_index(Some(&json!(3))), 3);
        assert_eq!(enum_index(None), 0);
        assert_eq!(sound(Some(&json!("None"))), None);
        assert_eq!(sound(Some(&json!("ui_click"))).as_deref(), Some("ui_click"));
        assert_eq!(text(Some(&json!({"SourceString": "A"}))), "A");
        let m = margin(Some(&json!({"Left": 2.0})), Margin { l: 0.0, t: 1.0, r: 0.0, b: 0.0 });
        assert_eq!((m.l, m.t), (2.0, 1.0));
    }
}
