//! Local Play's later player steps: cosmetics, perks, the ready confirmation and the name entry. The slot
//! (`local_play::MatchSetupPlayer`) calls them from Set State and binds their events.
//!
//! A call into a controller that is in hand (busy) is deferred until its method returns (`controller.rs`): a handler
//! of an event a step fires reads the argument, never the firing widget.

use std::any::Any;

use super::collection_chooser::CollectionChooser;
use super::controls::buttons::{ButtonMain, ButtonSecondary};
use super::controls::interactor::set_focused;
use super::controls::{find, prop_f64, wd};
use super::local_play::{LPCharacterInfo, set_input_player_index, set_interactors, set_label_player};
use super::modals::show_modal_generic;
use super::perks::{Perk, PerksEquipper, PerksLoadout, perk_data};
use crate::bundle::{RosterEntry, SkinInfo};
use crate::hud::LocalPlayNumbers;
use crate::menu::controller::{Controller, Cx, Event, Handler};
use crate::menu::hydra;
use crate::menu::screens::lab_selectors::{EquipperItem, EquipperTile, run_on};
use crate::menu::view::MenuView;
use crate::slate::{Margin, Vis, WidgetId};

fn handler(cx: &Cx<'_>, id: u32) -> Handler {
    Handler { owner: cx.w, id }
}

fn inter(cx: &Cx<'_>, button: WidgetId) -> WidgetId {
    cx.view.tree().find_in(button, "UI_Interactor")
}

/// UI_Functions "Perk Data From Slugs" (@0-1216): a slug the perk list lacks gives an empty entry (the array keeps
/// its length).
/// Deviation: the build looks the slug up in the PerkManager's cached global config (HydraPerkDataMap, @307); the
/// port reads the menu bundle's perks, whose slugs are the PerkData's own.
pub fn perk_data_from_slugs(view: &MenuView, slugs: &[String]) -> Vec<Option<Perk>> {
    let Some(menu) = view.bundle.menu.as_ref() else { return vec![None; slugs.len()] };
    slugs
        .iter()
        .map(|s| {
            if s.is_empty() {
                return None;
            }
            let id = menu.perks.iter().find(|(_, p)| p.slug == *s).map(|(id, _)| id.clone());
            perk_data(view, id.as_deref())
        })
        .collect()
}

/// The perks' slugs, an empty slot "".
pub fn perk_slugs_of(perks: &[Option<Perk>]) -> Vec<String> {
    perks.iter().map(|p| p.as_ref().map(|p| p.info.slug.clone()).unwrap_or_default()).collect()
}

/// GetLocalPlayUsername(Player Index): both numbers are Player Index + 1;
/// Username = the saved name, or PlayerNumberLong when it is empty; Custom Name Set = the saved name is not empty and
/// not PlayerNumberShort.
pub fn own_username(cx: &Cx<'_>, player: i32) -> (String, String, bool) {
    let p = usize::try_from(player).unwrap_or(0).min(3);
    let saved = cx.prefs.local_play.players[p].username.clone();
    let n = LocalPlayNumbers::new(&cx.view.text).local_play_username(p);
    let custom = !saved.is_empty() && saved != n.short;
    let username = if saved.is_empty() { n.long } else { saved };
    (username, n.short, custom)
}

/// The interactors of an equipper tile.
fn tile_interactors(cx: &mut Cx<'_>, w: WidgetId) -> Vec<WidgetId> {
    cx.visit_dyn(w, |c, cx| c.as_grid_item().map(|t| t.interactors(cx))).flatten().unwrap_or_default()
}

// ---------- UI_LP_Player_Cosmetics (state 6) ----------

/// The six equippers (UI_EquipperItem_V2) by their tag (Update Selections: "ringout", "banner", Format("taunt{number}")
/// of the taunt index + 1), in GetAllEquippers order; OffsetCloseTab of each equipper's Selected (@5512-5965)
const EQUIPPERS: [(&str, &str, f64); 6] = [
    ("equipperRingout", "ringout", 250.0),
    ("equipperBanner", "banner", 125.0),
    ("equipperTaunt1", "taunt1", 0.0),
    ("equipperTaunt2", "taunt2", 125.0),
    ("equipperTaunt3", "taunt3", 250.0),
    ("equipperTaunt4", "taunt4", 375.0),
];

const CO_BUTTON: u32 = 1;
const CO_CANCEL: u32 = 2;
const CO_CHOSEN: u32 = 3;
const CO_HIDDEN: u32 = 4;
const CO_EQUIPPER: u32 = 10;

/// GetEquippedSlugs' outputs
#[derive(Clone, Debug, Default)]
pub struct EquippedSlugs {
    pub ringout: String,
    pub banner: String,
    pub taunts: Vec<String>,
}

