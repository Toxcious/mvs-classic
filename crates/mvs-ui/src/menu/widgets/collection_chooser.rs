//! UI_CollectionChooser: the Collection's cosmetic chooser, its lists
//! filled from the cosmetics catalogue and its picks written to the Hydra or the Local Play preferences.

use std::any::Any;

use serde_json::{Value, json};

use crate::bundle::{CosmeticItem, Cosmetics, FighterInfo, RosterEntry};
use crate::menu::controller::{Controller, Cx, Event, Handler};
use crate::menu::hydra::{self, GlobalKey};
use crate::menu::screens::lab_selectors::{
    CosmeticTile, CosmeticType, EquipperItem, EquipperTile, GridItemList, SkinTile, TileData, TileEvent, run_on,
    selected_skin, set_text, set_vis,
};
use crate::menu::scroll::{scroll_list, set_scroll_offset, set_scrollbar_padding};
use crate::menu::widgets::controls::interactor::{
    batch_set_context, batch_set_scroll_box, set_focused, set_interactable,
};
use crate::menu::widgets::controls::{bind, find, prop_bool, prop_f64, wd};
use crate::slate::{Margin, VAlign, Vis, WidgetId};

const C_COLLAPSE: u32 = 1;
const C_COLLAPSE_1: u32 = 2;
const C_SCROLL: u32 = 3;
const C_SELECTED: u32 = 4;
const C_FOCUS_WAIT: u32 = 5;
const C_FOCUS_LAST: u32 = 6;

/// Init's Update Headers (@17883): the primary (and secondary) header by type.
fn headers(t: &str, character: &str) -> Option<(String, Option<&'static str>)> {
    Some(match t {
        "skin" => (format!("{character} Variants"), None),
        "taunt1" | "taunt2" | "taunt3" | "taunt4" => (format!("{character} Taunts"), Some("Stickers")),
        "ringout" => ("Ringouts".into(), None),
        "announcer" => ("Announcers".into(), None),
        "profileicon" => ("Profile Icons".into(), None),
        "stattrackers" | "stattrackers2" | "stattrackers3" => ("General Badges".into(), Some("Character Badges")),
        "banner" => ("Banner".into(), None),
        _ => return None,
    })
}

const STAT_TRACKER_DEFAULT: &str = "stat_tracking_bundle_default";

/// UI_Functions IsStatTrackerAvailable? (@0-1112): a badge of no character, or of a fighter of
/// GetFightersInCharacterSelect (the catalogue's selectFighters).
fn stat_tracker_available(c: &Cosmetics, e: &CosmeticItem) -> bool {
    e.fighter.as_deref().is_none_or(|f| f.is_empty() || c.select_fighters.iter().any(|s| s == f))
}

/// The slot of a `taunt1`..`taunt4` type.
fn taunt_slot(t: &str) -> Option<usize> {
    let n = t.strip_prefix("taunt")?;
    matches!(n, "1" | "2" | "3" | "4").then(|| usize::from(n.as_bytes()[0] - b'1'))
}

/// UI_CollectionChooser SortItems(Data) @0-1689: a selection sort; item j takes the lowest place when it is
/// unlocked and the lowest so far locked, or when its SortString compares lower (KismetStringLibrary
/// CompareStrings, native CompareStringW).
/// Unverified: read as an ordinal compare; only the badges set a SortString.
fn sort_items(list: Vec<CosmeticTile>) -> Vec<CosmeticTile> {
    let mut a = list;
    let locked = |d: &CosmeticTile| !hydra::is_cosmetic_owned(&d.item.slug);
    let key = |d: &CosmeticTile| d.item.sort.clone().unwrap_or_default();
    for i in 0..a.len() {
        let mut low = i;
        for j in i + 1..a.len() {
            if (!locked(&a[j]) && locked(&a[low])) || key(&a[j]) < key(&a[low]) {
                low = j;
            }
        }
        a.swap(i, low);
    }
    a
}

/// A tile widget's interactors.
fn tile_interactors(cx: &mut Cx<'_>, w: WidgetId) -> Vec<WidgetId> {
    cx.visit_dyn(w, |c, cx| c.as_grid_item().map(|t| t.interactors(cx)).unwrap_or_default()).unwrap_or_default()
}

/// A list's tiles and each one's data as the list holds it.
fn list_items(cx: &mut Cx<'_>, list: WidgetId) -> Vec<(WidgetId, Option<TileData>)> {
    cx.visit::<GridItemList, _>(list, |g, _| {
        g.items().into_iter().map(|w| (w, g.record(w).and_then(|r| r.0))).collect()
    })
    .unwrap_or_default()
}

