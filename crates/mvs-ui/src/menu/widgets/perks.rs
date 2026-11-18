//! The perk page: UI_Perks_Item (the tile), UI_PerkLockIndicator,
//! UI_Perks_StackedCallout, UI_Perks_AddButton, UI_TooltipPerkDescription, UI_TooltipPerk_v2, UI_Perks_Sublist (one
//! category's list), UI_Perks_Loadout (the four slots), UI_Perks_Equipper (the page: the lists, the slot paging,
//! pick / replace / clear / auto equip / ready), UI_PM_EditPerks and UI_Functions "Fly Perk Item".
//!
//! Port decision (the perks have no effect in a match, by the user's choice): what the PerkManager answers online is
//! answered here from the menu bundle. Every perk of the character's lists (the bundle's `fighters.<key>.perks.lists`
//! by EPerkCategory) is owned; mastery level 1; training and ally sharing off; auto equip and a bot take
//! RecommendedPerkDatas. A perk's id is its PerkData asset name.
//!
//! What a page keeps: a character's loadout for the program's life (GetCharacterSavedPerks /
//! UpdateCharacterSavedPerks: `saved_perks`), or with `remember` set (the pre-match screen's, the custom lobby's) the
//! machine's remembered perks of mvs-settings (`Settings::perks`), which the app writes to its settings file.
//!
//! Calls between the page's controllers go through `run_on` (at once, or after the call that holds the target
//! returns): a tile's click reaches the page through the list, the sublist and the page while each of them is busy
//! (controller.rs). What the page reads of its loadout and its lists it keeps itself (`slots`, `lists`), set by the
//! same calls that set them.

use std::any::Any;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use serde_json::{Value, json};

use crate::bundle::{FighterPerks, PerkInfo, Rgba};
use crate::menu::controller::{Controller, Cx, Event, Handler};
use crate::menu::manager::Manager;
use crate::menu::screens::lab_selectors::{GridItemList, Tile, TileData, TileEvent, run_on};
use crate::menu::scroll::{scroll_list, set_scrollbar_padding};
use crate::menu::view::MenuView;
use crate::menu::widgets::common::{Quadrilateral, palette_entry};
use crate::menu::widgets::controls::buttons::{ButtonMain, ButtonSecondary};
use crate::menu::widgets::controls::grid_item::{GRID_READY, GRID_WAIT, GridItem, GridItemState};
use crate::menu::widgets::controls::interactor::{
    TooltipParams, batch_set_context, no_tooltip, set_focused, set_interactable, toggle_tooltip,
};
use crate::menu::widgets::controls::tooltip::{TIP_SET_DATA, TIP_SET_TEXT, TIP_SHOW, tooltip_params};
use crate::menu::widgets::controls::{bind, find, ist, prop, prop_bool, prop_f64, set_tooltip, wd};
use crate::slate::{HAlign, Margin, Vis, WidgetId, resolve};

// ---------- what the classes share ----------

const WHITE: Rgba = [1.0, 1.0, 1.0, 1.0];
/// #F3FCF0
const OFF_WHITE: Rgba = [0.89627, 0.973446, 0.871367, 1.0];
/// EPerkCategory 0 Offense, 1 Defense, 2 Utility, 3 CharacterSpecific: the stacked glow and the stacked tag.
const CATEGORY_COLOR: [Rgba; 4] = [
    [0.938686, 0.078187, 0.084376, 1.0],
    [0.048172, 0.775822, 1.0, 1.0],
    [0.008023, 0.693872, 0.258183, 1.0],
    [0.0, 0.0, 0.0, 0.0],
];
/// UI_Perks_Item UpdateAppearance @1670: the icon of a perk without one.
const CATEGORY_ICON: [&str; 4] = ["ui_perkbg_attack", "ui_perkbg_defense", "ui_perkbg_utility", "UI_perkItemBG"];
const SELECT_SOUND: [&str; 4] = [
    "ui_prematch_perk_block_select_offense",
    "ui_prematch_perk_block_select_defense",
    "ui_prematch_perk_block_select_utility",
    "ui_prematch_perk_block_select_big",
];
const SLOTS: usize = 4;
const SIGNATURE: u32 = 3;
/// EPerkCategory's enumerators in byte order.
const PERK_CATEGORY: [&str; 4] = ["Offense", "Defense", "Utility", "CharacterSpecific"];

// The texts the graphs set (Game.locres keys of the classes' EX_TextConst, namespace "").
/// UI_Perks_Sublist: "offense", "defense", "utility", "signature" and "No {category} perks available for this
/// character yet."
const T_CATEGORY: [&str; 4] = [
    "5EA9FDBC463F44C8B3F719930B936C95",
    "802A5E1F4EFF4A72D99E75BF5DD7B185",
    "FE96651842C90BDABE82BB950CB034AC",
    "42FCB43F486CBD0DFA753C8E8847D6C5",
];
const T_NO_PERKS: &str = "58F7196B4CC88A81E541859BD931CF29";
/// "SIGNATURE PERK" (UI_Perks_Equipper and UI_TooltipPerk_v2), "PERK SLOT {slotNumber}", "NEXT", "LET'S GO",
/// "1 PERK SLOT UNUSED!", "{count} PERK SLOTS UNUSED!", "LVL {level}".
const T_SIGNATURE_PERK: &str = "266D47254235773A318F129D4DF42151";
const T_PERK_SLOT: &str = "7B4749734203EF6E4A4F59893DA8B2E4";
const T_NEXT: &str = "44A1A983440BDE410864CFA3D89DBD04";
const T_LETS_GO: &str = "E910828A42B847CCF817FBA60D8B1852";
const T_ONE_UNUSED: &str = "61B0F45D4968E785B4E2CAB4193613D9";
const T_UNUSED: &str = "2610EC7E42FD488E89EBD9AD245E08E2";
const T_EQUIPPER_LVL: &str = "571F6AEF44487426EE366893C7550AA9";
/// UI_Perks_Loadout: "LVL {number}", "Ally's Perks".
const T_LOADOUT_LVL: &str = "72786BB14825F4DBD449A29E888EBA98";
const T_ALLYS_PERKS: &str = "17C4A9584E829E90EB056BAA88E858AD";
/// UI_Perks_StackedCallout: "Slot {number} stacked!".
const T_SLOT_STACKED: &str = "F6AF33AA473D72F2C829508175D10A2C";
/// UI_Perks_AddButton: "Train a new Perk".
const T_TRAIN: &str = "3A1AB2194598992703699B91B34502F6";
/// UI_TooltipPerkDescription: "STACKED!", "On Stacked".
const T_STACKED: &str = "28B9CF544DE730AAFE1D4C9E8DAD5162";
const T_ON_STACKED: &str = "21F90CC147FCDEBFC30E64A77F6255F3";
/// UI_TooltipPerk_v2: "Perk".
const T_PERK: &str = "D6091ECB444C801A645C7E8FE418A40A";

fn game_text(cx: &Cx<'_>, key: &str) -> String {
    cx.view.game_text(key).to_string()
}

/// The PlayerMastery level the page shows (Deviation: the build shows the server's mastery level; the port's
/// player is level 49).
const PLAYER_LEVEL: i32 = 49;

fn is_visible(v: Vis) -> bool {
    v != Vis::Collapsed && v != Vis::Hidden
}

/// UI_Functions "Conditional Visibility": `t` when `c`, else `f`.
fn cond(cx: &mut Cx<'_>, w: WidgetId, c: bool, t: Vis, f: Vis) {
    wd(cx, w).vis = if c { t } else { f };
}

/// `find(name)` under widget `under` (another controller's tree, read by its owner).
fn find_under(cx: &Cx<'_>, under: WidgetId, name: &str) -> WidgetId {
    cx.view.tree().find_in(under, name)
}

/// EPerkCategory of a property ("EPerkCategory::Defense") or a byte.
fn enum_category(v: &Value) -> u32 {
    match v {
        Value::Number(n) => n.as_u64().map_or(0, |n| n as u32),
        Value::String(s) => {
            let tail = s.rsplit("::").next().unwrap_or_default();
            PERK_CATEGORY.iter().position(|c| *c == tail).map_or(0, |i| i as u32)
        }
        _ => 0,
    }
}

/// UE's KismetMathLibrary WrapInt (Value in Min..=Max).
fn wrap_int(v: i32, lo: i32, hi: i32) -> i32 {
    let n = hi - lo + 1;
    ((v - lo) % n + n) % n + lo
}

// ---------- the data ----------

/// A PerkData as the widgets read it: the bundle's row and its asset name. Two perks are the same perk by id.
#[derive(Clone, Debug)]
pub struct Perk {
    pub id: String,
    pub info: PerkInfo,
}

impl PartialEq for Perk {
    fn eq(&self, o: &Perk) -> bool {
        self.id == o.id
    }
}

impl Perk {
    fn category(&self) -> u32 {
        self.info.category
    }
}

/// The PerkData `id` of the menu bundle (None: no id, or one the bundle lacks).
pub fn perk_data(view: &MenuView, id: Option<&str>) -> Option<Perk> {
    let id = id?;
    let info = view.bundle.menu.as_ref()?.perks.get(id)?;
    Some(Perk { id: id.to_string(), info: info.clone() })
}

fn perks_of(view: &MenuView, ids: &[Option<String>]) -> Vec<Option<Perk>> {
    ids.iter().map(|id| perk_data(view, id.as_deref())).collect()
}

fn fighter_perks<'a>(view: &'a MenuView, character: Option<&str>) -> Option<&'a FighterPerks> {
    view.fighters().get(character?)?.perks.as_ref()
}

/// The four slots' ids of perks, an empty one None.
fn ids_of(perks: &[Option<Perk>]) -> Vec<Option<String>> {
    perks.iter().map(|p| p.as_ref().map(|p| p.id.clone())).collect()
}

thread_local! {
    /// The port's GetCharacterSavedPerks / UpdateCharacterSavedPerks: a character's loadout for the program's life,
    /// by the port's character name (the build keeps it in the backend's profile; forced offline mode saves
    /// nothing). The four slots in order, an empty one None (GetEquippedPerks(false, false, false), Equipper @1117 /
    /// @1165), so a perk keeps its slot.
    static SAVED: RefCell<BTreeMap<String, Vec<Option<String>>>> = const { RefCell::new(BTreeMap::new()) };
}

/// The loadout a page keeps for `character` (none: empty).
pub fn saved_perks(character: Option<&str>) -> Vec<Option<String>> {
    character.map_or_else(Vec::new, |c| SAVED.with(|s| s.borrow().get(c).cloned().unwrap_or_default()))
}

/// (the port's own) The loadout the machine remembers for `character` (the pre-match screen's and the custom
/// lobby's): its first four slots.
pub fn remembered_perks(cx: &Cx<'_>, character: Option<&str>) -> Vec<Option<String>> {
    let Some(c) = character else { return Vec::new() };
    cx.prefs.perks.get(c).map(|v| v.iter().take(SLOTS).cloned().collect()).unwrap_or_default()
}

fn loadout_ids(cx: &Cx<'_>, remember: bool, character: Option<&str>) -> Vec<Option<String>> {
    if remember { remembered_perks(cx, character) } else { saved_perks(character) }
}

/// The pictures the page draws for a character: its lists' icons and the category backings (`MenuView::hold_pictures`).
pub fn perk_pictures(view: &MenuView, character: Option<&str>) -> Vec<String> {
    let mut out: Vec<String> = CATEGORY_ICON.iter().map(|s| s.to_string()).collect();
    if let Some(f) = fighter_perks(view, character) {
        for id in f.lists.iter().flatten() {
            if let Some(icon) = perk_data(view, Some(id)).and_then(|p| p.info.icon)
                && !out.contains(&icon)
            {
                out.push(icon);
            }
        }
    }
    out
}

/// The pictures a loadout draws for a list of perk ids: their icons and the category backings.
pub fn perk_list_pictures(view: &MenuView, ids: &[Option<String>]) -> Vec<String> {
    let mut out: Vec<String> = CATEGORY_ICON.iter().map(|s| s.to_string()).collect();
    for id in ids {
        if let Some(icon) = perk_data(view, id.as_deref()).and_then(|p| p.info.icon)
            && !out.contains(&icon)
        {
            out.push(icon);
        }
    }
    out
}

/// UI_PerkTooltipData: what a tile gives its tooltip.
#[derive(Clone, Debug)]
pub struct PerkTooltipData {
    pub perk: Perk,
    pub character: Option<String>,
    pub player_index: i32,
    pub equipped: bool,
    pub ally_equipped: bool,
    pub cost: f64,
    pub discount: f64,
}

impl PerkTooltipData {
    fn is_stacked(&self) -> bool {
        self.equipped && self.ally_equipped
    }
}

/// A WBP_TextTag's text and backing colour (its own PreConstruct / Construct write them from the instance).
fn set_tag(cx: &mut Cx<'_>, tag: WidgetId, t: Option<&str>, color: Option<Rgba>, text_color: Option<Rgba>) {
    let label = find_under(cx, tag, "TagText");
    if let Some(t) = t {
        wd(cx, label).text = t.to_string();
    }
    if let Some(c) = text_color {
        wd(cx, label).color = c;
    }
    if let Some(c) = color {
        for s in ["Left", "Middle", "Right"] {
            let b = find_under(cx, tag, &format!("TagBacking_{s}"));
            wd(cx, b).color = c;
        }
    }
}

/// ValidatedPerkOrdering (B.1.3): four slots, the signature perk in slot 0, no general perk there.
pub fn validated_perk_ordering(perks: &[Option<Perk>]) -> Vec<Option<Perk>> {
    let mut a: Vec<Option<Perk>> = perks.iter().take(SLOTS).cloned().collect();
    a.resize(SLOTS, None);
    if let Some(sig) = a.iter().position(|p| p.as_ref().is_some_and(|p| p.category() == SIGNATURE)) {
        a.swap(0, sig);
    }
    if a[0].as_ref().is_some_and(|p| p.category() != SIGNATURE) {
        if let Some(empty) = a.iter().position(Option::is_none) {
            a.swap(0, empty);
        }
        if a[0].as_ref().is_some_and(|p| p.category() != SIGNATURE) {
            a[0] = None;
        }
    }
    a
}

/// `f` on the tile controller of `w` (at once, or after the call that holds it).
fn on_item(cx: &mut Cx<'_>, w: WidgetId, f: impl FnOnce(&mut PerkItem, &mut Cx<'_>) + 'static) {
    run_on::<PerkItem>(cx, w, f);
}

// ---------- UI_FlyingWidget (UI_Functions "Fly Perk Item") ----------

/// A flight in the air: its ends, its start (the view's clock) and its time.
#[derive(Clone, Copy, Debug)]
struct Flight {
    from: WidgetId,
    to: WidgetId,
    off: [f64; 2],
    t0: f64,
    time: f64,
}