fn tile_tag(d: &Option<EquipperTile>) -> String {
    match d {
        Some(EquipperTile::Cosmetic(c)) => c.tag.clone(),
        _ => String::new(),
    }
}

fn tile_slug(d: &Option<EquipperTile>) -> String {
    match d {
        Some(EquipperTile::Cosmetic(c)) => c.item.slug.clone(),
        Some(EquipperTile::Skin(s)) => s.skin.slug.clone(),
        None => String::new(),
    }
}

#[derive(Debug, Default)]
pub struct LPPlayerCosmetics {
    pub player_index: i32,
    /// CosmeticsDoneEvent(Player Index), CosmeticsStepBackEvent
    pub cosmetics_done_event: Event<i32>,
    pub cosmetics_step_back_event: Event<()>,
    pub character: Option<RosterEntry>,
    /// Last Selected Item: the index into `EQUIPPERS`
    last_selected: Option<usize>,
}

impl LPPlayerCosmetics {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::<LPPlayerCosmetics>::default()
    }

    fn chooser(cx: &Cx<'_>) -> WidgetId {
        find(cx, "UI_CollectionChooser")
    }

    fn button(cx: &Cx<'_>) -> WidgetId {
        find(cx, "UI_ButtonMain")
    }

    fn equipper(cx: &Cx<'_>, i: usize) -> WidgetId {
        find(cx, EQUIPPERS[i].0)
    }

    /// (the port's own) the equippers' pictures, per slot
    fn pictures_owner(&self) -> String {
        format!("lp:cosmetics:{}", self.player_index)
    }

    /// Update Context @3379: the interactors, the button's input player, the chooser, then @1491 every equipper's
    /// interactors. (UpdateColorsFromPrefs, the colour-blind palette of the UI preferences: the port draws the default
    /// one.)
    pub fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str, player: i32) {
        self.player_index = player;
        let b = inter(cx, Self::button(cx));
        set_interactors(cx, &[b], ctx, player);
        set_input_player_index(cx, Self::button(cx), player);
        let c = ctx.to_string();
        let ch = Self::chooser(cx);
        cx.visit::<CollectionChooser, _>(ch, |ch, cx| ch.update_context(cx, &c, usize::try_from(player).unwrap_or(0)));
        for i in 0..EQUIPPERS.len() {
            let w = Self::equipper(cx, i);
            let list = tile_interactors(cx, w);
            set_interactors(cx, &list, ctx, player);
        }
    }

    /// Set Character @4743: Character, Update Selections
    pub fn set_character(&mut self, cx: &mut Cx<'_>, c: Option<&RosterEntry>) {
        self.character = c.cloned();
        self.update_selections(cx);
    }

    /// Update Selections @3779. Forced offline (@3930): when the character has an entry in the Hydra character
    /// preferences, equipper n takes Update From Preferences("taunt{n}", Character, entry.TauntDatas[n - 1].Slug)
    /// (@2256); without one each of the four takes Update From Preferences("taunt1", Character, "") (@4522-4687, as the
    /// build has it). The LocalPlay preferences' taunt slugs are not read here offline: the tiles show the Hydra taunts
    /// while the chooser's picks go to the LocalPlay ones. Then the ring-out and banner from the LocalPlay preferences
    /// (@4262, @4434: Update From Preferences with the slot's slug as the manual one, the Hydra one when empty).
    pub fn update_selections(&mut self, cx: &mut Cx<'_>) {
        let c = self.character.clone();
        let entry = c.as_ref().and_then(|c| cx.prefs.hydra.characters.get(&c.slug)).cloned();
        for i in 0..4 {
            let w = Self::equipper(cx, 2 + i);
            let c2 = c.clone();
            let (tag, slug) = match &entry {
                Some(e) => (format!("taunt{}", i + 1), e.taunts.get(i).map(|t| t.slug.clone()).unwrap_or_default()),
                None => ("taunt1".to_string(), String::new()),
            };
            cx.visit::<EquipperItem, _>(w, |e, cx| e.update_from_preferences(cx, &tag, c2.as_ref(), &slug));
        }
        let p = usize::try_from(self.player_index).unwrap_or(0).min(3);
        let lp = cx.prefs.local_play.players[p].clone();
        for (i, slug) in [(0, lp.ringout_slug), (1, lp.banner_slug)] {
            let w = Self::equipper(cx, i);
            let c2 = c.clone();
            let tag = EQUIPPERS[i].1;
            cx.visit::<EquipperItem, _>(w, |e, cx| e.update_from_preferences(cx, tag, c2.as_ref(), &slug));
        }
        let mut names = Vec::new();
        for i in 0..EQUIPPERS.len() {
            let w = Self::equipper(cx, i);
            if let Some(Some(EquipperTile::Cosmetic(t))) = cx.visit::<EquipperItem, _>(w, |e, _| e.data())
                && let Some(n) = t.item.icon.as_deref().and_then(hydra::picture_name)
            {
                names.push(n);
            }
        }
        let owner = self.pictures_owner();
        cx.view.hold_pictures(&owner, names.iter().map(String::as_str));
    }

    /// Ready @3693: seen, CosmeticsDoneEvent
    pub fn ready(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        if !cx.view.visible_in_hierarchy(me) {
            return;
        }
        let p = self.player_index;
        cx.emit(self, |c| &c.cosmetics_done_event, &p);
    }

    /// Focus @6026: the last selected equipper's interactor focused, then Selected(it, its data's tag) (the chooser
    /// initialised for that equipper behind, not shown)
    pub fn focus(&mut self, cx: &mut Cx<'_>) {
        let Some(i) = self.last_selected else { return };
        let w = Self::equipper(cx, i);
        if let Some(&first) = tile_interactors(cx, w).first() {
            set_focused(cx, first, false);
        }
        let d = cx.visit::<EquipperItem, _>(w, |e, _| e.data()).flatten();
        let tag = tile_tag(&d);
        self.selected(cx, i, d, &tag);
    }

    /// Selected(Grid Item, Type) @5152: Last Selected Item, SetAllOff, the tile's Set On(true) (UI_TabInterface), the
    /// chooser's Init(data, Character, 0, Type)
    fn selected(&mut self, cx: &mut Cx<'_>, i: usize, data: Option<EquipperTile>, kind: &str) {
        self.last_selected = Some(i);
        // SetAllOff @1440: every equipper's Set On(false)
        for k in 0..EQUIPPERS.len() {
            let w = Self::equipper(cx, k);
            run_on::<EquipperItem>(cx, w, |e, cx| e.set_on(cx, false));
        }
        let w = Self::equipper(cx, i);
        run_on::<EquipperItem>(cx, w, |e, cx| e.set_on(cx, true));
        let ch = self.character.clone();
        let kind = kind.to_string();
        let c = Self::chooser(cx);
        cx.visit::<CollectionChooser, _>(c, |c, cx| c.init(cx, data, ch.as_ref(), 0.0, &kind));
    }

    /// GetEquippedSlugs: each equipper's data's slug, "" without data (UI_EquipperItem_V2 GetEquippedSlug @0-218)
    pub fn get_equipped_slugs(&self, cx: &mut Cx<'_>) -> EquippedSlugs {
        let slug = |cx: &mut Cx<'_>, i: usize| {
            let w = Self::equipper(cx, i);
            tile_slug(&cx.visit::<EquipperItem, _>(w, |e, _| e.data()).flatten())
        };
        EquippedSlugs { ringout: slug(cx, 0), banner: slug(cx, 1), taunts: (2..6).map(|i| slug(cx, i)).collect() }
    }

    /// OnCancel(Player Index) @6454: the slot's player, seen; an open chooser is hidden, else CosmeticsStepBackEvent
    fn on_cancel(&mut self, cx: &mut Cx<'_>, player: i32) {
        let me = cx.w;
        if player != self.player_index || !cx.view.visible_in_hierarchy(me) {
            return;
        }
        let c = Self::chooser(cx);
        let shown = cx.visit::<CollectionChooser, _>(c, |c, _| c.shown()).unwrap_or(false);
        if shown {
            cx.visit::<CollectionChooser, _>(c, |c, cx| c.hide(cx));
        } else {
            cx.emit(self, |c| &c.cosmetics_step_back_event, &());
        }
    }
}

