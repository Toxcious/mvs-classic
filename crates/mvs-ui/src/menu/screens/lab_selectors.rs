//! The grid lists and their tiles the Lab's pickers and the menus share: UI_GridItemList, the character grid
//! (UI_Collection_CharacterItem, UI_LAB_CharacterSelector), the map grid (UI_LAB_MapSelector) and the skin tiles
//! (UI_EquipperItem_V2, UI_LAB_SkinSelector).
//!
//! The list keeps its tiles' data, their lock and their interactors itself (a record per tile): a tile is busy
//! while it fires its own event, and what the list's handlers read of it must not need the tile
//! (controller.rs "busy"). The list's events carry the tile's data (`TileEvent`).
//!
//! Not ported: the sale ribbons and the new-content markers of the character tile.

use std::any::Any;

use serde_json::{Value, json};

use crate::bundle::{CosmeticItem, MapInfo, Rgba, RosterEntry, SkinInfo};

use crate::menu::controller::{Controller, Cx, Event, Handler};
use crate::menu::hydra;
use crate::menu::manager::{Manager, Modal};
use crate::menu::pictures::{PICTURES_SKINS, skin_pictures};
use crate::menu::scroll::scroll_list;
use crate::menu::stat_trackers::{badge_item, stat_tracker_count};
use crate::menu::widgets::badges::{badge_item_data, badge_tooltip_params};
use crate::menu::widgets::common::{self, ModalBase, Quadrilateral, fill_page, rgba};
use crate::menu::widgets::controls::buttons::ButtonMain;
use crate::menu::widgets::controls::grid_item::{GRID_READY, GRID_WAIT, GridItem, GridItemState};
use crate::menu::widgets::controls::input_label::InputLabel;
use crate::menu::widgets::controls::interactor::{FocusedCommon, batch_set_context, set_focused, set_interactable};
use crate::menu::widgets::controls::{bind, find, prop, prop_bool, prop_f64, prop_str, quad_color, set_tooltip, wd};
use crate::menu::widgets::reward_tooltip::{RewardInfo, reward_tooltip_params, thumb_of, unlock_byte};
use crate::slate::{HAlign, Margin, VAlign, Vis, WidgetId};

pub const CLEAR: Rgba = [0.0, 0.0, 0.0, 0.0];

// ---------- what the screens share ----------

pub(crate) fn set_vis(cx: &mut Cx<'_>, name: &str, v: Vis) {
    let w = find(cx, name);
    wd(cx, w).vis = v;
}

pub(crate) fn set_text(cx: &mut Cx<'_>, name: &str, t: &str) {
    let w = find(cx, name);
    wd(cx, w).text = t.to_string();
}

pub(crate) fn set_color(cx: &mut Cx<'_>, name: &str, c: Rgba) {
    let w = find(cx, name);
    wd(cx, w).color = c;
}

pub(crate) fn set_opacity(cx: &mut Cx<'_>, name: &str, o: f64) {
    let w = find(cx, name);
    wd(cx, w).opacity = o;
}

/// `f` on the controller of `w` as its own type: at once, or after the call that holds it returns (the list that
/// fires an event to a handler that calls it back); nothing for a widget that is gone.
pub(crate) fn run_on<T: Controller>(cx: &mut Cx<'_>, w: WidgetId, f: impl FnOnce(&mut T, &mut Cx<'_>) + 'static) {
    cx.run(
        w,
        Box::new(move |c, cx| {
            let any: &mut dyn Any = c;
            if let Some(t) = any.downcast_mut::<T>() {
                f(t, cx);
            }
        }),
    );
}

/// `f` on the tile controller of `w` (as `run_on`).
pub(crate) fn run_tile(cx: &mut Cx<'_>, w: WidgetId, f: impl FnOnce(&mut dyn Tile, &mut Cx<'_>) + 'static) {
    cx.run(
        w,
        Box::new(move |c, cx| {
            if let Some(t) = c.as_grid_item() {
                f(t, cx);
            }
        }),
    );
}

/// What a tile controller gives the list over `GridItem`: its interactors, its data.
pub trait Tile: GridItem {
    /// The tile's interactors (the context's widget is the tile's).
    fn interactors(&self, cx: &Cx<'_>) -> Vec<WidgetId>;
    fn set_data(&mut self, cx: &mut Cx<'_>, data: Option<TileData>);
}

/// A tile's data: the character grid's, the map grid's, the skin grid's.
#[derive(Clone, Debug)]
pub enum TileData {
    Roster(RosterEntry),
    Map(MapInfo),
    Skin(SkinTile),
    /// A reward tile (UI_EquipperItem_V2's cosmetic branch).
    Cosmetic(CosmeticTile),
    /// A PerkData asset name (the perk lists of the perks track).
    Perk(String),
    /// An advanced tutorial's tile (WBP_SubOptionModal's TutorialTypes: the key and its text).
    Tutorial {
        key: String,
        text: String,
    },
}

impl TileData {
    pub fn roster(&self) -> Option<&RosterEntry> {
        match self {
            TileData::Roster(r) => Some(r),
            _ => None,
        }
    }
    pub fn map(&self) -> Option<&MapInfo> {
        match self {
            TileData::Map(m) => Some(m),
            _ => None,
        }
    }
    pub fn skin(&self) -> Option<&SkinTile> {
        match self {
            TileData::Skin(s) => Some(s),
            _ => None,
        }
    }
    pub fn perk(&self) -> Option<&str> {
        match self {
            TileData::Perk(p) => Some(p),
            _ => None,
        }
    }
    pub fn tutorial(&self) -> Option<(&str, &str)> {
        match self {
            TileData::Tutorial { key, text } => Some((key, text)),
            _ => None,
        }
    }
}

/// The catalogue's lists, as the choosers name them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CosmeticType {
    Ringout,
    Announcer,
    Banner,
    ProfileIcon,
    StatTracker,
    Emote,
    Taunt,
}

/// The Tag each InitEquipperItemDataFrom* gives (UI_Functions; a taunt's or sticker's is its slot's "taunt{n}").
pub const KIND_TAG: [(CosmeticType, &str); 7] = [
    (CosmeticType::Ringout, "ringout"),
    (CosmeticType::Announcer, "announcer"),
    (CosmeticType::Banner, "banner"),
    (CosmeticType::ProfileIcon, "profileIcon"),
    (CosmeticType::StatTracker, "statTracker"),
    (CosmeticType::Emote, "taunt"),
    (CosmeticType::Taunt, "taunt"),
];

/// UI_EquipperItemData of a reward (InitEquipperItemDataFrom*): the item, Selected, the Tag and the character slug
/// it is for ("" for a global one).
#[derive(Clone, Debug)]
pub struct CosmeticTile {
    pub kind: CosmeticType,
    pub item: CosmeticItem,
    pub selected: bool,
    pub tag: String,
    pub character: String,
}

/// An equipper tile's data: a skin's or a reward's.
#[derive(Clone, Debug)]
pub enum EquipperTile {
    Skin(SkinTile),
    Cosmetic(CosmeticTile),
}

/// UI_EquipperItemData as InitEquipperItemDataFromSkin fills it (tag "skin"): the SkinData and Selected.
#[derive(Clone, Debug)]
pub struct SkinTile {
    pub skin: SkinInfo,
    pub selected: bool,
}

/// UI_Functions GetSelectedSkinForCharacter @0-874: the skin whose slug the character's CharacterPreferences entry
/// holds, else the IsDefaultSkin one.
pub fn selected_skin<'a>(f: &'a crate::bundle::FighterInfo, h: &mvs_settings::HydraSettings) -> Option<&'a SkinInfo> {
    let slug = h.character_skin_slug(&f.slug);
    (!slug.is_empty())
        .then(|| f.skins.iter().find(|s| s.slug == slug))
        .flatten()
        .or_else(|| f.skins.iter().find(|s| s.is_default))
}

fn f32x4(c: Rgba) -> [f32; 4] {
    [c[0] as f32, c[1] as f32, c[2] as f32, c[3] as f32]
}

/// A tile's event to the list's owner: the tile, the player, its data and its lock as the list holds them.
#[derive(Clone, Debug)]
pub struct TileEvent {
    pub item: WidgetId,
    pub player: i32,
    pub data: Option<TileData>,
    pub locked: bool,
}

/// The interactors' handler ids of the tiles.
pub(crate) const T_FOCUS: u32 = 1;
pub(crate) const T_CLICK: u32 = 2;
const T_LOOK_FOCUS: u32 = 3;
const T_LOOK_UNFOCUS: u32 = 4;
const T_INTERACTABLE: u32 = 5;
const T_NOT_INTERACTABLE: u32 = 6;
const T_UNFOCUS: u32 = 7;

fn player_of(arg: &dyn Any) -> i32 {
    arg.downcast_ref::<i32>().copied().unwrap_or(0)
}

// ---------- UI_Collection_CharacterItem (parent UI_GridItemBase): a tile of the character grid ----------

/// The tile's mastery level: the number and its bar (collapsed, see `CollectionCharacterItem`).
pub const LEVEL_PARTS: [&str; 2] = ["MLevelLabel", "ProgressBarOverlay"];

const TILE_ICON_BG: Rgba = [0.262251, 0.366253, 0.396755, 1.0];
const TILE_ICON_BG_FOCUS: Rgba = [0.89627, 0.973446, 0.871367, 1.0];

/// Not ported: the sale ribbons (SaleContainer, collapsed), the free rotation badge (collapsed for every fighter of
/// an offline account). attentionNew keeps its hidden state: UpdateAppearance @1879 shows it when the fighter's slug
/// is in LocalPreferences.UIPreferences.newContentSlugs (default empty), and a fighter's or skin's slug only gets
/// there from CheckForNewContent or an end of match mastery reward, neither of which the port has. The hover and
/// selected indicators are never shown by this class.
// Deviation: the tiles show no mastery level, on the Lab's, the Collection's and
// the fighter select's grids (UI_PM_CharacterItem builds on this tile). The build's UpdateMastery (after a Delay(0)
// from SetData) draws MLevelLabel and the level bar (ProgressBarOverlay), which read "1" and an empty bar for every
// fighter of an account without progress; here both are collapsed and UpdateMastery is not run.
#[derive(Debug, Default)]
pub struct CollectionCharacterItem {
    state: GridItemState,
    data: Option<RosterEntry>,
}

impl CollectionCharacterItem {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::<CollectionCharacterItem>::default()
    }

    fn interactor_list(cx: &Cx<'_>) -> Vec<WidgetId> {
        ["UI_InteractorP0", "UI_InteractorP1", "UI_InteractorP2", "UI_InteractorP3"]
            .iter()
            .map(|n| find(cx, n))
            .collect()
    }

    fn color(cx: &Cx<'_>, name: &str) -> Rgba {
        rgba(&prop(cx, name))
    }

    fn bg_color(cx: &mut Cx<'_>, name: &str) {
        let c = Self::color(cx, name);
        let bg = find(cx, "BG");
        cx.view.tree_mut().dynamic_material(bg).set_vector("QuadColor", f32x4(c));
    }
}

impl GridItem for CollectionCharacterItem {
    fn state(&mut self) -> &mut GridItemState {
        &mut self.state
    }

    /// The port's own lock, not the build's (its Lab unlocks every fighter through Force Unlocked): a fighter that
    /// is not ported cannot be picked.
    fn locked(&self) -> bool {
        self.data.as_ref().is_some_and(|d| d.fighter.is_none())
    }

    fn set_size(&mut self, cx: &mut Cx<'_>, x: f64, y: f64) {
        let b = find(cx, "sizeContainer");
        let w = wd(cx, b);
        w.w_override = x;
        w.h_override = y;
    }

