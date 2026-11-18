//! The main menu's Collection (UI_Collection*, UI_Perks_Trainer): the slide-out
//! modal `collection` (UI_CollectionsPanel) with its overview page (the fighter grid, the global cosmetics) and its
//! character page (banner, info, rank, mastery, perks, XP boost, the bottom buttons), the two cosmetic choosers, and
//! the perk trainer modal `perksTrainer`.
//!
//! What only the backend fills keeps its tree's state: the completion stats (UI_CollectionStats waits for the
//! InventoryManager, @921 Delay(0) -> @31, for ever offline), the rank (Loading Rank...), the toasts, tickets and
//! boosts, the perk lists of the trainer. A button that needs the backend, a match or a purchase does nothing but
//! warn once per id (`inert`).
//!
//! Ownership without an account: "Is Character Owned?" is the native inventory's. The live game with no backend (a
//! capture of the live game) shows every fighter owned: "23/23", no unlock button, the XP boost button up. The port
//! reads it so; the grid's tiles keep the port's own lock on a fighter the port does not have
//! (UI_Collection_CharacterItem), which the capture does not show.

use std::any::Any;
use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

use serde_json::Value;

use crate::bundle::Rgba;
use crate::bundle::RosterEntry;
use crate::menu::controller::{Controller, Cx, Event, Handler};
use crate::menu::hydra;
use crate::menu::manager::{Manager, Modal};
use crate::menu::pictures::roster_pictures;
use crate::menu::screens::lab_selectors::{
    CharacterTypeInfo, EquipperItem, EquipperTile, GridItemList, TileData, TileEvent, run_on, set_text, set_vis,
};
use crate::menu::screens::move_list::CharacterMoveList;
use crate::menu::scroll::scroll_list;
use crate::menu::widgets::battle_pass::{RewardTrack, mastery_pictures};
use crate::menu::widgets::collection_chooser::CollectionChooser;
use crate::menu::widgets::common::{ModalBase, fill_page, rgba};
use crate::menu::widgets::controls::buttons::{ButtonMain, ButtonSecondary};
use crate::menu::widgets::controls::grid_item::GridItem;
use crate::menu::widgets::controls::interactor::{batch_set_context, set_context, set_focused, set_interactable};
use crate::menu::widgets::controls::tooltip::TooltipTarget;
use crate::menu::widgets::controls::{bind, find, quad_color, wd};
use crate::menu::widgets::end_of_game::CollectionToasts;
use crate::menu::widgets::match_options::warn_once;
use crate::menu::widgets::nav::GlobalSlideOut;
use crate::menu::widgets::perks::{PerksEquipper, PerksLoadout, PerksSublist, perk_pictures};
use crate::menu::widgets::section_bar::SectionBar;
use crate::slate::{Margin, Vis, WidgetId};

const CLEAR: Rgba = [0.0, 0.0, 0.0, 0.0];
/// The port's owner keys of the pictures the modal holds.
const PICTURES_COLLECTION: &str = "collection";
const PICTURES_COLLECTION_PERKS: &str = "collection-perks";
const PICTURES_COLLECTION_TILES: &str = "collection-tiles";

/// ESlateVisibility of UI_Functions "Conditional Visibility"(widget, condition, when true, when false).
fn cond(cx: &mut Cx<'_>, w: WidgetId, c: bool, t: Vis, f: Vis) {
    wd(cx, w).vis = if c { t } else { f };
}

fn cond_named(cx: &mut Cx<'_>, name: &str, c: bool, t: Vis, f: Vis) {
    let w = find(cx, name);
    cond(cx, w, c, t, f);
}

/// UWidget::IsVisible: Visible, HitTestInvisible or SelfHitTestInvisible.
fn is_visible(cx: &Cx<'_>, w: WidgetId) -> bool {
    cx.view.tree().widget(w).shown()
}

fn lerp(a: Rgba, b: Rgba, t: f64) -> Rgba {
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
}

/// A click that needs the backend, a match or a purchase: one warning per id, then nothing.
fn inert(id: &str) {
    warn_once(id, "the Collection");
}

/// UI_Functions "Is Character Owned?" @176-258: in forced offline mode Owned = true and Success = true for every
/// fighter (see above).
fn owned(_c: &RosterEntry) -> bool {
    true
}

/// CharacterData.Difficulty (Low 0, Medium 1, High 2 in the bundle's roster).
fn difficulty_of(c: &RosterEntry) -> i32 {
    match c.difficulty.as_str() {
        "Low" => 0,
        "High" => 2,
        _ => 1,
    }
}

const TREE_BACKGROUND: Rgba = [0.806952, 0.270498, 0.0, 1.0];

/// CharacterData.BackgroundColor. The bundle has it for the ported fighters only (the menu bundle's `fighters`).
/// Deviation: another fighter takes the tree's BG_Color of UI_CollectionsPanel (not its own, which the build has), so
/// that the last fighter's colour does not stay.
fn background_of(cx: &Cx<'_>, c: &RosterEntry) -> Rgba {
    let f = c.fighter.as_ref().and_then(|k| cx.view.fighters().get(k));
    match f.map(|f| &f.background_color) {
        Some(b) if b.get("R").is_some() => rgba(b),
        _ => TREE_BACKGROUND,
    }
}

/// An int argument of Format: FText::AsNumber, grouped as the en culture groups.
fn int_text(n: i64) -> String {
    let s = n.abs().to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    if n < 0 { format!("-{out}") } else { out }
}

/// A button widget's interactor.
fn ib(cx: &Cx<'_>, w: WidgetId) -> WidgetId {
    cx.view.tree().find_in(w, "UI_Interactor")
}

/// A child widget of `w` by name.
fn find_in(cx: &Cx<'_>, w: WidgetId, name: &str) -> WidgetId {
    cx.view.tree().find_in(w, name)
}

fn button_click(cx: &mut Cx<'_>, name: &str, secondary: bool, id: u32) {
    let w = find(cx, name);
    let me = cx.w;
    if secondary {
        cx.visit::<ButtonSecondary, _>(w, |b, _| b.on_click.add(Handler { owner: me, id }));
    } else {
        cx.visit::<ButtonMain, _>(w, |b, _| b.on_click.add(Handler { owner: me, id }));
    }
}

// ---------- UI_CollectionBoostIcon ----------

/// Game.locres key of Format("x{count}").
const BOOST_COUNT_TEXT: &str = "73BD51804BA2EFC8FAD03B8BD0DC2231";

#[derive(Debug, Default)]
pub struct CollectionBoostIcon;

impl CollectionBoostIcon {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(CollectionBoostIcon)
    }

    /// SetCount @10-304: Format("x{count}") (Game.locres key) of the count.
    pub fn set_count(&mut self, cx: &mut Cx<'_>, n: i64) {
        let t = cx.view.game_text(BOOST_COUNT_TEXT).replace("{count}", &int_text(n));
        set_text(cx, "TextBlock_Count", &t);
    }
}

impl Controller for CollectionBoostIcon {}

// ---------- UI_CollectionXPBoost ----------

const XP_CLICK: u32 = 1;

/// Set Character @262. The boost count is InventoryManager.InventoryConsumables["character-xpboost"]: 0 without an
/// inventory, so the button is never interactable and its click (@15 ConsumeCharacterGift) is not reached.
#[derive(Debug, Default)]
pub struct CollectionXPBoost;

impl CollectionXPBoost {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(CollectionXPBoost)
    }

    pub fn set_character(&mut self, cx: &mut Cx<'_>, c: Option<&RosterEntry>) {
        let b = find(cx, "UI_ButtonSecondary");
        // "Is Character Owned?"(c, true): Owned AND Success
        let own = c.is_some_and(owned);
        wd(cx, b).vis = if own { Vis::Visible } else { Vis::Collapsed };
        let count = 0;
        let i = ib(cx, b);
        set_interactable(cx, i, own && count > 0);
        let icon = find(cx, "UI_CollectionBoostIcon");
        cx.visit::<CollectionBoostIcon, _>(icon, |i, cx| i.set_count(cx, count));
    }

    /// Update Context @2131.
    pub fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str) {
        let b = find(cx, "UI_ButtonSecondary");
        let i = ib(cx, b);
        batch_set_context(cx, &[i], ctx);
    }
}

impl Controller for CollectionXPBoost {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        button_click(cx, "UI_ButtonSecondary", true, XP_CLICK);
    }

    fn resume(&mut self, _cx: &mut Cx<'_>, _id: u32) {}

    fn handle(&mut self, _cx: &mut Cx<'_>, id: u32, _arg: &dyn Any) {
        if id == XP_CLICK {
            inert("collection:xpBoost");
        }
    }
}

// ---------- UI_CollectionBottomBtns ----------

const BB_MOVE_LIST: u32 = 1;
const BB_UNLOCK: u32 = 2;
const BB_CLAIM: u32 = 3;
const BB_SUBMIT2: u32 = 4;

#[derive(Debug, Default)]
pub struct CollectionBottomBtns {
    character: Option<RosterEntry>,
}

impl CollectionBottomBtns {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::<CollectionBottomBtns>::default()
    }

    /// SetCharacter @5108 -> @4781. buttonClaim keeps its tree state (Collapsed): only UpdateClaimRewardsButton
    /// (@5422, after a claim) touches it.
    pub fn set_character(&mut self, cx: &mut Cx<'_>, c: Option<&RosterEntry>) {
        self.character = c.cloned();
        let Some(c) = c else { return };
        let own = owned(c);
        let u = find(cx, "buttonUnlock");
        let i = ib(cx, u);
        set_interactable(cx, i, !own);
        cond(cx, u, own, Vis::Collapsed, Vis::SelfHitTestInvisible);
        // Unverified: the two label texts are the port's guess (no text constant read for them); the button is
        // Collapsed offline, where every fighter is owned, so neither shows.
        let label = if own { "UNLOCKED" } else { "UNLOCK" };
        cx.visit::<ButtonMain, _>(u, |b, cx| b.set_label(cx, label));
    }

    /// UpdateContext @3388 (GetInteractors: the three buttons).
    pub fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str) {
        let l: Vec<WidgetId> = ["buttonUnlock", "buttonMoveList", "buttonClaim"]
            .iter()
            .map(|n| {
                let b = find(cx, n);
                ib(cx, b)
            })
            .collect();
        batch_set_context(cx, &l, ctx);
    }

    /// OnSubmit2 @3813: the button is seen, interactable and in context, and the latest modal is this one.
    fn on_submit2(&mut self, cx: &mut Cx<'_>) {
        let b = find(cx, "buttonMoveList");
        let i = ib(cx, b);
        let ok = cx.view.visible_in_hierarchy(b)
            && Manager::is_in_current_context(cx, i, false, 0)
            && cx.view.manager.interactor(i).is_some_and(|s| s.get_interactable());
        if !ok || Manager::latest_modal_id(cx.view) != "collection" {
            return;
        }
        self.show_move_list(cx);
    }

    /// buttonMoveList OnClick @2577: ShowModal-Generic(UI_CharacterMoveList, "movelist") and SetCharacter. The
    /// port's Move List has the MoveList tables of its own fighters only (the bundle's fighters): another fighter's
    /// click does nothing.
    fn show_move_list(&mut self, cx: &mut Cx<'_>) {
        let Some(c) = self.character.clone() else { return };
        let Some(f) = c.fighter else {
            inert(&format!("collection:movelist:{}", c.slug));
            return;
        };
        if let Some(m) = Manager::show_modal(cx, "UI_CharacterMoveList", "movelist", "", "") {
            cx.visit::<CharacterMoveList, _>(m, |m, cx| m.set_character(cx, &f));
        }
    }
}

