//! UI_Functions PopInputLabel -> UI_PopInputLabel holding a UI_InputLabel: a viewport
//! widget centred on its actor's projected point (an item's InputUI, a fighter's labels); its visibility follows
//! the item's InputIconVisibilityCount (BPI_HideItemUI); Exit plays ExitAnim and destroys it. And a UI_InputLabel
//! placed in another widget's tree (UI_IGv3_Struggle's).

use std::sync::Arc;

use indexmap::IndexMap;

use crate::element::ElementList;
use crate::key_names::KeyNames;
use crate::slate::{Geometry, Tree, Vis, WidgetId, layout, measure_tree, paint};
use crate::text::Fonts;

use super::Res;
use super::common::{Anim, shown};

/// EInputAction (native) by value: the action's InputDefaults row name (the FEnumeratorParam table at 143d2fd50).
fn action_name(action: u8) -> &'static str {
    match action {
        0 => "Attack",
        1 => "Special",
        2 => "Evade",
        3 => "Jump",
        10 => "Pickup Item",
        _ => "",
    }
}

/// The action mappings a HUD label can show (the action names of `action_name`), in `LabelInput::slots`' order.
pub const LABEL_ACTIONS: [&str; 5] = ["Attack", "Special", "Evade", "Jump", "Pickup Item"];

/// An action's keys by binding slot: Keyboard 1 and 2, Gamepad 1 and 2.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Slots {
    pub k1: Option<String>,
    pub k2: Option<String>,
    pub g1: Option<String>,
    pub g2: Option<String>,
}

impl Slots {
    /// UI_Functions BreakApartMappings of an action mapping's keys as GetActionMappingByName gives them: walked from
    /// the last to the first, None skipped; a key that is no gamepad key fills Keyboard 1, then Keyboard 2, a
    /// gamepad key Gamepad 1, then Gamepad 2.
    pub fn break_apart<'a>(keys: impl DoubleEndedIterator<Item = &'a str>) -> Slots {
        let mut s = Slots::default();
        for k in keys.rev().filter(|k| !k.is_empty() && *k != "None") {
            let (a, b) = if k.starts_with("Gamepad_") { (&mut s.g1, &mut s.g2) } else { (&mut s.k1, &mut s.k2) };
            if a.is_none() {
                *a = Some(k.to_owned());
            } else if b.is_none() {
                *b = Some(k.to_owned());
            }
        }
        s
    }
}

/// What a player index's labels read: UIManager GetLastInteractionType(PlayerIndex) and
/// the stored mapping of each of `LABEL_ACTIONS` in the binding set the player index stands for.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LabelInput {
    /// 0 gamepad, 1 keyboard, 2 mouse.
    pub kind: u8,
    pub slots: [Slots; 5],
}

/// What every label reads of the players' input (the
/// HUD data's inputIcons). Not fed (the app gives none), a label is the keyboard form with InputDefaults' keys for
/// player 0 ("Attack-P0" J, "Pickup Item-P0" H), the fallback.
#[derive(Debug, Default)]
pub struct LabelSources {
    /// DataTable InputButtonIconsTable (built into the menu bundle): a gamepad key's icon-default texture, its store
    /// entry, by key name.
    icons: IndexMap<String, String>,
    /// By player index; None: not fed.
    players: Option<Vec<LabelInput>>,
    /// The player's key names (GetKeyLocalizedName).
    names: Arc<KeyNames>,
}

static NO_SLOTS: Slots = Slots { k1: None, k2: None, g1: None, g2: None };

impl LabelSources {
    /// The icon table, each player index's input and the key names.
    pub fn new(icons: IndexMap<String, String>, players: Vec<LabelInput>, names: Arc<KeyNames>) -> LabelSources {
        LabelSources { icons, players: Some(players), names }
    }

    /// Not fed, with the key names.
    pub fn named(names: Arc<KeyNames>) -> LabelSources {
        LabelSources { names, ..LabelSources::default() }
    }

    /// The key names its labels show.
    pub fn names(&self) -> &Arc<KeyNames> {
        &self.names
    }