impl Controller for LPPlayerCosmetics {
    /// OnInitialized @6339: OnCancel on the manager's OnCancelWithinModal, then @15 SetDefaultTaunts(false) (the Hydra
    /// character preferences' default taunts, run when the cosmetics catalogue that names them is in, then the tiles
    /// again). The component bindings: the NEXT button (@6672 -> Ready), each equipper's SelectedEvent (@5480-5934),
    /// the chooser's SelectedEvent (@6011 -> Update Selections) and HideEvent (@6677).
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let h = handler(cx, CO_CANCEL);
        cx.view.manager.on_cancel_within_modal.add(h);
        let h = handler(cx, CO_BUTTON);
        let b = Self::button(cx);
        cx.visit::<ButtonMain, _>(b, |b, _| b.on_click.add(h));
        for i in 0..EQUIPPERS.len() {
            let h = handler(cx, CO_EQUIPPER + i as u32);
            let w = Self::equipper(cx, i);
            cx.visit::<EquipperItem, _>(w, |e, _| e.selected_event.add(h));
        }
        let (h1, h2) = (handler(cx, CO_CHOSEN), handler(cx, CO_HIDDEN));
        let c = Self::chooser(cx);
        cx.visit::<CollectionChooser, _>(c, |c, _| {
            c.selected_event.add(h1);
            c.hide_event.add(h2);
        });
        // SetDefaultTaunts(false): the catalogue is in the bundle
        if let Some(cat) = hydra::cosmetics(cx.view).cloned() {
            hydra::set_default_taunts(&mut cx.prefs.hydra, &cat, false);
        }
    }

    fn destruct(&mut self, cx: &mut Cx<'_>) {
        let h = handler(cx, CO_CANCEL);
        cx.view.manager.on_cancel_within_modal.remove(h);
        let owner = self.pictures_owner();
        cx.view.release_pictures(&owner);
    }

    /// PreConstruct @4777: no tooltips on the equippers (the port's EquipperItem has none), Last Selected Item =
    /// equipperTaunt1 (@4777-5146; the graph ends there, SetAllOff at @5147 is not reached)
    fn pre_construct(&mut self, _cx: &mut Cx<'_>) {
        self.last_selected = Some(2);
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        match id {
            CO_BUTTON => self.ready(cx),
            CO_CANCEL => {
                if let Some(&p) = arg.downcast_ref::<i32>() {
                    self.on_cancel(cx, p);
                }
            }
            CO_CHOSEN => self.update_selections(cx),
            // the chooser's HideEvent @6677: Focus when the last selected equipper is valid and the step seen
            CO_HIDDEN => {
                let me = cx.w;
                if self.last_selected.is_some() && cx.view.visible_in_hierarchy(me) {
                    self.focus(cx);
                }
            }
            n if (CO_EQUIPPER..CO_EQUIPPER + 6).contains(&n) => {
                let i = (n - CO_EQUIPPER) as usize;
                // the tile's data at the click is the event's argument (the tile is busy in its own click)
                let data = arg.downcast_ref::<Option<EquipperTile>>().cloned().flatten();
                self.selected(cx, i, data, EQUIPPERS[i].1);
                let c = Self::chooser(cx);
                cx.visit::<CollectionChooser, _>(c, |c, cx| {
                    c.offset_close_tab(cx, EQUIPPERS[i].2);
                    c.show(cx);
                    c.focus_first(cx);
                });
            }
            _ => {}
        }
    }
}