impl Controller for CollectionBottomBtns {
    /// OnInitialized @5417 -> @4325: OnSubmit2 of the manager is the Move List's shortcut.
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        button_click(cx, "buttonMoveList", true, BB_MOVE_LIST);
        // @3003: the purchase path (the shop config, the instant purchase modal)
        button_click(cx, "buttonUnlock", false, BB_UNLOCK);
        button_click(cx, "buttonClaim", false, BB_CLAIM);
        cx.view.manager.on_submit2.add(Handler { owner: me, id: BB_SUBMIT2 });
    }

    /// Destruct @4492.
    fn destruct(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        cx.view.manager.on_submit2.remove(Handler { owner: me, id: BB_SUBMIT2 });
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, _arg: &dyn Any) {
        match id {
            BB_MOVE_LIST => self.show_move_list(cx),
            BB_UNLOCK => inert("collection:buttonUnlock"),
            BB_CLAIM => inert("collection:buttonClaim"),
            BB_SUBMIT2 => self.on_submit2(cx),
            _ => {}
        }
    }
}

// ---------- UI_CollectionPerks ----------

const CP_EDIT: u32 = 2;
/// The pencil's move without the train button beside it (Slate units): its centre under perkCommon3's (the loadout's
/// lower small diamond), just clear of its tip.
const EDIT_MOVE: [f64; 2] = [157.5, -42.0];
/// Its slot's Left (the tree's 10), with which EDIT_MOVE's x lands on the diamond's centre.
const EDIT_LEFT: f64 = 7.5;
const CP_INTERACTOR: u32 = 3;
const CP_SLOT: u32 = 4;

/// PerksTrainerVisitedSlugForCharacter / Get-UI-NewContentSlug: the preferences' "seen" slugs of the trainer, which
/// UI_Perks_Trainer writes on entering (@8317-8639).
/// Deviation: the port keeps them for the session only (it keeps no such preference).
fn trainer_visited() -> &'static Mutex<HashSet<String>> {
    static V: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    V.get_or_init(|| Mutex::new(HashSet::new()))
}

/// Set Character @254. The train button is not shown (the user's choice). Its UI_Perks_Loadout shows the character's loadout of
/// the page (none: four empty slots); a slot's click is EditPerksEvent (@818). ToggleFocused (@742 / @780) is empty
/// in the loadout.
#[derive(Debug, Default)]
pub struct CollectionPerks {
    /// TrainPerksEvent(Character), EditPerksEvent(Character, Slot).
    pub train_perks_event: Event<RosterEntry>,
    pub edit_perks_event: Event<(RosterEntry, i32)>,
    character: Option<RosterEntry>,
    context: String,
}

impl CollectionPerks {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::<CollectionPerks>::default()
    }

    /// Set Character @254 (@273 CloseTrainingTooltip first).
    pub fn set_character(&mut self, cx: &mut Cx<'_>, c: Option<&RosterEntry>) {
        self.character = c.cloned();
        self.close_training_tooltip(cx);
        // @287
        let lo = find(cx, "UI_Perks_Loadout");
        let f = c.and_then(|c| c.fighter.clone());
        cx.visit::<PerksLoadout, _>(lo, |l, cx| l.set_character(cx, f.as_deref()));
        // Deviation: no Train Perks button and no "Time to train!" callout
        // (@287's IsTrainingUnlocked, @629 Delay -> Update Training Callout): trainingbuttoncontainer is collapsed.
        let t = find(cx, "trainingbuttoncontainer");
        wd(cx, t).vis = Vis::Collapsed;
        // Deviation (the user's choice): the pencil, alone in its row, moves (EDIT_MOVE) to under the loadout's lower
        // small diamond.
        let ed = find(cx, "buttonEdit");
        let x = wd(cx, ed);
        x.pad.l = EDIT_LEFT;
        x.transform.translation = EDIT_MOVE;
    }

    /// CloseTrainingTooltip @1382.
    pub fn close_training_tooltip(&mut self, cx: &mut Cx<'_>) {
        let t = find(cx, "trainingAvailableTooltip");
        cx.visit::<TooltipTarget, _>(t, |t, cx| t.close_tooltip(cx));
    }

    /// UpdateContext @1350 -> @1221 (GetInteractors: the interactor and the two buttons).
    pub fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str) {
        self.context = ctx.to_string();
        let own = find(cx, "UI_Interactor");
        let tr = find(cx, "buttonTrain");
        let tr = ib(cx, tr);
        let ed = find(cx, "buttonEdit");
        let ed = ib(cx, ed);
        batch_set_context(cx, &[own, tr, ed], ctx);
        // @1299
        let lo = find(cx, "UI_Perks_Loadout");
        let c = ctx.to_string();
        cx.visit::<PerksLoadout, _>(lo, |l, cx| l.update_context(cx, &c, 0));
    }
}

impl Controller for CollectionPerks {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        // buttonEdit @732 / the interactor @737 -> @684 -> @717: the event, then CloseTrainingTooltip (buttonTrain
        // @860 is not shown)
        button_click(cx, "buttonEdit", true, CP_EDIT);
        let i = find(cx, "UI_Interactor");
        bind(cx, i, |s| &mut s.on_click, CP_INTERACTOR);
        let lo = find(cx, "UI_Perks_Loadout");
        cx.visit::<PerksLoadout, _>(lo, |l, _| l.perk_selected_event.add(Handler { owner: me, id: CP_SLOT }));
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        match id {
            CP_EDIT | CP_INTERACTOR => {
                if let Some(c) = self.character.clone() {
                    cx.emit(self, |s| &s.edit_perks_event, &(c, -1));
                }
                self.close_training_tooltip(cx);
            }
            CP_SLOT => {
                let Some(&(_, slot, _)) = arg.downcast_ref::<crate::menu::widgets::perks::LoadoutPick>() else {
                    return;
                };
                if let Some(c) = self.character.clone() {
                    cx.emit(self, |s| &s.edit_perks_event, &(c, slot));
                }
            }
            _ => {}
        }
    }
}

// ---------- UI_CollectionCharacterRank ----------

/// SetCharacter @311: SetLoading(true) @765 and LeaderboardManager.GetMyCharacterRank, which never answers offline:
/// "Loading Rank..." stays up.
#[derive(Debug, Default)]
pub struct CollectionCharacterRank;

impl CollectionCharacterRank {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(CollectionCharacterRank)
    }

    pub fn set_character(&mut self, cx: &mut Cx<'_>, c: Option<&RosterEntry>) {
        if c.is_some() {
            self.set_loading(cx, true);
        }
    }

    /// SetLoading @10.
    fn set_loading(&mut self, cx: &mut Cx<'_>, on: bool) {
        set_vis(cx, "LoadingIndicator", if on { Vis::Visible } else { Vis::Hidden });
        set_vis(cx, "RankContainer", if on { Vis::Hidden } else { Vis::Visible });
    }
}

impl Controller for CollectionCharacterRank {}

// ---------- UI_CollectionCharacterInfo ----------

#[derive(Debug, Default)]
pub struct CollectionCharacterInfo;

impl CollectionCharacterInfo {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(CollectionCharacterInfo)
    }

    /// SetCharacter @435 -> UpdateAppearance @15 / Update Homescreen Button @142.
    /// Deviation: a guard of the port's own: the build's UI_CharacterTypeInfo.SetCharacter reads None for a cleared
    /// character (an Accessed None warning).
    pub fn set_character(&mut self, cx: &mut Cx<'_>, c: Option<&RosterEntry>) {
        if let Some(c) = c {
            let t = find(cx, "UI_CharacterTypeInfo");
            let c = c.clone();
            cx.visit::<CharacterTypeInfo, _>(t, |t, cx| t.set_character(cx, &c));
        }
        let r = find(cx, "UI_CollectionCharacterRank");
        let cc = c.cloned();
        cx.visit::<CollectionCharacterRank, _>(r, |r, cx| r.set_character(cx, cc.as_ref()));
        if let Some(c) = c {
            set_text(cx, "nameLabel", &c.display_name);
        }
        set_vis(cx, "buttonHomescreen", if c.is_some() { Vis::Hidden } else { Vis::Collapsed });
    }

    pub fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str) {
        // GetInteractors: [buttonHomescreen.UI_Interactor]
        let b = find(cx, "buttonHomescreen");
        let i = ib(cx, b);
        batch_set_context(cx, &[i], ctx);
    }
}

impl Controller for CollectionCharacterInfo {}

// ---------- UI_CollectionCharacterDetails ----------

const MASTERY_LEVEL: i32 = 15;
/// UI_Perks_Trainer's Format patterns (Game.locres keys, namespace ""): SetCharacter @6592 "Perk Training -
/// {character}", @6979 "{character}'S KNOWN PERKS"; OnInitialized @4367 the instructions with {perkLimit}.
const TRAINER_TITLE_TEXT: &str = "30FB47774F27FD54A2A0F69806465271";
const TRAINER_KNOWN_TEXT: &str = "9CB70D9A43CCE73C6BBB06A88889E668";
const TRAINER_INSTRUCTIONS_TEXT: &str = "9AF7C5BD48F8ACF9041FC4BE6EBB975B";