    /// UI_GridItemBase.UpdateAppearance: the locked indicator.
    fn update_appearance(&mut self, cx: &mut Cx<'_>) {
        let v = if self.locked() { Vis::HitTestInvisible } else { Vis::Hidden };
        set_vis(cx, "LockedContainer", v);
    }
}

impl Tile for CollectionCharacterItem {
    fn interactors(&self, cx: &Cx<'_>) -> Vec<WidgetId> {
        Self::interactor_list(cx)
    }

    /// SetData: a CharacterData, or none for the Random tile.
    fn set_data(&mut self, cx: &mut Cx<'_>, data: Option<TileData>) {
        self.data = data.and_then(|d| d.roster().cloned());
        let bg = find(cx, "BG");
        if let Some(d) = self.data.clone() {
            cx.visit::<Quadrilateral, _>(bg, |q, cx| q.update_material(cx, &d.portrait));
            match cx.view.class_info(&d.class).map(|i| i.icon.clone()) {
                Some(icon) => {
                    let w = find(cx, "ClassIcon");
                    wd(cx, w).tex = Some(icon);
                }
                None => set_vis(cx, "ClassIcon", Vis::Hidden),
            }
        } else {
            set_vis(cx, "ClassIcon", Vis::Hidden);
            cx.visit::<Quadrilateral, _>(bg, |q, cx| q.update_material(cx, "MI_Quad_RandomChar"));
        }
        for n in LEVEL_PARTS {
            set_vis(cx, n, Vis::Collapsed);
        }
        Self::bg_color(cx, "BackingColor");
        self.update_appearance(cx);
        // Unverified: the build runs UpdateBadge from OnProfileUpdated / OnAccountReloaded (@4638), which the port
        // has no events for: it runs at SetData, as the Profile screen's OnAccountReloaded work does
        self.update_badge(cx);
    }
}

impl CollectionCharacterItem {
    /// UpdateBadge @3901: UI_Attention Set Count(UnclaimedRewardsForCharacter: 0 offline, so Toggle
    /// Visibility(false)), then for CalloutPerksTrainingForCharacter? Toggle Visibility(true) and SetBadgeText("!").
    /// Deviation: the badge stays hidden. Offline the callout is true for every
    /// fighter whose perks trainer was not opened this session, so the build's grid showed a "!" on every tile.
    fn update_badge(&mut self, cx: &mut Cx<'_>) {
        let att = find(cx, "UI_Attention");
        let main = cx.view.tree().find_in(att, "MainContainer");
        wd(cx, main).vis = Vis::Collapsed;
    }
}

impl Controller for CollectionCharacterItem {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        // UI_GridItemBase.OnInitialized
        self.update_appearance(cx);
        let list = Self::interactor_list(cx);
        for &i in &list {
            bind(cx, i, |s| &mut s.on_focus, T_FOCUS);
            bind(cx, i, |s| &mut s.on_click, T_CLICK);
            // the tile's own OnFocus / OnUnfocus
            bind(cx, i, |s| &mut s.on_focus, T_LOOK_FOCUS);
            bind(cx, i, |s| &mut s.on_unfocus, T_LOOK_UNFOCUS);
        }
        let fc = find(cx, "UI_FocusedCommon");
        cx.visit::<FocusedCommon, _>(fc, |f, cx| f.set_interactors(cx, &list));
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        match id {
            T_FOCUS => self.state.focus(cx, player_of(arg)),
            T_CLICK => GridItemState::select(self, cx, player_of(arg)),
            T_LOOK_FOCUS => {
                Self::bg_color(cx, "FocusColor");
                let q = find(cx, "Quad_IconBG");
                quad_color(cx, q, TILE_ICON_BG_FOCUS);
                set_opacity(cx, "ClassIcon", 1.0);
            }
            T_LOOK_UNFOCUS => {
                Self::bg_color(cx, "BackingColor");
                let q = find(cx, "Quad_IconBG");
                quad_color(cx, q, TILE_ICON_BG);
                set_opacity(cx, "ClassIcon", 0.75);
            }
            _ => {}
        }
    }

    fn resume(&mut self, cx: &mut Cx<'_>, id: u32) {
        match id {
            GRID_READY | GRID_WAIT => GridItemState::resume(self, cx, id),
            _ => {}
        }
    }

    fn as_grid_item(&mut self) -> Option<&mut dyn Tile> {
        Some(self)
    }
}

// ---------- UI_GridItemList ----------

/// A tile of the list: its widget, its data, the lock and the interactors it had when the list set it up.
#[derive(Clone, Debug)]
struct Rec {
    w: WidgetId,
    data: Option<TileData>,
    locked: bool,
    interactors: Vec<WidgetId>,
}

const L_FOCUSED: u32 = 1;
const L_SELECTED: u32 = 2;

/// The paths the Lab's selectors use: Populate without a delay, Include Random, Single Selection, a forced row or
/// column count, Ignore Custom Size and Auto Enter (the map selector). Cached items, parallax and the stacked layout
/// are off on those instances.
#[derive(Debug, Default)]
pub struct GridItemList {
    pub populated_event: Event<()>,
    pub focused_event: Event<TileEvent>,
    /// SelectedEvent(Grid Item, Player Index, ChoseRandom).
    pub selected_event: Event<(TileEvent, bool)>,
    pub unlock_all: bool,
    pub random_can_be_locked: bool,
    items: Vec<Rec>,
    guid: String,
    chose_random: [bool; 4],
}

impl GridItemList {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::<GridItemList>::default()
    }

    fn panel(cx: &Cx<'_>) -> WidgetId {
        find(cx, "gridPanelBase")
    }

    /// The tiles' widgets in order.
    pub fn items(&self) -> Vec<WidgetId> {
        self.items.iter().map(|r| r.w).collect()
    }

    /// The tile `w`'s data and lock as the list holds them.
    pub fn record(&self, w: WidgetId) -> Option<(Option<TileData>, bool)> {
        self.items.iter().find(|r| r.w == w).map(|r| (r.data.clone(), r.locked))
    }

    fn event(&self, i: usize, player: i32) -> TileEvent {
        let r = &self.items[i];
        TileEvent { item: r.w, player, data: r.data.clone(), locked: r.locked }
    }

    /// Populate @8463.
    pub fn populate(&mut self, cx: &mut Cx<'_>, data: Vec<TileData>) {
        self.items.clear();
        self.chose_random = [false; 4];
        let panel = Self::panel(cx);
        for c in cx.view.tree().widget(panel).children.clone() {
            crate::menu::view::MenuView::discard(cx, c);
        }
        for (i, d) in data.into_iter().enumerate() {
            self.add_item(cx, Some(d), i);
        }
        if prop_bool(cx, "Include Random") {
            let n = cx.view.tree().widget(panel).children.len();
            self.add_item(cx, None, n);
        }
        let g = self.guid.clone();
        self.update_context(cx, &g);
        cx.emit(self, |c| &c.populated_event, &());
    }

    /// RowAndColumnForIndex (@0-1062): with Force Grid Rows the grid fills column by column (@168), with Force Grid
    /// Columns row by row (@916); with neither (@319-911) one line along Orientation (Horizontal: the column is the
    /// index; Vertical: the row), the other axis 0, or with Stacked the index's parity (even 0, odd 1), both times
    /// -1 with Reverse. The class defaults where an instance leaves them out: Stacked true, Orientation Horizontal,
    /// Reverse false. (An Orientation of neither enumerator takes the Select's default, read as 0.)
    fn row_and_column(cx: &Cx<'_>, index: i32) -> [i32; 2] {
        let rows = prop_f64(cx, "Force Grid Rows", 0.0) as i32;
        let cols = prop_f64(cx, "Force Grid Columns", 0.0) as i32;
        if rows > 0 {
            return [index % rows, index / rows];
        }
        if cols > 0 {
            return [index / cols, index % cols];
        }
        let s = if prop_bool(cx, "Reverse") { -1 } else { 1 };
        let stacked = cx.view.tree().props(cx.w).get("Stacked").and_then(Value::as_bool).unwrap_or(true);
        let cross = if stacked && index % 2 != 0 { 1 } else { 0 };
        let o = {
            let o = prop_str(cx, "Orientation");
            if o.is_empty() { "Orient_Horizontal".to_string() } else { o.rsplit("::").next().unwrap_or("").to_string() }
        };
        let col = match o.as_str() {
            "Orient_Horizontal" => index,
            "Orient_Vertical" => cross,
            _ => 0,
        };
        let row = match o.as_str() {
            "Orient_Horizontal" => cross,
            "Orient_Vertical" => index,
            _ => 0,
        };
        [row * s, col * s]
    }

    /// AddItem @12280 / @8911, at RowAndColumnForIndex.
    fn add_item(&mut self, cx: &mut Cx<'_>, data: Option<TileData>, index: usize) {
        let cls = prop_str(cx, "Grid Item Class");
        let cls = cls.strip_suffix("_C").unwrap_or(&cls).to_string();
        let w = crate::menu::view::MenuView::create(cx, &cls);
        let unlock = self.unlock_all;
        run_tile(cx, w, move |t, _| t.state().force_unlocked = unlock);
        let [row, col] = Self::row_and_column(cx, index as i32);
        // @10469 Ignore Custom Size: the slot fills its cell and the tile keeps its own size
        let own_size = prop_bool(cx, "Ignore Custom Size");
        {
            let x = wd(cx, w);
            x.row = row;
            x.col = col;
            x.h_align = if own_size { HAlign::Fill } else { HAlign::Left };
            x.v_align = if own_size { VAlign::Fill } else { VAlign::Top };
        }
        let panel = Self::panel(cx);
        crate::menu::view::MenuView::add_child(cx, panel, w, None);
        let guid = self.guid.clone();
        let me = cx.w;
        let d2 = data.clone();
        let (interactors, locked) = cx
            .visit_dyn(w, |c, cx| {
                let t = c.as_grid_item()?;
                let list = t.interactors(cx);
                batch_set_context(cx, &list, &guid);
                t.set_data(cx, d2);
                t.state().focused_event.add(Handler { owner: me, id: L_FOCUSED });
                t.state().selected_event.add(Handler { owner: me, id: L_SELECTED });
                t.state().index = index as i32;
                let size = prop_size(cx, me);
                if !own_size {
                    t.set_size(cx, size[0], size[1]);
                }
                Some((list, t.locked()))
            })
            .flatten()
            .unwrap_or_default();
        self.items.push(Rec { w, data, locked, interactors });
        // @9201 Auto Enter: Enter((Index + 1) x Item Enter Delay, Index)
        if prop_bool(cx, "Auto Enter") {
            let delay = (index as f64 + 1.0) * prop_f64(cx, "Item Enter Delay", 0.0);
            run_tile(cx, w, move |t, cx| GridItemState::enter(t, cx, delay));
        }
    }

    /// SelectAll(Player Index, Respect Interactable) @17187: SetSelected and UpdateAppearance per tile, no events.
    pub fn select_all(&mut self, cx: &mut Cx<'_>, respect_interactable: bool) {
        for r in self.items.clone() {
            if respect_interactable && !r.interactors.first().is_some_and(|&i| interactable(cx, i)) {
                continue;
            }
            run_tile(cx, r.w, |t, cx| {
                t.state().selected = true;
                t.update_appearance(cx);
            });
        }
    }

    /// DeselectAll(Exclude Index -1, ...) @12275.
    pub fn deselect_all(&mut self, cx: &mut Cx<'_>) {
        for r in self.items.clone() {
            run_tile(cx, r.w, |t, cx| GridItemState::deselect(t, cx));
        }
    }

    /// A skin list's checkmark: each tile's `selected` is whether its skin has `slug`, the tile redrawn (the pre-match
    /// strip's UpdateAppearance writes the flag on each tile's data and calls the tile's UpdateAppearance).
    pub fn set_skin_selected(&mut self, cx: &mut Cx<'_>, slug: Option<&str>) {
        for r in &mut self.items {
            let Some(TileData::Skin(s)) = &mut r.data else { continue };
            s.selected = Some(s.skin.slug.as_str()) == slug;
            let d = r.data.clone();
            run_tile(cx, r.w, move |t, cx| t.set_data(cx, d));
        }
    }

    /// A tile's data replaced (the owner changed its Selected): the record and the tile, redrawn.
    pub fn set_tile_data(&mut self, cx: &mut Cx<'_>, w: WidgetId, data: Option<TileData>) {
        let Some(r) = self.items.iter_mut().find(|r| r.w == w) else { return };
        r.data = data.clone();
        run_tile(cx, w, move |t, cx| t.set_data(cx, data));
    }

    /// Whether the tile `w` is selected (GridItemBase Selected).
    pub fn tile_selected(&self, cx: &mut Cx<'_>, w: WidgetId) -> bool {
        cx.visit_dyn(w, |c, _| c.as_grid_item().is_some_and(|t| t.state().selected)).unwrap_or(false)
    }

    /// Update Appearance @16634.
    pub fn update_appearance(&mut self, cx: &mut Cx<'_>) {
        for r in self.items.clone() {
            run_tile(cx, r.w, |t, cx| t.update_appearance(cx));
        }
    }

    /// OnSelected @11425: the Random tile (no data) rolls, unless Disable Random.
    fn on_selected(&mut self, cx: &mut Cx<'_>, item: WidgetId, player: i32) {
        let Some(at) = self.items.iter().position(|r| r.w == item) else { return };
        if prop_bool(cx, "Single Selection") {
            for r in self.items.clone() {
                if r.w != item {
                    run_tile(cx, r.w, |t, cx| {
                        if t.state().selected {
                            GridItemState::deselect(t, cx);
                        }
                    });
                }
            }
        }
        if self.items[at].data.is_some() {
            let ev = self.event(at, player);
            let chose = self.chose_random[player.clamp(0, 3) as usize];
            cx.emit(self, |c| &c.selected_event, &(ev, chose));
        } else if !prop_bool(cx, "Disable Random") {
            self.randomize_selection(cx, item, player);
        }
    }

    /// RandomizeSelection @16662. GetRandomIndex draws RandomIntegerInRange(0, Length(Grid Items)), both ends
    /// included: the Random tile itself rolls again, and the index one past the last tile is no item, so that roll
    /// selects nothing (ChoseRandom stays set and the confirm sound plays).
    /// Deviation: the Random tile that fired the select is busy: its deselect runs when its call returns, and the
    /// rolled tile's select event reaches this list after this call.
    fn randomize_selection(&mut self, cx: &mut Cx<'_>, _busy: WidgetId, player: i32) {
        for r in self.items.clone() {
            run_tile(cx, r.w, |t, cx| {
                if t.state().selected {
                    GridItemState::deselect(t, cx);
                }
            });
        }
        Manager::unfocus_all(cx, player);
        let n = self.items.len();
        let roll = (cx.view.manager.random() * (n as f64 + 1.0)).floor() as usize;
        let p = player.clamp(0, 3) as usize;
        if let Some(r) = self.items.get(roll).cloned() {
            if let Some(&i) = r.interactors.get(p) {
                let _ = i;
            }
            run_tile(cx, r.w, move |t, cx| t.state().focus(cx, player));
            self.chose_random[p] = true;
            run_tile(cx, r.w, move |t, cx| GridItemState::select(t, cx, player));
        } else {
            self.chose_random[p] = true;
        }
        cx.post("ui_menu_confirm");
    }

    /// UI_CollectionChooser ReOrderWidgets' re-add (@1246-2143): the tiles in a new order, each at its index's
    /// RowAndColumnForIndex, Index set.
    pub fn reorder(&mut self, cx: &mut Cx<'_>, order: &[WidgetId]) {
        let mut items = Vec::new();
        for &w in order {
            if let Some(r) = self.items.iter().find(|r| r.w == w) {
                items.push(r.clone());
            }
        }
        self.items = items;
        for (index, r) in self.items.clone().iter().enumerate() {
            let [row, col] = Self::row_and_column(cx, index as i32);
            let x = wd(cx, r.w);
            x.row = row;
            x.col = col;
            run_tile(cx, r.w, move |t, _| t.state().index = index as i32);
        }
    }

    /// IndexForData: the first tile whose data `pred` takes.
    pub fn index_for_data(&self, pred: impl Fn(&TileData) -> bool) -> Option<usize> {
        self.items.iter().position(|r| r.data.as_ref().is_some_and(&pred))
    }

    /// FocusIndex @13374: GetInteractor(Player Index) is Array_Get of the item's interactors, none past their end (a
    /// one-interactor item for a player above 0): nothing focused, the event all the same.
    pub fn focus_index(&mut self, cx: &mut Cx<'_>, player: i32, index: usize) {
        let Some(r) = self.items.get(index) else { return };
        if let Some(&i) = usize::try_from(player).ok().and_then(|p| r.interactors.get(p)) {
            set_focused(cx, i, false);
        }
        let ev = self.event(index, player);
        cx.emit(self, |c| &c.focused_event, &ev);
    }

    /// UpdateContext.
    pub fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str) {
        self.guid = ctx.to_string();
        for r in &self.items {
            batch_set_context(cx, &r.interactors, ctx);
        }
    }
}