/// UIF AreWidgetsFlying?: a UI_FlyingWidget_C exists, here a flying tile on the view's viewport.
fn are_widgets_flying(cx: &mut Cx<'_>) -> bool {
    let roots: Vec<WidgetId> = cx.view.layers().iter().map(|l| l.w).collect();
    for r in roots {
        let kids = cx.view.tree().widget(r).children.clone();
        for w in std::iter::once(r).chain(kids) {
            if cx.view.controller::<PerkItem>(w).is_some_and(|i| i.flight.is_some()) {
                return true;
            }
        }
    }
    false
}

/// A tile's centre on the viewport plus the flight's offset (GetWidgetCenterPosition).
fn flight_centre(cx: &Cx<'_>, w: WidgetId, off: [f64; 2]) -> [f64; 2] {
    let [ww, wh] = cx.view.tree().arranged_size(w);
    let p = cx.view.local_to_viewport(w, ww / 2.0, wh / 2.0);
    [p[0] + off[0], p[1] + off[1]]
}

/// Fly Perk Item(From, To, Time): a new UI_Perks_Item with the source's perk and size on the viewport (z 5), the
/// target's content hidden for the flight and its Pulse at the end. UI_FlyingWidget: in real time, alpha =
/// clamp((now - start) / Time), Ease(0, 1, alpha, EaseOut, 3) = 1 - (1 - alpha)^3 clamped; the position (each end's
/// GetWidgetCenterPosition plus the offset -Dimensions / 2) set by SetPositionInViewport each frame (Delay 0 loop),
/// then the end position, EndAudio, the widget removed. StartAudio at the start (Play UI Audio If Visible). The
/// tracker widget (UI_FlyingWidgetTracker, an empty canvas) is not ported. `from` may be busy (the tile clicked):
/// its perk and size come with the call.
fn fly_perk_item(cx: &mut Cx<'_>, from: WidgetId, perk: Perk, dims: [f64; 2], to: WidgetId, time: f64) {
    let w = MenuView::create(cx, "UI_Perks_Item");
    cx.visit::<PerkItem, _>(w, |it, cx| {
        it.set_perk(cx, Some(perk));
        it.set_size(cx, dims[0], dims[1]);
    });
    let off = [dims[0] * -0.5, dims[1] * -0.5];
    let start = flight_centre(cx, from, off);
    MenuView::add_to_viewport_at(cx, w, 5, start[0], start[1]);
    on_item(cx, to, move |t, cx| {
        t.show_content(cx, false, 0.0);
        t.show_content(cx, true, time);
    });
    let t0 = cx.now();
    cx.post("ui_prematch_perk_block_fly_to_lockpoint");
    cx.visit::<PerkItem, _>(w, |it, cx| {
        it.flight = Some(Flight { from, to, off, t0, time });
        it.fly_step(cx);
    });
    on_item(cx, to, move |t, cx| t.pulse(cx, time, None));
}

// ---------- UI_Perks_Item ----------

const I_FOCUS: u32 = 1;
const I_UNFOCUS: u32 = 2;
const I_CLICK: u32 = 3;
const R_FLY: u32 = 4;
const R_PULSE: u32 = 5;
/// The delayed ShowContent: the id with the generation of the call in its low bits.
const R_SHOW_ON: u32 = 0x0001_0000;
const R_SHOW_OFF: u32 = 0x0002_0000;
/// ToggleEmptyAnimation's pulse: the generation in the low bits.
const R_EMPTY: u32 = 0x0003_0000;
const GEN_MASK: u32 = 0xFFFF;

/// UI_GridItemBase's tile with a perk (B.3.1). GetLocked is false here (every perk is owned: the port decision),
/// so the lock indicator never shows and no tile is dimmed by a lock.
#[derive(Debug)]
pub struct PerkItem {
    state: GridItemState,
    pub data: Option<Perk>,
    pub dims: [f64; 2],
    pub shows_tooltip: bool,
    pub shows_selected_state: bool,
    pub ally_equipped: bool,
    pub player_index: i32,
    pub character: Option<String>,
    empty_gen: u32,
    /// The delayed ShowContent calls' generation (a destructed tile's are dropped).
    show_gen: u32,
    /// The pulses waiting on their delay: when each is due and its audio.
    pulses: Vec<(f64, Option<String>)>,
    flight: Option<Flight>,
}

impl PerkItem {
    /// UI_GridItemBase CDO: Shows Tooltip, Shows Selected State; the instance's own.
    pub fn make(cx: &mut Cx<'_>) -> Box<dyn Controller> {
        let p = cx.view.tree().props(cx.w).clone();
        let b = |k: &str| p.get(k).and_then(Value::as_bool).unwrap_or(true);
        let dims = p
            .get("Dimensions")
            .map(|d| {
                [d.get("X").and_then(Value::as_f64).unwrap_or(64.0), d.get("Y").and_then(Value::as_f64).unwrap_or(64.0)]
            })
            .unwrap_or([64.0, 64.0]);
        Box::new(PerkItem {
            state: GridItemState::default(),
            data: None,
            dims,
            shows_tooltip: b("Shows Tooltip"),
            shows_selected_state: b("Shows Selected State"),
            ally_equipped: false,
            player_index: 0,
            character: None,
            empty_gen: 0,
            show_gen: 0,
            pulses: Vec::new(),
            flight: None,
        })
    }

    /// The tile's interactor.
    pub fn interactor(cx: &Cx<'_>, w: WidgetId) -> WidgetId {
        find_under(cx, w, "UI_Interactor")
    }

    pub fn selected(&mut self) -> bool {
        self.state.selected
    }

    /// Toggle Selected State / Shows Selected State of a loadout's slot.
    pub fn set_toggle_selected_state(&mut self, b: bool) {
        self.state.toggle_selected_state = b;
    }

    fn hover(cx: &Cx<'_>) -> WidgetId {
        find(cx, "hoverIndicator")
    }

    /// Focus @4542: a tile that is not focused shows its hover indicator, then FocusedEvent.
    fn focus(&mut self, cx: &mut Cx<'_>, player: i32) {
        let h = Self::hover(cx);
        if !is_visible(cx.view.tree().widget(h).vis) {
            wd(cx, h).vis = Vis::SelfHitTestInvisible;
        }
        self.state.focus(cx, player);
    }

    /// Unfocus: the hover indicator hidden.
    fn unfocus(&mut self, cx: &mut Cx<'_>) {
        let h = Self::hover(cx);
        wd(cx, h).vis = Vis::Hidden;
    }

    /// SetData @1116 (UI_GridItemBase SetData @7667 clears IsEntering).
    pub fn set_perk(&mut self, cx: &mut Cx<'_>, d: Option<Perk>) {
        self.data = d;
        self.state.is_entering = false;
        self.update_look(cx);
    }

    pub fn set_selected(&mut self, b: bool) {
        self.state.selected = b;
    }

    /// SetAllyEquipped @5378: Ally Equipped, then UpdateAppearance.
    pub fn set_ally_equipped(&mut self, cx: &mut Cx<'_>, b: bool) {
        self.ally_equipped = b;
        self.update_look(cx);
    }

    /// IsStacked?: selected, the ally has it too, and it has a stacked description.
    pub fn is_stacked(&self) -> bool {
        self.state.selected && self.ally_equipped && self.data.as_ref().is_some_and(|p| !p.info.stacked.is_empty())
    }

    /// GetTooltipData: UIF Get Tooltip Data for Perk.
    pub fn tooltip_data(&self) -> Option<PerkTooltipData> {
        let perk = self.data.clone()?;
        Some(PerkTooltipData {
            perk,
            character: self.character.clone(),
            player_index: self.player_index,
            equipped: self.state.selected,
            ally_equipped: self.ally_equipped,
            cost: 0.0,
            discount: 0.0,
        })
    }

    /// UI_GridItemBase ShowContent(Show, Delay): with a delay, again after it; a content that comes back unfocuses
    /// the tile for players 0..3.
    pub fn show_content(&mut self, cx: &mut Cx<'_>, show: bool, delay: f64) {
        if delay > 0.0 {
            let id = if show { R_SHOW_ON } else { R_SHOW_OFF } | (self.show_gen & GEN_MASK);
            cx.view.delay(delay, Handler { owner: cx.w, id });
            return;
        }
        let content = find(cx, "content");
        if show && !is_visible(cx.view.tree().widget(content).vis) {
            self.unfocus(cx);
        }
        cond(cx, content, show, Vis::SelfHitTestInvisible, Vis::Hidden);
    }

    /// UpdateAppearance @1260 (UI_GridItemBase's first, @6550: the selected and the locked indicators).
    fn update_look(&mut self, cx: &mut Cx<'_>) {
        let eq = find(cx, "equippedIndicatorP0");
        cond(cx, eq, self.state.selected && self.shows_selected_state, Vis::HitTestInvisible, Vis::Hidden);
        let lock = find(cx, "LockedIndicator");
        wd(cx, lock).vis = Vis::Hidden;
        let Some(p) = self.data.clone() else {
            self.show_content(cx, false, 0.0);
            self.update_tooltip(cx);
            return;
        };
        self.show_content(cx, true, 0.0);
        let initials: String = p.info.name.split_whitespace().filter_map(|s| s.chars().next()).collect();
        let ini = find(cx, "Initials");
        wd(cx, ini).text = initials;
        let cat = p.category() as usize;
        let icon = p.info.icon.clone().or_else(|| CATEGORY_ICON.get(cat).map(|s| s.to_string()));
        let entry = icon.as_deref().map(|i| resolve(&cx.view.bundle, i));
        // SetBrushFromSoftTexture
        if let (Some(_), Some(e)) = (&p.info.icon, &entry) {
            cx.view.load_texture(e);
        }
        let iw = find(cx, "icon");
        wd(cx, iw).tex = entry;
        let own_icon = icon.as_deref().is_some_and(|i| !i.to_lowercase().contains("ui_perkbg_"));
        cond(cx, ini, own_icon, Vis::Hidden, Vis::SelfHitTestInvisible);
        // @2204-2535: the ally's mark and the stacked look
        let ally = find(cx, "allyEquippedIndicator");
        let hide_ally = prop_bool(cx, "Hide Ally Equipped");
        cond(cx, ally, self.ally_equipped && !hide_ally, Vis::SelfHitTestInvisible, Vis::Hidden);
        let stacked = self.is_stacked();
        let a = if stacked { 42.0 } else { 32.0 };
        wd(cx, ally).transform.translation = [a, a];
        for n in ["stackedIndicator", "stackedGlow", "stackedColor"] {
            let w = find(cx, n);
            cond(cx, w, stacked, Vis::SelfHitTestInvisible, Vis::Hidden);
        }
        // @2811-4757: by category
        let sig = p.category() == SIGNATURE;
        let t = if sig { 48.0 } else { 32.0 };
        let s = if sig { 60.0 } else { 40.0 };
        {
            let e = wd(cx, eq);
            e.transform.translation = [t, t];
            e.brush_w = s;
            e.brush_h = s;
            e.color = [1.0, 0.527115, 0.0, 1.0];
        }
        {
            let l = wd(cx, lock);
            l.transform.translation = [t, t];
            l.w_override = s;
            l.h_override = s;
        }
        let c = CATEGORY_COLOR.get(cat).copied().unwrap_or(CATEGORY_COLOR[3]);
        for n in ["stackedGlow", "stackedColor"] {
            let w = find(cx, n);
            wd(cx, w).color = c;
        }
        let pe = palette_entry(cx, 0);
        wd(cx, ally).color = pe;
        // @5303: no ally group
        let g = find(cx, "groupIcon");
        wd(cx, g).vis = Vis::Hidden;
        self.update_tooltip(cx);
    }

    /// UpdateTooltip @800: the perk tooltip, following the mouse at (50, 50), MatchBottomLeft. Deviation: FollowMouse
    /// is not ported (tooltip.rs): the tip stays where it opened.
    fn update_tooltip(&mut self, cx: &mut Cx<'_>) {
        let i = Self::interactor(cx, cx.w);
        let p = match self.tooltip_data().filter(|_| self.shows_tooltip) {
            Some(d) => TooltipParams {
                tooltip: String::new(),
                cls: Some("UI_TooltipPerk_v2".into()),
                show: true,
                offset: [50.0, 50.0],
                delay: 0.0,
                placement: 12,
                data: Some(Rc::new(d)),
            },
            None => TooltipParams { cls: Some("UI_Tooltip".into()), ..no_tooltip() },
        };
        set_tooltip(cx, i, p);
    }

    /// Pulse(Delay, Audio) @5417.
    pub fn pulse(&mut self, cx: &mut Cx<'_>, delay: f64, audio: Option<String>) {
        if delay > 0.0 {
            let due = cx.now() + delay;
            self.pulses.push((due, audio));
            cx.view.delay(delay, Handler { owner: cx.w, id: R_PULSE });
        } else {
            Self::play_pulse(cx, audio);
        }
    }

    fn play_pulse(cx: &mut Cx<'_>, audio: Option<String>) {
        let me = cx.w;
        cx.view.play(me, "EquippedAnim", false, 1.0);
        if let Some(a) = audio {
            cx.post(&a);
        }
    }

    /// Set Dimmed @5524.
    pub fn set_dimmed(&mut self, cx: &mut Cx<'_>, d: bool) {
        let icon = find(cx, "icon");
        wd(cx, icon).color = if d { [0.066626, 0.104616, 0.155926, 1.0] } else { WHITE };
        let si = find(cx, "stackedIndicator");
        wd(cx, si).opacity = if d { 0.35 } else { 1.0 };
        let sc = find(cx, "stackedColor");
        wd(cx, sc).opacity = if d { 0.35 } else { 1.0 };
        let sg = find(cx, "stackedGlow");
        wd(cx, sg).opacity = if d { 0.0 } else { 1.0 };
        let bg = find(cx, "emptyBG");
        wd(cx, bg).color = [0.016807, 0.024158, 0.046665, 1.0];
    }

    /// Show Border @6034.
    pub fn show_border(&mut self, cx: &mut Cx<'_>, b: bool) {
        let w = find(cx, "EmptyBorder");
        cond(cx, w, b, Vis::SelfHitTestInvisible, Vis::Collapsed);
    }

    /// ToggleEmptyAnimation(Show, Delay) @6094: IsEmptyPulse after the delay, again every 3 s; off: StopAnimation
    /// (IsEmptyPulse) @6256.
    pub fn toggle_empty_animation(&mut self, cx: &mut Cx<'_>, show: bool, delay: f64) {
        self.empty_gen = self.empty_gen.wrapping_add(1);
        let me = cx.w;
        if !show {
            cx.view.stop_animation(me, "IsEmptyPulse");
            return;
        }
        cx.view.delay(delay, Handler { owner: me, id: R_EMPTY | (self.empty_gen & GEN_MASK) });
    }