/// SetCharacter @1725 -> SetRewardTrack @1987: the mastery track (WBP_RewardTrack of WBP_CharMastery_Card, the
/// rewards of the data's MasteryRewardTable) and the complete overlay; the level and the XP line are not shown.
/// Deviation: every fighter is at mastery level 15 (the track's MaximumTier: every card past, "All Mastery Rewards
/// Unlocked!") with the level's whole XP earned and no unclaimed reward. The build reads the level and XP from the
/// Hydra account (GetCharacterMasteryLevel @2240) and the unclaimed list from the native
/// CharacterMasteryRewardManager (@2115); the port has neither and shows each track complete.
/// Not ported: the bio (CharacterData.Bio is not in the bundle; panelBio is not the shown panel), the list bar
/// (Collapsed in the tree), LoadUnclaimedRewards and the claim, SetupRewardTrackProgress and the progress animation
/// it feeds (On Progress Bar Tick / Complete / Level: the XP boost's, not a page open).
#[derive(Debug, Default)]
pub struct CollectionCharacterDetails;

impl CollectionCharacterDetails {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(CollectionCharacterDetails)
    }

    pub fn set_character(&mut self, cx: &mut Cx<'_>, c: Option<&RosterEntry>) {
        if let Some(c) = c {
            self.set_reward_track(cx, &c.slug);
        }
    }

    /// SetRewardTrack @1987: OptionalSlug, SetCurrentTier(MasteryLevel, PercentProgress, the unclaimed tiers, 0, 0, 0)
    /// @2400, LevelNumber @2627, Overlay_MasteryComplete at MasteryLevel >= MaximumTier (@2745 / @2805), Number
    /// @2983-3441.
    fn set_reward_track(&mut self, cx: &mut Cx<'_>, slug: &str) {
        let t = find(cx, "WBP_RewardTrack");
        let xp = cx
            .view
            .bundle
            .menu
            .as_ref()
            .and_then(|m| m.mastery.get("levels"))
            .and_then(|l| l.get((MASTERY_LEVEL - 1) as usize))
            .and_then(Value::as_f64)
            .unwrap_or(1.0);
        // the port's account: the level's whole XP earned (MasteryXP), so PercentProgress (MasteryXP over the
        // level's XP) is 1
        let mastery_xp = xp;
        let s = slug.to_string();
        cx.visit::<RewardTrack, _>(t, |t, cx| {
            t.optional_slug = s;
            t.set_current_tier(cx, MASTERY_LEVEL, mastery_xp / xp, &[], 0, 0.0, 0.0);
        });
        // Deviation: no mastery level on the page, as on the character tiles. The
        // build's row above the track, "Level {LevelNumber}" (@2627) and NextXPContainer's "{Number}XP To Next Level"
        // (@2983-3441), is hidden here (its room kept, the track where the build has it); the track stays.
        set_vis(cx, "HorizontalBox_0", Vis::Hidden);
        let max = cx.view.tree().props(t).get("MaximumTier").and_then(Value::as_f64).unwrap_or(0.0);
        cond_named(
            cx,
            "Overlay_MasteryComplete",
            f64::from(MASTERY_LEVEL) >= max,
            Vis::SelfHitTestInvisible,
            Vis::Collapsed,
        );
    }

    /// UpdateContext @1459: BatchSetContext of GetInteractors (none), the list bar's, the track's.
    pub fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str) {
        let bar = find(cx, "UI_ListBar");
        let c = ctx.to_string();
        cx.visit::<SectionBar, _>(bar, |b, cx| b.update_context(cx, &c, None));
        let t = find(cx, "WBP_RewardTrack");
        let c = ctx.to_string();
        cx.visit::<RewardTrack, _>(t, |t, cx| t.update_context(cx, &c));
    }
}

impl Controller for CollectionCharacterDetails {}

// ---------- UI_CollectionCharacterCarousel ----------

const CC_PREV: u32 = 1;
const CC_NEXT: u32 = 2;
const CC_SWITCH: u32 = 3;
const CC_CONFIRM: u32 = 4;

/// The previous and next buttons step through Get Characters (the roster in its order). SetCharacter @2280 centres
/// the fighter in characterList (UI_GridItemList CenterItemInList: Shift Left / Right rotate the items so that it
/// sits at Length / 2), so OnSwitchPage always finds a neighbour: Q on the first fighter goes to the last, E on the
/// last to the first. The tile list (centerpanel) is Collapsed in the tree and its Populate (@3385, 23
/// UI_PM_CharacterItem tiles nobody sees) is not ported: the step wraps through the roster's own order, which is
/// the rotated list's. Not ported either: ScrollWidgetIntoView of that unseen list (@2366, @56) and its Update
/// Appearance (@124).
#[derive(Debug, Default)]
pub struct CollectionCharacterCarousel {
    pub character_selected_event: Event<RosterEntry>,
    pub back_event: Event<()>,
    character: Option<RosterEntry>,
    previous: Option<RosterEntry>,
}

impl CollectionCharacterCarousel {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::<CollectionCharacterCarousel>::default()
    }

    /// SetCharacter @2599: nothing when both it and Previous Character are None (@2753) or when both are set with
    /// the same slug (@2960); else @2936 Character, then @1834 (a valid one only): the buttons take its colour, and
    /// after Delay(0.033) (@2415 -> @15) Previous Character = Character and ui_menu_confirm.
    pub fn set_character(&mut self, cx: &mut Cx<'_>, c: Option<&RosterEntry>) {
        if c.is_none() && self.previous.is_none() {
            return;
        }
        if let (Some(c), Some(p)) = (c, &self.previous)
            && c.slug == p.slug
        {
            return;
        }
        self.character = c.cloned();
        let Some(c) = c else { return };
        let bg = background_of(cx, c);
        for n in ["Btn_Prev", "btn_Next"] {
            let b = find(cx, n);
            cx.visit::<ButtonSecondary, _>(b, |b, cx| b.reset_style(cx, Some(bg)));
            let q = find_in(cx, b, "QuadBorder");
            quad_color(cx, q, lerp(bg, CLEAR, 0.35));
        }
        let me = cx.w;
        cx.view.delay(0.033, Handler { owner: me, id: CC_CONFIRM });
    }

    /// CanPaginate?: seen, the latest modal is this panel's, no perk equipper or trainer up.
    fn can_paginate(cx: &Cx<'_>) -> bool {
        if !cx.view.visible_in_hierarchy(cx.w) {
            return false;
        }
        if Manager::is_modal_active(cx.view) && !Manager::latest_modal_id(cx.view).to_lowercase().contains("collection")
        {
            return false;
        }
        cx.view.manager.get_modal("perksTrainer").is_none()
    }

    /// OnSwitchPage(Direction, Player Index) @3652: the item at IndexForData(Character) + Direction of the centred
    /// list (@3729-3910), Selected -> CharacterSelectedEvent (@3934): the roster's neighbour, wrapping.
    fn on_switch_page(&mut self, cx: &mut Cx<'_>, direction: i32, player: i32) {
        if player != 0 || !Self::can_paginate(cx) {
            return;
        }
        let Some(cur) = self.character.clone() else { return };
        let roster = cx.view.roster();
        let Some(at) = roster.iter().position(|r| r.slug == cur.slug) else { return };
        let n = roster.len() as i32;
        let next = roster[(((at as i32 + direction) % n) + n) as usize % n as usize].clone();
        cx.emit(self, |c| &c.character_selected_event, &next);
    }

    /// UpdateContext @2470 (GetInteractors: the two buttons).
    pub fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str) {
        let l: Vec<WidgetId> = ["Btn_Prev", "btn_Next"]
            .iter()
            .map(|n| {
                let b = find(cx, n);
                ib(cx, b)
            })
            .collect();
        batch_set_context(cx, &l, ctx);
    }
}

impl Controller for CollectionCharacterCarousel {
    /// OnInitialized @3057: OnRequestSwitchPage of the manager; the buttons @4182 / @4187.
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        button_click(cx, "Btn_Prev", true, CC_PREV);
        button_click(cx, "btn_Next", true, CC_NEXT);
        cx.view.manager.on_request_switch_page.add(Handler { owner: me, id: CC_SWITCH });
    }

    fn destruct(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        cx.view.manager.on_request_switch_page.remove(Handler { owner: me, id: CC_SWITCH });
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        match id {
            CC_PREV => self.on_switch_page(cx, -1, 0),
            CC_NEXT => self.on_switch_page(cx, 1, 0),
            CC_SWITCH => {
                if let Some(&(player, direction)) = arg.downcast_ref::<(i32, i32)>() {
                    self.on_switch_page(cx, direction, player);
                }
            }
            _ => {}
        }
    }

    fn resume(&mut self, cx: &mut Cx<'_>, id: u32) {
        if id == CC_CONFIRM {
            self.previous = self.character.clone();
            cx.post("ui_menu_confirm");
        }
    }
}

// ---------- UI_CollectionCharacters ----------

const CH_SCROLL: u32 = 1;
const CH_SELECTED: u32 = 2;

#[derive(Debug)]
pub struct CollectionCharacters {
    pub selected_event: Event<RosterEntry>,
    pub can_scroll: bool,
}

impl Default for CollectionCharacters {
    fn default() -> Self {
        CollectionCharacters { selected_event: Event::default(), can_scroll: true }
    }
}

impl CollectionCharacters {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::<CollectionCharacters>::default()
    }

    pub fn focus_first(&mut self, cx: &mut Cx<'_>) {
        let g = find(cx, "CharacterGrid");
        cx.visit::<GridItemList, _>(g, |g, cx| g.focus_index(cx, 0, 0));
    }

    pub fn deselect_all(&mut self, cx: &mut Cx<'_>) {
        let g = find(cx, "CharacterGrid");
        cx.visit::<GridItemList, _>(g, |g, cx| g.deselect_all(cx));
    }

    /// UpdateContext @1321.
    pub fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str) {
        let g = find(cx, "CharacterGrid");
        let c = ctx.to_string();
        cx.visit::<GridItemList, _>(g, |g, cx| g.update_context(cx, &c));
    }
}