    /// A player index's GetLastInteractionType from now on (a player index not fed: nothing).
    pub fn set_kind(&mut self, player: usize, kind: u8) {
        if let Some(p) = self.players.as_mut().and_then(|p| p.get_mut(player)) {
            p.kind = kind;
        }
    }

    /// The input type a label of `player` shows: GetLastInteractionType; 1 (keyboard) when not fed. A player index
    /// given no input reads 0, the manager's default (no device).
    fn kind(&self, player: usize) -> u8 {
        match &self.players {
            None => 1,
            Some(p) => p.get(player).map_or(0, |p| p.kind),
        }
    }

    fn slots(&self, action: &str, player: usize) -> &Slots {
        let k = LABEL_ACTIONS.iter().position(|a| *a == action);
        match (&self.players, k) {
            (Some(p), Some(k)) => p.get(player).map_or(&NO_SLOTS, |p| &p.slots[k]),
            _ => &NO_SLOTS,
        }
    }

    /// UI_InputLabel KeyboardValue: the action's Binding Slot 1 keyboard key's GetKeyLocalizedName ("": none).
    fn key_name(&self, action: &str, player: usize) -> String {
        let key = if self.players.is_none() {
            match action {
                "Attack" => Some("J"),
                "Pickup Item" => Some("H"),
                _ => None,
            }
        } else {
            self.slots(action, player).k1.as_deref()
        };
        key.map_or_else(String::new, |k| self.names.name(k))
    }

    /// UI_InputLabel ButtonValue: the action's Binding Slot 1 gamepad key's InputButtonIconsTable texture; the
    /// generic USB controller's buttons have none.
    fn icon(&self, action: &str, player: usize) -> Option<String> {
        let k = self.slots(action, player).g1.as_deref()?;
        if is_generic_usb_button(k) {
            return None;
        }
        self.icons.get(k).cloned()
    }
}

/// `GenericUSBController_Button1` to `4`.
fn is_generic_usb_button(k: &str) -> bool {
    k.strip_prefix("GenericUSBController_Button").is_some_and(|n| matches!(n, "1" | "2" | "3" | "4"))
}

/// UI_Functions "Pickup Item Input Action For Player"(PlayerIndex) (@54-1049): with AttackPicksUpItems (on by
/// default), by GetLastInteractionType (@154): a gamepad (@373-711) 10 when "Pickup Item"'s stored mapping has a
/// gamepad key in either slot (@634), else 0 (Attack, @711); keyboard or mouse (@736-1049) the same with a keyboard
/// key (@997); any other type 0 (@343). Not fed: the keyboard with the default "H".
pub fn pickup_item_action(player: usize, src: &LabelSources) -> u8 {
    let action = "Pickup Item";
    let s = src.slots(action, player);
    let has = match src.kind(player) {
        0 => s.g1.is_some() || s.g2.is_some(),
        1 | 2 if src.players.is_none() => !src.key_name(action, player).is_empty(),
        1 | 2 => s.k1.is_some() || s.k2.is_some(),
        _ => false,
    };
    if has { 10 } else { 0 }
}

const TINT: [f64; 4] = [0.68, 0.8, 0.85, 1.0];
const KEY_TEXT: [f64; 4] = [0.0, 0.003922, 0.015686, 1.0];

/// UI_InputLabel's Refresh: a change of CurrentInputType (`drawn`) redraws the label. keyboardContainer and
/// buttonIcon collapsed; ButtonValue (type 0): buttonIcon = the gamepad key's icon and shown; KeyboardValue (else):
/// keyLabel = the key name and keyboardContainer shown. No key or no icon leaves both collapsed.
fn refresh_label(t: &mut Tree, l: WidgetId, action: &str, player: usize, src: &LabelSources, drawn: &mut i32) {
    let kind = src.kind(player);
    if i32::from(kind) == *drawn {
        return;
    }
    *drawn = i32::from(kind);
    let bx = t.find_in(l, "keyboardContainer");
    let icon = t.find_in(l, "buttonIcon");
    t.widget_mut(bx).vis = Vis::Collapsed;
    t.widget_mut(icon).vis = Vis::Collapsed;
    if kind == 0 {
        if let Some(tex) = src.icon(action, player) {
            let w = t.widget_mut(icon);
            w.tex = Some(tex);
            w.vis = Vis::SelfHitTestInvisible;
        }
        return;
    }
    let name = src.key_name(action, player);
    if !name.is_empty() {
        let k = t.find_in(l, "keyLabel");
        t.widget_mut(k).text = name;
        t.widget_mut(bx).vis = Vis::SelfHitTestInvisible;
    }
}