fn prop_size(cx: &Cx<'_>, w: WidgetId) -> [f64; 2] {
    let s = cx.view.tree().props(w).get("Item Size").cloned().unwrap_or(Value::Null);
    [s.get("X").and_then(Value::as_f64).unwrap_or(0.0), s.get("Y").and_then(Value::as_f64).unwrap_or(0.0)]
}

fn interactable(cx: &Cx<'_>, i: WidgetId) -> bool {
    cx.view.manager.interactor(i).is_some_and(|s| s.get_interactable())
}

impl Controller for GridItemList {
    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        let m = common::margin(
            prop(cx, "Item Padding").as_object().map(|_| ()).and(Some(&prop(cx, "Item Padding"))),
            Margin::default(),
        );
        let p = Self::panel(cx);
        wd(cx, p).slot_pad = m;
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        let Some(&(item, player)) = arg.downcast_ref::<(WidgetId, i32)>() else { return };
        let Some(at) = self.items.iter().position(|r| r.w == item) else { return };
        match id {
            // OnFocused @11177
            L_FOCUSED => {
                let ev = self.event(at, player);
                cx.emit(self, |c| &c.focused_event, &ev);
            }
            L_SELECTED => self.on_selected(cx, item, player),
            _ => {}
        }
    }
}

// ---------- UI_CharacterTypeInfo ----------

#[derive(Debug, Default)]
pub struct CharacterTypeInfo;

impl CharacterTypeInfo {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(CharacterTypeInfo)
    }

    pub fn set_character(&mut self, cx: &mut Cx<'_>, c: &RosterEntry) {
        set_text(cx, "propertyLabel", &c.property);
        set_text(cx, "typeLabel", &c.kind);
        let Some(info) = cx.view.class_info(&c.class).cloned() else { return };
        set_text(cx, "classLabel", &info.display_name);
        for n in ["classIcon", "classIcon_Shadow"] {
            let w = find(cx, n);
            wd(cx, w).tex = Some(info.icon.clone());
        }
    }
}

impl Controller for CharacterTypeInfo {}

// ---------- UI_PM_CharacterInfo_V2 ----------

const DIFFICULTY_LOW: Rgba = [0.008023, 0.693872, 0.258183, 1.0];
const DIFFICULTY_HIGH: Rgba = [0.904661, 0.0, 0.066626, 1.0];
const PM_ENTER: u32 = 1;

#[derive(Debug, Default)]
pub struct PMCharacterInfo {
    has_set_character: bool,
}

impl PMCharacterInfo {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::<PMCharacterInfo>::default()
    }

    /// Enter @2093: unseen until the Delay's frame.
    fn enter(cx: &mut Cx<'_>, delay: f64) {
        let me = cx.w;
        wd(cx, me).opacity = 0.0;
        cx.view.delay(delay, Handler { owner: me, id: PM_ENTER });
    }

    /// SetCharacter @31 -> UpdateAppearance @2463.
    pub fn set_character(&mut self, cx: &mut Cx<'_>, c: &RosterEntry) {
        self.has_set_character = true;
        let ty = find(cx, "UI_CharacterTypeInfo");
        let c2 = c.clone();
        cx.visit::<CharacterTypeInfo, _>(ty, |t, cx| t.set_character(cx, &c2));
        set_vis(cx, "contentOverlay", if self.has_set_character { Vis::SelfHitTestInvisible } else { Vis::Hidden });
        set_text(cx, "nameLabel", &c.display_name.to_uppercase());
        wd(cx, ty).vis = Vis::Visible;
        if c.difficulty == "Medium" {
            set_vis(cx, "Difficulty", Vis::Collapsed);
        } else {
            let high = c.difficulty == "High";
            set_color(cx, "Image_94", if high { DIFFICULTY_HIGH } else { DIFFICULTY_LOW });
            set_text(cx, "DifficultyLabel", if high { "Expert" } else { "Recommended" });
            set_vis(cx, "Difficulty", Vis::SelfHitTestInvisible);
        }
        set_vis(cx, "UI_PM_Experimental", if c.experimental { Vis::Visible } else { Vis::Collapsed });
        let pl = cx.view.tree().find_in(ty, "propertyLabel");
        wd(cx, pl).vis = Vis::Visible;
        Self::enter(cx, 0.0);
    }

    /// RandomCharacter @2183 (contentOverlay is not touched: it shows once a character was set).
    pub fn random_character(&mut self, cx: &mut Cx<'_>) {
        set_text(cx, "nameLabel", "RANDOM");
        for n in ["UI_PM_Experimental", "UI_CharacterTypeInfo", "Difficulty"] {
            set_vis(cx, n, Vis::Hidden);
        }
        Self::enter(cx, 0.0);
    }
}

impl Controller for PMCharacterInfo {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        set_vis(cx, "contentOverlay", Vis::Hidden);
    }

    fn resume(&mut self, cx: &mut Cx<'_>, id: u32) {
        if id == PM_ENTER {
            let me = cx.w;
            wd(cx, me).opacity = 1.0;
        }
    }
}

// ---------- UI_LAB_CharacterSelector ----------

const CS_POPULATED: u32 = 1;
const CS_FOCUSED: u32 = 2;
const CS_FOCUSED_SKINS: u32 = 3;
const CS_SELECTED: u32 = 4;
const CS_SETUP: u32 = 5;
const CS_BG_CLICK: u32 = 6;

#[derive(Debug, Default)]
pub struct LabCharacterSelector {
    base: ModalBase,
    /// SelectedEvent(Character, ChoseRandom).
    pub selected_event: Event<(RosterEntry, bool)>,
    pub canceled_event: Event<()>,
    character: String,
}

impl LabCharacterSelector {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::<LabCharacterSelector>::default()
    }

    fn dismiss(&mut self, cx: &mut Cx<'_>, from_cancel: bool) {
        let id = self.base.modal_id.clone();
        Manager::dismiss_modal(cx, &id);
        if from_cancel {
            cx.emit(self, |c| &c.canceled_event, &());
        }
    }
}

impl Modal for LabCharacterSelector {
    fn pre_show(&mut self, cx: &mut Cx<'_>, id: &str) {
        self.base.pre_show(cx, id);
        set_opacity(cx, "UI_PM_CharacterInfo_V2", 0.0);
    }