impl Controller for CollectionCharacters {
    /// OnInitialized @471: RegisterScrollbox, OnScroll, the grid's events, Populate(Get Characters(false, false)),
    /// GetStats.
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        let sb = find(cx, "ScrollBox_0");
        cx.view.manager.register_scrollbox(sb);
        cx.view.manager.on_scroll.add(Handler { owner: me, id: CH_SCROLL });
        let g = find(cx, "CharacterGrid");
        // the grid's SelectedEvent @1372
        cx.visit::<GridItemList, _>(g, |g, _| g.selected_event.add(Handler { owner: me, id: CH_SELECTED }));
        let roster: Vec<TileData> = cx.view.roster().iter().cloned().map(TileData::Roster).collect();
        cx.visit::<GridItemList, _>(g, |g, cx| g.populate(cx, roster));
        // OnItemAdded @1671: the tile's interactor scrolls ScrollBox_0
        let tiles = cx.visit::<GridItemList, _>(g, |g, _| g.items()).unwrap_or_default();
        for t in tiles {
            let ints = cx.visit_dyn(t, |c, cx| c.as_grid_item().map(|t| t.interactors(cx)).unwrap_or_default());
            if let Some(&i) = ints.unwrap_or_default().first()
                && let Some(s) = cx.view.manager.interactor_mut(i)
            {
                s.scroll_box = Some(sb);
            }
        }
        let n = cx.view.roster().len();
        let own = cx.view.roster().iter().filter(|r| owned(r)).count();
        set_text(cx, "totalLabel", &n.to_string());
        set_text(cx, "ownedLabel", &own.to_string());
    }

    fn destruct(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        cx.view.manager.on_scroll.remove(Handler { owner: me, id: CH_SCROLL });
        let sb = find(cx, "ScrollBox_0");
        cx.view.manager.unregister_scrollbox(sb);
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        match id {
            // OnScroll @1561
            CH_SCROLL => {
                if let Some(&(player, _, amount)) = arg.downcast_ref::<(i32, i32, f64)>()
                    && player == 0
                    && self.can_scroll
                {
                    let sb = find(cx, "ScrollBox_0");
                    scroll_list(cx.view.tree_mut(), sb, amount);
                }
            }
            CH_SELECTED => {
                let Some((ev, _)) = arg.downcast_ref::<(TileEvent, bool)>() else { return };
                if let Some(TileData::Roster(r)) = ev.data.clone() {
                    cx.emit(self, |c| &c.selected_event, &r);
                }
            }
            _ => {}
        }
    }
}

// ---------- UI_CollectionOverview (the cosmetics row: global, or the character's) ----------

const OVERVIEW_TILES: [(&str, &str); 13] = [
    ("equipperRingout", "ringout"),
    ("equipperAnnouncer", "announcer"),
    ("equipperBanner", "banner"),
    ("equipperPodium", "podium"),
    ("equipperProfileIcon", "profileicon"),
    ("equipperSkin", "skin"),
    ("equipperTaunt1", "taunt1"),
    ("equipperTaunt2", "taunt2"),
    ("equipperTaunt3", "taunt3"),
    ("equipperTaunt4", "taunt4"),
    ("equipperStatTracker", "statTrackers"),
    ("equipperStatTracker2", "statTrackers2"),
    ("equipperStatTracker3", "statTrackers3"),
];

/// OffsetForItem (@0-1185, case-insensitive): the chooser's pointer x under the chosen tile; 0 for any other.
fn offset_for_item(kind: &str) -> f64 {
    match kind.to_lowercase().as_str() {
        "skin" => -10.0,
        "taunt1" => 190.0,
        "taunt2" => 360.0,
        "taunt3" => 530.0,
        "taunt4" => 700.0,
        "ringout" => 50.0,
        "announcer" => 1050.0,
        "profileicon" => 575.0,
        "banner" => 250.0,
        "podium" => 870.0,
        "stattrackers" => 450.0,
        "stattrackers2" => 650.0,
        "stattrackers3" => 850.0,
        _ => 0.0,
    }
}

/// The overview's SelectedEvent(Item Data, Grid Item, X Offset, Type).
pub type OverviewPick = (Option<EquipperTile>, WidgetId, f64, String);

/// The first handler id of the overview's tiles (one per tile).
const OV_TILE: u32 = 100;

/// The tiles are filled by Update From Preferences (UI_EquipperItem_V2 @15364) from the Hydra preferences and the
/// cosmetics catalogue. The skin tile is the character's selected skin, GetSelectedSkinForCharacter: with no
/// preferences the default skin, which the bundle has for the ported fighters (as the Lab's skin selector reads it).
/// The profile icon and podium tiles are not in Update From Preferences' switch: they stay empty, as the build has
/// them.
#[derive(Default)]
pub struct CollectionOverview {
    /// SelectedEvent(Item Data, Grid Item, X Offset, Type).
    pub selected_event: Event<OverviewPick>,
    character: Option<RosterEntry>,
    /// The Hydra preferences as the tiles last read them (no change listener: compared at the tick).
    seen: Option<mvs_settings::HydraSettings>,
}

impl CollectionOverview {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::<CollectionOverview>::default()
    }

    /// FocusFirst @3661.
    pub fn focus_first(&mut self, cx: &mut Cx<'_>) {
        let t = find(cx, "equipperSkin");
        let i = ib(cx, t);
        set_focused(cx, i, false);
    }

    /// SetCharacter @1804: Character, UpdateAppearance @354 (the two containers), the skin tile's time viewed (the
    /// backend's new-content marks: none offline), UpdateSelections.
    pub fn set_character(&mut self, cx: &mut Cx<'_>, c: Option<&RosterEntry>) {
        self.character = c.cloned();
        set_vis(cx, "globalContainer", if c.is_some() { Vis::Collapsed } else { Vis::SelfHitTestInvisible });
        set_vis(cx, "characterContainer", if c.is_some() { Vis::SelfHitTestInvisible } else { Vis::Collapsed });
        self.update_selections(cx);
    }

    /// UpdateSelections @2279: a valid Character fills the skin and the four taunt tiles, else the global tiles.
    pub fn update_selections(&mut self, cx: &mut Cx<'_>) {
        self.seen = Some(cx.prefs.hydra.clone());
        if let Some(c) = self.character.clone() {
            let skin = find(cx, "equipperSkin");
            let c1 = c.clone();
            cx.visit::<EquipperItem, _>(skin, |t, cx| t.update_from_preferences(cx, "skin", Some(&c1), ""));
            // Deviation: a fighter the bundle has no skins of: UI_EquipperItem_V2's empty state keeps the last icon
            // in the port, so the tile's icon is hidden here.
            let empty = cx.visit::<EquipperItem, _>(skin, |t, _| t.data().is_none()).unwrap_or(false);
            if empty {
                let icon = find_in(cx, skin, "Icon");
                wd(cx, icon).vis = Vis::Hidden;
            }
            for n in 1..=4 {
                let t = find(cx, &format!("equipperTaunt{n}"));
                let c1 = c.clone();
                let tag = format!("taunt{n}");
                run_on::<EquipperItem>(cx, t, move |t, cx| t.update_from_preferences(cx, &tag, Some(&c1), ""));
            }
            return;
        }
        for (name, tag) in [
            ("equipperRingout", "ringout"),
            ("equipperAnnouncer", "announcer"),
            ("equipperBanner", "banner"),
            ("equipperStatTracker", "statTracker"),
            ("equipperStatTracker2", "statTracker2"),
            ("equipperStatTracker3", "statTracker3"),
            ("equipperProfileIcon", "profileIcon"),
        ] {
            let t = find(cx, name);
            run_on::<EquipperItem>(cx, t, move |t, cx| t.update_from_preferences(cx, tag, None, ""));
        }
    }

    /// The skin tile's icon (the pictures the character page holds).
    pub fn skin_icon(&self, cx: &mut Cx<'_>) -> Option<String> {
        let t = find(cx, "equipperSkin");
        match cx.visit::<EquipperItem, _>(t, |t, _| t.data()).flatten() {
            Some(EquipperTile::Skin(s)) => Some(s.skin.icon),
            _ => None,
        }
    }

    /// The icons of the tiles the shown page draws.
    pub fn tile_icons(&self, cx: &mut Cx<'_>) -> Vec<String> {
        let names: &[&str] = if self.character.is_some() {
            &["equipperTaunt1", "equipperTaunt2", "equipperTaunt3", "equipperTaunt4"]
        } else {
            &[
                "equipperRingout",
                "equipperAnnouncer",
                "equipperBanner",
                "equipperStatTracker",
                "equipperStatTracker2",
                "equipperStatTracker3",
            ]
        };
        let mut out = Vec::new();
        for n in names {
            let t = find(cx, n);
            if let Some(EquipperTile::Cosmetic(c)) = cx.visit::<EquipperItem, _>(t, |t, _| t.data()).flatten()
                && let Some(p) = c.item.icon.as_deref().and_then(hydra::picture_name)
            {
                out.push(p);
            }
        }
        out
    }

    /// SetAllOff: every tile's Set On(false).
    pub fn set_all_off(&mut self, cx: &mut Cx<'_>) {
        for (name, _) in OVERVIEW_TILES {
            let t = find(cx, name);
            run_on::<EquipperItem>(cx, t, |t, cx| t.set_on(cx, false));
        }
    }

    /// Selected(Grid Item, Type) @3119: SetAllOff, the tile's Set On(true), SelectedEvent(its data, it,
    /// OffsetForItem(Type), Type).
    fn selected(&mut self, cx: &mut Cx<'_>, index: usize, data: Option<EquipperTile>) {
        let (name, kind) = OVERVIEW_TILES[index];
        self.set_all_off(cx);
        let tile = find(cx, name);
        run_on::<EquipperItem>(cx, tile, |t, cx| t.set_on(cx, true));
        let ev = (data, tile, offset_for_item(kind), kind.to_string());
        cx.emit(self, |c| &c.selected_event, &ev);
    }

    /// ToggleLabels(Emotes, Variants) @3721.
    pub fn toggle_labels(&mut self, cx: &mut Cx<'_>, emotes: bool, variants: bool) {
        cond_named(cx, "VariantsLabel", variants, Vis::Visible, Vis::Hidden);
        cond_named(cx, "EmotesLabel", emotes, Vis::Visible, Vis::Hidden);
    }

    /// UpdateContext @1725 (GetInteractors: every tile's).
    pub fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str) {
        let l: Vec<WidgetId> = OVERVIEW_TILES
            .iter()
            .map(|(n, _)| {
                let t = find(cx, n);
                ib(cx, t)
            })
            .collect();
        batch_set_context(cx, &l, ctx);
    }
}