// ---------- UI_LP_Player_Perks (state 7) ----------

const PK_SAVED: u32 = 1;
const PK_EQUIPPED: u32 = 2;
const PK_CLEARED: u32 = 3;

/// The step's UI_Perks_Equipper (Minimal Mode, Don't Save Changes, Manually Set Equipped, Unlock All, Cancel Will
/// Confirm false).
/// Deviation: the port builds it with the slot, not at the step's first Set Character.
#[derive(Debug, Default)]
pub struct LPPlayerPerks {
    pub player_index: i32,
    /// PerksReadyEvent(Player Index), PerksChangedEvent(Player Index)
    pub perks_ready_event: Event<i32>,
    pub perks_changed_event: Event<i32>,
    pub character: Option<RosterEntry>,
}

impl LPPlayerPerks {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::<LPPlayerPerks>::default()
    }

    fn equipper(cx: &Cx<'_>) -> WidgetId {
        find(cx, "UI_Perks_Equipper")
    }

    /// Update Context @76: the interactors (none), the equipper's Player Index and UpdateContext
    pub fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str, player: i32) {
        self.player_index = player;
        let eq = Self::equipper(cx);
        let ctx = ctx.to_string();
        run_on::<PerksEquipper>(cx, eq, move |e, cx| {
            e.player_index = player;
            e.update_context(cx, &ctx);
        });
    }

    /// Set Character @390: a new character only; the equipper's SetCharacter(Character, []) and SetEquippedPerks of
    /// the LocalPlay preferences' perk slugs
    pub fn set_character(&mut self, cx: &mut Cx<'_>, c: Option<&RosterEntry>) {
        if self.character.as_ref().map(|c| &c.slug) == c.map(|c| &c.slug) {
            return;
        }
        self.character = c.cloned();
        let fighter = c.and_then(|c| c.fighter.clone());
        let p = usize::try_from(self.player_index).unwrap_or(0).min(3);
        let slugs = cx.prefs.local_play.players[p].perk_slugs.to_vec();
        let perks = perk_data_from_slugs(cx.view, &slugs);
        let eq = Self::equipper(cx);
        cx.visit::<PerksEquipper, _>(eq, |e, cx| {
            e.set_character(cx, fighter.as_deref());
            e.set_equipped_perks(cx, &perks);
        });
    }

    /// Focus @740: Enter(-1), Focus
    pub fn focus(&mut self, cx: &mut Cx<'_>) {
        let eq = Self::equipper(cx);
        cx.visit::<PerksEquipper, _>(eq, |e, cx| {
            e.enter(cx, -1);
            e.focus(cx);
        });
    }

    /// Get Perks: the loadout's GetEquippedPerks(false, ...): the four slots, empties kept
    pub fn get_perks(&self, cx: &mut Cx<'_>) -> Vec<Option<Perk>> {
        let lo = cx.view.tree().find_in(Self::equipper(cx), "UI_Perks_Loadout");
        cx.visit::<PerksLoadout, _>(lo, |l, _| l.equipped_perks(false)).unwrap_or_else(|| vec![None; 4])
    }
}