    /// The flight's frame (UI_FlyingWidget's Delay 0 loop).
    fn fly_step(&mut self, cx: &mut Cx<'_>) {
        let Some(f) = self.flight else { return };
        let me = cx.w;
        let a = if f.time <= 0.0 { 1.0 } else { ((cx.now() - f.t0) / f.time).clamp(0.0, 1.0) };
        let e = (1.0 - (1.0 - a).powi(3)).clamp(0.0, 1.0);
        let from = flight_centre(cx, f.from, f.off);
        let to = flight_centre(cx, f.to, f.off);
        let place = |cx: &mut Cx<'_>, p: [f64; 2]| {
            let o = &mut wd(cx, me).offsets;
            o.l = p[0];
            o.t = p[1];
        };
        place(cx, [from[0] + (to[0] - from[0]) * e, from[1] + (to[1] - from[1]) * e]);
        if a < 1.0 {
            cx.view.delay(0.0, Handler { owner: me, id: R_FLY });
            return;
        }
        place(cx, to);
        cx.post("ui_prematch_perk_block_exit");
        self.flight = None;
        MenuView::remove_from_viewport(cx, me);
    }
}

impl GridItem for PerkItem {
    fn state(&mut self) -> &mut GridItemState {
        &mut self.state
    }

    fn locked(&self) -> bool {
        false
    }

    /// SetSize @0: Dimensions, the size box's overrides and maximum, the stacked colour's brush at 0.625.
    fn set_size(&mut self, cx: &mut Cx<'_>, x: f64, y: f64) {
        self.dims = [x, y];
        let b = find(cx, "sizeContainer");
        {
            let w = wd(cx, b);
            w.w_override = x;
            w.h_override = y;
            w.max_w = x;
            w.max_h = y;
        }
        let c = find(cx, "stackedColor");
        let w = wd(cx, c);
        w.brush_w = x * 0.625;
        w.brush_h = y * 0.625;
    }

    fn update_appearance(&mut self, cx: &mut Cx<'_>) {
        self.update_look(cx);
    }
}

impl Tile for PerkItem {
    fn interactors(&self, cx: &Cx<'_>) -> Vec<WidgetId> {
        vec![Self::interactor(cx, cx.w)]
    }

    fn set_data(&mut self, cx: &mut Cx<'_>, data: Option<TileData>) {
        let p = data.and_then(|d| d.perk().map(str::to_string)).and_then(|id| perk_data(cx.view, Some(&id)));
        self.set_perk(cx, p);
    }
}

impl Controller for PerkItem {
    /// UI_GridItemBase.OnInitialized @7538: the interactor's focus, unfocus and click.
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let i = Self::interactor(cx, cx.w);
        bind(cx, i, |s| &mut s.on_focus, I_FOCUS);
        bind(cx, i, |s| &mut s.on_unfocus, I_UNFOCUS);
        bind(cx, i, |s| &mut s.on_click, I_CLICK);
    }

    /// PreConstruct @891: SetSize(Dimensions).
    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        let [x, y] = self.dims;
        self.set_size(cx, x, y);
        self.update_look(cx);
    }

    fn destruct(&mut self, _cx: &mut Cx<'_>) {
        self.empty_gen = self.empty_gen.wrapping_add(1);
        self.show_gen = self.show_gen.wrapping_add(1);
        self.pulses.clear();
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        let player = arg.downcast_ref::<i32>().copied().unwrap_or(0);
        match id {
            I_FOCUS => self.focus(cx, player),
            I_UNFOCUS => self.unfocus(cx),
            I_CLICK => GridItemState::select(self, cx, player),
            _ => {}
        }
    }

    fn resume(&mut self, cx: &mut Cx<'_>, id: u32) {
        match id {
            GRID_READY | GRID_WAIT => GridItemState::resume(self, cx, id),
            R_FLY => self.fly_step(cx),
            R_PULSE => {
                // the earliest pulse due
                let Some(at) = (0..self.pulses.len()).min_by(|&a, &b| self.pulses[a].0.total_cmp(&self.pulses[b].0))
                else {
                    return;
                };
                let (_, audio) = self.pulses.remove(at);
                Self::play_pulse(cx, audio);
            }
            _ if id & !GEN_MASK == R_SHOW_ON || id & !GEN_MASK == R_SHOW_OFF => {
                if id & GEN_MASK == self.show_gen & GEN_MASK {
                    self.show_content(cx, id & !GEN_MASK == R_SHOW_ON, 0.0);
                }
            }
            _ if id & !GEN_MASK == R_EMPTY => {
                if id & GEN_MASK != self.empty_gen & GEN_MASK {
                    return;
                }
                let me = cx.w;
                cx.view.play(me, "IsEmptyPulse", false, 1.0);
                cx.view.delay(3.0, Handler { owner: me, id });
            }
            _ => {}
        }
    }

    fn as_grid_item(&mut self) -> Option<&mut dyn Tile> {
        Some(self)
    }
}

// ---------- UI_PerkLockIndicator ----------

/// B.3.6. Every instance in the bundle is Disabled, so ToggleVisibility leaves it Hidden whatever IsLocked? says
/// (the server profile's slot counts, unanswered offline).
#[derive(Debug, Default)]
pub struct PerkLockIndicator;

impl PerkLockIndicator {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(PerkLockIndicator)
    }

    pub fn set_dimensions(&mut self, cx: &mut Cx<'_>, d: f64) {
        let b = find(cx, "customSize");
        let w = wd(cx, b);
        w.w_override = d.max(0.0);
        w.h_override = d.max(0.0);
    }

    /// SetCharacter @960 -> UpdateAppearance @15 -> ToggleVisibility @994.
    pub fn set_character(&mut self, cx: &mut Cx<'_>, c: Option<&str>) {
        let me = cx.w;
        let disabled = prop_bool(cx, "Disabled");
        cond(cx, me, c.is_some() && !disabled, Vis::SelfHitTestInvisible, Vis::Hidden);
    }
}

impl Controller for PerkLockIndicator {
    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        let d = prop_f64(cx, "Dimensions", -1.0);
        self.set_dimensions(cx, d);
    }
}

// ---------- UI_Perks_StackedCallout ----------

const R_CALLOUT_HIDE: u32 = 0x0001_0000;

/// B.3.4. Shown by the loadout (Do Refresh Stacked) for a slot whose perk the ally has too.
#[derive(Debug, Default)]
pub struct PerksStackedCallout {
    hide_gen: u32,
}

impl PerksStackedCallout {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::<PerksStackedCallout>::default()
    }

    /// Show @10: TriggerAnimation(EnterAnim), Hide after Visible Time.
    pub fn show(&mut self, cx: &mut Cx<'_>) {
        self.hide_gen = self.hide_gen.wrapping_add(1);
        let me = cx.w;
        cx.view.stop_all_animations(me);
        cx.view.play(me, "EnterAnim", false, 1.0);
        let t = prop_f64(cx, "Visible Time", 4.0);
        cx.view.delay(t, Handler { owner: me, id: R_CALLOUT_HIDE | (self.hide_gen & GEN_MASK) });
    }

    /// Hide @150.
    pub fn hide(&mut self, cx: &mut Cx<'_>) {
        let c = find(cx, "visibilityContainer");
        if cx.view.tree().widget(c).opacity <= 0.0 {
            return;
        }
        self.hide_gen = self.hide_gen.wrapping_add(1);
        let me = cx.w;
        cx.view.stop_all_animations(me);
        cx.view.play(me, "ExitAnim", false, 1.0);
    }
}

impl Controller for PerksStackedCallout {
    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        let n = prop_f64(cx, "Slot Number", 1.0);
        let t = game_text(cx, T_SLOT_STACKED).replace("{number}", &n.to_string());
        let l = find(cx, "calloutLabel");
        wd(cx, l).text = t;
    }

    fn construct(&mut self, cx: &mut Cx<'_>) {
        let c = find(cx, "visibilityContainer");
        wd(cx, c).opacity = 0.0;
    }

    fn destruct(&mut self, _cx: &mut Cx<'_>) {
        self.hide_gen = self.hide_gen.wrapping_add(1);
    }

    fn resume(&mut self, cx: &mut Cx<'_>, id: u32) {
        if id & !GEN_MASK == R_CALLOUT_HIDE && id & GEN_MASK == self.hide_gen & GEN_MASK {
            self.hide(cx);
        }
    }
}

// ---------- UI_Perks_AddButton ----------

const A_FOCUS: u32 = 1;
const A_UNFOCUS: u32 = 2;
const A_CLICK: u32 = 3;

/// B.3.3. A list shows it only when the character does not own all its perks: never here (all owned).
#[derive(Debug, Default)]
pub struct PerksAddButton {
    pub on_click: Event<i32>,
}

impl PerksAddButton {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::<PerksAddButton>::default()
    }

    /// SetDimensions @316.
    pub fn set_dimensions(&mut self, cx: &mut Cx<'_>, d: f64) {
        let s = find(cx, "size");
        let w = wd(cx, s);
        w.w_override = d;
        w.h_override = d;
    }

    /// UpdateContext @94.
    pub fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str, player: i32) {
        let i = find(cx, "UI_Interactor");
        if let Some(s) = ist(cx, i) {
            s.context = ctx.to_string();
            s.player_index = player;
        }
    }
}

impl Controller for PerksAddButton {
    /// @403: the tooltip; @197 / @240 focus; @283 click.
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let i = find(cx, "UI_Interactor");
        let tip = game_text(cx, T_TRAIN);
        set_tooltip(cx, i, tooltip_params(&tip, true, 0.0, 0.0, 2));
        bind(cx, i, |s| &mut s.on_focus, A_FOCUS);
        bind(cx, i, |s| &mut s.on_unfocus, A_UNFOCUS);
        bind(cx, i, |s| &mut s.on_click, A_CLICK);
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        let focused = find(cx, "focused");
        let backing = find(cx, "backing");
        match id {
            A_FOCUS => {
                wd(cx, focused).vis = Vis::SelfHitTestInvisible;
                wd(cx, backing).opacity = 1.0;
            }
            A_UNFOCUS => {
                wd(cx, focused).vis = Vis::Hidden;
                wd(cx, backing).opacity = 0.4;
            }
            A_CLICK => {
                let p = arg.downcast_ref::<i32>().copied().unwrap_or(0);
                cx.emit(self, |c| &c.on_click, &p);
            }
            _ => {}
        }
    }
}

// ---------- UI_TooltipPerkDescription ----------

#[derive(Debug, Default)]
pub struct TooltipPerkDescription;

impl TooltipPerkDescription {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(TooltipPerkDescription)
    }

    /// Set Perk Tooltip Data @3338 -> @2889.
    pub fn set_perk_tooltip_data(&mut self, cx: &mut Cx<'_>, d: Option<&PerkTooltipData>) {
        let Some(d) = d else { return };
        let p = &d.perk;
        let set_text = |cx: &mut Cx<'_>, n: &str, t: &str| {
            let w = find(cx, n);
            wd(cx, w).text = t.to_string();
        };
        let set_vis = |cx: &mut Cx<'_>, n: &str, v: Vis| {
            let w = find(cx, n);
            wd(cx, w).vis = v;
        };
        set_text(cx, "perkLabel", &p.info.name);
        set_text(cx, "richDescription", &p.info.description);
        set_vis(cx, "infoContainer", Vis::SelfHitTestInvisible);
        set_vis(cx, "unlockRequirementsContainer", Vis::Hidden);
        if p.info.stacked.is_empty() {
            set_vis(cx, "stackedIndicator", Vis::Collapsed);
            set_vis(cx, "richStacked", Vis::Collapsed);
            return;
        }
        set_text(cx, "richStacked", &p.info.stacked);
        set_vis(cx, "stackedIndicator", Vis::HitTestInvisible);
        set_vis(cx, "richStacked", Vis::HitTestInvisible);
        // @1401: the stacked look
        let stacked = d.is_stacked();
        let tag = find(cx, "stackedTag");
        wd(cx, tag).h_align = if stacked { HAlign::Fill } else { HAlign::Center };
        let div = find(cx, "stackeddiv");
        cond(cx, div, stacked, Vis::Collapsed, Vis::SelfHitTestInvisible);
        let color = if stacked {
            CATEGORY_COLOR.get(p.category() as usize).copied().unwrap_or(CATEGORY_COLOR[3])
        } else {
            [0.262251, 0.366253, 0.396755, 1.0]
        };
        wd(cx, div).color = color;
        let t = game_text(cx, if stacked { T_STACKED } else { T_ON_STACKED });
        set_tag(cx, tag, Some(&t), Some(color), Some(OFF_WHITE));
        let rich = find(cx, "richStacked");
        wd(cx, rich).opacity = if stacked { 1.0 } else { 0.6 };
    }
}

impl Controller for TooltipPerkDescription {
    /// PreConstruct: perkLabel in Text Color, and SetForegroundColor(Text Color). Deviation: the description's rich
    /// style (UI_PerkTooltipDescriptionStyle) uses the foreground colour, which the port's rich text does not take:
    /// its text keeps the style's own colour.
    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        let c = prop(cx, "Text Color");
        if c.is_object() {
            let l = find(cx, "perkLabel");
            wd(cx, l).color = crate::menu::widgets::common::rgba(&c);
        }
    }
}

// ---------- UI_TooltipPerk_v2 ----------

const R_TIP_SHOW: u32 = 1;

/// B.3.7: the perk tooltip of a tile. Set Text / Hide are empty; the cost part shows for a perk with a price (none
/// offline).
#[derive(Debug, Default)]
pub struct TooltipPerk;

impl TooltipPerk {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(TooltipPerk)
    }

    fn item(cx: &Cx<'_>) -> WidgetId {
        find(cx, "UI_Perks_Item")
    }

    /// Set Data @282 -> Update Appearance @524.
    fn set_data(&mut self, cx: &mut Cx<'_>, d: &PerkTooltipData) {
        let desc = find(cx, "UI_TooltipPerkDescription");
        let d2 = d.clone();
        cx.visit::<TooltipPerkDescription, _>(desc, move |t, cx| t.set_perk_tooltip_data(cx, Some(&d2)));
        let item = Self::item(cx);
        let perk = d.perk.clone();
        let stacked = d.is_stacked();
        cx.visit::<PerkItem, _>(item, move |it, cx| {
            it.set_perk(cx, Some(perk));
            let s = find(cx, "shadow");
            wd(cx, s).vis = Vis::Collapsed;
            for n in ["stackedIndicator", "stackedGlow", "stackedColor"] {
                let w = find(cx, n);
                cond(cx, w, stacked, Vis::SelfHitTestInvisible, Vis::Collapsed);
            }
        });
        // (@1042 sets the tile's Dimensions variable to 240 / 180 without SetSize: nothing drawn changes)
        let sig = d.perk.category() == SIGNATURE;
        let label = find(cx, "RewardType");
        let t = game_text(cx, if sig { T_SIGNATURE_PERK } else { T_PERK });
        let c: Rgba = if sig { [1.0, 0.223228, 0.002428, 1.0] } else { [0.194618, 0.651406, 0.630757, 1.0] };
        {
            let l = wd(cx, label);
            l.text = t;
            l.color = c;
        }
        let q = find(cx, "QuadBG_Trim");
        cx.visit::<Quadrilateral, _>(q, |q, cx| q.update_color(cx, c));
        // TogglePurchaseable(Cost > 0) @2442
        let buy = d.cost > 0.0;
        let cost = find(cx, "costInfoContainer");
        cond(cx, cost, buy, Vis::SelfHitTestInvisible, Vis::Collapsed);
        if !buy {
            let o = find(cx, "otherCharacterLabel");
            wd(cx, o).vis = Vis::Collapsed;
        }
    }
}