impl Controller for CollectionOverview {
    /// OnInitialized @1953: SetCharacter(None), every tile DimsIcon false, the tiles' SelectedEvent -> Selected, and
    /// PreferenceTauntsAutoUpdated -> TauntsAutoUpdate -> UpdateSelections. The preferences have no change event
    /// here: the tick compares them (any write, PreferenceTauntsAutoUpdated among them; UpdateSelections reads the
    /// preferences again, so a call for another write shows the same).
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        // PreConstruct @3489: equipperSkin's SizeContainer 190 x 260 (set here, after the tile's own construction
        // has sized it)
        let skin = find(cx, "equipperSkin");
        cx.visit::<EquipperItem, _>(skin, |t, cx| GridItem::set_size(t, cx, 190.0, 260.0));
        for (i, (name, _)) in OVERVIEW_TILES.iter().enumerate() {
            let t = find(cx, name);
            cx.view.tree_mut().set_prop(t, "DimsIcon", Value::Bool(false));
            cx.visit::<EquipperItem, _>(t, |t, _| t.selected_event.add(Handler { owner: me, id: OV_TILE + i as u32 }));
        }
        self.set_character(cx, None);
        // The catalogue is in the bundle: the SetDefault* rules run where the Collection first lists it.
        let bundle = cx.view.bundle.clone();
        if let Some(cat) = hydra::cosmetics_in(&bundle) {
            let h = &mut cx.prefs.hydra;
            let m = &mut cx.view.manager;
            hydra::load_defaults(h, cat, &mut || m.random());
        }
        self.update_selections(cx);
    }

    fn tick(&mut self, cx: &mut Cx<'_>, _dt: f64) {
        if self.seen.as_ref() != Some(&cx.prefs.hydra) {
            self.update_selections(cx);
        }
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        if (OV_TILE..OV_TILE + OVERVIEW_TILES.len() as u32).contains(&id)
            && let Some(d) = arg.downcast_ref::<Option<EquipperTile>>()
        {
            self.selected(cx, (id - OV_TILE) as usize, d.clone());
        }
    }
}

// ---------- UI_CollectionStats ----------

/// OnInitialized @1075 -> RefreshData @15: without the InventoryManager the Delay(0) loop @921 never ends, so no
/// stat enters and IsLoaded? stays false (the stats keep their tree's Collapsed).
#[derive(Debug, Default)]
pub struct CollectionStats;

impl CollectionStats {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(CollectionStats)
    }

    /// UpdateContext @1080 (Get Interactors: the toasts' button).
    pub fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str) {
        let t = find(cx, "UI_CollectionToasts");
        let c = ctx.to_string();
        cx.visit::<CollectionToasts, _>(t, |t, cx| t.update_context(cx, &c));
    }
}

impl Controller for CollectionStats {}

// ---------- UI_CollectionsPanel (the modal `collection`) ----------

const PN_MODAL_DISMISSED: u32 = 1;
const PN_CHARACTER: u32 = 2;
const PN_CAROUSEL: u32 = 3;
const PN_CAROUSEL_BACK: u32 = 4;
const PN_BACK: u32 = 5;
const PN_TRAIN: u32 = 6;
const PN_EDIT: u32 = 7;
const PN_DISMISSER: u32 = 8;
const PN_GLOBAL_ITEM: u32 = 9;
const PN_CHARACTER_ITEM: u32 = 10;
const PN_GLOBAL_HIDE: u32 = 11;
const PN_CHARACTER_HIDE: u32 = 12;
const PN_GLOBAL_SELECTED: u32 = 13;
const PN_CHARACTER_SELECTED: u32 = 14;
const PN_PERKS_SAVED: u32 = 15;
const PN_PERKS_EXITED: u32 = 16;
const PN_FIRST_CONTEXT: u32 = 17;
const PN_CLOSE: u32 = 18;

#[derive(Default)]
pub struct CollectionsPanel {
    base: ModalBase,
    character: Option<RosterEntry>,
    last_overview_item: Option<WidgetId>,
    last_overview_type: String,
    last_overview_x: f64,
}