impl Controller for LPPlayerPerks {
    /// The component bindings: PerksSavedEvent @385 -> @43 -> @10 (PerksReadyEvent, then PerksChangedEvent),
    /// PerkEquippedEvent @822 and PerksClearedEvent @855
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let (a, b, c) = (handler(cx, PK_SAVED), handler(cx, PK_EQUIPPED), handler(cx, PK_CLEARED));
        let eq = Self::equipper(cx);
        cx.visit::<PerksEquipper, _>(eq, |e, _| {
            e.perks_saved_event.add(a);
            e.perk_equipped_event.add(b);
            e.cleared_event.add(c);
        });
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, _arg: &dyn Any) {
        let p = self.player_index;
        match id {
            PK_SAVED => {
                cx.emit(self, |c| &c.perks_ready_event, &p);
                cx.emit(self, |c| &c.perks_changed_event, &p);
            }
            PK_EQUIPPED | PK_CLEARED => cx.emit(self, |c| &c.perks_changed_event, &p),
            _ => {}
        }
    }
}

// ---------- UI_GlyphEntries / UI_GlyphEntry (the name entry of the confirm step) ----------

/// UI_GlyphEntry's GlyphSet (its class default); Index starts 0 (labelGlyph's tree text "A" is GlyphSet[0])
const GLYPH_SET: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789 ";

fn glyph_at(i: usize) -> char {
    GLYPH_SET.chars().nth(i).unwrap_or('A')
}

/// Set Glyph @0: Array_Find in the set's characters (FString ==, case-insensitive); a glyph not in the set leaves the
/// index
fn find_glyph(g: char) -> Option<usize> {
    GLYPH_SET.chars().position(|c| c.to_lowercase().eq(g.to_lowercase()))
}

const ORANGE: [f64; 4] = [0.982251, 0.520996, 0.000304, 1.0];
const DIM: [f64; 4] = [1.0, 1.0, 1.0, 0.5];
const WHITE: [f64; 4] = [1.0; 4];

const GE_UP_FOCUS: u32 = 1;
const GE_UP_UNFOCUS: u32 = 2;
const GE_DOWN_FOCUS: u32 = 3;
const GE_DOWN_UNFOCUS: u32 = 4;
const GE_UP_CLICK: u32 = 5;
const GE_DOWN_CLICK: u32 = 6;

/// UI_GlyphEntry: one letter with an arrow above and below it.
#[derive(Debug, Default)]
pub struct GlyphEntry {
    /// GlyphChangedEvent(this entry, the glyph)
    pub glyph_changed_event: Event<(WidgetId, String)>,
    index: usize,
}

impl GlyphEntry {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::<GlyphEntry>::default()
    }

    /// FocusArrow(Focused, Icon) @0: the arrow doubles and turns orange, the outline and the letter follow
    fn focus_arrow(cx: &mut Cx<'_>, focused: bool, icon: &str) {
        let i = find(cx, icon);
        let x = wd(cx, i);
        let s = if focused { 2.0 } else { 1.0 };
        x.transform.scale = [s, s];
        x.color = if focused { ORANGE } else { DIM };
        let o = find(cx, "outline");
        wd(cx, o).color = if focused { ORANGE } else { DIM };
        let l = find(cx, "labelGlyph");
        wd(cx, l).color = if focused { ORANGE } else { WHITE };
    }

    pub fn glyph(&self) -> char {
        glyph_at(self.index)
    }

    /// Set Glyph(Glyph) @0
    pub fn set_glyph(&mut self, cx: &mut Cx<'_>, g: char) {
        if let Some(at) = find_glyph(g) {
            self.index = at;
            self.update_appearance(cx);
        }
    }

    fn update_appearance(&self, cx: &mut Cx<'_>) {
        let l = find(cx, "labelGlyph");
        wd(cx, l).text = self.glyph().to_string();
    }

    /// Increment(Amount) @1268: WrapInt over the set, Update Appearance, ChangeAnim, GlyphChangedEvent
    fn increment(&mut self, cx: &mut Cx<'_>, amount: i32) {
        let n = GLYPH_SET.chars().count() as i32;
        self.index = (self.index as i32 + amount).rem_euclid(n) as usize;
        self.update_appearance(cx);
        let me = cx.w;
        cx.view.play(me, "ChangeAnim", false, 1.0);
        let ev = (me, self.glyph().to_string());
        cx.emit(self, |c| &c.glyph_changed_event, &ev);
    }

    /// Update Context @: the two arrows' interactors
    pub fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str, player: i32) {
        let list = [find(cx, "UI_Interactor_Up"), find(cx, "UI_Interactor_Down")];
        set_interactors(cx, &list, ctx, player);
    }
}