/// UpdateColors (not dark) and SetFontSize of a label: keyboardContainer's brush and buttonIcon = Tint, buttonIcon's
/// size = SizeIconSize, keyLabel = KeyTextColor at KeyFontInfo's size 16 with letter spacing 0.
fn label_colors(t: &mut Tree, l: WidgetId, icon_size: f64) {
    let bx = t.find_in(l, "keyboardContainer");
    t.widget_mut(bx).brush_color = TINT;
    let icon = t.find_in(l, "buttonIcon");
    let i = t.widget_mut(icon);
    i.brush_w = icon_size;
    i.brush_h = icon_size;
    i.color = TINT;
    let k = t.find_in(l, "keyLabel");
    let k = t.widget_mut(k);
    k.font_size = 16.0;
    k.letter_spacing = 0.0;
    k.color = KEY_TEXT;
}

/// A UI_InputLabel placed in another widget's tree with its Params given (UI_IGv3_Struggle's): what Construct and
/// ForceRefresh leave of it for an empty Label and no modifier.
#[derive(Debug)]
pub struct PlacedInputLabel {
    l: WidgetId,
    action: &'static str,
    player: Option<usize>,
    /// CurrentInputType (-1: none yet).
    input_type: i32,
}

impl PlacedInputLabel {
    pub fn new(t: &mut Tree, l: WidgetId, icon_size: f64) -> PlacedInputLabel {
        for n in ["modifierLabel", "textLabel"] {
            let w = t.find_in(l, n);
            t.widget_mut(w).vis = Vis::Collapsed;
        }
        label_colors(t, l, icon_size);
        let bx = t.find_in(l, "keyboardContainer");
        let icon = t.find_in(l, "buttonIcon");
        t.widget_mut(bx).vis = Vis::Collapsed;
        t.widget_mut(icon).vis = Vis::Collapsed;
        PlacedInputLabel { l, action: "", player: None, input_type: -1 }
    }

    /// Params.Action / PlayerIndex written and ForceRefresh.
    pub fn set(&mut self, t: &mut Tree, action: u8, player: usize, src: &LabelSources) {
        self.action = action_name(action);
        self.player = Some(player);
        self.input_type = -1;
        self.refresh(t, src);
    }

    /// Refresh (the UIManager's input type delegate while Auto Refresh is set, its default): a change of
    /// CurrentInputType redraws the label; nothing before the first `set`.
    pub fn refresh(&mut self, t: &mut Tree, src: &LabelSources) {
        let Some(player) = self.player else { return };
        refresh_label(t, self.l, self.action, player, src, &mut self.input_type);
    }
}

/// A widget animation's run on real time.
#[derive(Clone, Copy, Debug)]
struct Run {
    start: f64,
    speed: f64,
    last: f64,
}

/// UI_PopInputLabel.
#[derive(Debug)]
pub struct PopInputLabel {
    pub t: Tree,
    container: WidgetId,
    enter: Option<Run>,
    attention: Option<Run>,
    exit: Option<Run>,
    last_tick: f64,
    destroyed: bool,
    /// ItemActorParent InputIconVisibilityCount.
    count: i32,
    location: [f64; 3],
    /// The fighter the positioning's AttachActor is (None: an item, by the label's key).
    attach: Option<usize>,
    pos: Option<[f64; 2]>,
    /// The cached geometry's size from the last paint, Slate units.
    size: [f64; 2],
    /// AddToViewport(0)'s place among the viewport's widgets.
    pub order: u64,
    label: WidgetId,
    action: &'static str,
    player: usize,
    custom_icon: Option<String>,
    drawn_type: i32,
}