impl CollectionsPanel {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::<CollectionsPanel>::default()
    }

    fn chooser_shown(cx: &mut Cx<'_>, name: &str) -> bool {
        let c = find(cx, name);
        cx.visit::<CollectionChooser, _>(c, |c, _| c.shown()).unwrap_or(false)
    }

    /// Update Appearance @4186.
    fn update_appearance(&mut self, cx: &mut Cx<'_>) {
        let c = self.character.clone();
        let valid = c.is_some();
        if let Some(c) = &c {
            set_text(cx, "CharName_BG", &c.display_name);
        }
        let mc = find(cx, "MainContainer");
        wd(cx, mc).active = i32::from(valid);
        let v = Vis::SelfHitTestInvisible;
        cond_named(cx, "UI_CollectionCharacters", valid, Vis::Collapsed, v);
        cond_named(cx, "UI_GlobalCosmetics", valid, Vis::Collapsed, v);
        cond_named(cx, "UI_CharacterCosmetics", valid, v, Vis::Collapsed);
        let gc = find(cx, "UI_GlobalCollectionChooser");
        let details_on = valid && !cx.view.visible_in_hierarchy(gc);
        cond_named(cx, "UI_CollectionCharacterDetails", details_on, v, Vis::Collapsed);
        cond_named(cx, "UI_CollectionPerks", valid, v, Vis::Collapsed);
        cond_named(cx, "CollectionHomeHeader", valid, Vis::Collapsed, v);
        cond_named(cx, "HeaderBanner", valid, Vis::Collapsed, v);
        cond_named(cx, "CharacterInfoHeader", valid, v, Vis::Collapsed);
        cond_named(cx, "CharBanner", valid, v, Vis::Collapsed);
        cond_named(cx, "UI_CollectionStats", valid, Vis::Hidden, v);
        cond_named(cx, "UI_CollectionBottomBtns", valid, v, Vis::Hidden);
        cond_named(cx, "UI_CollectionXPBoost", valid, v, Vis::Hidden);
        let global_shown = Self::chooser_shown(cx, "UI_GlobalCollectionChooser");
        let character_shown = Self::chooser_shown(cx, "UI_CharacterCollectionChooser");
        cond_named(cx, "TrayCoverGlobal", global_shown, Vis::Visible, Vis::Collapsed);
        cond_named(cx, "CosmeticsLabel", global_shown, Vis::Hidden, v);
        cond_named(cx, "TrayCoverCharacter", character_shown, Vis::Visible, Vis::Collapsed);
        // @7014 -> @1358-1954: TrayCoverCharacter's RenderOpacity eases from 0 to 1 in 0.3 s and Can Scroll follows
        // the global chooser.
        // Deviation: the port sets the end of the fade at once.
        let tc = find(cx, "TrayCoverCharacter");
        wd(cx, tc).opacity = 1.0;
        let chars = find(cx, "UI_CollectionCharacters");
        cx.visit::<CollectionCharacters, _>(chars, |c, _| c.can_scroll = !global_shown);
        // @6031: the grid and the details leave the context while a chooser covers them
        let ignore = cx.view.manager.ignore_context.clone();
        let ctx = self.base.context.clone();
        let grid_ctx = if global_shown { ignore.clone() } else { ctx.clone() };
        cx.visit::<CollectionCharacters, _>(chars, |c, cx| c.update_context(cx, &grid_ctx));
        let details = find(cx, "UI_CollectionCharacterDetails");
        let details_ctx = if character_shown { ignore } else { ctx };
        cx.visit::<CollectionCharacterDetails, _>(details, |d, cx| d.update_context(cx, &details_ctx));
        if let Some(c) = &c {
            // @6511-6873.
            // Deviation: CollectionsPortraitMaterial is not read: characterIcon (the tree's Bugs portrait) is
            // hidden.
            let bg = background_of(cx, c);
            set_color_named(cx, "BG_Color", bg);
            let back = find(cx, "Btn_Back");
            cx.visit::<ButtonSecondary, _>(back, |b, cx| b.reset_style(cx, Some(bg)));
            let q = find_in(cx, back, "QuadBorder");
            quad_color(cx, q, lerp(bg, CLEAR, 0.35));
            set_vis(cx, "characterIcon", Vis::Hidden);
            self.update_difficulty(cx, c);
        } else if is_visible(cx, chars) {
            cx.visit::<CollectionCharacters, _>(chars, |c, cx| c.deselect_all(cx));
        }
    }

    /// UpdateDifficulty.
    fn update_difficulty(&mut self, cx: &mut Cx<'_>, c: &RosterEntry) {
        let d = difficulty_of(c);
        let tag = find(cx, "DifficultyTag");
        let ribbons = find(cx, "RibbonsContainer");
        if d == 1 {
            wd(cx, tag).vis = Vis::Collapsed;
            wd(cx, ribbons).transform.translation[0] = 0.0;
            return;
        }
        set_color_named(
            cx,
            "DifficultyColor",
            if d == 0 { [0.008023, 0.693872, 0.258183, 1.0] } else { [1.0, 0.0, 0.070588, 1.0] },
        );
        set_text(cx, "DifficultyLabel", if d == 0 { "Recommended" } else { "Expert" });
        wd(cx, tag).vis = Vis::SelfHitTestInvisible;
        wd(cx, ribbons).transform.translation[0] = -12.0;
    }

    /// SetCharacter @15.
    fn set_character(&mut self, cx: &mut Cx<'_>, c: Option<RosterEntry>) {
        self.character = c.clone();
        let cc = find(cx, "UI_CharacterCosmetics");
        let c1 = c.clone();
        cx.visit::<CollectionOverview, _>(cc, |o, cx| o.set_character(cx, c1.as_ref()));
        macro_rules! to {
            ($name:literal, $ty:ty) => {{
                let w = find(cx, $name);
                let c1 = c.clone();
                cx.visit::<$ty, _>(w, |o, cx| o.set_character(cx, c1.as_ref()));
            }};
        }
        to!("UI_CollectionCharacterInfo", CollectionCharacterInfo);
        to!("UI_CollectionPerks", CollectionPerks);
        // the carousel is busy when its own step reached here: the call waits for it
        let car = find(cx, "UI_CollectionCharacterCarousel");
        let c1 = c.clone();
        run_on::<CollectionCharacterCarousel>(cx, car, move |o, cx| o.set_character(cx, c1.as_ref()));
        to!("UI_CollectionCharacterDetails", CollectionCharacterDetails);
        to!("UI_CollectionBottomBtns", CollectionBottomBtns);
        to!("UI_CollectionXPBoost", CollectionXPBoost);
        let me = cx.w;
        cx.view.play(me, "CharAnimIn", false, 1.25);
        // the port's own: what the shown page draws: the character page its skin tile, the overview the grid's
        // portraits (a new hold takes the place of the last)
        let names: Vec<String> = match &c {
            Some(c) => {
                let skin = cx.visit::<CollectionOverview, _>(cc, |o, cx| o.skin_icon(cx)).flatten();
                let (track, _sampled) = mastery_pictures(cx.view, &c.slug);
                skin.into_iter().chain(track).collect()
            }
            None => roster_pictures(cx.view),
        };
        cx.view.hold_pictures(PICTURES_COLLECTION, names.iter().map(String::as_str));
        self.hold_tile_pictures(cx);
        let item = self.last_overview_item;
        if let (Some(item), true) = (item, Self::chooser_shown(cx, "UI_GlobalCollectionChooser")) {
            let d = Self::item_data(cx, item);
            self.overview_grid_item_selected(cx, d);
        } else if let (Some(item), true) = (item, Self::chooser_shown(cx, "UI_CharacterCollectionChooser")) {
            let d = Self::item_data(cx, item);
            self.character_grid_item_selected(cx, d);
        } else {
            // @892
            self.update_appearance(cx);
            cx.visit::<CollectionOverview, _>(cc, |o, cx| o.focus_first(cx));
        }
    }

    fn item_data(cx: &mut Cx<'_>, item: WidgetId) -> Option<EquipperTile> {
        cx.visit::<EquipperItem, _>(item, |t, _| t.data()).flatten()
    }

    /// The overview's SelectedEvent @3899: a second click on the tile closes the chooser.
    fn global_item_selected(&mut self, cx: &mut Cx<'_>, ev: &OverviewPick) {
        let (data, item, x, kind) = ev.clone();
        if Some(item) == self.last_overview_item && Self::chooser_shown(cx, "UI_GlobalCollectionChooser") {
            self.toggle_global_chooser(cx, false);
            return;
        }
        self.last_overview_item = Some(item);
        self.last_overview_type = kind;
        self.last_overview_x = x;
        self.overview_grid_item_selected(cx, data);
    }

    /// UI_CharacterCosmetics' SelectedEvent @11758.
    fn character_item_selected(&mut self, cx: &mut Cx<'_>, ev: &OverviewPick) {
        let (data, item, x, kind) = ev.clone();
        if Some(item) == self.last_overview_item && Self::chooser_shown(cx, "UI_CharacterCollectionChooser") {
            self.toggle_character_chooser(cx, false);
            return;
        }
        self.last_overview_item = Some(item);
        self.last_overview_type = kind;
        self.last_overview_x = x;
        self.character_grid_item_selected(cx, data);
    }

    /// Overview Grid Item Selected @10332: the global chooser's Init(Item Data, Character, X Offset, Type), shown.
    fn overview_grid_item_selected(&mut self, cx: &mut Cx<'_>, data: Option<EquipperTile>) {
        let ch = find(cx, "UI_GlobalCollectionChooser");
        let (c, x, t) = (self.character.clone(), self.last_overview_x, self.last_overview_type.clone());
        cx.visit::<CollectionChooser, _>(ch, |ch, cx| ch.init(cx, data, c.as_ref(), x, &t));
        self.toggle_global_chooser(cx, true);
    }

    /// CharacterGridItemSelected @12332.
    fn character_grid_item_selected(&mut self, cx: &mut Cx<'_>, data: Option<EquipperTile>) {
        let ch = find(cx, "UI_CharacterCollectionChooser");
        let (c, x, t) = (self.character.clone(), self.last_overview_x, self.last_overview_type.clone());
        cx.visit::<CollectionChooser, _>(ch, |ch, cx| ch.init(cx, data, c.as_ref(), x, &t));
        self.toggle_character_chooser(cx, true);
    }

    /// The overview tiles' pictures of the shown page.
    fn hold_tile_pictures(&mut self, cx: &mut Cx<'_>) {
        let o = find(cx, if self.character.is_some() { "UI_CharacterCosmetics" } else { "UI_GlobalCosmetics" });
        let names = cx.visit::<CollectionOverview, _>(o, |o, cx| o.tile_icons(cx)).unwrap_or_default();
        cx.view.hold_pictures(PICTURES_COLLECTION_TILES, names.iter().map(String::as_str));
    }

    /// ToggleGlobalCollectionChooser @8027.
    fn toggle_global_chooser(&mut self, cx: &mut Cx<'_>, on: bool) {
        let ch = find(cx, "UI_GlobalCollectionChooser");
        cx.visit::<CollectionChooser, _>(ch, |c, cx| if on { c.show(cx) } else { c.hide(cx) });
        self.update_appearance(cx);
    }

    /// ToggleCharacterCollectionChooser @12021.
    fn toggle_character_chooser(&mut self, cx: &mut Cx<'_>, on: bool) {
        let ch = find(cx, "UI_CharacterCollectionChooser");
        let cc = find(cx, "UI_CharacterCosmetics");
        if on {
            // @12076
            let p = find(cx, "UI_CollectionPerks");
            cx.visit::<CollectionPerks, _>(p, |p, cx| p.close_training_tooltip(cx));
            cx.visit::<CollectionChooser, _>(ch, |c, cx| c.show(cx));
            let t = self.last_overview_type.to_lowercase();
            let (e, s) = (t.contains("taunt"), t.contains("skin"));
            run_on::<CollectionOverview>(cx, cc, move |o, cx| o.toggle_labels(cx, e, s));
        } else {
            cx.visit::<CollectionChooser, _>(ch, |c, cx| c.hide(cx));
            run_on::<CollectionOverview>(cx, cc, |o, cx| o.toggle_labels(cx, true, true));
        }
        self.update_appearance(cx);
    }

    /// EditPerksEvent(Character, Perk Slot) @8361: the page's SetCharacter(Character, no ally perks), Enter(Slot),
    /// TogglePerksContainer(true).
    fn edit_perks(&mut self, cx: &mut Cx<'_>, c: &RosterEntry, slot: i32) {
        let eq = find(cx, "UI_Perks_Equipper");
        let f = c.fighter.clone();
        run_on::<PerksEquipper>(cx, eq, move |e, cx| {
            e.set_character(cx, f.as_deref());
            e.enter(cx, slot);
        });
        // the page's icons, for the modal's life
        let names = perk_pictures(cx.view, c.fighter.as_deref());
        cx.view.hold_pictures(PICTURES_COLLECTION_PERKS, names.iter().map(String::as_str));
        self.toggle_perks_container(cx, true);
    }

    /// TogglePerksContainer(Visible) @10993: the dismisser takes the page's context (Use Own Context: a new one at
    /// Enter), PerksScalebox SelfHitTestInvisible / Collapsed.
    fn toggle_perks_container(&mut self, cx: &mut Cx<'_>, show: bool) {
        let eq = find(cx, "UI_Perks_Equipper");
        if let Some(ctx) = cx.visit::<PerksEquipper, _>(eq, |e, _| e.context.clone()) {
            let d = find(cx, "buttonPerksDismisser");
            set_context(cx, d, &ctx);
        }
        set_vis(cx, "PerksScalebox", if show { Vis::SelfHitTestInvisible } else { Vis::Collapsed });
    }

    /// StepBack @7350.
    fn step_back(&mut self, cx: &mut Cx<'_>) {
        let p = find(cx, "UI_CollectionPerks");
        cx.visit::<CollectionPerks, _>(p, |p, cx| p.close_training_tooltip(cx));
        if cx.view.manager.get_modal("instantPurchase").is_some() || cx.view.manager.get_modal("perksTrainer").is_some()
        {
            return;
        }
        // @7669: the perk page up: its Ready(false)
        let ps = find(cx, "PerksScalebox");
        if is_visible(cx, ps) {
            let eq = find(cx, "UI_Perks_Equipper");
            run_on::<PerksEquipper>(cx, eq, |e, cx| e.ready(cx, false));
            return;
        }
        if Self::chooser_shown(cx, "UI_GlobalCollectionChooser") {
            self.toggle_global_chooser(cx, false);
            return;
        }
        if Self::chooser_shown(cx, "UI_CharacterCollectionChooser") {
            self.toggle_character_chooser(cx, false);
            return;
        }
        if self.character.is_some() {
            self.set_character(cx, None);
            let mc = find(cx, "MainContainer");
            wd(cx, mc).active = 0;
            return;
        }
        self.close(cx);
    }

    /// Close @11491: Delay(0) -> @2019 CloseSlideOut, RecordClosingAnalyticEvents @124 (its analytics not ported; it
    /// calls SetCharacter(None) whatever the page: RequestDismiss("close") from a fighter's page turns the closing
    /// panel to its overview), the unbinding.
    fn close(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        cx.view.delay(0.0, Handler { owner: me, id: PN_CLOSE });
    }

    fn unbind(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        cx.view.manager.modal_dismissed.remove(Handler { owner: me, id: PN_MODAL_DISMISSED });
    }

    fn slide_out(cx: &Cx<'_>) -> WidgetId {
        find(cx, "WBP_GlobalSlideOut")
    }
}

impl Modal for CollectionsPanel {
    fn pre_show(&mut self, cx: &mut Cx<'_>, id: &str) {
        self.base.pre_show(cx, id);
    }

    /// PostShow @8578. Not ported: the instructions ShortcutCharacter / QuickClaim (no caller in the port) and
    /// Construct's waiting cover (@2299: IsLoaded? needs UI_CollectionStats, which never loads offline; the port's
    /// slide-out has no SlideoutLoaded cover).
    fn post_show(&mut self, cx: &mut Cx<'_>, _instructions: &str, _extra_data: &str) {
        let so = Self::slide_out(cx);
        let id = self.base.modal_id.clone();
        cx.visit::<GlobalSlideOut, _>(so, |s, cx| s.open_slide_out(cx, &id));
        // the fighter grid's portraits
        let names = roster_pictures(cx.view);
        cx.view.hold_pictures(PICTURES_COLLECTION, names.iter().map(String::as_str));
    }

    fn pre_dismiss(&mut self, cx: &mut Cx<'_>) {
        self.base.pre_dismiss(cx);
    }

    /// The pictures go with the modal.
    fn post_dismiss(&mut self, cx: &mut Cx<'_>) {
        self.base.post_dismiss(cx);
        for o in [PICTURES_COLLECTION, PICTURES_COLLECTION_PERKS, PICTURES_COLLECTION_TILES] {
            cx.view.release_pictures(o);
        }
        for n in ["UI_GlobalCollectionChooser", "UI_CharacterCollectionChooser"] {
            let c = find(cx, n);
            cx.visit::<CollectionChooser, _>(c, |c, cx| c.release_pictures(cx));
        }
    }