impl Controller for GlyphEntry {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        use super::controls::bind;
        let (up, down) = (find(cx, "UI_Interactor_Up"), find(cx, "UI_Interactor_Down"));
        bind(cx, up, |s| &mut s.on_focus, GE_UP_FOCUS);
        bind(cx, up, |s| &mut s.on_unfocus, GE_UP_UNFOCUS);
        bind(cx, up, |s| &mut s.on_click, GE_UP_CLICK);
        bind(cx, down, |s| &mut s.on_focus, GE_DOWN_FOCUS);
        bind(cx, down, |s| &mut s.on_unfocus, GE_DOWN_UNFOCUS);
        bind(cx, down, |s| &mut s.on_click, GE_DOWN_CLICK);
    }

    /// PreConstruct (not in the designer): both arrows unfocused
    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        Self::focus_arrow(cx, false, "iconUp");
        Self::focus_arrow(cx, false, "iconDown");
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, _arg: &dyn Any) {
        match id {
            GE_UP_FOCUS => Self::focus_arrow(cx, true, "iconUp"),
            GE_UP_UNFOCUS => Self::focus_arrow(cx, false, "iconUp"),
            GE_DOWN_FOCUS => Self::focus_arrow(cx, true, "iconDown"),
            GE_DOWN_UNFOCUS => Self::focus_arrow(cx, false, "iconDown"),
            GE_UP_CLICK => self.increment(cx, 1),
            GE_DOWN_CLICK => self.increment(cx, -1),
            _ => {}
        }
    }
}

const GS_CHANGED: u32 = 1;

/// The entry row: PreConstruct @563 makes Length entries in entryBox (a DynamicEntryBox of UI_GlyphEntry_C) and binds
/// each one's GlyphChangedEvent to OnGlyphChanged (@970: GlyphsChangedEvent(Get Glyphs)).
#[derive(Debug, Default)]
pub struct GlyphEntries {
    /// GlyphsChangedEvent(Glyphs)
    pub glyphs_changed_event: Event<String>,
    entries: Vec<WidgetId>,
    /// the entries' glyphs, as their events and Set Glyphs left them (a busy entry is not read)
    glyphs: Vec<char>,
}

impl GlyphEntries {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::<GlyphEntries>::default()
    }

    /// Update Context(Context, Player Index) @1022 -> @15: each entry's interactors
    pub fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str, player: i32) {
        for &e in &self.entries.clone() {
            let ctx = ctx.to_string();
            run_on::<GlyphEntry>(cx, e, move |g, cx| g.update_context(cx, &ctx, player));
        }
    }

    /// Get Glyphs: the entries' glyphs in order
    pub fn get_glyphs(&self) -> String {
        self.glyphs.iter().collect()
    }

    /// Set Glyphs(Glyphs): entry i takes the i-th character, a space past the string's end (SelectString @599); then
    /// GlyphsChangedEvent(Get Glyphs) @694
    pub fn set_glyphs(&mut self, cx: &mut Cx<'_>, s: &str) {
        let chars: Vec<char> = s.chars().collect();
        for (i, &e) in self.entries.clone().iter().enumerate() {
            let g = chars.get(i).copied().unwrap_or(' ');
            if find_glyph(g).is_some() {
                self.glyphs[i] = glyph_at(find_glyph(g).unwrap_or(0));
            }
            run_on::<GlyphEntry>(cx, e, move |en, cx| en.set_glyph(cx, g));
        }
        let ev = self.get_glyphs();
        cx.emit(self, |c| &c.glyphs_changed_event, &ev);
    }
}