/// What a label is made with (UI_InputLabelParams and UI_PopPositioningStruct).
#[derive(Clone, Debug)]
pub struct LabelParams {
    pub action: u8,
    pub location: [f64; 3],
    /// UI_InputLabelParams.Label (empty: the text collapsed) and its TextFontInfo's size.
    pub label: String,
    pub text_size: f64,
    pub attach: Option<usize>,
    pub player: usize,
    pub icon_size: f64,
    /// CustomActionIcon (a texture).
    pub custom_icon: Option<String>,
    /// The modifier text Refresh writes ("" for None: collapsed).
    pub modifier: String,
}

impl LabelParams {
    /// An item's label (ItemActorParent SetPickupIcon's params; PlayerIndex 0, @947).
    pub fn item(action: u8, on_ground: bool) -> LabelParams {
        LabelParams {
            action,
            location: [0.0, 0.0, if on_ground { 100.0 } else { 0.0 }],
            label: String::new(),
            text_size: 16.0,
            attach: None,
            player: 0,
            icon_size: 32.0,
            custom_icon: None,
            modifier: String::new(),
        }
    }
}

impl PopInputLabel {
    fn new(proto: &Tree, p: LabelParams, real: f64, order: u64, src: &LabelSources) -> PopInputLabel {
        let mut t = proto.clone();
        let container = t.find("Container");
        let l = t.find("UI_InputLabel");
        let m = t.find_in(l, "modifierLabel");
        let mw = t.widget_mut(m);
        if p.modifier.is_empty() {
            mw.vis = Vis::Collapsed;
        } else {
            mw.text = p.modifier.clone();
            mw.vis = Vis::SelfHitTestInvisible;
            mw.color = TINT;
        }
        // Refresh: textLabel = Label, shown unless empty; UpdateColors: its colour = Tint, SetFontSize(TextFontInfo's
        // size) with letter spacing 0.
        let tl = t.find_in(l, "textLabel");
        let tw = t.widget_mut(tl);
        if p.label.is_empty() {
            tw.vis = Vis::Collapsed;
        } else {
            tw.text = p.label.clone();
            tw.vis = Vis::SelfHitTestInvisible;
            tw.font_size = p.text_size;
            tw.letter_spacing = 0.0;
            tw.color = TINT;
        }
        label_colors(&mut t, l, p.icon_size);
        let mut s = PopInputLabel {
            t,
            container,
            enter: None,
            attention: None,
            exit: None,
            last_tick: -1.0,
            destroyed: false,
            count: 0,
            location: p.location,
            attach: p.attach,
            pos: None,
            size: [0.0; 2],
            order,
            label: l,
            action: action_name(p.action),
            player: p.player,
            custom_icon: p.custom_icon,
            drawn_type: -1,
        };
        s.refresh(src);
        // Init: AddToViewport(0), UpdatePositioning, Enter. Lifetime 0: no exit timer.
        s.enter(real);
        s
    }

    /// (the port's own) Another action and player for a live label, redrawn at once (InputLabels.show_item_for).
    fn retarget(&mut self, action: u8, player: usize, src: &LabelSources) {
        let name = action_name(action);
        if name == self.action && player == self.player {
            return;
        }
        self.action = name;
        self.player = player;
        self.drawn_type = -1;
        self.refresh(src);
    }

    /// Refresh: a change of CurrentInputType redraws the label. ForceRefresh @3903-4125 with a valid
    /// CustomActionIcon: buttonIcon = it and SelfHitTestInvisible, nothing of the input type read.
    fn refresh(&mut self, src: &LabelSources) {
        let t = &mut self.t;
        if let Some(icon) = &self.custom_icon {
            if self.drawn_type == -2 {
                return;
            }
            self.drawn_type = -2;
            let bx = t.find_in(self.label, "keyboardContainer");
            t.widget_mut(bx).vis = Vis::Collapsed;
            let i = t.find_in(self.label, "buttonIcon");
            let w = t.widget_mut(i);
            w.tex = Some(icon.clone());
            w.vis = Vis::SelfHitTestInvisible;
            return;
        }
        refresh_label(t, self.label, self.action, self.player, src, &mut self.drawn_type);
    }