/// UI_CollectionChooser: the lists of the Collection's rewards (ring-outs, announcers, banners, profile icons,
/// taunts and stickers, badges) and of a character's skins. Every item is owned (Deviation; `is_cosmetic_owned`), so
/// no tile is locked, HideIfUnowned hides none, the owned count is the total and the shop (ShowShop) is never opened.
/// The skin list (Force Grid Columns 5, Filtered Skins for Character: the menu bundle's skins) writes the
/// character's skin (SetCharacterSkin), or the Local Play slot's (Set-LocalPlay-SkinForPlayer).
#[derive(Default)]
pub struct CollectionChooser {
    pub hide_event: Event<()>,
    pub show_event: Event<()>,
    /// SelectedEvent(Item Data).
    pub selected_event: Event<Option<EquipperTile>>,
    /// The player the chooser is for (Local Play's slot; 0 elsewhere).
    pub player_index: usize,
    /// Equipper Item Data: the overview tile's data Init was given (kept; no graph of the port's reads it).
    pub equipper_item_data: Option<EquipperTile>,
    character: Option<RosterEntry>,
    kind: String,
    context: String,
    shown: bool,
    /// The show animation playing (its heights are held at their last key).
    anim: &'static str,
    /// A profile icon was picked here and its listener (the Profile's UpdateProfileIcon, which runs inside
    /// the pick, before UpdateSelections) has not run yet: `take_icon_pick`.
    icon_picked: bool,
}