impl Controller for GlyphEntries {
    /// PreConstruct @563: entryBox.Reset, Length entries (the class default 3; this instance's 5). Each entry's slot
    /// takes half the box's EntrySpacing on each side.
    /// Unverified: SDynamicEntryBox's spacing as slot padding.
    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        let bx = find(cx, "entryBox");
        for e in std::mem::take(&mut self.entries) {
            MenuView::discard(cx, e);
        }
        self.glyphs.clear();
        let n = prop_f64(cx, "Length", 3.0) as usize;
        let spacing = cx
            .view
            .tree()
            .props(bx)
            .get("EntrySpacing")
            .and_then(|v| Some([v.get("X")?.as_f64()?, v.get("Y")?.as_f64()?]))
            .unwrap_or([0.0, 0.0]);
        for _ in 0..n {
            let e = MenuView::create(cx, "UI_GlyphEntry");
            MenuView::add_child(cx, bx, e, None);
            let h = handler(cx, GS_CHANGED);
            wd(cx, e).pad =
                Margin { l: spacing[0] / 2.0, t: spacing[1] / 2.0, r: spacing[0] / 2.0, b: spacing[1] / 2.0 };
            cx.visit::<GlyphEntry, _>(e, |g, _| g.glyph_changed_event.add(h));
            self.entries.push(e);
            self.glyphs.push(glyph_at(0));
        }
    }

    /// an entry's GlyphChangedEvent -> OnGlyphChanged @970
    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        if id != GS_CHANGED {
            return;
        }
        if let Some((w, g)) = arg.downcast_ref::<(WidgetId, String)>()
            && let Some(at) = self.entries.iter().position(|e| e == w)
            && let Some(c) = g.chars().next()
        {
            self.glyphs[at] = c;
        }
        let ev = self.get_glyphs();
        cx.emit(self, |c| &c.glyphs_changed_event, &ev);
    }
}

// ---------- UI_LP_Player_ConfirmReady (state 8) ----------

const CR_GLYPHS: u32 = 1;
const CR_READY: u32 = 2;
const CR_CONTROLS: u32 = 3;

/// The port is forced offline (BPI_IsForcedOfflineMode true). What the steps choose reaches the LocalPlay preferences
/// the build's way: the slot's Get Selections reads the steps (GetEquippedSlugs, Get Perks, the glyphs) and
/// UI_LocalPlay's Save All Settings writes them; the confirm step writes the name itself
/// (Set-LocalPlay-UsernameForPlayer @1393).
#[derive(Debug, Default)]
pub struct LPPlayerConfirmReady {
    pub player_index: i32,
    /// ReadyEvent(Player Index)
    pub ready_event: Event<i32>,
    throttle_timer: f64,
}

impl LPPlayerConfirmReady {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::<LPPlayerConfirmReady>::default()
    }

    fn button_ready(cx: &Cx<'_>) -> WidgetId {
        find(cx, "buttonReady")
    }

    fn button_controls(cx: &Cx<'_>) -> WidgetId {
        find(cx, "buttonControls")
    }

    fn glyphs(cx: &Cx<'_>) -> WidgetId {
        find(cx, "UI_GlyphEntries")
    }

    /// Update Context @79: the interactors, the loadout, both buttons' input player, the name row
    pub fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str, player: i32) {
        self.player_index = player;
        let list = [inter(cx, Self::button_controls(cx)), inter(cx, Self::button_ready(cx))];
        set_interactors(cx, &list, ctx, player);
        let lo = find(cx, "UI_Perks_Loadout");
        let c = ctx.to_string();
        run_on::<PerksLoadout>(cx, lo, move |l, cx| l.update_context(cx, &c, player));
        set_input_player_index(cx, Self::button_ready(cx), player);
        set_input_player_index(cx, Self::button_controls(cx), player);
        let g = Self::glyphs(cx);
        let c = ctx.to_string();
        run_on::<GlyphEntries>(cx, g, move |g, cx| g.update_context(cx, &c, player));
    }

    /// Set Data(Character, Skin, Perks) @492: the loadout's perks, then Update Appearance @594 (the card, not ready)
    pub fn set_data(
        &mut self,
        cx: &mut Cx<'_>,
        c: Option<&RosterEntry>,
        skin: Option<&SkinInfo>,
        perks: &[Option<Perk>],
    ) {
        let lo = find(cx, "UI_Perks_Loadout");
        let p = perks.to_vec();
        run_on::<PerksLoadout>(cx, lo, move |l, cx| l.set_perks(cx, &p));
        let info = find(cx, "UI_LP_CharacterInfo");
        cx.visit::<LPCharacterInfo, _>(info, |i, cx| i.set_data(cx, c, skin, false));
    }

    /// Reset Name @1115: GetLocalPlayUsername's Custom Name Set picks the saved name, else PlayerNumberShort; Set
    /// Glyphs (whose GlyphsChangedEvent writes it back, padded to the row's length)
    pub fn reset_name(&mut self, cx: &mut Cx<'_>) {
        let (name, short, custom) = own_username(cx, self.player_index);
        let n = if custom { name } else { short };
        let g = Self::glyphs(cx);
        cx.visit::<GlyphEntries, _>(g, |g, cx| g.set_glyphs(cx, &n));
    }

    /// Focus @995: Ready's interactor focused, Throttle Timer = now
    pub fn focus(&mut self, cx: &mut Cx<'_>) {
        let i = inter(cx, Self::button_ready(cx));
        set_focused(cx, i, false);
        self.throttle_timer = cx.now();
    }

    /// Ready @916: TimeIntervalCheck(Throttle Timer, 0.25) (real time; a clock behind the timer passes too), then
    /// ReadyEvent
    pub fn ready(&mut self, cx: &mut Cx<'_>) {
        let now = cx.now();
        let d = now - self.throttle_timer;
        if !(d >= 0.25 || d < 0.0) {
            return;
        }
        self.throttle_timer = now;
        let p = self.player_index;
        cx.emit(self, |c| &c.ready_event, &p);
    }

    /// @654-848: ShowModal-Generic(false, UI_InputBindingModal, inputSettings, Conv_IntToString(Player Index)); the
    /// modal returned is dropped
    fn show_controls(&mut self, cx: &mut Cx<'_>) {
        let p = self.player_index.to_string();
        show_modal_generic(cx, false, "UI_InputBindingModal", "inputSettings", &p);
    }
}