    /// Enter: PlayAnimationForward(EnterAnim) (a player still running is left as it is), PlayAnimation(AttentionAnim,
    /// endless) from the start.
    fn enter(&mut self, real: f64) {
        if self.enter.is_none() {
            self.enter = Some(Run { start: real, speed: 1.0, last: 0.0 });
        }
        self.attention = Some(Run { start: real, speed: 1.0, last: 0.0 });
    }

    /// Exit: PlayAnimation(ExitAnim, speed 1.5); its Finished destroys the widget.
    fn exit(&mut self, real: f64) {
        if self.exit.is_none() {
            self.exit = Some(Run { start: real, speed: 1.5, last: 0.0 });
        }
    }

    /// BPI_HideItemUI: hide -> count = max(0, count - 1), show -> count + 1; Hidden at 0, else Visible; Enter whenever
    /// the count is 1 afterwards.
    fn hide(&mut self, hide: bool, real: f64) {
        self.count = if hide { (self.count - 1).max(0) } else { self.count + 1 };
        let r = self.t.root();
        self.t.widget_mut(r).vis = if self.count == 0 { Vis::Hidden } else { Vis::Visible };
        if self.count == 1 {
            self.enter(real);
        }
    }

    fn hidden(&self) -> bool {
        self.t.widget(self.t.root()).vis == Vis::Hidden
    }

    /// Widget animations on real time. Animations driving the same property at once (EnterAnim and AttentionAnim on
    /// the Container scale, EnterAnim and ExitAnim on its opacity) are averaged: one linker per widget, absolute
    /// sections of weight 1 (exe 1425b5a20 / 142536370). While the widget itself is Hidden its animations stand
    /// still (UUMGSequenceTickManager::TickWidgetAnimations, exe 142619f30, test at 1425d63b0).
    fn tick(&mut self, a: &Res, real: f64) {
        let dt = if self.last_tick < 0.0 { 0.0 } else { real - self.last_tick };
        self.last_tick = real;
        self.refresh(&a.labels);
        if self.hidden() {
            for r in [&mut self.enter, &mut self.attention, &mut self.exit].into_iter().flatten() {
                r.start += dt;
            }
            return;
        }
        let c = self.container;
        let root = self.t.root();
        let prev = props(&self.t, c);
        let mut sum = [0.0; 3];
        let mut count = [0; 3];
        for k in 0..3 {
            let (run, anim, looped): (&mut Option<Run>, &Anim, bool) = match k {
                0 => (&mut self.enter, &a.label_enter, false),
                1 => (&mut self.attention, &a.label_attention, true),
                _ => (&mut self.exit, &a.label_exit, false),
            };
            let Some(r) = run else { continue };
            let end = anim.end();
            let mut sec = (real - r.start) * r.speed;
            let done = !looped && sec >= end;
            sec = if looped { sec % end } else { sec.min(end) };
            set_props(&mut self.t, c, [f64::NAN; 3]);
            anim.apply(&mut self.t, root, sec, if looped { sec } else { r.last });
            r.last = sec;
            let x = props(&self.t, c);
            for i in 0..3 {
                if !x[i].is_nan() {
                    sum[i] += x[i];
                    count[i] += 1;
                }
            }
            if done {
                if k == 2 {
                    self.destroyed = true;
                }
                if k == 0 {
                    self.enter = None;
                }
            }
        }
        set_props(&mut self.t, c, [0, 1, 2].map(|i| if count[i] > 0 { sum[i] / f64::from(count[i]) } else { prev[i] }));
    }

    /// Exit was called while Hidden: its ExitAnim stands still, and the build's widget goes by Exit's Delay(1.0)
    /// (RemoveFromParent, DestroyWidget @251) a second after Exit.
    // Unverified: a Hidden widget's latent Delay taken as running (the world's latent action manager, not read).
    fn stuck(&self) -> bool {
        self.exit.is_some() && self.hidden()
    }

    /// UpdatePositioning (Tick, space 3): SetPositionInViewport(PositionForPopParams - absolute size / 2, remove
    /// DPI), the absolute size the last frame's geometry (zero before the first paint).
    fn place(&mut self, p: Option<[f64; 2]>, dpi: f64) {
        if let Some(p) = p {
            self.pos = Some([p[0] - self.size[0] * dpi / 2.0, p[1] - self.size[1] * dpi / 2.0]);
        }
    }