impl Controller for TooltipPerk {
    /// OnInitialized @212.
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        wd(cx, me).opacity = 0.0;
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        match id {
            TIP_SET_TEXT => {}
            // Show @157: on the next frame
            TIP_SHOW => {
                let me = cx.w;
                cx.view.delay(0.0, Handler { owner: me, id: R_TIP_SHOW });
            }
            TIP_SET_DATA => {
                let Some(d) = arg.downcast_ref::<Rc<dyn Any>>() else { return };
                if let Some(d) = d.downcast_ref::<PerkTooltipData>() {
                    let d = d.clone();
                    self.set_data(cx, &d);
                }
            }
            _ => {}
        }
    }

    /// Show's next frame: seen, and its tile takes no input.
    fn resume(&mut self, cx: &mut Cx<'_>, id: u32) {
        if id != R_TIP_SHOW {
            return;
        }
        let me = cx.w;
        wd(cx, me).opacity = 1.0;
        let item = Self::item(cx);
        let i = PerkItem::interactor(cx, item);
        set_interactable(cx, i, false);
        wd(cx, i).vis = Vis::Hidden;
    }
}

// ---------- UI_Perks_Sublist ----------

/// A tile's event of a list: the perk, the player, the tile.
#[derive(Clone, Debug)]
pub struct PerkEvent {
    pub perk: Perk,
    pub player: i32,
    pub item: WidgetId,
}

const S_SELECTED: u32 = 1;
const S_FOCUSED: u32 = 2;

/// B.3.2: one category's list (its `list` a UI_GridItemList of UI_Perks_Item). Its perks are the character's list
/// of that category from the bundle (the port decision: all owned, so no tile is locked or dimmed and the add button
/// never shows). The perk trainer modal keeps its lists empty through SetPerks.
#[derive(Debug)]
pub struct PerksSublist {
    pub perk_selected_event: Event<PerkEvent>,
    pub perk_focused_event: Event<PerkEvent>,
    /// PerksPopulatedEvent(the sublist's widget).
    pub perks_populated_event: Event<WidgetId>,
    pub add_perk_event: Event<(i32, i32)>,
    pub player_index: i32,
    pub hide_tooltips: bool,
    pub unlock_all: bool,
    pub scrollbox: Option<WidgetId>,
    character: Option<String>,
    /// The list's tiles and their perks, as its last Populate made them.
    items: Vec<(WidgetId, Option<String>)>,
    /// Ally Equipped Perks (SetAllyEquipped), by perk id.
    ally_equipped: Vec<String>,
}

impl PerksSublist {
    pub fn make(cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(PerksSublist {
            perk_selected_event: Event::default(),
            perk_focused_event: Event::default(),
            perks_populated_event: Event::default(),
            add_perk_event: Event::default(),
            player_index: 0,
            hide_tooltips: prop_bool(cx, "Hide Tooltips"),
            unlock_all: prop_bool(cx, "Unlock All"),
            scrollbox: None,
            character: None,
            items: Vec::new(),
            ally_equipped: Vec::new(),
        })
    }

    fn list(cx: &Cx<'_>) -> WidgetId {
        find(cx, "list")
    }

    /// The Category property of sublist widget `w`.
    pub fn category_of(cx: &Cx<'_>, w: WidgetId) -> u32 {
        enum_category(cx.view.tree().props(w).get("Category").unwrap_or(&Value::Null))
    }

    fn show_empty_text(cx: &Cx<'_>) -> bool {
        cx.view.tree().props(cx.w).get("Show Empty Text").and_then(Value::as_bool).unwrap_or(true)
    }

    /// The tiles' widgets in order.
    pub fn items(&self) -> Vec<WidgetId> {
        self.items.iter().map(|i| i.0).collect()
    }

    /// list.Item Size = (Perk Dimensions, Perk Dimensions): the next Populate's tiles.
    pub fn set_perk_dimensions(&mut self, cx: &mut Cx<'_>, d: f64) {
        let l = Self::list(cx);
        cx.view.tree_mut().set_prop(l, "Item Size", json!({ "X": d, "Y": d }));
    }

    /// @3934: Force Grid Columns, 3 for the signature list.
    pub fn set_force_grid_columns(&mut self, cx: &mut Cx<'_>, n: i32) {
        let l = Self::list(cx);
        let n = if Self::category_of(cx, cx.w) == SIGNATURE { 3 } else { n };
        cx.view.tree_mut().set_prop(l, "Force Grid Columns", json!(n));
    }

    pub fn set_header_text(&mut self, cx: &mut Cx<'_>, t: &str) {
        let h = find(cx, "headerLabel");
        wd(cx, h).text = t.to_string();
    }

    /// SetCharacter(Character, Ally Perks) @4815: Unlock All or not, the list is the character's list of this
    /// category (all owned).
    pub fn set_character(&mut self, cx: &mut Cx<'_>, c: Option<&str>) {
        self.character = c.map(str::to_string);
        let cat = Self::category_of(cx, cx.w) as usize;
        let ids = fighter_perks(cx.view, c).and_then(|f| f.lists.get(cat)).cloned().unwrap_or_default();
        let perks: Vec<String> = ids.into_iter().filter(|id| perk_data(cx.view, Some(id)).is_some()).collect();
        self.set_perks(cx, perks);
    }

    /// SetPerks @9278 -> @8225 (no ally group: the ally perks are [] without ally mode). The list's
    /// PerksPopulated (OnListPopulated) runs as Populate returns.
    pub fn set_perks(&mut self, cx: &mut Cx<'_>, perks: Vec<String>) {
        let n = perks.len();
        let l = Self::list(cx);
        let data: Vec<TileData> = perks.into_iter().map(TileData::Perk).collect();
        // the list fills itself at once: its records are read as Populate returns
        let recs = cx
            .visit::<GridItemList, _>(l, move |list, cx| {
                list.populate(cx, data);
                list.items().into_iter().map(|w| (w, list.record(w).and_then(|r| r.0))).collect::<Vec<_>>()
            })
            .unwrap_or_default();
        self.items = recs.into_iter().map(|(w, d)| (w, d.and_then(|d| d.perk().map(str::to_string)))).collect();
        self.on_list_populated(cx);
        let empty = find(cx, "noPerksLabel");
        let show = Self::show_empty_text(cx) && n == 0;
        cond(cx, empty, show, Vis::SelfHitTestInvisible, Vis::Collapsed);
    }

    /// Clear @9366.
    pub fn clear(&mut self, cx: &mut Cx<'_>) {
        self.set_perks(cx, Vec::new());
    }

    /// SetAllyEquipped @8590: Ally Equipped Perks, then each tile's SetAllyEquipped(PerksContainsPerk?) (@1764).
    pub fn set_ally_equipped(&mut self, cx: &mut Cx<'_>, ids: &[String]) {
        self.ally_equipped = ids.to_vec();
        for (w, perk) in self.items.clone() {
            let on = perk.is_some_and(|p| ids.contains(&p));
            on_item(cx, w, move |it, cx| it.set_ally_equipped(cx, on));
        }
    }

    /// OnItemAdded @6491 for each tile, then OnListPopulated @8668 (SetAllyEquipped(Ally Equipped Perks); no add
    /// button: all owned).
    fn on_list_populated(&mut self, cx: &mut Cx<'_>) {
        let character = if self.unlock_all { None } else { self.character.clone() };
        for (w, perk) in self.items.clone() {
            let ally = perk.is_some_and(|p| self.ally_equipped.contains(&p));
            let sb = self.scrollbox;
            let show = !self.hide_tooltips;
            let p = self.player_index;
            let c = character.clone();
            on_item(cx, w, move |it, cx| {
                let i = PerkItem::interactor(cx, cx.w);
                if let Some(sb) = sb
                    && let Some(s) = ist(cx, i)
                {
                    s.scroll_box = Some(sb);
                }
                it.shows_tooltip = show;
                it.player_index = p;
                it.set_dimmed(cx, false);
                // @7530: with Unlock All the tile has no Character
                it.character = c;
                it.ally_equipped = ally;
                it.update_look(cx);
            });
        }
        let me = cx.w;
        cx.emit(self, |c| &c.perks_populated_event, &me);
    }

    /// SetEquippedPerks @9218, and the equipper's own marking of its lists (Equip Perk @2523, UpdateAppearance):
    /// each tile selected when its perk is among `ids`. `deselect` first: the list's DeselectAll.
    pub fn mark_equipped(&mut self, cx: &mut Cx<'_>, ids: &[String], deselect: bool) {
        for (w, perk) in self.items.clone() {
            let on = perk.as_ref().is_some_and(|p| ids.contains(p));
            on_item(cx, w, move |it, cx| {
                if deselect {
                    GridItemState::deselect(it, cx);
                }
                it.set_selected(on);
                it.update_look(cx);
            });
        }
    }

    /// EquippablePerks: forced offline counts every tile.
    pub fn equippable_perks(&self) -> Vec<String> {
        self.items.iter().filter_map(|i| i.1.clone()).collect()
    }

    /// IndexForData.
    pub fn index_for_data(&self, id: &str) -> Option<usize> {
        self.items.iter().position(|i| i.1.as_deref() == Some(id))
    }

    pub fn focus_index(&mut self, cx: &mut Cx<'_>, player: i32, index: usize) {
        let l = Self::list(cx);
        run_on::<GridItemList>(cx, l, move |list, cx| list.focus_index(cx, player, index));
    }

    pub fn deselect_all(&mut self, cx: &mut Cx<'_>) {
        for (w, _) in self.items.clone() {
            on_item(cx, w, |it, cx| GridItemState::deselect(it, cx));
        }
    }

    pub fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str) {
        let l = Self::list(cx);
        let c = ctx.to_string();
        run_on::<GridItemList>(cx, l, move |list, cx| list.update_context(cx, &c));
        for (w, _) in self.items.clone() {
            let i = PerkItem::interactor(cx, w);
            if let Some(s) = ist(cx, i) {
                s.player_index = self.player_index;
            }
        }
    }

    fn perk_event(&self, cx: &Cx<'_>, ev: &TileEvent) -> Option<PerkEvent> {
        let id = ev.data.as_ref()?.perk()?;
        Some(PerkEvent { perk: perk_data(cx.view, Some(id))?, player: ev.player, item: ev.item })
    }
}

impl Controller for PerksSublist {
    /// Add List Bindings @9356.
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        let l = Self::list(cx);
        cx.visit::<GridItemList, _>(l, |list, _| {
            list.selected_event.add(Handler { owner: me, id: S_SELECTED });
            list.focused_event.add(Handler { owner: me, id: S_FOCUSED });
        });
    }

    /// PreConstruct @2283.
    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        let p = cx.view.tree().props(cx.w).clone();
        let cat = Self::category_of(cx, cx.w) as usize;
        let src = |v: Option<&Value>| -> String {
            v.and_then(|t| t.get("SourceString").or_else(|| t.get("CultureInvariantString")))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string()
        };
        let custom_header = src(p.get("Custom Header"));
        let name = T_CATEGORY.get(cat).map(|k| game_text(cx, k)).unwrap_or_default();
        self.set_perk_dimensions(cx, p.get("Perk Dimensions").and_then(Value::as_f64).unwrap_or(120.0));
        let l = Self::list(cx);
        wd(cx, l).transform.translation[0] = p.get("List X Offset").and_then(Value::as_f64).unwrap_or(0.0);
        let header = find(cx, "headerLabel");
        let show_header = p.get("Show Header").and_then(Value::as_bool).unwrap_or(true);
        cond(cx, header, show_header, Vis::SelfHitTestInvisible, Vis::Collapsed);
        wd(cx, header).text = if custom_header.is_empty() { name.clone() } else { custom_header.clone() };
        // @3000-3758: the switch reads Custom Header's emptiness for the empty text too
        let empty_text = if custom_header.is_empty() {
            game_text(cx, T_NO_PERKS).replace("{category}", &name)
        } else {
            src(p.get("Custom Empty Text"))
        };
        let empty = find(cx, "noPerksLabel");
        wd(cx, empty).text = empty_text;
        let cols = p.get("ForceGridColumns").and_then(Value::as_i64).unwrap_or(5) as i32;
        self.set_force_grid_columns(cx, cols);
        // @4195 / @4624: the list's slot
        let sig = cat as u32 == SIGNATURE;
        {
            let w = wd(cx, l);
            w.h_align = if sig { HAlign::Center } else { HAlign::Left };
            w.pad = if sig {
                Margin { l: 0.0, t: 0.0, r: 0.0, b: -30.0 }
            } else {
                Margin { l: 0.0, t: 0.0, r: -50.0, b: -30.0 }
            };
        }
        let show = Self::show_empty_text(cx);
        cond(cx, empty, show, Vis::SelfHitTestInvisible, Vis::Collapsed);
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        match id {
            S_SELECTED => {
                let Some((ev, _)) = arg.downcast_ref::<(TileEvent, bool)>() else { return };
                // @7674: a locked tile (none here) is deselected silently
                if ev.player != self.player_index {
                    return;
                }
                let Some(pe) = self.perk_event(cx, ev) else { return };
                cx.emit(self, |c| &c.perk_selected_event, &pe);
            }
            S_FOCUSED => {
                let Some(ev) = arg.downcast_ref::<TileEvent>() else { return };
                let Some(pe) = self.perk_event(cx, ev) else { return };
                cx.emit(self, |c| &c.perk_focused_event, &pe);
            }
            _ => {}
        }
    }
}

// ---------- UI_Perks_Loadout ----------

/// The slot tiles of a loadout, the signature slot first (GetAllPerks).
const SLOT_ITEMS: [&str; 4] = ["perkSignature", "perkCommon1", "perkCommon2", "perkCommon3"];
const SLOT_LOCKS: [&str; 4] = ["perkLockSignature", "perkLockCommon1", "perkLockCommon2", "perkLockCommon3"];
/// A slot tile's SelectedEvent: the slot in the low bits.
const L_SLOT_SELECTED: u32 = 0x0001_0000;
const R_CHARACTER_INFO: u32 = 1;
/// Do Refresh Stacked's delay: the generation in the low bits.
const R_STACKED: u32 = 0x0001_0000;