    /// PostShow @87; SetupList @15 after a Delay(0).
    fn post_show(&mut self, cx: &mut Cx<'_>, instructions: &str, extra_data: &str) {
        self.character = extra_data.to_string();
        let unlock = ["tutorial", "lab", "bot"].iter().any(|k| instructions.contains(k));
        let ctx = cx.view.manager.new_guid();
        self.base.context = ctx.clone();
        let grid = find(cx, "CharacterGrid");
        cx.visit::<GridItemList, _>(grid, |g, cx| {
            g.unlock_all = unlock;
            g.random_can_be_locked = true;
            g.update_context(cx, &ctx);
        });
        let bg = find(cx, "buttonBG");
        batch_set_context(cx, &[bg], &ctx);
        set_focused(cx, bg, true);
        let me = cx.w;
        cx.view.delay(0.0, Handler { owner: me, id: CS_SETUP });
    }

    fn pre_dismiss(&mut self, cx: &mut Cx<'_>) {
        self.base.pre_dismiss(cx);
    }

    /// (the port's own) the skins go a few seconds after their last draw (the skin selector that follows draws
    /// them).
    fn post_dismiss(&mut self, cx: &mut Cx<'_>) {
        cx.view.release_pictures(PICTURES_SKINS);
    }

    fn request_dismiss(&mut self, cx: &mut Cx<'_>, _reason: &str, from_cancel: bool) {
        self.dismiss(cx, from_cancel);
    }

    fn get_modal_context(&self) -> String {
        self.base.context.clone()
    }
}

impl Controller for LabCharacterSelector {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let bg = find(cx, "buttonBG");
        bind(cx, bg, |s| &mut s.on_click, CS_BG_CLICK);
    }

    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        let info = find(cx, "UI_PM_CharacterInfo_V2");
        let ty = cx.view.tree().find_in(info, "UI_CharacterTypeInfo");
        wd(cx, ty).opacity = 0.5;
    }

    // Deviation: the 16:9 box follows the page.
    fn construct(&mut self, cx: &mut Cx<'_>) {
        let b = find(cx, "SizeBox_0");
        fill_page(cx, b);
    }

    fn tick(&mut self, cx: &mut Cx<'_>, _dt: f64) {
        let b = find(cx, "SizeBox_0");
        fill_page(cx, b);
    }

    fn resume(&mut self, cx: &mut Cx<'_>, id: u32) {
        match id {
            CS_SETUP => {
                let me = cx.w;
                let grid = find(cx, "CharacterGrid");
                cx.visit::<GridItemList, _>(grid, |g, _| {
                    // OnCharacterGridPopulated @1951
                    g.populated_event.add(Handler { owner: me, id: CS_POPULATED });
                    // OnCharacterFocused @1707
                    g.focused_event.add(Handler { owner: me, id: CS_FOCUSED });
                    // (the port's own) the focused fighter's skins are a click away: the skin selector's tiles
                    g.focused_event.add(Handler { owner: me, id: CS_FOCUSED_SKINS });
                    // OnCharacterSelected
                    g.selected_event.add(Handler { owner: me, id: CS_SELECTED });
                });
                let roster: Vec<TileData> = cx.view.roster().iter().cloned().map(TileData::Roster).collect();
                run_on::<GridItemList>(cx, grid, move |g, cx| g.populate(cx, roster));
            }
            CS_BG_CLICK => self.dismiss(cx, true),
            _ => {}
        }
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        match id {
            CS_BG_CLICK => self.dismiss(cx, true),
            CS_POPULATED => {
                let grid = find(cx, "CharacterGrid");
                let character = self.character.clone();
                run_on::<GridItemList>(cx, grid, move |g, cx| {
                    let at = g
                        .index_for_data(|d| {
                            d.roster().is_some_and(|r| {
                                r.fighter.as_deref() == Some(character.as_str()) && !character.is_empty()
                            })
                        })
                        .unwrap_or(0);
                    g.focus_index(cx, 0, at);
                });
            }
            CS_FOCUSED => {
                let Some(ev) = arg.downcast_ref::<TileEvent>() else { return };
                let info = find(cx, "UI_PM_CharacterInfo_V2");
                match ev.data.as_ref().and_then(TileData::roster).cloned() {
                    Some(r) => {
                        cx.visit::<PMCharacterInfo, _>(info, |p, cx| p.set_character(cx, &r));
                    }
                    None => {
                        cx.visit::<PMCharacterInfo, _>(info, |p, cx| p.random_character(cx));
                    }
                }
            }
            CS_FOCUSED_SKINS => {
                let Some(ev) = arg.downcast_ref::<TileEvent>() else { return };
                let fighter = ev.data.as_ref().and_then(TileData::roster).and_then(|r| r.fighter.clone());
                let names = skin_pictures(cx.view, fighter.as_deref());
                cx.view.hold_pictures(PICTURES_SKINS, names.iter().map(String::as_str));
            }
            CS_SELECTED => {
                let Some((ev, chose)) = arg.downcast_ref::<(TileEvent, bool)>() else { return };
                // the port's lock keeps a fighter that is not ported from being picked; a Random pick does not use
                // the rolled fighter, so it goes through
                let Some(r) = ev.data.as_ref().and_then(TileData::roster).cloned() else { return };
                if ev.locked && !*chose {
                    return;
                }
                cx.emit(self, |c| &c.selected_event, &(r, *chose));
                self.dismiss(cx, false);
            }
            _ => {}
        }
    }

    fn as_modal(&mut self) -> Option<&mut dyn Modal> {
        Some(self)
    }
}

// ---------- UI_LAB_PlayerCountLabel ----------

#[derive(Debug, Default)]
pub struct LabPlayerCountLabel;

impl LabPlayerCountLabel {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(LabPlayerCountLabel)
    }

    pub fn set_count(&mut self, cx: &mut Cx<'_>, t: &str) {
        set_text(cx, "NumberPlayers", t);
    }

    pub fn update_bg_color(&mut self, cx: &mut Cx<'_>, c: Rgba) {
        let q = find(cx, "WBP_Quadrilateral");
        quad_color(cx, q, c);
    }
}

impl Controller for LabPlayerCountLabel {}

// ---------- UI_LAB_MapItem (parent UI_GridItemBase): a tile of the map selector ----------

const MAP_ITEM: Rgba = [0.194618, 0.571125, 0.552012, 1.0];
const MAP_ITEM_OFF: Rgba = [0.048172, 0.061246, 0.088656, 1.0];
const MAP_NAME: Rgba = [0.89627, 0.973446, 0.871367, 1.0];
const MAP_NAME_OFF: Rgba = [0.119538, 0.165132, 0.191202, 1.0];

/// No focus, select or error animation or sound (GetAnimations, GetAudioEvents).
#[derive(Debug, Default)]
pub struct LabMapItem {
    state: GridItemState,
    data: Option<MapInfo>,
}

impl LabMapItem {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::<LabMapItem>::default()
    }

    fn count(cx: &Cx<'_>) -> WidgetId {
        find(cx, "UI_LAB_PlayerCountLabel")
    }

    fn count_color(cx: &mut Cx<'_>, c: Rgba) {
        let w = Self::count(cx);
        cx.visit::<LabPlayerCountLabel, _>(w, |l, cx| l.update_bg_color(cx, c));
    }
}

impl GridItem for LabMapItem {
    fn state(&mut self) -> &mut GridItemState {
        &mut self.state
    }

    fn locked(&self) -> bool {
        false
    }

    /// (Ignore Custom Size on the selector's grid)
    fn set_size(&mut self, _cx: &mut Cx<'_>, _x: f64, _y: f64) {}

    fn enter_anim(&self) -> Option<&'static str> {
        Some("EnterAnim")
    }

    /// UpdateAppearance @687: the base's selected indicator (Shows Selected State), then the map's texts and
    /// picture.
    fn update_appearance(&mut self, cx: &mut Cx<'_>) {
        set_vis(cx, "selectedIndicator", if self.state.selected { Vis::HitTestInvisible } else { Vis::Hidden });
        let Some(m) = self.data.clone() else { return };
        set_text(cx, "nameLabel", &m.data.display_name);
        set_text(cx, "propertyLabel", &m.data.property);
        cx.view.load_texture(&m.data.texture);
        let t = find(cx, "thumbnail");
        wd(cx, t).tex = Some(m.data.texture.clone());
        set_vis(cx, "UI_PM_Experimental", if m.data.experimental { Vis::Visible } else { Vis::Collapsed });
    }
}

impl Tile for LabMapItem {
    fn interactors(&self, cx: &Cx<'_>) -> Vec<WidgetId> {
        vec![find(cx, "UI_Interactor")]
    }

    /// SetData @1812 -> UI_GridItemBase.SetData (UpdateAppearance) -> @702.
    fn set_data(&mut self, cx: &mut Cx<'_>, data: Option<TileData>) {
        self.data = data.and_then(|d| d.map().cloned());
        self.state.is_entering = false;
        self.update_appearance(cx);
        // "{MinNumPlayers}-{MaxNumPlayers}", or "{NumPlayers}" when the two are equal
        if let Some(m) = &self.data {
            let (lo, hi) = (m.data.min, m.data.max);
            let t = if lo == hi { format!("{lo}") } else { format!("{lo}-{hi}") };
            let c = Self::count(cx);
            cx.visit::<LabPlayerCountLabel, _>(c, |l, cx| l.set_count(cx, &t));
        }
    }
}

impl Controller for LabMapItem {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        // UI_GridItemBase.OnInitialized
        self.update_appearance(cx);
        let i = find(cx, "UI_Interactor");
        bind(cx, i, |s| &mut s.on_focus, T_FOCUS);
        bind(cx, i, |s| &mut s.on_click, T_CLICK);
        // @337
        let fc = find(cx, "UI_FocusedCommon");
        cx.visit::<FocusedCommon, _>(fc, |f, cx| f.set_interactor(cx, i));
        // OnFocus @393 / OnUnfocus @540
        bind(cx, i, |s| &mut s.on_focus, T_LOOK_FOCUS);
        bind(cx, i, |s| &mut s.on_unfocus, T_LOOK_UNFOCUS);
        // OnInteractable @1911 / OnNotInteractable @2242
        bind(cx, i, |s| &mut s.on_interactable, T_INTERACTABLE);
        bind(cx, i, |s| &mut s.on_not_interactable, T_NOT_INTERACTABLE);
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        match id {
            T_FOCUS => self.state.focus(cx, player_of(arg)),
            T_CLICK => GridItemState::select(self, cx, player_of(arg)),
            T_LOOK_FOCUS => {
                Self::count_color(cx, MAP_NAME);
                set_vis(cx, "QuadBorder", Vis::Hidden);
                set_vis(cx, "QuadSelectedBorder", Vis::Hidden);
            }
            T_LOOK_UNFOCUS => {
                Self::count_color(cx, MAP_ITEM);
                set_vis(cx, "QuadBorder", Vis::SelfHitTestInvisible);
                set_vis(cx, "QuadSelectedBorder", Vis::SelfHitTestInvisible);
            }
            T_INTERACTABLE => {
                set_opacity(cx, "thumbnail", 1.0);
                let q = find(cx, "QuadBorder");
                quad_color(cx, q, MAP_ITEM);
                Self::count_color(cx, MAP_ITEM);
                set_color(cx, "nameLabel", MAP_NAME);
            }
            T_NOT_INTERACTABLE => {
                set_opacity(cx, "thumbnail", 0.15);
                let q = find(cx, "QuadBorder");
                quad_color(cx, q, MAP_ITEM_OFF);
                Self::count_color(cx, MAP_ITEM_OFF);
                set_color(cx, "nameLabel", MAP_NAME_OFF);
            }
            _ => {}
        }
    }

    fn resume(&mut self, cx: &mut Cx<'_>, id: u32) {
        if matches!(id, GRID_READY | GRID_WAIT) {
            GridItemState::resume(self, cx, id);
        }
    }

    fn as_grid_item(&mut self) -> Option<&mut dyn Tile> {
        Some(self)
    }
}