    /// In the viewport at its desired size.
    /// -> whether its root was painted.
    pub fn draw(&mut self, fonts: &mut Fonts, out: &mut ElementList, dpi: f64) -> bool {
        let r = self.t.root();
        let pos = self.pos.unwrap_or([0.0; 2]);
        let at = [pos[0] / dpi, pos[1] / dpi];
        // measure, then arrange at the desired size
        measure_tree(&mut self.t, fonts, dpi);
        let size = self.t.desired_size(r);
        layout(&mut self.t, fonts, Geometry { pos: at, size }, dpi);
        let painted = self.pos.is_some() && shown(self.t.widget(r).vis);
        if painted {
            paint(&self.t, fonts, out, 1.0);
        }
        self.size = size;
        painted
    }
}

/// Container's channels the animations share: scale x, y and opacity.
fn props(t: &Tree, w: WidgetId) -> [f64; 3] {
    let x = t.widget(w);
    [x.transform.scale[0], x.transform.scale[1], x.opacity]
}

fn set_props(t: &mut Tree, w: WidgetId, v: [f64; 3]) {
    let x = t.widget_mut(w);
    x.transform.scale = [v[0], v[1]];
    x.opacity = v[2];
}

/// The viewport's input labels by key.
#[derive(Debug)]
pub struct InputLabels {
    proto: Tree,
    labels: IndexMap<i64, PopInputLabel>,
}

impl InputLabels {
    pub fn new(proto: Tree) -> InputLabels {
        InputLabels { proto, labels: IndexMap::new() }
    }

    /// SetPickupIcon (ItemActorParent @809-1677): only while InputUI is not valid. (the port's own) A label whose
    /// Exit has begun is not a valid InputUI: a rollback's corrected create makes a new widget in its place.
    pub fn create(&mut self, item: i64, action: u8, on_ground: bool, real: f64, order: &mut u64, src: &LabelSources) {
        if self.labels.get(&item).is_some_and(|l| l.exit.is_none()) {
            return;
        }
        let l = PopInputLabel::new(&self.proto, LabelParams::item(action, on_ground), real, next(order), src);
        self.labels.insert(item, l);
    }

    pub fn hide(&mut self, item: i64, hide: bool, real: f64) {
        if let Some(l) = self.labels.get_mut(&item) {
            l.hide(hide, real);
        }
    }

    /// Deviation: the build fixes an item label's action when the first fighter to reach the item makes it, and
    /// reads player 0's keys; the port gives a shown label the action and keys of the local human it is shown for.
    pub fn show_item_for(&mut self, item: i64, action: u8, player: usize, src: &LabelSources) {
        if let Some(l) = self.labels.get_mut(&item) {
            l.retarget(action, player, src);
        }
    }

    pub fn exit(&mut self, item: i64, real: f64) {
        if let Some(l) = self.labels.get_mut(&item) {
            l.exit(real);
        }
    }

    /// A label attached to a fighter (Pawn_C017 RageCallout @22557), kept apart from the items' by a negative key
    /// (`fighter_key`, or the variable that holds the widget when it is not the fighter's own). PopInputLabel(params,
    /// positioning) makes a new widget (Init: AddToViewport, Enter), then SetVisibility(Visible) and Enter again in
    /// the same step. The earlier widget under the key, if any, was hidden by the caller's last off call
    /// (SetVisibility(Hidden), then Exit): its ExitAnim stands still while Hidden and Exit's Delay(1.0) destroys it a
    /// second later, unseen; the port drops it at once.
    pub fn show_for(&mut self, key: i64, p: LabelParams, real: f64, order: &mut u64, src: &LabelSources) {
        let l = PopInputLabel::new(&self.proto, p, real, next(order), src);
        self.labels.insert(key, l);
    }

    /// The label under `key` gone at once (SetVisibility(Hidden), then Exit or DestroyWidget: never seen again).
    pub fn hide_for(&mut self, key: i64) {
        self.labels.shift_remove(&key);
    }