    /// RequestDismiss @10184 (case-insensitive): "close" closes, anything else steps back.
    fn request_dismiss(&mut self, cx: &mut Cx<'_>, reason: &str, _from_cancel: bool) {
        if reason.eq_ignore_ascii_case("close") {
            self.close(cx);
        } else {
            self.step_back(cx);
        }
    }

    fn get_modal_context(&self) -> String {
        self.base.context.clone()
    }
}

impl Controller for CollectionsPanel {
    fn as_modal(&mut self) -> Option<&mut dyn Modal> {
        Some(self)
    }

    /// OnInitialized @7345 -> @2428: the bindings, Update Appearance, then Delay(0.2) @3082 -> @929: a new context
    /// and the first tile's focus. Not ported: the analytics (Record UIFlow Event @1094, RecordCharacterViewEvent),
    /// the shop's product and sale ribbons (FetchProductFromInventorySlug @128), the mastery reward claim
    /// (AttemptClaimMasteryRewardsForCharacter @244), the inventory's OnInventoryUpdated (@2983).
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        cx.view.manager.modal_dismissed.add(Handler { owner: me, id: PN_MODAL_DISMISSED });
        let chars = find(cx, "UI_CollectionCharacters");
        cx.visit::<CollectionCharacters, _>(chars, |c, _| {
            c.selected_event.add(Handler { owner: me, id: PN_CHARACTER })
        });
        let car = find(cx, "UI_CollectionCharacterCarousel");
        cx.visit::<CollectionCharacterCarousel, _>(car, |c, _| {
            c.character_selected_event.add(Handler { owner: me, id: PN_CAROUSEL });
            c.back_event.add(Handler { owner: me, id: PN_CAROUSEL_BACK });
        });
        button_click(cx, "Btn_Back", true, PN_BACK);
        let perks = find(cx, "UI_CollectionPerks");
        cx.visit::<CollectionPerks, _>(perks, |p, _| {
            p.train_perks_event.add(Handler { owner: me, id: PN_TRAIN });
            p.edit_perks_event.add(Handler { owner: me, id: PN_EDIT });
        });
        // buttonPerksDismisser OnClick @10988 -> @7725: the page's Ready(false)
        let d = find(cx, "buttonPerksDismisser");
        bind(cx, d, |s| &mut s.on_click, PN_DISMISSER);
        let (g, c) = (find(cx, "UI_GlobalCosmetics"), find(cx, "UI_CharacterCosmetics"));
        cx.visit::<CollectionOverview, _>(g, |o, _| o.selected_event.add(Handler { owner: me, id: PN_GLOBAL_ITEM }));
        cx.visit::<CollectionOverview, _>(c, |o, _| o.selected_event.add(Handler { owner: me, id: PN_CHARACTER_ITEM }));
        // the choosers' HideEvent @8151 / @12680 and SelectedEvent @8202 / @12521
        let (gc, cc) = (find(cx, "UI_GlobalCollectionChooser"), find(cx, "UI_CharacterCollectionChooser"));
        cx.visit::<CollectionChooser, _>(gc, |c, _| {
            c.hide_event.add(Handler { owner: me, id: PN_GLOBAL_HIDE });
            c.selected_event.add(Handler { owner: me, id: PN_GLOBAL_SELECTED });
        });
        cx.visit::<CollectionChooser, _>(cc, |c, _| {
            c.hide_event.add(Handler { owner: me, id: PN_CHARACTER_HIDE });
            c.selected_event.add(Handler { owner: me, id: PN_CHARACTER_SELECTED });
        });
        // the perk page: PerksSavedEvent @13020 -> @8487: the tab's Set Character (the saved loadout);
        // PerksEquipperExitedEvent @13015 -> @11475: TogglePerksContainer(false)
        let eq = find(cx, "UI_Perks_Equipper");
        cx.visit::<PerksEquipper, _>(eq, |e, _| {
            e.perks_saved_event.add(Handler { owner: me, id: PN_PERKS_SAVED });
            e.exited_event.add(Handler { owner: me, id: PN_PERKS_EXITED });
        });
        self.update_appearance(cx);
        cx.view.delay(0.2, Handler { owner: me, id: PN_FIRST_CONTEXT });
    }

    /// PreConstruct @11650: SlotAsCanvasSlot(Overlay_ClipBounds).SetSize((1720, 0)).
    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        let so = Self::slide_out(cx);
        let o = find_in(cx, so, "Overlay_ClipBounds");
        let m = &mut wd(cx, o).offsets;
        *m = Margin { r: 1720.0, b: 0.0, ..*m };
    }

    /// Deviation: the perk page fills the window: PerksScalebox's SizeBox_0.
    fn construct(&mut self, cx: &mut Cx<'_>) {
        let ps = find(cx, "PerksScalebox");
        let b = find_in(cx, ps, "SizeBox_0");
        fill_page(cx, b);
    }

    fn tick(&mut self, cx: &mut Cx<'_>, _dt: f64) {
        let ps = find(cx, "PerksScalebox");
        let b = find_in(cx, ps, "SizeBox_0");
        fill_page(cx, b);
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        match id {
            // OnModalDismiss @13425: this modal dismissed by the manager itself (another section's click) goes to
            // StepBack @7350.
            // Deviation: the port's manager has removed and destructed the panel's widget by then: what StepBack
            // would change (a chooser, the fighter page, Close's slide-out) is no longer drawn, so only the
            // unbinding runs.
            PN_MODAL_DISMISSED => {
                if arg.downcast_ref::<String>() == Some(&self.base.modal_id) {
                    self.unbind(cx);
                }
            }
            PN_CHARACTER | PN_CAROUSEL => {
                if let Some(c) = arg.downcast_ref::<RosterEntry>() {
                    self.set_character(cx, Some(c.clone()));
                }
            }
            PN_CAROUSEL_BACK | PN_BACK => self.step_back(cx),
            PN_TRAIN => {
                // TrainPerksEvent @13025: ShowModal-Generic(UI_Perks_Trainer, "perksTrainer", "", Character). The
                // port's ExtraData is a string: the character's slug. (Not reached offline: buttonTrain is not
                // interactable.)
                if let Some(c) = arg.downcast_ref::<RosterEntry>() {
                    Manager::show_modal(cx, "UI_Perks_Trainer", "perksTrainer", "", &c.slug);
                }
            }
            PN_EDIT => {
                if let Some((c, slot)) = arg.downcast_ref::<(RosterEntry, i32)>() {
                    self.edit_perks(cx, c, *slot);
                }
            }
            PN_DISMISSER => {
                let eq = find(cx, "UI_Perks_Equipper");
                run_on::<PerksEquipper>(cx, eq, |e, cx| e.ready(cx, false));
            }
            PN_GLOBAL_ITEM => {
                if let Some(ev) = arg.downcast_ref::<OverviewPick>() {
                    self.global_item_selected(cx, ev);
                }
            }
            PN_CHARACTER_ITEM => {
                if let Some(ev) = arg.downcast_ref::<OverviewPick>() {
                    self.character_item_selected(cx, ev);
                }
            }
            PN_GLOBAL_HIDE | PN_CHARACTER_HIDE => {
                self.update_appearance(cx);
                let o = find(cx, if id == PN_GLOBAL_HIDE { "UI_GlobalCosmetics" } else { "UI_CharacterCosmetics" });
                cx.visit::<CollectionOverview, _>(o, |o, cx| o.set_all_off(cx));
            }
            // the choosers' SelectedEvent @8202 / @12521: the overview's SetCharacter(Character) (its tiles again),
            // the last overview tile's Set On(true)
            PN_GLOBAL_SELECTED | PN_CHARACTER_SELECTED => {
                let o = find(cx, if id == PN_GLOBAL_SELECTED { "UI_GlobalCosmetics" } else { "UI_CharacterCosmetics" });
                let c = self.character.clone();
                cx.visit::<CollectionOverview, _>(o, |o, cx| o.set_character(cx, c.as_ref()));
                if let Some(t) = self.last_overview_item {
                    run_on::<EquipperItem>(cx, t, |t, cx| t.set_on(cx, true));
                }
                self.hold_tile_pictures(cx);
            }
            PN_PERKS_SAVED => {
                let p = find(cx, "UI_CollectionPerks");
                let c = self.character.clone();
                cx.visit::<CollectionPerks, _>(p, |p, cx| p.set_character(cx, c.as_ref()));
            }
            PN_PERKS_EXITED => self.toggle_perks_container(cx, false),
            _ => {}
        }
    }

    fn resume(&mut self, cx: &mut Cx<'_>, id: u32) {
        match id {
            PN_FIRST_CONTEXT => {
                let g = cx.view.manager.new_guid();
                self.update_context(cx, &g);
                let chars = find(cx, "UI_CollectionCharacters");
                cx.visit::<CollectionCharacters, _>(chars, |c, cx| c.focus_first(cx));
            }
            PN_CLOSE => {
                let so = Self::slide_out(cx);
                let mid = self.base.modal_id.clone();
                cx.visit::<GlobalSlideOut, _>(so, |s, cx| s.close_slide_out(cx, &mid));
                self.set_character(cx, None);
                self.unbind(cx);
            }
            _ => {}
        }
    }
}

impl CollectionsPanel {
    /// GetInteractors: Btn_Back's; UpdateContext @3198.
    fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str) {
        self.base.context = ctx.to_string();
        let back = find(cx, "Btn_Back");
        let i = ib(cx, back);
        batch_set_context(cx, &[i], ctx);
        let so = Self::slide_out(cx);
        let c = ctx.to_string();
        cx.visit::<GlobalSlideOut, _>(so, |s, cx| s.update_context(cx, &c));
        macro_rules! upd {
            ($name:literal, $ty:ty) => {{
                let w = find(cx, $name);
                let c = ctx.to_string();
                cx.visit::<$ty, _>(w, |o, cx| o.update_context(cx, &c));
            }};
        }
        upd!("UI_CollectionCharacterInfo", CollectionCharacterInfo);
        upd!("UI_GlobalCosmetics", CollectionOverview);
        upd!("UI_CharacterCosmetics", CollectionOverview);
        upd!("UI_CollectionCharacters", CollectionCharacters);
        for n in ["UI_CharacterCollectionChooser", "UI_GlobalCollectionChooser"] {
            let w = find(cx, n);
            let c = ctx.to_string();
            cx.visit::<CollectionChooser, _>(w, |o, cx| o.update_context(cx, &c, 0));
        }
        upd!("UI_CollectionPerks", CollectionPerks);
        upd!("UI_CollectionCharacterDetails", CollectionCharacterDetails);
        upd!("UI_CollectionCharacterCarousel", CollectionCharacterCarousel);
        upd!("UI_CollectionBottomBtns", CollectionBottomBtns);
        upd!("UI_CollectionStats", CollectionStats);
        upd!("UI_CollectionXPBoost", CollectionXPBoost);
    }
}