// ---------- UI_LAB_MapSelector ----------

/// IsMapAvailable?: in forced offline mode (the port's, as for the fighters and skins), for map_training, or
/// without the game instance's Lab map restriction every map is available; else a map is when its MapRotations hold
/// the selector's rotation.
/// Deviation, a rule on top: a map whose level the port does not have is unavailable, so the build's rules for an
/// unavailable map show it (sorted last, dimmed, not interactable): the stage manifest's (stageOfLevel) has its
/// first level.
pub fn is_map_available(m: &MapInfo) -> bool {
    m.stage.is_some()
}

const MS_CANCEL: u32 = 1;
const MS_SUBMIT3: u32 = 2;
const MS_SCROLL: u32 = 3;
const MS_ACCEPT: u32 = 4;
const MS_BTN_CANCEL: u32 = 5;
const MS_CLEAR: u32 = 6;
const MS_SELECT_ALL: u32 = 7;
const MS_GRID_SELECTED: u32 = 8;
const MS_GRID_POPULATED: u32 = 9;
const MS_POST_SHOW: u32 = 10;

#[derive(Debug, Default)]
pub struct LabMapSelector {
    base: ModalBase,
    /// SelectedEvent(Map), MultiSelectedEvent(Maps), CanceledEvent.
    pub selected_event: Event<MapInfo>,
    pub multi_selected_event: Event<Vec<MapInfo>>,
    pub canceled_event: Event<()>,
    /// EMapRotation, set by the opener.
    pub selected_rotation: i32,
    allow_multi_select: bool,
    prevent_auto_dismiss: bool,
    selected_maps: Vec<String>,
}

impl LabMapSelector {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::<LabMapSelector>::default()
    }

    fn grid(cx: &Cx<'_>) -> WidgetId {
        find(cx, "MapGrid")
    }

    fn button(cx: &Cx<'_>, name: &str) -> WidgetId {
        find(cx, name)
    }

    fn interactor_of(cx: &Cx<'_>, button: WidgetId) -> WidgetId {
        cx.view.tree().find_in(button, "UI_Interactor")
    }

    /// Init(All Maps, Selected Maps, Player Count, Allow Multi Select, Prevent Auto Dismiss) @4417. The build loads
    /// the soft maps one after the other (LoadAsset) and fills the grid when the last is in; the port has them.
    pub fn init(
        &mut self,
        cx: &mut Cx<'_>,
        all_maps: &[String],
        selected: &[String],
        player_count: i32,
        allow_multi_select: bool,
        prevent_auto_dismiss: bool,
    ) {
        self.allow_multi_select = allow_multi_select;
        let grid = Self::grid(cx);
        cx.view.tree_mut().set_prop(grid, "Single Selection", json!(!allow_multi_select));
        self.prevent_auto_dismiss = prevent_auto_dismiss;
        self.selected_maps = selected.to_vec();
        let count = find(cx, "UI_LAB_PlayerCountLabel");
        let t = player_count.to_string();
        cx.visit::<LabPlayerCountLabel, _>(count, |l, cx| l.set_count(cx, &t));
        wd(cx, count).vis = if player_count > 0 { Vis::SelfHitTestInvisible } else { Vis::Collapsed };
        let loaded: Vec<MapInfo> = all_maps.iter().filter_map(|n| cx.view.map(n)).collect();
        // SortedMapsByAvailability: the available maps in their order, then the others
        let mut data: Vec<TileData> =
            loaded.iter().filter(|m| is_map_available(m)).cloned().map(TileData::Map).collect();
        data.extend(loaded.iter().filter(|m| !is_map_available(m)).cloned().map(TileData::Map));
        run_on::<GridItemList>(cx, grid, move |g, cx| g.populate(cx, data));
    }

    /// GetSelectedMaps: the tiles that are selected.
    fn get_selected_maps(&self, cx: &mut Cx<'_>) -> Vec<MapInfo> {
        let grid = Self::grid(cx);
        let items = cx.visit::<GridItemList, _>(grid, |g, _| {
            g.items()
                .into_iter()
                .filter_map(|w| g.record(w).and_then(|(d, _)| d.and_then(|d| d.map().cloned())).map(|m| (w, m)))
                .collect::<Vec<_>>()
        });
        items
            .unwrap_or_default()
            .into_iter()
            .filter(|(w, _)| {
                cx.visit_dyn(*w, |c, _| c.as_grid_item().is_some_and(|t| t.state().selected)).unwrap_or(false)
            })
            .map(|(_, m)| m)
            .collect()
    }

    /// UpdateAppearance @2701.
    fn update_appearance(&mut self, cx: &mut Cx<'_>) {
        let multi = self.allow_multi_select;
        let shown = |on: bool| if on { Vis::SelfHitTestInvisible } else { Vis::Collapsed };
        set_vis(cx, "actionsContainer", shown(multi));
        set_vis(cx, "instructionsLabel", shown(multi));
        set_vis(cx, "UI_InputLabel", shown(!multi));
        set_text(cx, "titleLabel", if multi { "Map Rotation" } else { "Select a Map" });
        let n = self.get_selected_maps(cx).len();
        let accept = find(cx, "buttonAccept");
        let ai = Self::interactor_of(cx, accept);
        set_interactable(cx, ai, n > 0);
        let label = format!("Accept ({} {})", n, if n == 1 { "Map" } else { "Maps" });
        cx.visit::<ButtonMain, _>(accept, |b, cx| b.set_label(cx, &label));
    }

    /// Accept @5661.
    fn accept_maps(&mut self, cx: &mut Cx<'_>) {
        let accept = find(cx, "buttonAccept");
        let ai = Self::interactor_of(cx, accept);
        if !interactable(cx, ai) || !cx.view.visible_in_hierarchy(cx.w) {
            return;
        }
        let maps = self.get_selected_maps(cx);
        cx.emit(self, |c| &c.multi_selected_event, &maps);
        if !self.prevent_auto_dismiss {
            self.dismiss(cx, false);
        }
    }

    /// RequestDismiss @2111.
    fn dismiss(&mut self, cx: &mut Cx<'_>, canceling: bool) {
        let id = self.base.modal_id.clone();
        Manager::dismiss_modal(cx, &id);
        if canceling {
            cx.emit(self, |c| &c.canceled_event, &());
        }
    }

    /// UpdateContext @1955 (GetInteractors).
    pub fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str) {
        self.base.context = ctx.to_string();
        let list: Vec<WidgetId> = ["buttonAccept", "btn_Clear", "btn_Cancel", "btn_SelectAll"]
            .iter()
            .map(|n| {
                let b = Self::button(cx, n);
                Self::interactor_of(cx, b)
            })
            .collect();
        batch_set_context(cx, &list, ctx);
        let grid = Self::grid(cx);
        let ctx = ctx.to_string();
        cx.visit::<GridItemList, _>(grid, |g, cx| g.update_context(cx, &ctx));
    }

    /// OnMapGridPopulated @5119.
    fn on_map_grid_populated(&mut self, cx: &mut Cx<'_>) {
        let grid = Self::grid(cx);
        let multi = self.allow_multi_select;
        let selected = self.selected_maps.clone();
        run_on::<GridItemList>(cx, grid, move |g, cx| {
            for w in g.items() {
                let Some((Some(TileData::Map(m)), _)) = g.record(w) else { continue };
                let available = is_map_available(&m);
                let sel = available && selected.contains(&m.name);
                let ii = g
                    .record(w)
                    .map(|_| ())
                    .and(cx.visit_dyn(w, |c, cx| c.as_grid_item().map(|t| t.interactors(cx))).flatten());
                run_tile(cx, w, move |t, cx| {
                    t.state().toggle_selected_state = multi;
                    if sel {
                        t.state().selected = true;
                        t.update_appearance(cx);
                    }
                });
                if let Some(&i) = ii.as_ref().and_then(|l| l.first()) {
                    set_interactable(cx, i, available);
                }
            }
            let at = if selected.is_empty() {
                0
            } else {
                g.index_for_data(|d| d.map().is_some_and(|m| m.name == selected[0])).unwrap_or(0)
            };
            g.focus_index(cx, 0, at);
        });
        // (UpdateAppearance after the grid's call has run: the tiles' selection is in)
        let me = cx.w;
        cx.view.delay(0.0, Handler { owner: me, id: MS_POST_SHOW + 100 });
    }
}

impl Modal for LabMapSelector {
    fn pre_show(&mut self, cx: &mut Cx<'_>, id: &str) {
        self.base.pre_show(cx, id);
    }

    /// PostShow @1872: 0.1 s later a new context, and Cancel takes the focus.
    fn post_show(&mut self, cx: &mut Cx<'_>, _instructions: &str, _extra_data: &str) {
        let me = cx.w;
        cx.view.delay(0.1, Handler { owner: me, id: MS_POST_SHOW });
    }

    fn pre_dismiss(&mut self, cx: &mut Cx<'_>) {
        self.base.pre_dismiss(cx);
    }

    fn post_dismiss(&mut self, cx: &mut Cx<'_>) {
        self.base.post_dismiss(cx);
    }

    fn request_dismiss(&mut self, cx: &mut Cx<'_>, _reason: &str, from_cancel: bool) {
        self.dismiss(cx, from_cancel);
    }

    fn get_modal_context(&self) -> String {
        self.base.context.clone()
    }
}