/// The selector's character the loadout's info shows (labelCharacter, labelSkin, the level tag).
#[derive(Clone, Debug)]
pub struct LoadoutSelection {
    pub name: String,
    pub skin: String,
    /// The level the tag shows; None: a bot, whose tag reads `custom_text::BOT_TAG` (the port's own).
    pub level: Option<i32>,
}

/// PerkSelectedEvent(Perk Data, Slot, Grid Item).
pub type LoadoutPick = (Option<Perk>, i32, WidgetId);

/// B.1: the four slots (0 the signature slot). The character's info comes from the pre-match selector in the
/// build: `selection` here, set by the host; none, nothing is written.
#[derive(Debug)]
pub struct PerksLoadout {
    pub perk_selected_event: Event<LoadoutPick>,
    pub character: Option<String>,
    pub player_index: i32,
    pub large_perk_size: f64,
    pub perk_margin: f64,
    pub show_stacked_callouts: bool,
    pub hide_tooltips: bool,
    pub selection: Option<LoadoutSelection>,
    /// The loadout is the machine's remembered one (`remembered_perks`), not the page's.
    pub remember: bool,
    /// The slots' perks, as SetPerkForSlot left them.
    perks: [Option<Perk>; 4],
    /// Ally Equipped Perks (SetAllyEquipped), by perk id.
    ally_equipped: Vec<String>,
    /// Stacked Timer: only the latest Refresh Stacked's delay runs.
    stacked_gen: u32,
}

impl PerksLoadout {
    pub fn make(cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(PerksLoadout {
            perk_selected_event: Event::default(),
            character: None,
            player_index: prop_f64(cx, "Player Index", 0.0) as i32,
            large_perk_size: prop_f64(cx, "Large Perk Size", 200.0),
            perk_margin: prop_f64(cx, "Perk Margin", 20.0),
            show_stacked_callouts: prop_bool(cx, "Show Stacked Callouts"),
            hide_tooltips: prop_bool(cx, "Hide Tooltips"),
            selection: None,
            remember: false,
            perks: Default::default(),
            ally_equipped: Vec::new(),
            stacked_gen: 0,
        })
    }

    /// The slot tiles' widgets of loadout widget `w` (GetAllPerks).
    pub fn items_of(cx: &Cx<'_>, w: WidgetId) -> [WidgetId; 4] {
        SLOT_ITEMS.map(|n| find_under(cx, w, n))
    }

    fn items(cx: &Cx<'_>) -> [WidgetId; 4] {
        Self::items_of(cx, cx.w)
    }

    fn locks(cx: &Cx<'_>) -> [WidgetId; 4] {
        SLOT_LOCKS.map(|n| find(cx, n))
    }

    /// The four slots' perks.
    pub fn perks(&self) -> &[Option<Perk>; 4] {
        &self.perks
    }

    /// SetCharacter @10436: the saved loadout (the page's, or the machine's where `remember` is set).
    pub fn set_character(&mut self, cx: &mut Cx<'_>, c: Option<&str>) {
        self.character = c.map(str::to_string);
        self.update_character_info(cx);
        for l in Self::locks(cx) {
            cx.visit::<PerkLockIndicator, _>(l, |li, cx| li.set_character(cx, c));
        }
        let ids = loadout_ids(cx, self.remember, c);
        let perks = perks_of(cx.view, &ids);
        self.set_perks(cx, &perks);
    }

    /// SetPerks @8137.
    pub fn set_perks(&mut self, cx: &mut Cx<'_>, perks: &[Option<Perk>]) {
        self.clear(cx);
        for (i, p) in validated_perk_ordering(perks).into_iter().enumerate() {
            self.set_perk_for_slot(cx, p, i);
        }
    }

    /// SetPerkForSlot @8853.
    pub fn set_perk_for_slot(&mut self, cx: &mut Cx<'_>, p: Option<Perk>, slot: usize) {
        if slot >= SLOTS {
            return;
        }
        self.perks[slot] = p.clone();
        let item = Self::items(cx)[slot];
        on_item(cx, item, move |it, cx| {
            let on = p.is_some();
            it.set_perk(cx, p);
            it.set_selected(on);
            it.update_look(cx);
        });
        // @9145
        let ally = self.ally_equipped.clone();
        self.set_ally_equipped(cx, &ally);
    }

    /// SetAllyEquipped @9174 -> @2608: Ally Equipped Perks, each slot's SetAllyEquipped(PerksContainsPerk?)
    /// @1023-1357, then Refresh Stacked @1403.
    pub fn set_ally_equipped(&mut self, cx: &mut Cx<'_>, ids: &[String]) {
        self.ally_equipped = ids.to_vec();
        for (slot, item) in Self::items(cx).into_iter().enumerate() {
            let on = self.perks[slot].as_ref().is_some_and(|p| ids.contains(&p.id));
            on_item(cx, item, move |it, cx| it.set_ally_equipped(cx, on));
        }
        self.refresh_stacked(cx);
    }

    /// Refresh Stacked @18029: K2_InvalidateTimerHandle(Stacked Timer), then K2_SetTimerDelegate(Do Refresh
    /// Stacked, 0.25): setting the same delegate's timer again restarts it, so Do Refresh Stacked runs once, 0.25 s
    /// after the last call (the generation drops the earlier delays).
    fn refresh_stacked(&mut self, cx: &mut Cx<'_>) {
        self.stacked_gen = self.stacked_gen.wrapping_add(1);
        let me = cx.w;
        cx.view.delay(0.25, Handler { owner: me, id: R_STACKED | (self.stacked_gen & GEN_MASK) });
    }

    /// Do Refresh Stacked @18170 -> @3751: for the common slots 1-3, UI_Perks_StackedCallout{slot} Show when the
    /// slot's perk is valid, among Ally Equipped Perks and has a StackedDescription, else Hide (@2712-3519).
    fn do_refresh_stacked(&mut self, cx: &mut Cx<'_>) {
        for slot in 1..SLOTS {
            let stacked = self.perks[slot]
                .as_ref()
                .is_some_and(|p| self.ally_equipped.contains(&p.id) && !p.info.stacked.is_empty());
            let c = find(cx, &format!("UI_Perks_StackedCallout{slot}"));
            cx.visit::<PerksStackedCallout, _>(c, move |c, cx| if stacked { c.show(cx) } else { c.hide(cx) });
        }
    }

    /// Clear @11040.
    pub fn clear(&mut self, cx: &mut Cx<'_>) {
        self.dismiss_tooltips(cx);
        for i in 0..SLOTS {
            self.set_perk_for_slot(cx, None, i);
        }
    }

    /// GetEquippedPerks(Omit Invalids, ...): every slot's perk is owned.
    pub fn equipped_perks(&self, omit_invalids: bool) -> Vec<Option<Perk>> {
        self.perks.iter().filter(|p| !omit_invalids || p.is_some()).cloned().collect()
    }

    /// Update Character Info @10302: after 0.2 s, from the selector (also the pre-match screen's, PM@40548).
    pub fn update_character_info(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        cx.view.delay(0.2, Handler { owner: me, id: R_CHARACTER_INFO });
    }

    /// SetStatus @18757.
    pub fn set_status(&mut self, cx: &mut Cx<'_>, ready_check: bool, spinner: bool) {
        self.show_ready_check(cx, ready_check);
        self.show_status_spinner(cx, spinner);
    }

    /// ShowReadyCheck @18175.
    pub fn show_ready_check(&mut self, cx: &mut Cx<'_>, b: bool) {
        let s = find(cx, "SizeBox_2");
        cond(cx, s, b, Vis::HitTestInvisible, Vis::Collapsed);
        if !b {
            return;
        }
        let me = cx.w;
        if cx.view.is_animation_playing(me, "ReadyAnim") && !cx.view.is_animation_playing_forward(me, "ReadyAnim") {
            cx.view.reverse_animation(me, "ReadyAnim");
        } else {
            cx.view.play(me, "ReadyAnim", false, 1.0);
        }
        cx.post("ui_prematch_perk_loadout_ready");
    }

    /// ShowStatusSpinner @18466.
    pub fn show_status_spinner(&mut self, cx: &mut Cx<'_>, b: bool) {
        let s = find(cx, "SpinnerHolder");
        cond(cx, s, b, Vis::HitTestInvisible, Vis::Hidden);
        if b {
            let me = cx.w;
            cx.view.play(me, "WaitingAnim", false, 1.0);
        }
    }

    /// Enter @11070.
    pub fn enter(&mut self, cx: &mut Cx<'_>) {
        self.toggle_perks_display(cx, true);
        let d = find(cx, "perksDisplay");
        wd(cx, d).transform.translation = [0.0, 0.0];
        let me = cx.w;
        cx.view.play(me, "EnterAnim", false, 1.0);
        cx.post("ui_prematch_perk_card_fly_in");
        let a = find(cx, "allyHeader");
        wd(cx, a).opacity = 1.0;
        self.allow_tooltips(cx, true);
    }

    /// Exit @11295.
    pub fn exit(&mut self, cx: &mut Cx<'_>) {
        self.dismiss_tooltips(cx);
        self.allow_tooltips(cx, false);
        let me = cx.w;
        let visible = cx.view.visible_in_hierarchy(me);
        let d = find(cx, "perksDisplay");
        if visible && cx.view.tree().widget(d).opacity > 0.0 {
            cx.view.stop_all_animations(me);
            cx.view.play(me, "EnterAnim", true, 1.0);
        }
        if visible {
            cx.post("ui_prematch_perk_card_fly_out");
        }
    }

    /// Toggle Perks Display @17936.
    pub fn toggle_perks_display(&mut self, cx: &mut Cx<'_>, show: bool) {
        let d = find(cx, "perksDisplay");
        wd(cx, d).opacity = if show { 1.0 } else { 0.0 };
        for it in Self::items(cx) {
            let i = PerkItem::interactor(cx, it);
            set_interactable(cx, i, show);
        }
    }

    /// Exit Ally Header @17888.
    pub fn exit_ally_header(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        cx.view.play(me, "ExitAllyHeaderAnim", false, 1.0);
    }

    /// Set Hide Tooltips @18863.
    pub fn set_hide_tooltips(&mut self, cx: &mut Cx<'_>, b: bool) {
        self.hide_tooltips = b;
        for it in Self::items(cx) {
            on_item(cx, it, move |i, cx| {
                i.shows_tooltip = !b;
                i.update_look(cx);
            });
        }
    }

    /// Allow Tooltips @18858.
    pub fn allow_tooltips(&mut self, cx: &mut Cx<'_>, allow: bool) {
        for it in Self::items(cx) {
            let i = PerkItem::interactor(cx, it);
            let t = find_under(cx, i, "UI_TooltipTarget");
            cx.view.tree_mut().set_prop(t, "Tooltips Disabled", Value::Bool(!allow));
        }
    }

    /// DismissTooltips @9169.
    pub fn dismiss_tooltips(&mut self, cx: &mut Cx<'_>) {
        for it in Self::items(cx) {
            let i = PerkItem::interactor(cx, it);
            toggle_tooltip(cx, i, false);
        }
    }

    /// UpdateContext @7949.
    pub fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str, player: i32) {
        let items = Self::items(cx);
        let is: Vec<WidgetId> = items.iter().map(|&w| PerkItem::interactor(cx, w)).collect();
        batch_set_context(cx, &is, ctx);
        for (&w, &i) in items.iter().zip(&is) {
            if let Some(s) = ist(cx, i) {
                s.player_index = player;
            }
            on_item(cx, w, move |it, _| it.player_index = player);
        }
    }

    /// Set Straight Layout @18887: Arrange Perks @11578, then InitManualNavigation.
    pub fn set_straight_layout(&mut self, cx: &mut Cx<'_>, straight: bool) {
        self.arrange_perks(cx, straight);
        self.init_manual_navigation(cx, straight);
    }

    /// Arrange Perks @11578 (B.1.5).
    fn arrange_perks(&mut self, cx: &mut Cx<'_>, straight: bool) {
        let l = self.large_perk_size;
        let m = self.perk_margin;
        let s = (l - m) * 0.5;
        for n in ["perksContainer", "bgContainer"] {
            let w = find(cx, n);
            wd(cx, w).transform.translation = [(l - m) * 0.25, 0.0];
        }
        let items = Self::items(cx);
        let (common, at): (f64, [(f64, f64, f64); 4]) = if !straight {
            let c1 = (0.5 * m, -0.5 * s - 0.5 * m);
            (s, [(l, -0.5 * l, 0.0), (s, c1.0, c1.1), (s, 0.5 * s + m, 0.0), (s, c1.0, -c1.1)])
        } else {
            let c = l - m;
            (c, [(l, -0.65 * l, 0.0), (c, c - 0.75 * m, 0.0), (c, 2.0 * c, 0.0), (c, 3.0 * c + 0.75 * m, 0.0)])
        };
        for (i, &(size, x, y)) in at.iter().enumerate() {
            let w = items[i];
            on_item(cx, w, move |it, cx| it.set_size(cx, size, size));
            wd(cx, w).transform.translation = [x, y];
        }
        for (i, lw) in Self::locks(cx).into_iter().enumerate() {
            let d = if i == 0 { l } else { common };
            cx.visit::<PerkLockIndicator, _>(lw, |li, cx| li.set_dimensions(cx, d));
            wd(cx, lw).transform.translation = [at[i].1, at[i].2];
        }
        let bg1 = find(cx, "bg1");
        let bg2 = find(cx, "bg2");
        for b in [bg1, bg2] {
            let w = wd(cx, b);
            w.brush_w = l + 2.0 * m;
            w.brush_h = l + 2.0 * m;
        }
        wd(cx, bg1).transform.translation = [at[0].1, at[0].2];
        wd(cx, bg2).transform.translation = [0.5 * m, 0.0];
        let c = find(cx, "UI_Perks_StackedCallout2");
        let w = wd(cx, c);
        w.pad.t = 0.5 * m;
        w.pad.b = 0.5 * m;
    }

    /// InitManualNavigation: SetManualNavigationTargets(Up, Right, Down, Left) (the function's parameter order,
    /// UI_Interactor), normal @19364, straight @18935.
    fn init_manual_navigation(&mut self, cx: &mut Cx<'_>, straight: bool) {
        let [sig, c1, c2, c3] = Self::items(cx).map(|w| PerkItem::interactor(cx, w));
        let nav = |cx: &mut Cx<'_>, i, up, right, down, left| {
            if let Some(s) = ist(cx, i) {
                s.manual_up = up;
                s.manual_right = right;
                s.manual_down = down;
                s.manual_left = left;
            }
        };
        if straight {
            nav(cx, sig, None, Some(c1), None, None);
            nav(cx, c1, None, Some(c2), None, Some(sig));
            nav(cx, c2, None, Some(c3), None, Some(c1));
            nav(cx, c3, None, None, None, Some(c2));
        } else {
            nav(cx, sig, None, Some(c1), Some(c3), None);
            nav(cx, c1, None, Some(c2), Some(c3), Some(sig));
            nav(cx, c2, Some(c1), None, Some(c3), Some(c1));
            nav(cx, c3, Some(c1), Some(c2), None, Some(sig));
        }
    }
}