    /// items: each live item's actor location (UE) by id; fighters: the fighters' actor locations. A label
    /// attached to a fighter is placed on him, any other by its key's item.
    pub fn tick(
        &mut self,
        a: &Res,
        dpi: f64,
        real: f64,
        items: &[(i32, [f64; 3])],
        fighters: &[Option<[f64; 3]>],
        project: &dyn Fn([f64; 3]) -> Option<[f64; 2]>,
    ) {
        self.labels.retain(|&id, l| {
            l.tick(a, real);
            if l.destroyed {
                return false;
            }
            let at = match l.attach {
                Some(f) => fighters.get(f).copied().flatten(),
                None => items.iter().find(|i| i64::from(i.0) == id).map(|i| i.1),
            };
            // An Exited label Hidden with its actor gone: the build's goes by Exit's Delay(1.0), the port's at once.
            if at.is_none() && l.stuck() {
                return false;
            }
            if let Some(p) = at {
                let o = l.location;
                l.place(project([p[0] + o[0], p[1] + o[1], p[2] + o[2]]), dpi);
            }
            true
        });
    }

    pub fn labels_mut(&mut self) -> impl Iterator<Item = &mut PopInputLabel> {
        self.labels.values_mut()
    }

    pub fn labels(&self) -> impl Iterator<Item = &PopInputLabel> {
        self.labels.values()
    }
}

/// The key of the label a fighter's own variable holds (RageCallout's), by the pawn's player index: below every
/// item's id.
pub fn fighter_key(slot: usize) -> i64 {
    -1 - i64::try_from(slot).expect("a player index fits i64")
}

/// The viewport's next AddToViewport(0) place.
pub fn next(order: &mut u64) -> u64 {
    *order += 1;
    *order
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(k: &[&str]) -> Vec<String> {
        k.iter().map(|k| (*k).to_owned()).collect()
    }

    #[test]
    fn break_apart_fills_the_slots_first_key_first() {
        // (GetActionMappingByName's order: the mapping's last key first)
        let m = keys(&["Gamepad_DPad_Left", "None", "H", "Gamepad_FaceButton_Left", "J", "K"]);
        let s = Slots::break_apart(m.iter().map(String::as_str));
        assert_eq!(s.k1.as_deref(), Some("K"));
        assert_eq!(s.k2.as_deref(), Some("J"));
        assert_eq!(s.g1.as_deref(), Some("Gamepad_FaceButton_Left"));
        assert_eq!(s.g2.as_deref(), Some("Gamepad_DPad_Left"));
    }

    fn fed(kind: u8) -> LabelSources {
        let pickup = Slots { k1: Some("H".into()), g1: Some("Gamepad_DPad_Left".into()), ..Slots::default() };
        let attack =
            Slots { k1: Some("SpaceBar".into()), g1: Some("GenericUSBController_Button2".into()), ..Slots::default() };
        let mut slots: [Slots; 5] = Default::default();
        slots[0] = attack;
        slots[4] = pickup;
        let icons = [("Gamepad_DPad_Left", "/Game/T_DPad_Left")].map(|(k, v)| (k.to_owned(), v.to_owned()));
        let display = [("SpaceBar", "Space Bar")].map(|(k, v)| (k.to_owned(), v.to_owned())).into_iter().collect();
        let names = Arc::new(KeyNames::new(None, display));
        LabelSources::new(icons.into_iter().collect(), vec![LabelInput { kind, slots }], names)
    }

    #[test]
    fn a_label_shows_its_players_device() {
        let src = fed(0);
        assert_eq!(src.icon("Pickup Item", 0).as_deref(), Some("/Game/T_DPad_Left"));
        // (the generic USB controller's buttons have no icon)
        assert_eq!(src.icon("Attack", 0), None);
        // (no Game target: the Engine target's display name)
        assert_eq!(src.key_name("Attack", 0), "Space Bar");
        // (a player index with no input: type 0, no keys)
        assert_eq!((src.kind(1), src.icon("Pickup Item", 1)), (0, None));
        assert_eq!(pickup_item_action(0, &src), 10);
        assert_eq!(pickup_item_action(1, &src), 0);
        let none = LabelSources::default();
        assert_eq!((none.kind(0), none.key_name("Attack", 3).as_str()), (1, "J"));
        assert_eq!(pickup_item_action(0, &none), 10);
    }
}