impl Controller for LabMapSelector {
    /// OnInitialized @2261.
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        let sc = find(cx, "scrollContainer");
        {
            let m = &mut cx.view.manager;
            m.on_cancel.add(Handler { owner: me, id: MS_CANCEL });
            m.on_submit3.add(Handler { owner: me, id: MS_SUBMIT3 });
            m.on_scroll.add(Handler { owner: me, id: MS_SCROLL });
            m.register_scrollbox(sc);
        }
        let btn = |cx: &mut Cx<'_>, n: &str, id: u32| {
            let b = find(cx, n);
            let i = cx.view.tree().find_in(b, "UI_Interactor");
            bind(cx, i, |s| &mut s.on_click, id);
        };
        btn(cx, "buttonAccept", MS_ACCEPT); // @4157
        btn(cx, "btn_Cancel", MS_BTN_CANCEL); // @4256
        btn(cx, "btn_Clear", MS_CLEAR); // @4289
        btn(cx, "btn_SelectAll", MS_SELECT_ALL); // @4345
        let grid = Self::grid(cx);
        cx.visit::<GridItemList, _>(grid, |g, _| {
            // MapGrid SelectedEvent @4988
            g.selected_event.add(Handler { owner: me, id: MS_GRID_SELECTED });
            g.populated_event.add(Handler { owner: me, id: MS_GRID_POPULATED });
        });
    }

    /// Destruct @5500.
    fn destruct(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        let sc = find(cx, "scrollContainer");
        let m = &mut cx.view.manager;
        m.on_cancel.remove_owner(me);
        m.on_submit3.remove_owner(me);
        m.on_scroll.remove_owner(me);
        m.unregister_scrollbox(sc);
    }

    // Deviation: the 16:9 box follows the page.
    fn construct(&mut self, cx: &mut Cx<'_>) {
        let b = find(cx, "SizeBox_0");
        fill_page(cx, b);
    }

    fn tick(&mut self, cx: &mut Cx<'_>, _dt: f64) {
        let b = find(cx, "SizeBox_0");
        fill_page(cx, b);
    }

    fn resume(&mut self, cx: &mut Cx<'_>, id: u32) {
        match id {
            MS_POST_SHOW => {
                let ctx = cx.view.manager.new_guid();
                self.update_context(cx, &ctx);
                let b = Self::button(cx, "btn_Cancel");
                let i = Self::interactor_of(cx, b);
                set_focused(cx, i, true);
            }
            x if x == MS_POST_SHOW + 100 => self.update_appearance(cx),
            _ => {}
        }
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        match id {
            // OnCancel @2683
            MS_CANCEL => self.dismiss(cx, false),
            // OnSubmit3 @5602
            MS_SUBMIT3 => {
                if player_of(arg) == 0 {
                    self.accept_maps(cx);
                }
            }
            // OnScroll @5454 (UI_Functions.ScrollList): as the other lists
            MS_SCROLL => {
                if let Some(&(_, _, amount)) = arg.downcast_ref::<(i32, i32, f64)>() {
                    let sc = find(cx, "scrollContainer");
                    scroll_list(cx.view.tree_mut(), sc, amount);
                }
            }
            MS_ACCEPT => self.accept_maps(cx),
            MS_BTN_CANCEL => self.dismiss(cx, true),
            MS_CLEAR => {
                let grid = Self::grid(cx);
                cx.visit::<GridItemList, _>(grid, |g, cx| g.deselect_all(cx)); // @4289
                self.update_appearance(cx);
            }
            MS_SELECT_ALL => {
                let grid = Self::grid(cx);
                cx.visit::<GridItemList, _>(grid, |g, cx| g.select_all(cx, true)); // @4345
                self.update_appearance(cx);
            }
            MS_GRID_SELECTED => {
                let Some((ev, _)) = arg.downcast_ref::<(TileEvent, bool)>() else { return };
                if self.allow_multi_select {
                    // Deviation: the clicked tile is busy while it fires this event (its controller is checked
                    // out), so GetSelectedMaps cannot read its Selected: the count is taken after the call.
                    let me = cx.w;
                    cx.view.delay(0.0, Handler { owner: me, id: MS_POST_SHOW + 100 });
                    let grid = Self::grid(cx);
                    run_on::<GridItemList>(cx, grid, |g, cx| g.update_appearance(cx));
                } else if let Some(m) = ev.data.as_ref().and_then(TileData::map).cloned() {
                    cx.emit(self, |c| &c.selected_event, &m);
                    self.dismiss(cx, false);
                }
            }
            MS_GRID_POPULATED => self.on_map_grid_populated(cx),
            _ => {}
        }
    }

    fn as_modal(&mut self) -> Option<&mut dyn Modal> {
        Some(self)
    }
}

// ---------- UI_EquipperItem_V2 (parent UI_GridItemBase): a tile of the skin grid and of the reward choosers ----------

const EQ_WHITE: Rgba = [0.89627, 0.973446, 0.871367, 1.0];
const EQ_OFF_BG: Rgba = [0.048172, 0.061246, 0.088656, 1.0];
/// A ring-out tile's dark backing (@11591 / @12684).
const EQ_RINGOUT_BG: Rgba = [0.009721, 0.011612, 0.033105, 1.0];
/// The taunt slot's input label tint.
const EQ_TAUNT_TINT: Rgba = [0.68, 0.8, 0.85, 1.0];
/// The default badge's slug: it carries no tooltip.
const EQ_DEFAULT_BADGE: &str = "stat_tracking_bundle_default";

/// UpdateAppearance. Not ported: the outline quads (ItemBorder, ItemBorder_Focus, TabOnIndicator), the sale ribbon
/// and its 1 s timer. EnterAnim never plays here (the skin grid's Auto Enter is false and UI_LAB_SkinSelector does
/// not call Enter); SelectAnim starts at the select, whose
/// handler dismisses the modal in the same call (UI_LAB_SkinSelector @2126-2309); ErrorAnim needs a locked tile,
/// which no list has (every item is owned). attentionNew stays hidden (see UI_Collection_CharacterItem). The title
/// is empty on the skin grid (LabelContainer hidden). GetLocked is false without a connection and GetClaimable
/// false without a reward, so the locked look (indicator, dimmer) never shows.
/// The other rewards (SetData @8602-9074, @14933-15020): a sticker's or a taunt's RewardThumbnailMaterial, the
/// texture of the rest (DisplayTextureRef / TextureRef; the catalogue names the menu texture), the ring-out's dark
/// backing and the taunt slot's input label (@6971). Update From Preferences (@15364) fills an overview or a Local
/// Play slot from the preferences.
#[derive(Debug, Default)]
pub struct EquipperItem {
    state: GridItemState,
    data: Option<EquipperTile>,
    is_ring_out: bool,
    /// UI_GridItemBase's Shows Tooltip once Set Shows Tooltip set it (None: the instance's property).
    shows_tooltip: Option<bool>,
    /// The tile was clicked (the owner reads its data through `data`).
    pub selected_event: Event<Option<EquipperTile>>,
}

/// The catalogue's list of a kind.
fn kind_list(c: &crate::bundle::Cosmetics, k: CosmeticType) -> &[CosmeticItem] {
    match k {
        CosmeticType::Ringout => &c.ringout,
        CosmeticType::Announcer => &c.announcer,
        CosmeticType::Banner => &c.banner,
        CosmeticType::ProfileIcon => &c.profile_icon,
        CosmeticType::StatTracker => &c.stat_tracker,
        CosmeticType::Emote => &c.emote,
        CosmeticType::Taunt => &c.taunt,
    }
}

/// RewardInfo CreateFromAsset of a tile's asset (a badge's is badges.rs `badge_item_data`'s): its ERewardType,
/// DisplayName, Rarity, Tier, UnlockLocation, AssociatedCharacter, RewardThumbnail and ToolTipPreviewMaterial;
/// Count 1, StarLevel 0, Category 0.
fn reward_info(d: &EquipperTile) -> RewardInfo {
    match d {
        EquipperTile::Skin(s) => RewardInfo {
            reward_type: 1,
            display_name: s.skin.display_name.clone(),
            count: 1.0,
            rarity: s.skin.rarity.clone(),
            tier: s.skin.tier,
            unlock: unlock_byte(s.skin.unlock.as_deref()),
            fighter: s.skin.character.clone(),
            thumb: s.skin.thumb.clone(),
            asset: s.skin.name.clone(),
            ..RewardInfo::default()
        },
        EquipperTile::Cosmetic(c) => RewardInfo {
            reward_type: match c.kind {
                CosmeticType::Taunt => 2,
                CosmeticType::StatTracker => 4,
                CosmeticType::ProfileIcon => 5,
                CosmeticType::Ringout => 6,
                CosmeticType::Emote => 8,
                CosmeticType::Announcer => 11,
                CosmeticType::Banner => 15,
            },
            display_name: c.item.name.clone(),
            count: 1.0,
            rarity: c.item.rarity.clone(),
            tier: c.item.tier.unwrap_or(0),
            unlock: unlock_byte(c.item.unlock.as_deref()),
            fighter: c.item.fighter.clone(),
            thumb: thumb_of(c.item.thumb.as_deref()),
            preview: thumb_of(c.item.preview.as_deref()),
            asset: hydra::picture_name(&c.item.path).unwrap_or_default(),
            ..RewardInfo::default()
        },
    }
}

/// The Tag an InitEquipperItemDataFrom* gives a kind's tile.
pub fn kind_tag(k: CosmeticType) -> &'static str {
    KIND_TAG.iter().find(|(t, _)| *t == k).map_or("", |(_, tag)| tag)
}