impl Controller for PerksLoadout {
    /// OnInitialized @8534.
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        for (slot, w) in Self::items(cx).into_iter().enumerate() {
            cx.visit::<PerkItem, _>(w, |it, cx| {
                it.state.toggle_selected_state = false;
                it.shows_selected_state = false;
                for n in ["equippedIndicatorP0", "allyEquippedIndicator"] {
                    let x = find(cx, n);
                    wd(cx, x).vis = Vis::Hidden;
                }
                it.state.selected_event.add(Handler { owner: me, id: L_SLOT_SELECTED | slot as u32 });
            });
        }
        self.update_character_info(cx);
    }

    /// PreConstruct @9249.
    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        let ally = prop_bool(cx, "Is Ally Preview");
        let header = find(cx, "allyHeader");
        let t = if ally { game_text(cx, T_ALLYS_PERKS) } else { String::new() };
        set_tag(cx, header, Some(&t), None, None);
        cond(cx, header, ally, Vis::SelfHitTestInvisible, Vis::Collapsed);
        let info = find(cx, "CharacterInfo");
        let show_info = prop_bool(cx, "Show Character Info");
        cond(cx, info, show_info, Vis::SelfHitTestInvisible, Vis::Hidden);
        let shadow = cx.view.tree().props(cx.w).get("Show Shadow").and_then(Value::as_bool).unwrap_or(true);
        for it in Self::items(cx) {
            let s = find_under(cx, it, "shadow");
            cond(cx, s, shadow, Vis::SelfHitTestInvisible, Vis::Hidden);
        }
        let h = self.hide_tooltips;
        self.set_hide_tooltips(cx, h);
        let hide_locks = prop_bool(cx, "Hide Locks");
        for l in Self::locks(cx) {
            cond(cx, l, hide_locks, Vis::Hidden, Vis::SelfHitTestInvisible);
            wd(cx, l).opacity = 0.0;
        }
        let sc = find(cx, "stackedCallouts");
        cond(cx, sc, self.show_stacked_callouts, Vis::SelfHitTestInvisible, Vis::Hidden);
        let straight = prop_bool(cx, "Straight Layout");
        self.set_straight_layout(cx, straight);
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        if id & !GEN_MASK != L_SLOT_SELECTED {
            return;
        }
        let slot = (id & GEN_MASK) as usize;
        let Some(&(item, _)) = arg.downcast_ref::<(WidgetId, i32)>() else { return };
        let p = self.perks.get(slot).cloned().flatten();
        cx.emit(self, |c| &c.perk_selected_event, &(p, slot as i32, item));
    }

    fn resume(&mut self, cx: &mut Cx<'_>, id: u32) {
        if id & !GEN_MASK == R_STACKED {
            if id & GEN_MASK == self.stacked_gen & GEN_MASK {
                self.do_refresh_stacked(cx);
            }
            return;
        }
        if id != R_CHARACTER_INFO {
            return;
        }
        let Some(s) = self.selection.clone() else { return };
        let set = |cx: &mut Cx<'_>, n: &str, t: &str| {
            let w = find(cx, n);
            wd(cx, w).text = t.to_string();
        };
        set(cx, "labelCharacter", &s.name);
        set(cx, "labelSkin", &s.skin);
        let tag = find(cx, "labelCharacterLevel");
        // Deviation: a bot's tag reads BOT where the build shows its level (the port's own, a user request).
        let t = match s.level {
            Some(n) => game_text(cx, T_LOADOUT_LVL).replace("{number}", &n.to_string()),
            None => crate::menu::custom_text::BOT_TAG.to_string(),
        };
        set_tag(cx, tag, Some(&t), None, None);
    }
}

// ---------- UI_Perks_Equipper ----------

const E_READY: u32 = 1;
const E_AUTO: u32 = 2;
const E_CLEAR: u32 = 3;
const E_LOADOUT_PICK: u32 = 4;
const E_DISMISS: u32 = 5;
const E_LIST_SELECTED: u32 = 6;
const E_LIST_FOCUSED: u32 = 7;
const E_LIST_POPULATED: u32 = 8;
const M_SWITCH_PAGE: u32 = 9;
const M_SCROLL: u32 = 10;
const M_SUBMIT3: u32 = 11;
const M_CANCEL: u32 = 12;
const M_SUBMIT2: u32 = 13;
const R_DIM: u32 = 14;
/// Slot Change Animation's frame: the generation in the low bits.
const R_SLOT_ANIM: u32 = 0x0001_0000;

/// The lists of the page: the signature list first.
const LISTS: [&str; 4] = ["listSignature", "listOffense", "listDefense", "listUtility"];

/// PerksSavedEvent(Player Index, Character, Perks): the four slots in order, an empty one None.
pub type PerksSaved = (i32, Option<String>, Vec<Option<String>>);

/// B.2: the page. The build's lists copy Unlock All at PreConstruct; offline every perk is owned either way here.
#[derive(Debug)]
pub struct PerksEquipper {
    /// PerkEquippedEvent(Player Index, Perk, Slot).
    pub perk_equipped_event: Event<(i32, Perk, i32)>,
    pub perks_saved_event: Event<PerksSaved>,
    /// PerksEquipperEnteredEvent(Player Index, Character).
    pub entered_event: Event<(i32, Option<String>)>,
    pub exited_event: Event<()>,
    /// PerksClearedEvent(Player Index, Character).
    pub cleared_event: Event<(i32, Option<String>)>,
    pub character: Option<String>,
    /// The loadout is the machine's remembered one (`remembered_perks`), not the page's.
    pub remember: bool,
    pub current_slot: i32,
    pub context: String,
    pub player_index: i32,
    pub unlock_all: bool,
    dont_reload_loadout: bool,
    slot_gen: u32,
    /// Slot Change Animation in progress: its start and the label's direction.
    slot_anim: (f64, f64),
    /// The loadout's slots, as this page set them.
    slots: [Option<Perk>; 4],
    /// Each list's perks (LISTS order), as SetCharacter filled them.
    lists: [Vec<String>; 4],
    /// The handlers `open_perk_page` gave: each called once with the next PerksSavedEvent.
    once: Vec<Handler>,
}

impl PerksEquipper {
    pub fn make(cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(PerksEquipper {
            perk_equipped_event: Event::default(),
            perks_saved_event: Event::default(),
            entered_event: Event::default(),
            exited_event: Event::default(),
            cleared_event: Event::default(),
            character: None,
            remember: false,
            current_slot: 0,
            context: crate::menu::manager::NULL_GUID.to_string(),
            player_index: prop_f64(cx, "Player Index", 0.0) as i32,
            unlock_all: prop_bool(cx, "Unlock All"),
            dont_reload_loadout: false,
            slot_gen: 0,
            slot_anim: (0.0, 0.0),
            slots: Default::default(),
            lists: Default::default(),
            once: Vec::new(),
        })
    }

    fn minimal(cx: &Cx<'_>) -> bool {
        prop_bool(cx, "Minimal Mode")
    }

    fn show_inline(cx: &Cx<'_>) -> bool {
        prop_bool(cx, "Show Inline Tooltips")
    }

    /// The page's loadout widget (UI_Perks_Loadout).
    pub fn loadout(cx: &Cx<'_>) -> WidgetId {
        find(cx, "UI_Perks_Loadout")
    }

    fn list_widgets(cx: &Cx<'_>) -> [WidgetId; 4] {
        LISTS.map(|n| find(cx, n))
    }

    fn button_ready(cx: &Cx<'_>) -> WidgetId {
        find(cx, "buttonReady")
    }

    fn buttons(cx: &Cx<'_>) -> [WidgetId; 3] {
        ["buttonAutoEquip", "buttonClear", "buttonClear_Icon"].map(|n| find(cx, n))
    }

    fn interactor_of(cx: &Cx<'_>, w: WidgetId) -> WidgetId {
        find_under(cx, w, "UI_Interactor")
    }