impl CollectionChooser {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::<CollectionChooser>::default()
    }

    fn local_play(cx: &Cx<'_>) -> bool {
        prop_bool(cx, "Local Play Settings")
    }

    fn lists(cx: &Cx<'_>) -> [WidgetId; 2] {
        [find(cx, "primaryList"), find(cx, "secondaryList")]
    }

    fn pictures_owner(&self, cx: &Cx<'_>) -> String {
        if Self::local_play(cx) {
            format!("collection:chooser:lp{}", self.player_index)
        } else {
            format!("collection:chooser:{}", cx.view.tree().widget(cx.w).name)
        }
    }

    /// The tiles' pictures let go (the modal or step that shows the chooser closes).
    pub fn release_pictures(&self, cx: &mut Cx<'_>) {
        let owner = self.pictures_owner(cx);
        cx.view.release_pictures(&owner);
    }

    /// Init(Equipper Item Data, Character, Close Tab X Offset, Type) @17295: the fields, Update Appearance(-100),
    /// OffsetCloseTab. Update Appearance (@17530): the secondary header and list for the taunts and badges, Update
    /// Headers, Populate, SetScrollOffset(-100).
    /// Deviation: the offset is set as 0: the port's scroll box does not clamp a desired offset below its start.
    pub fn init(
        &mut self,
        cx: &mut Cx<'_>,
        data: Option<EquipperTile>,
        character: Option<&RosterEntry>,
        close_tab_x: f64,
        kind: &str,
    ) {
        self.equipper_item_data = data;
        self.character = character.cloned();
        self.kind = kind.to_string();
        let t = kind.to_lowercase();
        let secondary = t.contains("stattrackers") || t.contains("taunt");
        let v = if secondary { Vis::SelfHitTestInvisible } else { Vis::Collapsed };
        set_vis(cx, "secondaryHeader", v);
        set_vis(cx, "secondaryList", v);
        if let Some((a, b)) = headers(&t, character.map_or("", |c| c.display_name.as_str())) {
            set_text(cx, "primaryHeaderLabel", &a);
            if let Some(b) = b {
                set_text(cx, "secondaryHeaderLabel", b);
            }
        }
        self.populate(cx);
        let sc = find(cx, "scrollcontainer");
        set_scroll_offset(cx.view.tree_mut(), sc, 0.0);
        self.offset_close_tab(cx, close_tab_x);
    }

    /// OffsetCloseTab(Offset) @38052: the Pointer's canvas slot x.
    pub fn offset_close_tab(&mut self, cx: &mut Cx<'_>, x: f64) {
        let p = find(cx, "Pointer");
        wd(cx, p).offsets.l = x;
    }

    /// Size for Type @0-525: the class default's Item Sizes (a TMap<FString, Vector2D>), looked up
    /// case-insensitively (FString keys): "taunt1" or "statTrackers2" is not a key and takes "default".
    fn size_for_type(cx: &Cx<'_>, kind: &str) -> [f64; 2] {
        if Self::local_play(cx) {
            return [130.0, 130.0];
        }
        let sizes = cx
            .view
            .bundle
            .widgets
            .get("UI_CollectionChooser")
            .and_then(|c| c.defaults.get("Item Sizes"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let find = |k: &str| {
            sizes.iter().find(|e| e.get("Key").and_then(Value::as_str).is_some_and(|s| s.to_lowercase() == k)).and_then(
                |e| {
                    let v = e.get("Value")?;
                    Some([v.get("X")?.as_f64()?, v.get("Y")?.as_f64()?])
                },
            )
        };
        find(&kind.to_lowercase()).or_else(|| find("default")).unwrap_or([100.0, 100.0])
    }

    /// The chooser's Character as the menu bundle's fighter (none for a character the port does not have).
    fn fighter(&self, cx: &Cx<'_>) -> Option<FighterInfo> {
        let k = self.character.as_ref()?.fighter.as_ref()?;
        cx.view.fighters().get(k).cloned()
    }

    /// Populate @19895. The badges' (Type containing statTrackers) wait for FetchUpdatedProfileData's batch (@20132,
    /// FetchComplete @40975 -> @20359).
    /// Unverified: the batch's answer offline is not read; the port runs the fill at once.
    /// Then, in the build's order: the counts zeroed, Size for Type into both lists' Item Size, TempData / TempData2
    /// cleared and the type's fill, each list's tiles DimsIcon false with the scroll box on their interactor, the
    /// owned / total labels, UpdateContext(Context, Player Index).
    fn populate(&mut self, cx: &mut Cx<'_>) {
        let t = self.kind.to_lowercase();
        let [px, py] = Self::size_for_type(cx, &self.kind);
        let [primary_w, secondary_w] = Self::lists(cx);
        for l in [primary_w, secondary_w] {
            cx.view.tree_mut().set_prop(l, "Item Size", json!({ "X": px, "Y": py }));
        }
        let g = cx.prefs.hydra.clone();
        let lp = cx.prefs.local_play.players[self.player_index.min(3)].clone();
        let local = Self::local_play(cx);
        let bundle = cx.view.bundle.clone();
        let cat = hydra::cosmetics_in(&bundle);
        let spacers = |cx: &mut Cx<'_>, one: Vis, three: Vis| {
            set_vis(cx, "Spacer_1", one);
            set_vis(cx, "Spacer_3", three);
        };
        // InitEquipperItemDataFrom*(asset, selected slug): Selected is slug = that slug
        let tile = |kind: CosmeticType, e: &CosmeticItem, selected: &str, tag: &str, character: &str| CosmeticTile {
            kind,
            item: e.clone(),
            selected: e.slug == selected,
            tag: tag.to_string(),
            character: character.to_string(),
        };
        let mut primary: Vec<CosmeticTile> = Vec::new();
        let mut second: Vec<CosmeticTile> = Vec::new();
        let mut skins: Option<Vec<SkinTile>> = None;
        let mut reorder = false;
        // (the catalogue not in yet: empty lists)
        if let Some(cat) = &cat {
            if t == "ringout" {
                let sel = if local { lp.ringout_slug.as_str() } else { g.ring_out_vfx.slug.as_str() };
                primary = cat.ringout.iter().map(|e| tile(CosmeticType::Ringout, e, sel, "ringout", "")).collect();
                spacers(cx, Vis::Collapsed, Vis::Collapsed);
            } else if t == "announcer" {
                primary = cat
                    .announcer
                    .iter()
                    .map(|e| tile(CosmeticType::Announcer, e, &g.announcer_pack.slug, "announcer", ""))
                    .collect();
                spacers(cx, Vis::Collapsed, Vis::Collapsed);
            } else if t == "banner" {
                let sel = if local { lp.banner_slug.as_str() } else { g.banner.slug.as_str() };
                primary = cat.banner.iter().map(|e| tile(CosmeticType::Banner, e, sel, "banner", "")).collect();
                spacers(cx, Vis::Collapsed, Vis::Collapsed);
            } else if t == "profileicon" {
                primary = cat
                    .profile_icon
                    .iter()
                    .map(|e| tile(CosmeticType::ProfileIcon, e, &g.profile_icon.slug, "profileIcon", ""))
                    .collect();
                spacers(cx, Vis::SelfHitTestInvisible, Vis::Collapsed);
            } else if let Some(slot) = taunt_slot(&t) {
                // GetTauntForSlot(n - 1, Character.Slug), GetCharacterTaunts: the character's AvailableTauntData
                // (the catalogue's, the fighter's tauntTable) to the primary list, every sticker to the secondary;
                // each list ReOrderWidgets after its Populate
                let c = self.character.as_ref();
                let sel = c.map_or("", |c| g.taunt_for_slot(slot, &c.slug)).to_string();
                let tag = format!("taunt{}", slot + 1);
                let cslug = c.map_or("", |c| c.slug.as_str());
                let own = c.and_then(|c| cat.characters.get(&c.slug)).map(|ch| ch.taunts.clone()).unwrap_or_default();
                primary = own
                    .iter()
                    .filter_map(|s| cat.taunt.iter().find(|e| &e.slug == s))
                    .map(|e| tile(CosmeticType::Taunt, e, &sel, &tag, cslug))
                    .collect();
                second = cat.emote.iter().map(|e| tile(CosmeticType::Emote, e, &sel, &tag, cslug)).collect();
                reorder = true;
                spacers(cx, Vis::SelfHitTestInvisible, Vis::SelfHitTestInvisible);
            } else if matches!(t.as_str(), "stattrackers" | "stattrackers2" | "stattrackers3") {
                // @23756-25089: the slot's badge, the two other slots' to hide; each available badge (the default
                // always, the others when neither other slot holds it): UI_BadgeInitItemData with the slot's slug,
                // or the default's when empty. A badge of no character goes to the primary list (the default at
                // its front), the rest to the secondary.
                let slot = match t.as_str() {
                    "stattrackers" => 0,
                    "stattrackers2" => 1,
                    _ => 2,
                };
                let st = &g.stat_trackers;
                let hide: Vec<&str> = (0..3).filter(|&i| i != slot).map(|i| st[i].slug.as_str()).collect();
                let sel = if st[slot].slug.is_empty() { STAT_TRACKER_DEFAULT } else { st[slot].slug.as_str() };
                for e in &cat.stat_tracker {
                    if !((e.slug == STAT_TRACKER_DEFAULT || !hide.contains(&e.slug.as_str()))
                        && stat_tracker_available(cat, e))
                    {
                        continue;
                    }
                    let d = tile(CosmeticType::StatTracker, e, sel, "statTracker", "");
                    if e.fighter.as_deref().is_some_and(|f| !f.is_empty()) {
                        second.push(d);
                    } else if e.slug == STAT_TRACKER_DEFAULT {
                        primary.insert(0, d);
                    } else {
                        primary.push(d);
                    }
                }
                spacers(cx, Vis::SelfHitTestInvisible, Vis::SelfHitTestInvisible);
            } else if t == "skin" {
                // @23196: Force Grid Columns 5 (kept for the chooser's later types too, as the build has it),
                // Filtered Skins for Character (the bundle's skins), @14467-15221: per skin
                // InitEquipperItemDataFromSkin with GetSelectedSkinForCharacter's slug (the Hydra preference, in
                // Local Play too), IsSkinOwned? (every skin)
                cx.view.tree_mut().set_prop(primary_w, "Force Grid Columns", json!(5));
                let f = self.fighter(cx);
                let sel = f.as_ref().and_then(|f| selected_skin(f, &g)).map(|s| s.slug.clone()).unwrap_or_default();
                skins = Some(
                    f.map(|f| f.skins)
                        .unwrap_or_default()
                        .into_iter()
                        .map(|s| SkinTile { selected: s.slug == sel, skin: s })
                        .collect(),
                );
                spacers(cx, Vis::Collapsed, Vis::Collapsed);
            }
        }
        // SortTempData @39077: SortItems of each list's data (a skin has no SortString and none is locked:
        // SortItems leaves the skins in their order)
        primary = sort_items(primary);
        second = sort_items(second);
        let first: Vec<TileData> = match &skins {
            Some(s) => s.iter().cloned().map(TileData::Skin).collect(),
            None => primary.iter().cloned().map(TileData::Cosmetic).collect(),
        };
        let other: Vec<TileData> = second.iter().cloned().map(TileData::Cosmetic).collect();
        cx.visit::<GridItemList, _>(primary_w, |l, cx| l.populate(cx, first));
        cx.visit::<GridItemList, _>(secondary_w, |l, cx| l.populate(cx, other));
        // OnItemAdded @41029 (each list's ItemAddedEvent, GridItemList AddItem @9172): a tile's Set Shows Tooltip(NOT
        // Prevent Tooltips)
        let shows = !prop_bool(cx, "Prevent Tooltips");
        for l in [primary_w, secondary_w] {
            for (w, _) in list_items(cx, l) {
                cx.visit::<EquipperItem, _>(w, |t, cx| t.set_shows_tooltip(cx, shows));
            }
        }
        if reorder {
            for l in [primary_w, secondary_w] {
                let items = list_items(cx, l);
                // ReOrderWidgets(UI Grid) @0-2414: each tile in turn inserted before the first of the new order whose
                // reward tier (RewardInfo Tier) is greater, then re-added at its index's row and column
                let tier = |d: &Option<TileData>| match d {
                    Some(TileData::Cosmetic(c)) => c.item.tier.unwrap_or(0),
                    _ => 0,
                };
                let mut out: Vec<(WidgetId, u32)> = Vec::new();
                for (w, d) in &items {
                    let ti = tier(d);
                    match out.iter().position(|o| o.1 > ti) {
                        Some(at) => out.insert(at, (*w, ti)),
                        None => out.push((*w, ti)),
                    }
                }
                let order: Vec<WidgetId> = out.into_iter().map(|o| o.0).collect();
                cx.visit::<GridItemList, _>(l, |g, cx| g.reorder(cx, &order));
            }
        }
        // @4640 / @21008: DimsIcon false, the interactor's ScrollBox
        let sc = find(cx, "scrollcontainer");
        for l in [primary_w, secondary_w] {
            for (w, _) in list_items(cx, l) {
                cx.view.tree_mut().set_prop(w, "DimsIcon", json!(false));
                cx.visit_dyn(w, |c, cx| {
                    if let Some(t) = c.as_grid_item() {
                        t.update_appearance(cx);
                    }
                });
                let ints = tile_interactors(cx, w);
                batch_set_scroll_box(cx, &ints, sc);
            }
        }
        // @20469: the owned counts (IsXOwned? per item: every item) and the lists' lengths
        let owned = |l: &[CosmeticTile]| l.iter().filter(|d| hydra::is_cosmetic_owned(&d.item.slug)).count();
        let n_primary = skins.as_ref().map_or_else(|| owned(&primary), Vec::len);
        set_text(cx, "primaryOwnedLabel", &n_primary.to_string());
        set_text(cx, "secondaryOwnedLabel", &owned(&second).to_string());
        let totals: Vec<usize> = [primary_w, secondary_w].iter().map(|&l| list_items(cx, l).len()).collect();
        set_text(cx, "primaryTotalLabel", &totals[0].to_string());
        set_text(cx, "secondaryTotalLabel", &totals[1].to_string());
        let (ctx, p) = (self.context.clone(), self.player_index);
        self.update_context(cx, &ctx, p);
        // the shown tiles' pictures; a new hold takes the place of the last
        let mut names: Vec<String> = skins.iter().flatten().map(|d| d.skin.icon.clone()).collect();
        names.extend(
            primary.iter().chain(second.iter()).filter_map(|d| d.item.icon.as_deref().and_then(hydra::picture_name)),
        );
        let owner = self.pictures_owner(cx);
        cx.view.hold_pictures(&owner, names.iter().map(String::as_str));
    }

    /// ToggleLoading(b) @38888: LoadingContainer Visible / Hidden, and both lists' SetInteractable(0, not b), read as
    /// each tile's interactor.
    pub fn toggle_loading(&mut self, cx: &mut Cx<'_>, loading: bool) {
        set_vis(cx, "LoadingContainer", if loading { Vis::Visible } else { Vis::Hidden });
        for l in Self::lists(cx) {
            for (w, _) in list_items(cx, l) {
                for i in tile_interactors(cx, w) {
                    set_interactable(cx, i, !loading);
                }
            }
        }
    }

    /// Selected(Grid Item, Is Locked) @26236.
    fn selected(&mut self, cx: &mut Cx<'_>, ev: &TileEvent) {
        let lc = find(cx, "LoadingContainer");
        if cx.view.tree().widget(lc).shown() {
            return;
        }
        let Some(d) = ev.data.clone() else { return };
        // a locked item opens the shop unless Prevent Opening Shop: none is locked (every item owned)
        if ev.locked {
            return;
        }
        let t = self.kind.to_lowercase();
        let local = Self::local_play(cx);
        let p = self.player_index.min(3);
        let picked = match d {
            TileData::Skin(s) => {
                // @26453 Type "skin" -> @32178: the SkinData; Set-LocalPlay-SkinForPlayer with Local Play Settings,
                // else SetCharacterSkin(Character.Slug, it); then SavePreferences, UpdateSelections, SelectedEvent
                if t != "skin" {
                    return;
                }
                if local {
                    let lp = &mut cx.prefs.local_play.players[p];
                    lp.skin_slug = s.skin.slug.clone();
                    lp.skin_reference = s.skin.path.clone();
                } else if let Some(c) = &self.character {
                    cx.prefs.hydra.set_character_skin(
                        &c.slug,
                        mvs_settings::AssetRef { slug: s.skin.slug.clone(), path: s.skin.path.clone() },
                    );
                }
                EquipperTile::Skin(s)
            }
            TileData::Cosmetic(c) => {
                let r = hydra::ref_of(&c.item);
                let h = &mut cx.prefs.hydra;
                match t.as_str() {
                    "stattrackers" => hydra::set_stat_tracker(h, 0, r),
                    "stattrackers2" => hydra::set_stat_tracker(h, 1, r),
                    "stattrackers3" => hydra::set_stat_tracker(h, 2, r),
                    // SetMyProfileIcon unless it is the icon already
                    "profileicon" => {
                        if h.profile_icon.slug != r.slug {
                            hydra::set_profile_icon(h, r);
                            self.icon_picked = true;
                        }
                    }
                    // SetAnnouncerPack, then LoadStoredAnnouncerPackAssetData, InitVOData,
                    // PlayVO(announcer_characterselect_intro) (the sink's)
                    "announcer" => {
                        let path = r.path.clone();
                        hydra::set_global_equippable(h, GlobalKey::AnnouncerPack, r);
                        cx.sink.announcer_equipped(&path);
                    }
                    "ringout" => {
                        if local {
                            cx.prefs.local_play.players[p].ringout_slug = r.slug;
                        } else {
                            hydra::set_global_equippable(h, GlobalKey::RingOutVfx, r);
                        }
                    }
                    "banner" => {
                        if local {
                            cx.prefs.local_play.players[p].banner_slug = r.slug;
                        } else {
                            hydra::set_global_equippable(h, GlobalKey::Banner, r);
                        }
                    }
                    _ => {
                        if let Some(slot) = taunt_slot(&t) {
                            // Set-LocalPlay-TauntForPlayer with Local Play Settings (@30871 / @31237 / @31603 /
                            // @31969), then, with no pop_flow, SetCharacterTaunt-Rework in either mode (@30998 /
                            // @31364 / @31730 / @32096): a Local Play pick writes both stores (ring-out and banner
                            // write one)
                            if local {
                                cx.prefs.local_play.players[p].taunts[slot] = r.slug.clone();
                            }
                            if let Some(ch) = &self.character {
                                cx.prefs.hydra.set_character_taunt(&ch.slug, slot, r);
                            }
                        }
                    }
                }
                EquipperTile::Cosmetic(c)
            }
            _ => return,
        };
        // SavePreferences: the settings save themselves when they change.
        for l in Self::lists(cx) {
            self.update_selections(cx, l);
        }
        cx.emit(self, |c| &c.selected_event, &Some(picked));
    }

    /// Whether a profile icon was picked since the last call. The Profile's icon listener runs at its next tick, after
    /// the pick's UpdateSelections: it calls `update_all_selections` again, so the picked tile ends selected
    /// (the listener inside the pick, then UpdateSelections).
    pub fn take_icon_pick(&mut self) -> bool {
        std::mem::take(&mut self.icon_picked)
    }

    /// UpdateSelections of both lists.
    pub fn update_all_selections(&mut self, cx: &mut Cx<'_>) {
        for l in Self::lists(cx) {
            self.update_selections(cx, l);
        }
    }

    /// UpdateSelections(List) @32997: each tile's Selected is the slot's current slug, then its UpdateAppearance.
    /// The list runs it as a call of its own (`run_on`): a pick reaches `selected` from inside the list's
    /// SelectedEvent, so that list is busy and a `visit` of it finds nothing. Its tiles take their Selected when the
    /// list's call returns, after this chooser's SelectedEvent.
    fn update_selections(&mut self, cx: &mut Cx<'_>, list: WidgetId) {
        let t = self.kind.to_lowercase();
        let local = Self::local_play(cx);
        let g = cx.prefs.hydra.clone();
        let lp = cx.prefs.local_play.players[self.player_index.min(3)].clone();
        if t == "skin" {
            // @37574-37875: Selected = SelectString(the Local Play slot's skinSlug, the tile's slug, Local Play
            // Settings) == GetSelectedSkinForCharacter's slug. As the build has it: on a Local Play instance every
            // tile compares the slot's slug, so all or none are selected.
            let f = self.fighter(cx);
            let sel = f.as_ref().and_then(|f| selected_skin(f, &g)).map(|s| s.slug.clone()).unwrap_or_default();
            run_on::<GridItemList>(cx, list, move |l, cx| {
                for w in l.items() {
                    let Some((Some(TileData::Skin(mut s)), _)) = l.record(w) else { continue };
                    s.selected = (if local { lp.skin_slug.as_str() } else { s.skin.slug.as_str() }) == sel;
                    l.set_tile_data(cx, w, Some(TileData::Skin(s)));
                }
            });
            return;
        }
        let cur: String = match t.as_str() {
            "banner" => {
                if local {
                    lp.banner_slug.clone()
                } else {
                    g.banner.slug.clone()
                }
            }
            "ringout" => {
                if local {
                    lp.ringout_slug.clone()
                } else {
                    g.ring_out_vfx.slug.clone()
                }
            }
            "stattrackers" => g.stat_trackers[0].slug.clone(),
            "stattrackers2" => g.stat_trackers[1].slug.clone(),
            "stattrackers3" => g.stat_trackers[2].slug.clone(),
            "profileicon" => g.profile_icon.slug.clone(),
            "announcer" => g.announcer_pack.slug.clone(),
            _ => match taunt_slot(&t) {
                Some(slot) => {
                    if local {
                        lp.taunts[slot].clone()
                    } else {
                        self.character.as_ref().map_or(String::new(), |c| g.taunt_for_slot(slot, &c.slug).to_string())
                    }
                }
                None => return,
            },
        };
        run_on::<GridItemList>(cx, list, move |l, cx| {
            for w in l.items() {
                let Some((Some(TileData::Cosmetic(mut c)), _)) = l.record(w) else { continue };
                c.selected = c.item.slug == cur;
                l.set_tile_data(cx, w, Some(TileData::Cosmetic(c)));
            }
        });
    }

    /// FocusFirst -> FocusAfterAnimating @41104: Delay(0.25); while ShowAnim or ShowAnim_Short plays, Delay(0.1) and
    /// again; then Delay(0.25) and, in order, every selected tile of each list focused, the primary list's first
    /// tile focused, and buttonCollapse.SetFocused(true) (context only: every focus of the player taken off). The
    /// last call leaves no tile focused, as the build has it.
    pub fn focus_first(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        cx.view.delay(0.25, Handler { owner: me, id: C_FOCUS_WAIT });
    }

    fn focus_after_animating(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        if cx.view.is_animation_playing(me, "ShowAnim") || cx.view.is_animation_playing(me, "ShowAnim_Short") {
            cx.view.delay(0.1, Handler { owner: me, id: C_FOCUS_WAIT });
            return;
        }
        cx.view.delay(0.25, Handler { owner: me, id: C_FOCUS_LAST });
    }

    fn focus_last(&mut self, cx: &mut Cx<'_>) {
        let [primary, secondary] = Self::lists(cx);
        for l in [primary, secondary] {
            for (w, _) in list_items(cx, l) {
                let sel = cx.visit::<GridItemList, _>(l, |g, cx| g.tile_selected(cx, w)).unwrap_or(false);
                if sel && let Some(&i) = tile_interactors(cx, w).first() {
                    set_focused(cx, i, false);
                }
            }
        }
        if let Some((w, _)) = list_items(cx, primary).into_iter().next()
            && let Some(&i) = tile_interactors(cx, w).first()
        {
            set_focused(cx, i, false);
        }
        let b = find(cx, "buttonCollapse");
        set_focused(cx, b, true);
    }

    pub fn shown(&self) -> bool {
        self.shown
    }

    /// Show @17089: SetVisibility(Visible), the height from 0 and ShowAnim (ShowAnim_Short with IsShortDisplay).
    pub fn show(&mut self, cx: &mut Cx<'_>) {
        if self.shown {
            return;
        }
        self.shown = true;
        let me = cx.w;
        wd(cx, me).vis = Vis::Visible;
        let b = find(cx, "SizeBox_2");
        wd(cx, b).h_override = 0.0;
        let anim: &'static str = if prop_bool(cx, "IsShortDisplay") { "ShowAnim_Short" } else { "ShowAnim" };
        cx.view.play(me, anim, false, 1.0);
        self.anim = anim;
        self.heights_at_end(cx, anim);
        cx.emit(self, |c| &c.show_event, &());
    }

    /// Hide @16886.
    pub fn hide(&mut self, cx: &mut Cx<'_>) {
        self.shown = false;
        let me = cx.w;
        wd(cx, me).vis = Vis::Collapsed;
        let b = find(cx, "SizeBox_2");
        wd(cx, b).h_override = 0.0;
        cx.emit(self, |c| &c.hide_event, &());
    }

    /// Deviation: the slate plays no HeightOverride float track (ShowAnim grows SizeBox_2 and ScrollSizeBox from 0 to
    /// their last key): the boxes take the last key at once.
    fn heights_at_end(&mut self, cx: &mut Cx<'_>, anim: &str) {
        let Some(a) = cx.view.bundle.widgets.get("UI_CollectionChooser").and_then(|c| c.anims.get(anim)) else {
            return;
        };
        let mut set: Vec<(String, f64)> = Vec::new();
        for tr in &a.tracks {
            if tr.kind != "MovieSceneFloatTrack" || tr.prop.as_deref() != Some("HeightOverride") {
                continue;
            }
            let last = tr
                .sections
                .first()
                .and_then(|s| s.ch.get("FloatCurve"))
                .and_then(|c| c.v.last())
                .and_then(|v| v.first());
            if let Some(&v) = last {
                set.push((tr.widget.clone(), v));
            }
        }
        for (name, v) in set {
            let w = find(cx, &name);
            wd(cx, w).h_override = v;
        }
    }

    /// UpdateContext(Context, Player Index) @16507: the collapse buttons and both lists, for the player.
    pub fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str, player: usize) {
        self.context = ctx.to_string();
        self.player_index = player;
        let own = [find(cx, "buttonCollapse"), find(cx, "buttonCollapse_1")];
        batch_set_context(cx, &own, ctx);
        let mut all: Vec<WidgetId> = own.to_vec();
        for l in Self::lists(cx) {
            cx.visit::<GridItemList, _>(l, |g, cx| g.update_context(cx, ctx));
            for (w, _) in list_items(cx, l) {
                all.extend(tile_interactors(cx, w));
            }
        }
        for i in all {
            if let Some(s) = cx.view.manager.interactor_mut(i) {
                s.player_index = player as i32;
            }
        }
    }
}