impl EquipperItem {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::<EquipperItem>::default()
    }

    /// The tile's data (None: empty).
    pub fn data(&self) -> Option<EquipperTile> {
        self.data.clone()
    }

    fn color(cx: &Cx<'_>, name: &str) -> Rgba {
        rgba(&prop(cx, name))
    }

    fn rarity(&self) -> &str {
        let r = match &self.data {
            Some(EquipperTile::Cosmetic(c)) => c.item.rarity.as_str(),
            Some(EquipperTile::Skin(s)) => s.skin.rarity.as_str(),
            None => "",
        };
        if r.is_empty() { "None" } else { r }
    }

    /// GetIsFocused: the hover indicator is visible.
    fn is_focused(cx: &Cx<'_>) -> bool {
        let h = find(cx, "HoverIndicator");
        cx.view.tree().widget(h).shown()
    }

    /// Focus: the parent's @4542 (nothing for a tile that is focused already), then UpdateFocus.
    fn focus(&mut self, cx: &mut Cx<'_>, player: i32) {
        if !Self::is_focused(cx) {
            let h = find(cx, "HoverIndicator");
            wd(cx, h).vis = Vis::SelfHitTestInvisible;
            self.state.focus(cx, player);
        }
        self.update_focus(cx);
    }

    /// Unfocus: the parent's @5123, then UpdateFocus.
    fn unfocus(&mut self, cx: &mut Cx<'_>) {
        if Self::is_focused(cx) {
            let h = find(cx, "HoverIndicator");
            wd(cx, h).vis = Vis::Hidden;
        }
        self.update_focus(cx);
    }

    /// UpdateFocus @12216.
    fn update_focus(&mut self, cx: &mut Cx<'_>) {
        let f = Self::is_focused(cx);
        let h = find(cx, "HoverIndicator");
        cx.visit::<Quadrilateral, _>(h, |q, cx| q.wobble(cx, f));
        let dims = prop_bool(cx, "DimsIcon");
        set_opacity(cx, "Icon", if f || !dims { 1.0 } else { 0.6 });
        set_vis(cx, "Shadow", if f { Vis::Hidden } else { Vis::SelfHitTestInvisible });
        set_vis(cx, "ItemBorder", if f { Vis::Hidden } else { Vis::SelfHitTestInvisible });
        set_vis(cx, "ItemBorder_Focus", if f { Vis::SelfHitTestInvisible } else { Vis::Collapsed });
        // @11591 / @12684: a ring-out keeps its dark backing
        let bg = if self.is_ring_out {
            EQ_RINGOUT_BG
        } else {
            Self::color(cx, if f { "BorderColor" } else { "BackingColor" })
        };
        set_color(cx, "ItemBG", bg);
        set_vis(cx, "LockedDimmer", Vis::Collapsed); // (never locked)
        let lock = find(cx, "QUAD_LockBG");
        let rc = rarity_color(self.rarity());
        quad_color(cx, lock, if f { EQ_WHITE } else { rc });
        let claim = find(cx, "QUAD_ClaimableBG");
        let uc = Self::color(cx, "UnlockedBorderColor");
        quad_color(cx, claim, if f { EQ_WHITE } else { uc });
    }

    /// SetInteractable.
    fn set_tile_interactable(&mut self, cx: &mut Cx<'_>, b: bool) {
        let i = find(cx, "UI_Interactor");
        set_interactable(cx, i, b);
        let bg = if b { Self::color(cx, "BackingColor") } else { EQ_OFF_BG };
        set_color(cx, "ItemBG", bg);
        set_opacity(cx, "Icon", if b { 1.0 } else { 0.35 });
        set_opacity(cx, "LabelContainer", if b { 1.0 } else { 0.35 });
    }

    /// SetData @14207 (SetupRewardInfo: the reward's Rarity is the item's or the SkinData's).
    fn set_equipper(&mut self, cx: &mut Cx<'_>, data: Option<EquipperTile>) {
        self.data = data;
        self.update_appearance(cx);
    }

    /// The Shows Tooltip variable.
    fn shows_tooltip(&self, cx: &Cx<'_>) -> bool {
        self.shows_tooltip
            .unwrap_or_else(|| cx.view.tree().props(cx.w).get("Shows Tooltip").and_then(Value::as_bool).unwrap_or(true))
    }

    /// UI_GridItemBase Set Shows Tooltip @8137 -> @1602: the variable, and each interactor's Tooltip
    /// Params.ShowTooltip.
    pub fn set_shows_tooltip(&mut self, cx: &mut Cx<'_>, on: bool) {
        self.shows_tooltip = Some(on);
        let i = find(cx, "UI_Interactor");
        if let Some(s) = cx.view.manager.interactor_mut(i) {
            s.tooltip_params.show = on;
        }
    }

    /// UI_GridItemBase ShowContent(Show, Delay 0) @7712-7993: Content SelfHitTestInvisible / Hidden; a hidden content
    /// shown again unfocuses the tile first (@2170).
    fn show_content(&mut self, cx: &mut Cx<'_>, show: bool) {
        let c = find(cx, "Content");
        if show && !cx.view.tree().widget(c).shown() {
            self.unfocus(cx);
        }
        wd(cx, c).vis = if show { Vis::SelfHitTestInvisible } else { Vis::Hidden };
    }

    /// Set On @13586 (UI_TabInterface): SetTimeViewed (the backend's new-content marks: none offline), Is On, the
    /// TabOnIndicator shown (Conditional Visibility 4 / 2).
    pub fn set_on(&mut self, cx: &mut Cx<'_>, on: bool) {
        set_vis(cx, "TabOnIndicator", if on { Vis::SelfHitTestInvisible } else { Vis::Hidden });
    }

    /// Update From Preferences(tag, Character, Manual Slug) @15364 (case-insensitive switch on the tag). The slot's
    /// reward is found in the catalogue (the build: the asset registry's assets of the class, or PGI GetTaunts) by
    /// the manual slug, else the Hydra preferences'; none found leaves the tile as it was. "profileIcon" and
    /// "podium" are not in the switch (nothing).
    pub fn update_from_preferences(
        &mut self,
        cx: &mut Cx<'_>,
        tag: &str,
        character: Option<&RosterEntry>,
        manual: &str,
    ) {
        let t = tag.to_lowercase();
        let g = cx.prefs.hydra.clone();
        // InitEquipperItemDataFromX(asset, SelectString(stored, manual)): Selected is slug = that slug (true)
        let pick = |me: &mut Self, cx: &mut Cx<'_>, kind: CosmeticType, stored: &str| {
            let slug = if manual.is_empty() { stored } else { manual };
            let e = hydra::cosmetics(cx.view).and_then(|c| kind_list(c, kind).iter().find(|x| x.slug == slug).cloned());
            if let Some(item) = e {
                let tag = kind_tag(kind).to_string();
                me.set_equipper(
                    cx,
                    Some(EquipperTile::Cosmetic(CosmeticTile {
                        kind,
                        item,
                        selected: true,
                        tag,
                        character: String::new(),
                    })),
                );
            }
        };
        if t == "skin" {
            // @17015-17279: the tile's skin is GetSelectedSkinForCharacter(Character) (the Hydra preferences' skin,
            // else the default), Selected its slug = SelectString(its slug, Manual Slug, IsEmpty(manual)) (a Local
            // Play slot's manual slug other than the Hydra skin's leaves the tile unselected)
            let skin = character
                .and_then(|c| c.fighter.as_ref())
                .and_then(|k| cx.view.fighters().get(k))
                .and_then(|f| selected_skin(f, &g))
                .cloned();
            let d = skin.map(|skin| {
                let selected = skin.slug == if manual.is_empty() { skin.slug.as_str() } else { manual };
                EquipperTile::Skin(SkinTile { skin, selected })
            });
            self.set_equipper(cx, d);
            return;
        }
        if matches!(t.as_str(), "taunt1" | "taunt2" | "taunt3" | "taunt4") {
            // @16990 ShowContent(false), then @1086: the first of PGI GetTaunts whose slug is the manual one, else
            // GetTauntForSlot(n - 1): InitEquipperItemDataFromTaunt, SetData, ShowContent(true)
            self.show_content(cx, false);
            let slot = usize::from(t.as_bytes()[5] - b'1');
            let slug = if !manual.is_empty() {
                manual.to_string()
            } else {
                character.map(|c| g.taunt_for_slot(slot, &c.slug).to_string()).unwrap_or_default()
            };
            if slug.is_empty() {
                return;
            }
            let found = hydra::cosmetics(cx.view).and_then(|c| {
                let e = hydra::cosmetic_by_slug(c, &slug)?;
                if c.taunt.contains(e) {
                    Some((CosmeticType::Taunt, e.clone()))
                } else if c.emote.contains(e) {
                    Some((CosmeticType::Emote, e.clone()))
                } else {
                    None
                }
            });
            let Some((kind, item)) = found else { return };
            let d = CosmeticTile {
                kind,
                item,
                selected: true,
                tag: format!("taunt{}", slot + 1),
                character: character.map(|c| c.slug.clone()).unwrap_or_default(),
            };
            self.set_equipper(cx, Some(EquipperTile::Cosmetic(d)));
            self.show_content(cx, true);
            return;
        }
        match t.as_str() {
            "ringout" => pick(self, cx, CosmeticType::Ringout, &g.ring_out_vfx.slug),
            "announcer" => pick(self, cx, CosmeticType::Announcer, &g.announcer_pack.slug),
            "banner" => pick(self, cx, CosmeticType::Banner, &g.banner.slug),
            "stattracker" | "stattracker2" | "stattracker3" => {
                // @16059-16711: L_StatTrackerEquippable = the slot's; @10021 its slug, StatTrackingBundle_Default
                // when empty
                let st = match t.as_str() {
                    "stattracker" => 0,
                    "stattracker2" => 1,
                    _ => 2,
                };
                let stored = if g.stat_trackers[st].slug.is_empty() {
                    hydra::cosmetics(cx.view).map(|c| c.defaults.stat_tracker.clone()).unwrap_or_default()
                } else {
                    g.stat_trackers[st].slug.clone()
                };
                pick(self, cx, CosmeticType::StatTracker, &stored);
            }
            _ => {}
        }
    }

    /// InputActionForTaunt @0-1524: the GameTaunt action (EInputAction 4-7) of a TauntData tile with a Character: a
    /// parent slot's from its tag's number, a chooser tile's from the slot whose GetTauntForSlot is its slug.
    fn input_action_for_taunt(cx: &Cx<'_>, d: &CosmeticTile) -> Option<String> {
        if d.character.is_empty() || !matches!(d.kind, CosmeticType::Taunt | CosmeticType::Emote) {
            return None;
        }
        if prop_bool(cx, "Is Parent Slot") {
            let tag = d.tag.to_lowercase();
            let n = tag.split("taunt").nth(1).and_then(|s| s.parse::<i32>().ok())? - 1;
            return (0..=3).contains(&n).then(|| format!("GameTaunt{}", n + 1));
        }
        (0..=3usize)
            .find(|&i| cx.prefs.hydra.taunt_for_slot(i, &d.character) == d.item.slug)
            .map(|i| format!("GameTaunt{}", i + 1))
    }

    /// @6971: the label Hidden; a tile whose tag holds "taunt" and that InputActionForTaunt answers shows the action.
    fn update_input_label(&mut self, cx: &mut Cx<'_>, d: &EquipperTile) {
        let lw = find(cx, "UI_InputLabel");
        wd(cx, lw).vis = Vis::Hidden;
        let EquipperTile::Cosmetic(c) = d else { return };
        if !c.tag.to_lowercase().contains("taunt") {
            return;
        }
        let Some(action) = Self::input_action_for_taunt(cx, c) else { return };
        cx.visit::<InputLabel, _>(lw, |l, cx| {
            let mut p = l.params.clone();
            p.action = action;
            p.modifier = "None".into();
            p.label = String::new();
            p.player_index = 0;
            p.automatic_input_type = true;
            p.fixed_input_type = 0;
            p.tint = EQ_TAUNT_TINT;
            p.text_size = 16.0;
            p.key_size = 16.0;
            p.icon_size = [32.0, 32.0];
            l.update_params(cx, p);
        });
        wd(cx, lw).vis = Vis::SelfHitTestInvisible;
    }
}

fn rarity_color(r: &str) -> Rgba {
    common::RARITY.iter().find(|(n, _)| *n == r).map_or(common::RARITY[0].1, |(_, c)| *c)
}

impl GridItem for EquipperItem {
    fn state(&mut self) -> &mut GridItemState {
        &mut self.state
    }

    /// GetLocked is false without a connection (forced offline), and every item and every skin is owned (Deviation;
    /// IsSkinOwned? true, `is_cosmetic_owned`).
    fn locked(&self) -> bool {
        match &self.data {
            Some(EquipperTile::Cosmetic(c)) => !hydra::is_cosmetic_owned(&c.item.slug),
            _ => false,
        }
    }

    fn set_size(&mut self, cx: &mut Cx<'_>, x: f64, y: f64) {
        let b = find(cx, "SizeContainer");
        let w = wd(cx, b);
        w.w_override = x;
        w.h_override = y;
    }