    /// `f` on the page's loadout (at once, or after the call that holds it).
    fn on_loadout(cx: &mut Cx<'_>, f: impl FnOnce(&mut PerksLoadout, &mut Cx<'_>) + 'static) {
        let lo = Self::loadout(cx);
        run_on::<PerksLoadout>(cx, lo, f);
    }

    /// `f` on each list.
    fn on_lists(cx: &mut Cx<'_>, f: impl Fn(&mut PerksSublist, &mut Cx<'_>) + Clone + 'static) {
        for l in Self::list_widgets(cx) {
            let f = f.clone();
            run_on::<PerksSublist>(cx, l, move |s, cx| f(s, cx));
        }
    }

    /// SetAllyEquipped @23177: each list's (GetAllLists), then the loadout's.
    pub fn set_ally_equipped(&mut self, cx: &mut Cx<'_>, ids: &[String]) {
        let l = ids.to_vec();
        Self::on_lists(cx, move |s, cx| s.set_ally_equipped(cx, &l));
        let l = ids.to_vec();
        Self::on_loadout(cx, move |lo, cx| lo.set_ally_equipped(cx, &l));
    }

    /// The equipped perks' ids (GetEquippedPerks(true)).
    fn equipped_ids(&self) -> Vec<String> {
        self.slots.iter().flatten().map(|p| p.id.clone()).collect()
    }

    /// The loadout's slots after SetPerks(perks) (ValidatedPerkOrdering).
    fn set_slots(&mut self, perks: &[Option<Perk>]) {
        let v = validated_perk_ordering(perks);
        for (i, p) in v.into_iter().enumerate() {
            self.slots[i] = p;
        }
    }

    // B.2.3: the buttons, the loadout's and the lists' events (on_initialized)

    /// Enter(Current Slot) @20797 (`slot` -1: the first slot to fill).
    pub fn enter(&mut self, cx: &mut Cx<'_>, slot: i32) {
        self.unlock_all = true;
        let me = cx.w;
        cx.view.stop_all_animations(me);
        wd(cx, me).vis = Vis::SelfHitTestInvisible;
        if prop_bool(cx, "Use Own Context") {
            let g = cx.view.manager.new_guid();
            self.update_context(cx, &g);
        }
        self.focus(cx);
        if !prop_bool(cx, "Manually Set Equipped") {
            self.toggle_busy(cx, true);
            let ids = loadout_ids(cx, self.remember, self.character.as_deref());
            let perks = perks_of(cx.view, &ids);
            self.set_equipped_perks(cx, &perks);
        }
        self.toggle_busy(cx, false);
        let s = if slot >= 0 { slot } else { self.first_focused_slot() };
        self.set_current_slot(cx, s, false);
        if !prop_bool(cx, "Don't Animate In") {
            cx.view.play(me, "EnterAnim", false, 1.0);
        }
        cx.post("ui_prematch_perk_card_fly_in");
        let ev = (self.player_index, self.character.clone());
        cx.emit(self, |c| &c.entered_event, &ev);
        if self.total_perks_available() == 0 {
            cx.view.delay(0.1, Handler { owner: me, id: R_DIM });
        }
    }

    /// Focus @28264.
    pub fn focus(&mut self, cx: &mut Cx<'_>) {
        let b = Self::button_ready(cx);
        let i = Self::interactor_of(cx, b);
        set_focused(cx, i, false);
    }

    /// SetCharacter(Character, Ally Perks) @11260 (no ally perks: ally mode is off).
    pub fn set_character(&mut self, cx: &mut Cx<'_>, c: Option<&str>) {
        self.character = c.map(str::to_string);
        let name = c.and_then(|c| cx.view.fighters().get(c)).map(|f| f.display_name.clone()).unwrap_or_default();
        let label = find(cx, "characterLabel");
        wd(cx, label).text = name;
        // Deviation: the build shows the server's mastery level; the port's player is level 49
        let tag = find(cx, "WBP_TextTag");
        let lvl = game_text(cx, T_EQUIPPER_LVL).replace("{level}", &PLAYER_LEVEL.to_string());
        set_tag(cx, tag, Some(&lvl), None, None);
        let lo = Self::loadout(cx);
        let lo_character = cx.view.controller::<PerksLoadout>(lo).map(|l| l.character.clone());
        if !(self.dont_reload_loadout && lo_character.as_ref().is_some_and(|l| l.as_deref() == c)) {
            let remember = self.remember;
            let ids = loadout_ids(cx, remember, c);
            let perks = perks_of(cx.view, &ids);
            self.set_slots(&perks);
            let cc = c.map(str::to_string);
            Self::on_loadout(cx, move |l, cx| {
                l.remember = remember;
                l.set_character(cx, cc.as_deref());
            });
        }
        let lws = Self::list_widgets(cx);
        for (i, l) in lws.into_iter().enumerate() {
            let cat = PerksSublist::category_of(cx, l) as usize;
            let ids = fighter_perks(cx.view, c).and_then(|f| f.lists.get(cat)).cloned().unwrap_or_default();
            self.lists[i] = ids.into_iter().filter(|id| perk_data(cx.view, Some(id)).is_some()).collect();
            let cc = c.map(str::to_string);
            run_on::<PerksSublist>(cx, l, move |s, cx| {
                s.clear(cx);
                s.set_character(cx, cc.as_deref());
            });
        }
        // @5162-5581: the info panels
        let any = self.total_perks_available() > 0 || Self::minimal(cx);
        let set = |cx: &mut Cx<'_>, n: &str, v: Vis| {
            let w = find(cx, n);
            wd(cx, w).vis = v;
        };
        if any {
            set(cx, "firstTimeInfo", Vis::Collapsed);
            set(cx, "noPerksInfo", Vis::Collapsed);
            set(cx, "perksPanel", Vis::SelfHitTestInvisible);
        } else {
            // mastery level 1: the first time panel
            set(cx, "firstTimeInfo", Vis::SelfHitTestInvisible);
            set(cx, "noPerksInfo", Vis::Collapsed);
            set(cx, "perksPanel", Vis::Collapsed);
        }
        self.toggle_equipping_enabled(cx, any);
        self.dont_reload_loadout = false;
    }

    /// SetEquippedPerks @24450.
    pub fn set_equipped_perks(&mut self, cx: &mut Cx<'_>, perks: &[Option<Perk>]) {
        self.set_slots(perks);
        let p = perks.to_vec();
        Self::on_loadout(cx, move |l, cx| l.set_perks(cx, &p));
        let eq = self.equipped_ids();
        Self::on_lists(cx, move |s, cx| s.mark_equipped(cx, &eq, true));
        self.update_appearance(cx);
    }

    /// The slots' perks of the page's loadout (the four slots, an empty one None).
    pub fn equipped(&self) -> Vec<Option<String>> {
        ids_of(&self.slots)
    }

    /// SetCurrentSlot(Slot, Focus Loadout?) @14518.
    pub fn set_current_slot(&mut self, cx: &mut Cx<'_>, slot: i32, focus_loadout: bool) {
        self.slot_change_animation(cx, self.current_slot, slot);
        self.current_slot = slot;
        let sig = slot == 0;
        let label = find(cx, "slotLabel");
        let t = if sig {
            game_text(cx, T_SIGNATURE_PERK)
        } else {
            game_text(cx, T_PERK_SLOT).replace("{slotNumber}", &slot.to_string())
        };
        wd(cx, label).text = t;
        let lws = Self::list_widgets(cx);
        cond(cx, lws[0], sig, Vis::SelfHitTestInvisible, Vis::Collapsed);
        for &l in &lws[1..] {
            cond(cx, l, sig, Vis::Collapsed, Vis::SelfHitTestInvisible);
        }
        let lo = Self::loadout(cx);
        let items = PerksLoadout::items_of(cx, lo);
        for (i, &w) in items.iter().enumerate() {
            let s = match &self.slots[i] {
                Some(p) if i as i32 == slot => {
                    if p.category() == SIGNATURE {
                        1.0
                    } else {
                        1.15
                    }
                }
                Some(_) => 0.95,
                None if slot == 0 => 0.95,
                None => 1.0,
            };
            wd(cx, w).transform.scale = [s, s];
            let on = i as i32 == slot;
            on_item(cx, w, move |it, cx| it.show_border(cx, on));
        }
        Self::on_loadout(cx, |l, cx| l.dismiss_tooltips(cx));
        self.set_current_focus_for_slot(cx, slot, focus_loadout);
        self.update_appearance(cx);
    }

    /// Slot Change Animation @26926: 0.25 s real time, EaseOut exponent 2.
    fn slot_change_animation(&mut self, cx: &mut Cx<'_>, from: i32, to: i32) {
        self.slot_gen = self.slot_gen.wrapping_add(1);
        self.slot_anim = (cx.now(), if from < to { 50.0 } else { -50.0 });
        self.slot_anim_step(cx);
    }

    fn slot_anim_step(&mut self, cx: &mut Cx<'_>) {
        let (t0, sign) = self.slot_anim;
        let a = ((cx.now() - t0) / 0.25).min(1.0);
        let e = 1.0 - (1.0 - a).powi(2);
        let label = find(cx, "slotLabel");
        {
            let w = wd(cx, label);
            w.opacity = e;
            w.transform.translation[0] = (1.0 - e) * sign;
        }
        if a < 1.0 {
            let me = cx.w;
            cx.view.delay(0.0, Handler { owner: me, id: R_SLOT_ANIM | (self.slot_gen & GEN_MASK) });
        }
    }

    /// Set Current Focus For Slot.
    fn set_current_focus_for_slot(&mut self, cx: &mut Cx<'_>, slot: i32, focus_loadout: bool) {
        let p = self.player_index;
        let lo = Self::loadout(cx);
        let items = PerksLoadout::items_of(cx, lo);
        let loadout_item = |cx: &mut Cx<'_>| {
            if let Some(&w) = usize::try_from(slot).ok().and_then(|s| items.get(s)) {
                let i = PerkItem::interactor(cx, w);
                set_focused(cx, i, false);
            }
        };
        if focus_loadout {
            return loadout_item(cx);
        }
        let lws = Self::list_widgets(cx);
        let perk = usize::try_from(slot).ok().and_then(|s| self.slots.get(s)).cloned().flatten();
        let target = if let Some(perk) = perk {
            self.lists.iter().enumerate().find_map(|(li, l)| l.iter().position(|id| *id == perk.id).map(|x| (li, x)))
        } else if slot == 0 {
            (!self.lists[0].is_empty()).then_some((0, 0))
        } else {
            (1..4).find(|&li| !self.lists[li].is_empty()).map(|li| (li, 0))
        };
        match target {
            Some((li, idx)) => run_on::<PerksSublist>(cx, lws[li], move |s, cx| s.focus_index(cx, p, idx)),
            None => loadout_item(cx),
        }
    }

    /// FirstFocusedSlot.
    fn first_focused_slot(&self) -> i32 {
        for i in 0..SLOTS {
            if self.slots[i].is_none() && self.perks_available_for_slot(i) > 0 {
                return i as i32;
            }
        }
        if self.perks_available_for_slot(0) > 0 { 0 } else { 1 }
    }

    /// PerksAvailableForSlot.
    fn perks_available_for_slot(&self, slot: usize) -> usize {
        if slot == 0 { self.lists[0].len() } else { self.lists[1..].iter().map(Vec::len).sum() }
    }

    fn total_perks_available(&self) -> usize {
        self.lists.iter().map(Vec::len).sum()
    }

    /// IsSlotUnused?
    fn is_slot_unused(&self, slot: usize) -> bool {
        if self.slots[slot].is_some() {
            return false;
        }
        if slot == 0 {
            return self.perks_available_for_slot(0) > 0;
        }
        let general = &self.slots[1..];
        let occupied = general.iter().filter(|p| p.is_some()).count();
        self.perks_available_for_slot(1).saturating_sub(occupied) >= general.len() - occupied
    }

    /// IsBusy?
    pub fn is_busy(&self, cx: &mut Cx<'_>) -> bool {
        let me = cx.w;
        let busy = find(cx, "BusyIndicator");
        cx.view.visible_in_hierarchy(me) && (is_visible(cx.view.tree().widget(busy).vis) || are_widgets_flying(cx))
    }

    /// CanEquipPerkInSlot?
    fn can_equip(&self, cx: &Cx<'_>, p: &Perk) -> bool {
        if self.slots.iter().flatten().any(|x| x == p) {
            return false;
        }
        if p.category() == SIGNATURE
            && !fighter_perks(cx.view, self.character.as_deref())
                .and_then(|f| f.lists.get(3))
                .is_some_and(|l| l.contains(&p.id))
        {
            return false;
        }
        true
    }

    /// Equip Perk @21938 (`item` may be busy: the tile clicked).
    fn equip_perk(&mut self, cx: &mut Cx<'_>, ev: &PerkEvent, slot: i32) {
        let perk = ev.perk.clone();
        if self.is_busy(cx) || ev.player != self.player_index || !self.can_equip(cx, &perk) {
            let on = self.slots.iter().flatten().any(|x| *x == perk);
            on_item(cx, ev.item, move |it, cx| {
                it.set_selected(on);
                it.update_look(cx);
            });
            self.update_appearance(cx);
            return;
        }
        let Ok(s) = usize::try_from(slot) else { return };
        if s >= SLOTS {
            return;
        }
        self.slots[s] = Some(perk.clone());
        let p2 = perk.clone();
        Self::on_loadout(cx, move |l, cx| l.set_perk_for_slot(cx, Some(p2), s));
        cx.post(SELECT_SOUND.get(perk.category() as usize).copied().unwrap_or(SELECT_SOUND[3]));
        // @2523: each list's selection is the equipped perks
        let eq = self.equipped_ids();
        Self::on_lists(cx, move |l, cx| l.mark_equipped(cx, &eq, true));
        let lo = Self::loadout(cx);
        let to = PerksLoadout::items_of(cx, lo)[s];
        let size = find_under(cx, ev.item, "sizeContainer");
        let w = cx.view.tree().widget(size);
        let dims = [w.w_override, w.h_override];
        fly_perk_item(cx, ev.item, perk.clone(), dims, to, 0.35);
        let pe = (ev.player, perk, slot);
        cx.emit(self, |c| &c.perk_equipped_event, &pe);
        let auto_next =
            cx.view.tree().props(cx.w).get("Auto Select Next Slot").and_then(Value::as_bool).unwrap_or(true);
        if auto_next {
            let next = wrap_int(self.current_slot + 1, 0, 3);
            self.set_current_slot(cx, next, false);
        }
        self.update_appearance(cx);
    }

    /// Clear @20527.
    pub fn clear(&mut self, cx: &mut Cx<'_>) {
        if self.is_busy(cx) {
            return;
        }
        self.slots = Default::default();
        Self::on_loadout(cx, |l, cx| l.clear(cx));
        self.update_appearance(cx);
        Self::on_lists(cx, |l, cx| l.deselect_all(cx));
        let ev = (self.player_index, self.character.clone());
        cx.emit(self, |c| &c.cleared_event, &ev);
        cx.post("ui_prematch_perk_block_remove");
    }

    /// Auto equip @20244: RecommendedPerkDatas (Unlock All, set by Enter offline: unfiltered).
    pub fn auto_equip(&mut self, cx: &mut Cx<'_>) {
        if self.is_busy(cx) || self.character.is_none() {
            return;
        }
        let rec: Vec<Option<String>> = fighter_perks(cx.view, self.character.as_deref())
            .map(|f| f.recommended.iter().cloned().map(Some).collect())
            .unwrap_or_default();
        let perks = perks_of(cx.view, &rec);
        self.set_equipped_perks(cx, &perks);
        let lo = Self::loadout(cx);
        let items = PerksLoadout::items_of(cx, lo);
        for (i, &w) in items.iter().enumerate() {
            if self.slots[i].is_some() {
                let d = i as f64 * 0.1;
                on_item(cx, w, move |it, cx| it.pulse(cx, d, Some("ui_prematch_perk_block_exit".into())));
            }
        }
    }

    /// Ready(Force) @24114: forced offline, PerksSavedEvent and Exit. The port keeps the loadout (unless Don't Save
    /// Changes, which skips the build's save online too).
    pub fn ready(&mut self, cx: &mut Cx<'_>, force: bool) {
        if !force && self.is_busy(cx) {
            return;
        }
        self.toggle_busy(cx, true);
        // @1117 / @1165 GetEquippedPerks(false, false, false): the four slots, empties kept
        let ids = ids_of(&self.slots);
        if let Some(c) = self.character.clone()
            && !prop_bool(cx, "Don't Save Changes")
        {
            if self.remember {
                cx.prefs.perks.insert(c, ids.clone());
            } else {
                SAVED.with(|s| s.borrow_mut().insert(c, ids.clone()));
            }
        }
        let ev = (self.player_index, self.character.clone(), ids);
        cx.emit(self, |c| &c.perks_saved_event, &ev);
        for h in std::mem::take(&mut self.once) {
            cx.call(h, &ev);
        }
        self.exit(cx);
    }

    /// Exit @23641.
    pub fn exit(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        wd(cx, me).vis = Vis::Collapsed;
        cx.emit(self, |c| &c.exited_event, &());
    }

    /// ToggleBusy @23581.
    pub fn toggle_busy(&mut self, cx: &mut Cx<'_>, b: bool) {
        let w = find(cx, "BusyIndicator");
        cond(cx, w, b, Vis::Visible, Vis::Hidden);
    }

    /// ToggleEquippingEnabled @25905.
    fn toggle_equipping_enabled(&mut self, cx: &mut Cx<'_>, b: bool) {
        for w in Self::buttons(cx) {
            let i = Self::interactor_of(cx, w);
            set_interactable(cx, i, b);
        }
    }

    /// UpdateAppearance @24924.
    pub fn update_appearance(&mut self, cx: &mut Cx<'_>) {
        let lo = Self::loadout(cx);
        let items = PerksLoadout::items_of(cx, lo);
        let mut unused = 0;
        for (i, &w) in items.iter().enumerate() {
            // @599-695: the count incremented first (0.15, 0.30, 0.45 s)
            if self.is_slot_unused(i) {
                unused += 1;
                let d = f64::from(unused) * 0.15;
                on_item(cx, w, move |it, cx| it.toggle_empty_animation(cx, true, d));
            } else {
                on_item(cx, w, |it, cx| it.toggle_empty_animation(cx, false, 0.0));
            }
        }
        let warn = find(cx, "slotWarningLabel");
        cond(cx, warn, unused > 0, Vis::SelfHitTestInvisible, Vis::Hidden);
        let t = if unused == 1 {
            game_text(cx, T_ONE_UNUSED)
        } else {
            game_text(cx, T_UNUSED).replace("{count}", &unused.to_string())
        };
        wd(cx, warn).text = t;
        let legend = find(cx, "legendPanel");
        wd(cx, legend).vis = if Self::minimal(cx) { Vis::Collapsed } else { Vis::Hidden };
        let eq = self.equipped_ids();
        Self::on_lists(cx, move |l, cx| l.mark_equipped(cx, &eq, false));
    }

    /// UpdateContext @10311.
    pub fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str) {
        self.context = ctx.to_string();
        let mut is = vec![Self::interactor_of(cx, Self::button_ready(cx))];
        for b in Self::buttons(cx) {
            is.push(Self::interactor_of(cx, b));
        }
        is.push(find(cx, "buttonDismisser"));
        batch_set_context(cx, &is, ctx);
        for &i in &is {
            if let Some(s) = ist(cx, i) {
                s.player_index = self.player_index;
            }
        }
        let c = ctx.to_string();
        Self::on_lists(cx, move |l, cx| l.update_context(cx, &c));
        let c = ctx.to_string();
        let p = self.player_index;
        Self::on_loadout(cx, move |l, cx| l.update_context(cx, &c, p));
    }

    /// CanPaginate?: seen, no dialog, the player's current context is this page's.
    fn can_paginate(&self, cx: &mut Cx<'_>, player: i32) -> bool {
        let me = cx.w;
        cx.view.visible_in_hierarchy(me)
            && !Manager::is_dialog_active(cx.view)
            && cx.view.manager.get_current_context(player) == self.context
    }

    /// OnCancel @24219: Cancel Will Confirm saves and leaves.
    fn on_cancel(&mut self, cx: &mut Cx<'_>, player: i32) {
        let me = cx.w;
        let confirm = cx.view.tree().props(me).get("Cancel Will Confirm").and_then(Value::as_bool).unwrap_or(true);
        if player == self.player_index && cx.view.visible_in_hierarchy(me) && confirm {
            self.ready(cx, false);
        }
    }

    /// Perk Focused @27945.
    fn perk_focused(&mut self, cx: &mut Cx<'_>, ev: &PerkEvent) {
        if ev.player != self.player_index || !Self::show_inline(cx) {
            return;
        }
        // the tile's GetTooltipData (the tile is busy: its look is the page's own)
        let d = PerkTooltipData {
            perk: ev.perk.clone(),
            character: if self.unlock_all { None } else { self.character.clone() },
            player_index: self.player_index,
            equipped: self.slots.iter().flatten().any(|p| *p == ev.perk),
            ally_equipped: false,
            cost: 0.0,
            discount: 0.0,
        };
        let desc = find(cx, "UI_TooltipPerkDescription");
        run_on::<TooltipPerkDescription>(cx, desc, move |t, cx| t.set_perk_tooltip_data(cx, Some(&d)));
        let l = find(cx, "perkLabel");
        wd(cx, l).text = ev.perk.info.name.clone();
    }
}