impl Controller for LPPlayerConfirmReady {
    /// The name row's GlyphsChangedEvent -> OnGlyphsChangedEvent; the buttons: Ready @864, Controls @654.
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let (a, b, c) = (handler(cx, CR_GLYPHS), handler(cx, CR_READY), handler(cx, CR_CONTROLS));
        let g = Self::glyphs(cx);
        cx.visit::<GlyphEntries, _>(g, |g, _| g.glyphs_changed_event.add(a));
        let br = Self::button_ready(cx);
        cx.visit::<ButtonMain, _>(br, |b2, _| b2.on_click.add(b));
        let bc = Self::button_controls(cx);
        cx.visit::<ButtonSecondary, _>(bc, |b2, _| b2.on_click.add(c));
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        match id {
            // OnGlyphsChangedEvent @1347: Set-LocalPlay-UsernameForPlayer(Player Index, Glyphs) (PreferencesManager
            // @0-168: the username of the player's settings)
            CR_GLYPHS => {
                if let Some(g) = arg.downcast_ref::<String>() {
                    let p = usize::try_from(self.player_index).unwrap_or(0).min(3);
                    cx.prefs.local_play.players[p].username = g.clone();
                }
            }
            CR_READY => self.ready(cx),
            CR_CONTROLS => self.show_controls(cx),
            _ => {}
        }
    }
}

// ---------- UI_LP_Player_Ready (state 9) ----------

#[derive(Debug, Default)]
pub struct LPPlayerReady {
    pub player_index: i32,
    pub is_waiting: bool,
}

impl LPPlayerReady {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::<LPPlayerReady>::default()
    }

    /// Update Context @10: the interactors (none), the loadout, the cancel label's player
    pub fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str, player: i32) {
        self.player_index = player;
        let lo = find(cx, "UI_Perks_Loadout");
        let c = ctx.to_string();
        run_on::<PerksLoadout>(cx, lo, move |l, cx| l.update_context(cx, &c, player));
        let label = find(cx, "UI_InputLabel_Cancel");
        set_label_player(cx, label, player);
    }

    /// Set Data(Character, Skin, Perks) @324: the loadout's perks, then Update Appearance @426: the card (ready) and
    /// labelUsername = GetLocalPlayUsername's Username
    pub fn set_data(
        &mut self,
        cx: &mut Cx<'_>,
        c: Option<&RosterEntry>,
        skin: Option<&SkinInfo>,
        perks: &[Option<Perk>],
    ) {
        let lo = find(cx, "UI_Perks_Loadout");
        let p = perks.to_vec();
        run_on::<PerksLoadout>(cx, lo, move |l, cx| l.set_perks(cx, &p));
        let info = find(cx, "UI_LP_CharacterInfo");
        cx.visit::<LPCharacterInfo, _>(info, |i, cx| i.set_data(cx, c, skin, true));
        let (name, _, _) = own_username(cx, self.player_index);
        let l = find(cx, "labelUsername");
        wd(cx, l).text = name;
    }

    /// Set Waiting(Is Waiting) @613: waitingContainer (Conditional Visibility 4 / 1)
    pub fn set_waiting(&mut self, cx: &mut Cx<'_>, w: bool) {
        self.is_waiting = w;
        let c = find(cx, "waitingContainer");
        wd(cx, c).vis = if w { Vis::SelfHitTestInvisible } else { Vis::Collapsed };
    }
}

impl Controller for LPPlayerReady {}