impl Controller for CollectionChooser {
    /// The slate here plays the HeightOverride tracks, which the port's boxes take at their last key at once: held
    /// while the show animation runs.
    fn tick(&mut self, cx: &mut Cx<'_>, _dt: f64) {
        let me = cx.w;
        if self.shown && !self.anim.is_empty() && cx.view.is_animation_playing(me, self.anim) {
            let anim = self.anim;
            self.heights_at_end(cx, anim);
        }
    }

    /// OnInitialized @38271: RegisterScrollbox(scrollcontainer), the manager's OnScroll, the two lists' SelectedEvent
    /// -> Selected(item, its Is Locked); buttonCollapse @16871 (both interactors) -> Hide.
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        self.shown = cx.view.tree().widget(me).shown();
        let a = find(cx, "buttonCollapse");
        bind(cx, a, |s| &mut s.on_click, C_COLLAPSE);
        let b = find(cx, "buttonCollapse_1");
        bind(cx, b, |s| &mut s.on_click, C_COLLAPSE_1);
        let sc = find(cx, "scrollcontainer");
        cx.view.manager.register_scrollbox(sc);
        cx.view.manager.on_scroll.add(Handler { owner: me, id: C_SCROLL });
        for l in Self::lists(cx) {
            cx.visit::<GridItemList, _>(l, |g, _| g.selected_event.add(Handler { owner: me, id: C_SELECTED }));
        }
    }

    fn destruct(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        cx.view.manager.on_scroll.remove(Handler { owner: me, id: C_SCROLL });
        let sc = find(cx, "scrollcontainer");
        cx.view.manager.unregister_scrollbox(sc);
        self.release_pictures(cx);
    }

    /// PreConstruct @39129: the lists' columns, the header strip at the bottom or the top (the content's padding, 30
    /// at the sides with Local Play Settings, else 50; the scroll bar's padding; the header's alignment; the
    /// pointer), the container width.
    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        let cols = cx.view.tree().props(cx.w).get("Items Per Row").cloned();
        if let Some(c) = cols {
            for l in Self::lists(cx) {
                cx.view.tree_mut().set_prop(l, "Force Grid Columns", c.clone());
            }
        }
        let bottom = prop_bool(cx, "HeaderPositionBottom");
        let side = if Self::local_play(cx) { 30.0 } else { 50.0 };
        let vb = find(cx, "VerticalBox_0");
        wd(cx, vb).pad = if bottom {
            Margin { l: side, t: 50.0, r: side, b: 50.0 }
        } else {
            Margin { l: side, t: 70.0, r: side, b: 0.0 }
        };
        let sc = find(cx, "scrollcontainer");
        let pad = if bottom {
            Margin { l: 0.0, t: 50.0, r: 10.0, b: 100.0 }
        } else {
            Margin { l: 0.0, t: 80.0, r: 10.0, b: 80.0 }
        };
        set_scrollbar_padding(cx.view.tree_mut(), sc, pad);
        let va = if bottom { VAlign::Bottom } else { VAlign::Top };
        for n in ["headerContainer", "headerStrip"] {
            let w = find(cx, n);
            wd(cx, w).v_align = va;
        }
        set_vis(cx, "PointerBottom", if bottom { Vis::SelfHitTestInvisible } else { Vis::Collapsed });
        set_vis(cx, "PointerTop", if bottom { Vis::Collapsed } else { Vis::SelfHitTestInvisible });
        let width = prop_f64(cx, "Manual Container Width", 0.0);
        let b = find(cx, "SizeBox_2");
        wd(cx, b).w_override = if width > 0.0 { width } else { -1.0 };
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        match id {
            C_COLLAPSE | C_COLLAPSE_1 => self.hide(cx),
            // OnScoll @38711: this chooser's player, seen: ScrollList(scrollcontainer, Amount x 2)
            C_SCROLL => {
                let Some(&(player, _dir, amount)) = arg.downcast_ref::<(i32, i32, f64)>() else { return };
                let me = cx.w;
                if player as usize == self.player_index && cx.view.tree().widget(me).shown() {
                    let sc = find(cx, "scrollcontainer");
                    scroll_list(cx.view.tree_mut(), sc, amount * 2.0);
                }
            }
            C_SELECTED => {
                let Some((ev, _)) = arg.downcast_ref::<(TileEvent, bool)>() else { return };
                let ev = ev.clone();
                self.selected(cx, &ev);
            }
            _ => {}
        }
    }

    fn resume(&mut self, cx: &mut Cx<'_>, id: u32) {
        match id {
            C_FOCUS_WAIT => self.focus_after_animating(cx),
            C_FOCUS_LAST => self.focus_last(cx),
            _ => {}
        }
    }
}