impl Controller for PerksEquipper {
    /// B.2.3: the buttons, the loadout's and the lists' events.
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        let h = |id| Handler { owner: me, id };
        let ready = Self::button_ready(cx);
        cx.visit::<ButtonMain, _>(ready, |b, _| b.on_click.add(h(E_READY)));
        let [auto, clear, clear_icon] = Self::buttons(cx);
        cx.visit::<ButtonSecondary, _>(auto, |b, _| b.on_click.add(h(E_AUTO)));
        for b in [clear, clear_icon] {
            cx.visit::<ButtonSecondary, _>(b, |b, _| b.on_click.add(h(E_CLEAR)));
        }
        let lo = Self::loadout(cx);
        cx.visit::<PerksLoadout, _>(lo, |l, _| l.perk_selected_event.add(h(E_LOADOUT_PICK)));
        let d = find(cx, "buttonDismisser");
        bind(cx, d, |s| &mut s.on_click, E_DISMISS);
        for l in Self::list_widgets(cx) {
            cx.visit::<PerksSublist, _>(l, |s, _| {
                s.perk_selected_event.add(h(E_LIST_SELECTED));
                s.perk_focused_event.add(h(E_LIST_FOCUSED));
                s.perks_populated_event.add(h(E_LIST_POPULATED));
            });
        }
    }

    /// AddEventBindings @13008: the manager's input.
    fn construct(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        let m = &mut cx.view.manager;
        m.on_request_switch_page.add(Handler { owner: me, id: M_SWITCH_PAGE });
        m.on_scroll.add(Handler { owner: me, id: M_SCROLL });
        m.on_submit3.add(Handler { owner: me, id: M_SUBMIT3 });
        m.on_cancel.add(Handler { owner: me, id: M_CANCEL });
        m.on_submit2.add(Handler { owner: me, id: M_SUBMIT2 });
    }

    fn destruct(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        let m = &mut cx.view.manager;
        m.on_request_switch_page.remove(Handler { owner: me, id: M_SWITCH_PAGE });
        m.on_scroll.remove(Handler { owner: me, id: M_SCROLL });
        m.on_submit3.remove(Handler { owner: me, id: M_SUBMIT3 });
        m.on_cancel.remove(Handler { owner: me, id: M_CANCEL });
        m.on_submit2.remove(Handler { owner: me, id: M_SUBMIT2 });
        self.slot_gen = self.slot_gen.wrapping_add(1);
    }

    /// PreConstruct @15423 (B.2.6): normal and Minimal Mode.
    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        let min = Self::minimal(cx);
        let inline = Self::show_inline(cx);
        let ip = find(cx, "inlinePerkInfoPanel");
        cond(cx, ip, inline, Vis::SelfHitTestInvisible, Vis::Collapsed);
        let lo = Self::loadout(cx);
        cx.visit::<PerksLoadout, _>(lo, |l, cx| l.set_hide_tooltips(cx, inline));
        let k = if min { 0.62 } else { 1.0 };
        let lws = Self::list_widgets(cx);
        for (i, &l) in lws.iter().enumerate() {
            let d = if i == 0 { 200.0 } else { 120.0 } * k;
            cx.visit::<PerksSublist, _>(l, |s, cx| {
                s.hide_tooltips = inline;
                s.set_perk_dimensions(cx, d);
            });
        }
        cx.visit::<PerksSublist, _>(lws[0], |s, cx| s.set_force_grid_columns(cx, if min { 1 } else { 3 }));
        // @15970 the scroll box's overlay slot, @16286 its bar's padding, @16670 / @16711 the two lists' slots
        let sb = find(cx, "perksScrollbox");
        wd(cx, sb).pad = Margin { l: if min { 30.0 } else { 60.0 }, t: 0.0, r: 0.0, b: 0.0 };
        let bar = if min {
            Margin { l: 10.0, t: 220.0, r: 10.0, b: 15.0 }
        } else {
            Margin { l: 30.0, t: 380.0, r: 10.0, b: 15.0 }
        };
        set_scrollbar_padding(cx.view.tree_mut(), sb, bar);
        for n in ["listOffense", "listSignature"] {
            let w = find(cx, n);
            wd(cx, w).pad = Margin { l: 0.0, t: if min { 220.0 } else { 380.0 }, r: 0.0, b: 0.0 };
        }
        let desc = find(cx, "UI_TooltipPerkDescription");
        let pl = find_under(cx, desc, "perkLabel");
        wd(cx, pl).vis = Vis::Collapsed;
        let actions = find(cx, "ActionsVerticalBox");
        let side = if min { 30.0 } else { 60.0 };
        wd(cx, actions).pad = Margin { l: side, t: 30.0, r: side, b: 30.0 };
        let [_, clear, clear_icon] = Self::buttons(cx);
        cond(cx, clear, min, Vis::Collapsed, Vis::SelfHitTestInvisible);
        cond(cx, clear_icon, min, Vis::SelfHitTestInvisible, Vis::Collapsed);
        let label = game_text(cx, if min { T_NEXT } else { T_LETS_GO });
        let ready = Self::button_ready(cx);
        cx.visit::<ButtonMain, _>(ready, |b, cx| b.set_label(cx, &label));
        for n in ["bgFade", "buttonDismisser", "legendPanel", "characterPanel", "WBP_Quadrilateral_1"] {
            let w = find(cx, n);
            cond(cx, w, min, Vis::Collapsed, Vis::SelfHitTestInvisible);
        }
        cx.visit::<PerksLoadout, _>(lo, |l, cx| {
            l.show_stacked_callouts = !min;
            let sc = find(cx, "stackedCallouts");
            cond(cx, sc, !min, Vis::SelfHitTestInvisible, Vis::Hidden);
        });
        let hs = find(cx, "HeightSpacer");
        wd(cx, hs).brush_h = if min { 240.0 } else { 400.0 };
        let hc = find(cx, "HeaderContent");
        wd(cx, hc).pad.t = if min { 0.0 } else { 30.0 };
        {
            let w = wd(cx, lo);
            w.h_align = if min { HAlign::Center } else { HAlign::Right };
            if min {
                w.pad.l = -110.0;
                w.pad.r = 0.0;
            } else {
                w.pad.l = 0.0;
                w.pad.r = 60.0;
            }
        }
        let ssp = find(cx, "slotSelectorPanel");
        wd(cx, ssp).pad.t = if min { -20.0 } else { 30.0 };
        let bg = find(cx, "mainBG");
        wd(cx, bg).vis = if min { Vis::Hidden } else { Vis::Visible };
        cx.visit::<PerksLoadout, _>(lo, |l, cx| {
            l.large_perk_size = if min { 120.0 } else { 250.0 };
            l.set_straight_layout(cx, min);
        });
        let size = find(cx, "sizeContainer");
        {
            let w = wd(cx, size);
            w.w_override = if min { f64::NAN } else { 830.0 };
            w.h_align = if min { HAlign::Fill } else { HAlign::Left };
        }
        let (p, unlock) = (self.player_index, self.unlock_all);
        for l in lws {
            cx.visit::<PerksSublist, _>(l, |s, _| {
                s.player_index = p;
                s.scrollbox = Some(sb);
                s.unlock_all = unlock;
            });
        }
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        let me = cx.w;
        match id {
            E_READY => self.ready(cx, false),
            E_AUTO => self.auto_equip(cx),
            E_CLEAR => self.clear(cx),
            E_LOADOUT_PICK => {
                let Some((_, slot, _)) = arg.downcast_ref::<LoadoutPick>() else { return };
                if !self.is_busy(cx) {
                    self.set_current_slot(cx, *slot, true);
                }
            }
            E_DISMISS => {
                let p = arg.downcast_ref::<i32>().copied().unwrap_or(0);
                self.on_cancel(cx, p);
            }
            E_LIST_SELECTED => {
                let Some(ev) = arg.downcast_ref::<PerkEvent>() else { return };
                let ev = ev.clone();
                let slot = self.current_slot;
                self.equip_perk(cx, &ev, slot);
            }
            E_LIST_FOCUSED => {
                let Some(ev) = arg.downcast_ref::<PerkEvent>() else { return };
                let ev = ev.clone();
                self.perk_focused(cx, &ev);
            }
            E_LIST_POPULATED => {
                let Some(&l) = arg.downcast_ref::<WidgetId>() else { return };
                let c = self.context.clone();
                run_on::<PerksSublist>(cx, l, move |s, cx| s.update_context(cx, &c));
            }
            // OnSwitchPage @14095
            M_SWITCH_PAGE => {
                let Some(&(player, dir)) = arg.downcast_ref::<(i32, i32)>() else { return };
                if player != self.player_index || !self.can_paginate(cx, player) {
                    return;
                }
                let s = wrap_int(self.current_slot + dir, 0, 3);
                self.set_current_slot(cx, s, false);
            }
            // OnScroll @15262
            M_SCROLL => {
                let Some(&(_, _, amount)) = arg.downcast_ref::<(i32, i32, f64)>() else { return };
                if cx.view.visible_in_hierarchy(me) {
                    let sb = find(cx, "perksScrollbox");
                    scroll_list(cx.view.tree_mut(), sb, amount);
                }
            }
            // OnSubmit3 @23797
            M_SUBMIT3 => {
                let p = arg.downcast_ref::<i32>().copied().unwrap_or(-1);
                if p == self.player_index
                    && cx.view.visible_in_hierarchy(me)
                    && cx.view.manager.get_modal("perksTrainer").is_none()
                {
                    self.ready(cx, false);
                }
            }
            M_CANCEL => {
                let Some(&(p, _)) = arg.downcast_ref::<(i32, bool)>() else { return };
                self.on_cancel(cx, p);
            }
            // OnSubmit2 @27507: Toggle Inline Tips
            M_SUBMIT2 => {
                if !Self::show_inline(cx) || !cx.view.visible_in_hierarchy(me) {
                    return;
                }
                let d = find(cx, "UI_TooltipPerkDescription");
                let v = is_visible(cx.view.tree().widget(d).vis);
                cond(cx, d, !v, Vis::SelfHitTestInvisible, Vis::Collapsed);
            }
            _ => {}
        }
    }

    fn resume(&mut self, cx: &mut Cx<'_>, id: u32) {
        if id == R_DIM {
            let lo = Self::loadout(cx);
            for w in PerksLoadout::items_of(cx, lo) {
                on_item(cx, w, |it, cx| {
                    it.set_dimmed(cx, true);
                    it.show_border(cx, false);
                });
            }
        } else if id & !GEN_MASK == R_SLOT_ANIM && id & GEN_MASK == self.slot_gen & GEN_MASK {
            self.slot_anim_step(cx);
        }
    }
}

// ---------- UI_PM_EditPerks ----------

const P_CLICK: u32 = 1;

/// The pre-match screen's perk button: one icon button; its click is the screen's.
#[derive(Debug, Default)]
pub struct PMEditPerks {
    /// ClickEvent(Player Index).
    pub click_event: Event<i32>,
}

impl PMEditPerks {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::<PMEditPerks>::default()
    }

    fn button(cx: &Cx<'_>) -> WidgetId {
        find(cx, "UI_ButtonSecondary")
    }

    pub fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str, player: i32) {
        let b = Self::button(cx);
        let i = ButtonSecondary::interactor(cx, b);
        if let Some(s) = ist(cx, i) {
            s.context = ctx.to_string();
            s.player_index = player;
        }
    }
}

impl Controller for PMEditPerks {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        let b = Self::button(cx);
        cx.visit::<ButtonSecondary, _>(b, |b, _| b.on_click.add(Handler { owner: me, id: P_CLICK }));
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, _arg: &dyn Any) {
        if id != P_CLICK {
            return;
        }
        let b = Self::button(cx);
        let i = ButtonSecondary::interactor(cx, b);
        let p = ist(cx, i).map_or(0, |s| s.player_index);
        cx.emit(self, |c| &c.click_event, &p);
    }
}

// ---------- the API of the screens that open the page ----------

/// The page for a player: the equipper `eq`'s character, Enter, and `on_ready` called once
/// with the next PerksSavedEvent (its argument a `PerksSaved`: the four slots in order, an empty one None).
/// `remember`: the loadout is the machine's remembered one (the custom lobby's, the pre-match screen's), else the
/// page's own (the Lab's, the Collection's).
pub fn open_perk_page(cx: &mut Cx<'_>, eq: WidgetId, character: &str, remember: bool, on_ready: Option<Handler>) {
    let c = character.to_string();
    run_on::<PerksEquipper>(cx, eq, move |e, cx| {
        e.remember = remember;
        e.set_character(cx, Some(&c));
        if let Some(h) = on_ready {
            e.once.push(h);
        }
        e.enter(cx, -1);
    });
}

/// A loadout card of another player: its perks, in the build's slot order.
pub fn show_loadout(cx: &mut Cx<'_>, lo: WidgetId, character: Option<&str>, perks: &[Option<String>]) {
    let c = character.map(str::to_string);
    let p = perks_of(cx.view, perks);
    run_on::<PerksLoadout>(cx, lo, move |l, cx| {
        l.character = c;
        l.set_perks(cx, &p);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn perk(id: &str, category: u32) -> Option<Perk> {
        Some(Perk { id: id.into(), info: PerkInfo { category, ..PerkInfo::default() } })
    }

    #[test]
    fn the_signature_perk_goes_to_slot_zero() {
        let v = validated_perk_ordering(&[perk("a", 0), perk("s", 3), None]);
        assert_eq!(ids_of(&v), vec![Some("s".into()), Some("a".into()), None, None]);
    }

    #[test]
    fn a_general_perk_leaves_slot_zero_for_an_empty_slot() {
        let v = validated_perk_ordering(&[perk("a", 0), perk("b", 1), None, perk("c", 2)]);
        assert_eq!(ids_of(&v), vec![None, Some("b".into()), Some("a".into()), Some("c".into())]);
        // no empty slot: slot zero is emptied
        let v = validated_perk_ordering(&[perk("a", 0), perk("b", 1), perk("c", 2), perk("d", 1), perk("e", 0)]);
        assert_eq!(ids_of(&v), vec![None, Some("b".into()), Some("c".into()), Some("d".into())]);
    }

    #[test]
    fn wrap_int_wraps_both_ways() {
        assert_eq!(wrap_int(4, 0, 3), 0);
        assert_eq!(wrap_int(-1, 0, 3), 3);
        assert_eq!(wrap_int(2, 0, 3), 2);
    }

    #[test]
    fn a_category_reads_from_a_name_or_a_byte() {
        assert_eq!(enum_category(&json!("EPerkCategory::Utility")), 2);
        assert_eq!(enum_category(&json!(3)), 3);
        assert_eq!(enum_category(&Value::Null), 0);
    }
}