fn set_color_named(cx: &mut Cx<'_>, name: &str, c: Rgba) {
    let w = find(cx, name);
    wd(cx, w).color = c;
}

// ---------- UI_Perks_Trainer (the modal `perksTrainer`) ----------

const PT_DONE: u32 = 1;
const PT_TRAIN: u32 = 2;
const PT_CANCEL: u32 = 3;
const PT_CATEGORY: u32 = 4;
const PT_SCROLL: u32 = 5;

const PURCHASE_LIMIT_DEFAULT: f64 = 15.0;
const TRAINER_LISTS: [&str; 3] = ["listThisCharacter", "listOtherCharacters", "listRest"];

/// The texts, the category tabs and the exit. Not ported: the perk lists' perks (PerkManager), the cart, the price
/// and the purchase (TrainPerks), the gold balance (UI_CurrencyLabel), the celebration, the tabs' UpdateQuadShape
/// (@260), the visited slugs of the preferences beyond the session's (@8317-8639).
#[derive(Default)]
pub struct PerksTrainer {
    base: ModalBase,
    character: Option<RosterEntry>,
}

impl PerksTrainer {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::<PerksTrainer>::default()
    }

    /// SetCharacter @6335.
    fn set_character(&mut self, cx: &mut Cx<'_>, c: RosterEntry) {
        // Format(EX_TextConst, character = DisplayName) @6592 / @6979 (Game.locres)
        let title = cx.view.game_text(TRAINER_TITLE_TEXT).replace("{character}", &c.display_name);
        set_text(cx, "titleLabel", &title);
        let l = find(cx, "listThisCharacter");
        let h = cx.view.game_text(TRAINER_KNOWN_TEXT).replace("{character}", &c.display_name);
        cx.visit::<PerksSublist, _>(l, |l, cx| l.set_header_text(cx, &h));
        self.character = Some(c);
        self.refresh_lists(cx);
    }

    /// Refresh Lists @8011 -> @7141: every list's perks (none here).
    fn refresh_lists(&mut self, cx: &mut Cx<'_>) {
        for n in TRAINER_LISTS {
            let l = find(cx, n);
            cx.visit::<PerksSublist, _>(l, |l, cx| l.set_perks(cx, Vec::new()));
        }
    }

    /// Enter @8055.
    fn enter(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        cx.view.stop_all_animations(me);
        cx.view.play(me, "AnimIn", false, 1.0);
        wd(cx, me).vis = Vis::SelfHitTestInvisible;
        self.set_current_category(cx, 0, false);
        let ci = find(cx, "contextInteractor");
        set_focused(cx, ci, true);
        self.toggle_busy(cx, false);
        let g = cx.view.manager.new_guid();
        self.update_context(cx, &g);
        let done = find(cx, "buttonDone");
        let i = ib(cx, done);
        set_focused(cx, i, false);
        // @8317-8639: the trainer is visited (perksTrainerHasBeenVisited and the fighter's slug)
        if let Some(c) = &self.character {
            trainer_visited().lock().unwrap_or_else(std::sync::PoisonError::into_inner).insert(c.slug.clone());
        }
        self.toggle_purchase_info(cx, false);
    }

    /// SetCurrentCategory(Category, All Mode) @5087.
    fn set_current_category(&mut self, cx: &mut Cx<'_>, category: usize, all_mode: bool) {
        let id = if all_mode { "all" } else { ["offense", "defense", "utility"].get(category).copied().unwrap_or("") };
        let bar = find(cx, "UI_SectionBar_Category");
        cx.visit::<SectionBar, _>(bar, |b, cx| b.set_current_index_by_id(cx, id));
        self.refresh_lists(cx);
    }

    /// ToggleBusy @5483.
    fn toggle_busy(&mut self, cx: &mut Cx<'_>, busy: bool) {
        cond_named(cx, "BusyIndicator", busy, Vis::Visible, Vis::Hidden);
    }

    /// Toggle Purchase Info @10534.
    fn toggle_purchase_info(&mut self, cx: &mut Cx<'_>, on: bool) {
        cond_named(cx, "actionsNormal", on, Vis::Collapsed, Vis::SelfHitTestInvisible);
        cond_named(cx, "actionsDiv", on, Vis::Collapsed, Vis::SelfHitTestInvisible);
        cond_named(cx, "actionsTrain", on, Vis::SelfHitTestInvisible, Vis::Collapsed);
        cond_named(cx, "actionsBG_1", on, Vis::SelfHitTestInvisible, Vis::Hidden);
    }

    /// UpdateContext @5862.
    fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str) {
        self.base.context = ctx.to_string();
        let mut l: Vec<WidgetId> = vec![find(cx, "contextInteractor"), find(cx, "buttonCancelCelebration")];
        for n in ["buttonDone", "buttonCancelTrainPerk", "buttonTrainPerk"] {
            let b = find(cx, n);
            l.push(ib(cx, b));
        }
        batch_set_context(cx, &l, ctx);
        for n in TRAINER_LISTS {
            let w = find(cx, n);
            let c = ctx.to_string();
            cx.visit::<PerksSublist, _>(w, |s, cx| s.update_context(cx, &c));
        }
        let bar = find(cx, "UI_SectionBar_Category");
        let c = ctx.to_string();
        cx.visit::<SectionBar, _>(bar, |b, cx| b.update_context(cx, &c, None));
    }

    /// buttonDone @3395 -> Exit @8691 -> RequestDismiss("", false).
    fn exit(&mut self, cx: &mut Cx<'_>) {
        let b = find(cx, "BusyIndicator");
        if is_visible(cx, b) {
            return;
        }
        self.request_dismiss(cx, "", false);
    }

    fn fit(cx: &mut Cx<'_>) {
        let sb = find(cx, "ScaleBox_0");
        let b = find_in(cx, sb, "SizeBox_2");
        fill_page(cx, b);
    }
}

impl Modal for PerksTrainer {
    fn pre_show(&mut self, cx: &mut Cx<'_>, id: &str) {
        self.base.pre_show(cx, id);
    }

    /// PostShow @11180: ExtraData is the character (the port: its slug); SetCharacter and Enter.
    fn post_show(&mut self, cx: &mut Cx<'_>, _instructions: &str, extra_data: &str) {
        let Some(c) = cx.view.roster().iter().find(|r| r.slug == extra_data).cloned() else { return };
        self.set_character(cx, c);
        self.enter(cx);
    }

    fn pre_dismiss(&mut self, cx: &mut Cx<'_>) {
        self.base.pre_dismiss(cx);
    }

    fn post_dismiss(&mut self, cx: &mut Cx<'_>) {
        self.base.post_dismiss(cx);
    }

    /// RequestDismiss @10852: a cancel with the purchase info up closes that; else DismissModal.
    fn request_dismiss(&mut self, cx: &mut Cx<'_>, _reason: &str, canceling: bool) {
        let a = find(cx, "actionsTrain");
        if canceling && is_visible(cx, a) {
            self.toggle_purchase_info(cx, false);
            return;
        }
        let id = self.base.modal_id.clone();
        Manager::dismiss_modal(cx, &id);
    }

    fn get_modal_context(&self) -> String {
        self.base.context.clone()
    }
}

impl Controller for PerksTrainer {
    fn as_modal(&mut self) -> Option<&mut dyn Modal> {
        Some(self)
    }

    /// OnInitialized @4112: PreConstrust @3467 (the first list at half opacity), AddEventBindings @4571, the
    /// instructions.
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        let l = find(cx, "listThisCharacter");
        let list = find_in(cx, l, "list");
        wd(cx, list).opacity = 0.5;
        let limit =
            cx.view.tree().props(cx.w).get("Purchase Limit").and_then(Value::as_f64).unwrap_or(PURCHASE_LIMIT_DEFAULT);
        // Format(EX_TextConst, perkLimit = Purchase Limit) @4367 (Game.locres)
        let text = cx.view.game_text(TRAINER_INSTRUCTIONS_TEXT).replace("{perkLimit}", &(limit as i64).to_string());
        set_text(cx, "instructionsLabel", &text);
        button_click(cx, "buttonDone", false, PT_DONE);
        button_click(cx, "buttonTrainPerk", false, PT_TRAIN);
        button_click(cx, "buttonCancelTrainPerk", true, PT_CANCEL);
        let bar = find(cx, "UI_SectionBar_Category");
        cx.visit::<SectionBar, _>(bar, |b, _| b.on_click.add(Handler { owner: me, id: PT_CATEGORY }));
        cx.view.manager.on_scroll.add(Handler { owner: me, id: PT_SCROLL });
    }

    fn destruct(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        cx.view.manager.on_scroll.remove(Handler { owner: me, id: PT_SCROLL });
    }

    /// Deviation: the trainer fills the window: its ScaleBox_0's SizeBox_2.
    fn construct(&mut self, cx: &mut Cx<'_>) {
        Self::fit(cx);
    }

    fn tick(&mut self, cx: &mut Cx<'_>, _dt: f64) {
        Self::fit(cx);
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        match id {
            PT_DONE => self.exit(cx),
            PT_TRAIN => inert("perksTrainer:buttonTrainPerk"),
            PT_CANCEL => self.toggle_purchase_info(cx, false),
            // the bar's OnClick @9144
            PT_CATEGORY => {
                let Some((_, id)) = arg.downcast_ref::<(i32, String)>() else { return };
                match ["offense", "defense", "utility", "all"].iter().position(|k| *k == id.to_lowercase()) {
                    Some(3) => self.set_current_category(cx, 0, true),
                    Some(at) => self.set_current_category(cx, at, false),
                    None => {}
                }
            }
            // OnScroll @4902
            PT_SCROLL => {
                let Some(&(_, _, amount)) = arg.downcast_ref::<(i32, i32, f64)>() else { return };
                let me = cx.w;
                if cx.view.visible_in_hierarchy(me) {
                    let sb = find(cx, "perksScrollbox");
                    scroll_list(cx.view.tree_mut(), sb, amount);
                }
            }
            _ => {}
        }
    }
}