    /// UpdateAppearance @11281: the parent's indicators (@6550), then @4660.
    fn update_appearance(&mut self, cx: &mut Cx<'_>) {
        let show_sel = prop_bool(cx, "ShowSelectedState");
        let selected = match &self.data {
            Some(EquipperTile::Skin(s)) => s.selected,
            Some(EquipperTile::Cosmetic(c)) => c.selected,
            None => false,
        };
        set_vis(cx, "SelectedIndicator", if selected && show_sel { Vis::HitTestInvisible } else { Vis::Hidden });
        set_vis(cx, "ClaimableIndicator", Vis::Hidden);
        let locked = self.locked();
        set_vis(cx, "LockedIndicator", if locked { Vis::HitTestInvisible } else { Vis::Hidden });
        let Some(d) = self.data.clone() else {
            set_vis(cx, "Overlay_RewardType", Vis::Collapsed);
            let on = prop_bool(cx, "InteractableWhenEmpty");
            self.set_tile_interactable(cx, on);
            return;
        };
        set_vis(cx, "Overlay_RewardType", Vis::HitTestInvisible);
        self.set_tile_interactable(cx, true);
        let icon = find(cx, "Icon");
        // a badge's item data (UI_BadgeInitItemData with GetStatTrackerCount: UI_CollectionChooser @8994, the
        // overview's Update From Preferences @10347): the icon and the star level of its count (the stats file's
        // tracker counts and the arcade record's Hard completions) Deviation: the counts are the stats file's at each
        // read; not those of the last own profile read (0 until the Profile screen first loads one).
        let badge = match &d {
            EquipperTile::Cosmetic(c) if c.kind == CosmeticType::StatTracker => {
                let stats = &mut cx.view.session.stats;
                let trackers = stats.data().trackers.clone();
                let hard = stats.arcade().hard_completions;
                let count =
                    hydra::cosmetics(cx.view).map_or(0.0, |cat| stat_tracker_count(&c.item, cat, &trackers, hard));
                Some(badge_item_data(cx, &badge_item(&c.item), count))
            }
            _ => None,
        };
        match &d {
            EquipperTile::Skin(s) => {
                // the skin branch @8136
                for n in ["ItemBorder", "TabOnIndicator"] {
                    let w = find(cx, n);
                    cx.view.tree_mut().set_prop(w, "OutlineThickness", json!(0.03)); // @8421
                }
                wd(cx, icon).tex = Some(s.skin.icon.clone());
            }
            EquipperTile::Cosmetic(c) => {
                wd(cx, icon).tex = if let Some(b) = &badge {
                    b.icon.clone()
                } else {
                    // @8768 / @9002 SetBrushFromSoftMaterial (a sticker, a taunt), the others' SetBrushFromTexture
                    // after their LoadAsset.
                    // Unverified: the catalogue's null icon is a reward whose RewardThumbnailMaterial is None (the
                    // stickers Emote_Global, TestEmote and Emote_StarEyes, listed since the offline filter is not
                    // read): @8768 SetBrushFromSoftMaterial of a None reference, read as the Image with no
                    // resource: the port draws its tint alone; the native's brush for a null material is not read.
                    c.item.icon.as_deref().and_then(hydra::picture_name)
                };
            }
        }
        // @8511 / @15004
        self.is_ring_out = matches!(&d, EquipperTile::Cosmetic(c) if c.kind == CosmeticType::Ringout);
        let bg = if self.is_ring_out { EQ_RINGOUT_BG } else { Self::color(cx, "BackingColor") };
        set_color(cx, "ItemBG", bg);
        wd(cx, icon).vis = Vis::Visible;
        // @6513: no slant for a reward without a rarity
        let r = self.rarity().to_string();
        let slant = find(cx, "QuadTypeSlant");
        quad_color(cx, slant, if r == "None" { CLEAR } else { rarity_color(&r) });
        self.update_input_label(cx, &d);
        // @6921 UpdateTooltip(IsClickable) @15020: none for the default bundle; else the interactor's Tooltip Params
        // are GetDefaultTooltipParams (Override Tooltip is false on every instance): the reward type's tooltip class
        // (UI_Functions TooltipClassForRewardType), Offset (SizeContainer width x 0.25, -125), ShowTooltip = Shows
        // Tooltip, Placement 12, Data = the RewardInfo. Its Tooltip text is EquipperItemData.Description (@299): a
        // badge's "Badge", the taunts' Format("{number}"), the other Init*s' an EX_TextConst the pseudo does not print;
        // no reward tooltip draws it (WBP_RichTooltip Set Text is pop_flow @10398), so the others pass none.
        let default_badge = matches!(&d, EquipperTile::Cosmetic(c) if c.item.slug == EQ_DEFAULT_BADGE);
        if !default_badge {
            let sb = find(cx, "SizeContainer");
            let w = wd(cx, sb).w_override;
            let w = if w.is_nan() { 0.0 } else { w };
            let shows = self.shows_tooltip(cx);
            let i = find(cx, "UI_Interactor");
            let p = match &badge {
                Some(b) => badge_tooltip_params(b, shows, w * 0.25, -125.0),
                None => reward_tooltip_params(&reward_info(&d), String::new(), shows, w * 0.25, -125.0),
            };
            set_tooltip(cx, i, p);
        }
        self.update_focus(cx);
    }
}

impl Tile for EquipperItem {
    fn interactors(&self, cx: &Cx<'_>) -> Vec<WidgetId> {
        vec![find(cx, "UI_Interactor")]
    }

    fn set_data(&mut self, cx: &mut Cx<'_>, data: Option<TileData>) {
        let d = match data {
            Some(TileData::Skin(s)) => Some(EquipperTile::Skin(s)),
            Some(TileData::Cosmetic(c)) => Some(EquipperTile::Cosmetic(c)),
            _ => None,
        };
        self.set_equipper(cx, d);
    }
}

impl Controller for EquipperItem {
    /// UI_GridItemBase.OnInitialized @7538.
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        self.update_appearance(cx);
        let i = find(cx, "UI_Interactor");
        bind(cx, i, |s| &mut s.on_focus, T_FOCUS);
        bind(cx, i, |s| &mut s.on_unfocus, T_UNFOCUS);
        bind(cx, i, |s| &mut s.on_click, T_CLICK);
    }

    /// PreConstruct @13452.
    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        let title = common::text(prop_value(cx, "Title").as_ref());
        set_text(cx, "titleLabel", &title);
        set_vis(cx, "LabelContainer", if title.is_empty() { Vis::Hidden } else { Vis::SelfHitTestInvisible });
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        match id {
            T_FOCUS => self.focus(cx, player_of(arg)),
            T_UNFOCUS => self.unfocus(cx),
            T_CLICK => {
                GridItemState::select(self, cx, player_of(arg));
                let d = self.data.clone();
                cx.emit(self, |c| &c.selected_event, &d);
            }
            _ => {}
        }
    }

    fn resume(&mut self, cx: &mut Cx<'_>, id: u32) {
        if matches!(id, GRID_READY | GRID_WAIT) {
            GridItemState::resume(self, cx, id);
        }
    }

    fn as_grid_item(&mut self) -> Option<&mut dyn Tile> {
        Some(self)
    }
}

fn prop_value(cx: &Cx<'_>, k: &str) -> Option<Value> {
    cx.view.tree().props(cx.w).get(k).cloned()
}

// ---------- UI_LAB_SkinSelector ----------

const SS_SETUP: u32 = 1;
const SS_FOCUSED: u32 = 2;
const SS_SELECTED: u32 = 3;
const SS_BG_CLICK: u32 = 4;

/// Populate: UI_Functions "Filtered Skins for Character" read as forced offline mode (the fighters' skins of the
/// menu bundle). The checkmark (Populate @488-602: InitEquipperItemDataFromSkin with the Selected Slug of
/// GetSelectedSkinForCharacter) is `selected_skin`'s, but for a Lab slot that has the fighter.
// Deviation: the Lab slot's own skin is ticked when the slot has this fighter; the build ticks the
// profile's saved skin, which a Lab pick never writes. The select (@1932-2309) takes any tile with a SkinData: no
/// lock is read (every skin owned).
#[derive(Debug, Default)]
pub struct LabSkinSelector {
    base: ModalBase,
    /// SelectedEvent(Character, SkinData), CanceledEvent.
    pub selected_event: Event<(String, SkinInfo)>,
    pub canceled_event: Event<()>,
    character: String,
    /// The Lab slot's skin name ('' the default) when the slot has this fighter (`skin_selector_data`).
    slot_skin: Option<String>,
}

/// Between the fighter and the slot's skin in the page's extra data.
const SLOT_SKIN_SEP: char = '|';

/// The page's extra data: the fighter, and the Lab slot's skin name when it has one.
pub fn skin_selector_data(fighter: &str, slot_skin: Option<&str>) -> String {
    match slot_skin {
        Some(s) => format!("{fighter}{SLOT_SKIN_SEP}{s}"),
        None => fighter.to_string(),
    }
}

impl LabSkinSelector {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::<LabSkinSelector>::default()
    }

    fn dismiss(&mut self, cx: &mut Cx<'_>, from_cancel: bool) {
        if from_cancel {
            cx.emit(self, |c| &c.canceled_event, &());
        }
        let id = self.base.modal_id.clone();
        Manager::dismiss_modal(cx, &id);
    }
}

impl Modal for LabSkinSelector {
    fn pre_show(&mut self, cx: &mut Cx<'_>, id: &str) {
        self.base.pre_show(cx, id);
    }

    fn post_show(&mut self, cx: &mut Cx<'_>, instructions: &str, extra_data: &str) {
        let unlock = ["lab", "tutorial"].iter().any(|k| instructions.contains(k));
        let (character, slot_skin) = match extra_data.split_once(SLOT_SKIN_SEP) {
            Some((f, s)) => (f, Some(s.to_string())),
            None => (extra_data, None),
        };
        self.character = character.to_string();
        self.slot_skin = slot_skin;
        let ctx = cx.view.manager.new_guid();
        self.base.context = ctx.clone();
        let grid = find(cx, "SkinGrid");
        cx.visit::<GridItemList, _>(grid, |g, cx| {
            g.unlock_all = unlock;
            g.update_context(cx, &ctx);
        });
        let bg = find(cx, "buttonBG");
        batch_set_context(cx, &[bg], &ctx);
        set_focused(cx, bg, true);
        let me = cx.w;
        cx.view.delay(0.0, Handler { owner: me, id: SS_SETUP });
    }

    fn pre_dismiss(&mut self, cx: &mut Cx<'_>) {
        self.base.pre_dismiss(cx);
    }

    fn post_dismiss(&mut self, cx: &mut Cx<'_>) {
        self.base.post_dismiss(cx);
    }

    fn request_dismiss(&mut self, cx: &mut Cx<'_>, _reason: &str, from_cancel: bool) {
        self.dismiss(cx, from_cancel);
    }

    fn get_modal_context(&self) -> String {
        self.base.context.clone()
    }
}

impl Controller for LabSkinSelector {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let bg = find(cx, "buttonBG");
        bind(cx, bg, |s| &mut s.on_click, SS_BG_CLICK);
    }

    // Deviation: the 16:9 box follows the page.
    fn construct(&mut self, cx: &mut Cx<'_>) {
        let b = find(cx, "SizeBox_0");
        fill_page(cx, b);
    }

    fn tick(&mut self, cx: &mut Cx<'_>, _dt: f64) {
        let b = find(cx, "SizeBox_0");
        fill_page(cx, b);
    }

    fn resume(&mut self, cx: &mut Cx<'_>, id: u32) {
        match id {
            SS_SETUP => {
                let me = cx.w;
                let grid = find(cx, "SkinGrid");
                cx.visit::<GridItemList, _>(grid, |g, _| {
                    g.focused_event.add(Handler { owner: me, id: SS_FOCUSED });
                    g.selected_event.add(Handler { owner: me, id: SS_SELECTED });
                });
                let tiles: Vec<TileData> = match cx.view.fighters().get(&self.character) {
                    Some(f) => {
                        let slot = self.slot_skin.as_deref().and_then(|n| {
                            f.skins.iter().find(|s| if n.is_empty() { s.is_default } else { s.name == n })
                        });
                        let sel = slot.or_else(|| selected_skin(f, &cx.prefs.hydra)).map(|s| s.slug.clone());
                        f.skins
                            .iter()
                            .map(|s| {
                                TileData::Skin(SkinTile { skin: s.clone(), selected: Some(&s.slug) == sel.as_ref() })
                            })
                            .collect()
                    }
                    None => Vec::new(),
                };
                // Populate @1830: the grid's Populate, then Set Shows Tooltip(false) on every tile (@76-448); then
                // FocusIndex(0, 0) @29
                run_on::<GridItemList>(cx, grid, move |g, cx| {
                    g.populate(cx, tiles);
                    for w in g.items() {
                        run_on::<EquipperItem>(cx, w, |t, cx| t.set_shows_tooltip(cx, false));
                    }
                    g.focus_index(cx, 0, 0);
                });
            }
            SS_BG_CLICK => self.dismiss(cx, true),
            _ => {}
        }
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        match id {
            SS_BG_CLICK => self.dismiss(cx, true),
            SS_FOCUSED => {
                let Some(ev) = arg.downcast_ref::<TileEvent>() else { return };
                if let Some(s) = ev.data.as_ref().and_then(TileData::skin) {
                    set_text(cx, "nameLabel", &s.skin.display_name);
                }
            }
            SS_SELECTED => {
                let Some((ev, _)) = arg.downcast_ref::<(TileEvent, bool)>() else { return };
                let Some(s) = ev.data.as_ref().and_then(TileData::skin).cloned() else { return };
                let c = self.character.clone();
                cx.emit(self, |c| &c.selected_event, &(c, s.skin));
                cx.post("ui_skinselect_select");
                self.dismiss(cx, false);
            }
            _ => {}
        }
    }

    fn as_modal(&mut self) -> Option<&mut dyn Modal> {
        Some(self)
    }
}
