//! The main menu's PLAY screen: WBP_MatchOptions (the modal `matchoptions`)
//! with its cards (WBP_GameModeCard) and its Style bar's tabs (UI_SectionBarHorizontalMenusRework2), the advanced
//! tutorials (WBP_SubOptionModal `SubOption` over WBP_TutorialType tiles), the ranked help
//! (UI_RankedExplanationModal `rankedExplanation`, UI_RankedTierOverview) and the server prompt
//! (UI_ServerSettingsPrompt `serverSettings`). WBP_TextTag (a card's tag) is the badges' module's.
//!
//! Every screen opens, shows and closes; what needs the backend, a match or a purchase warns once per button and does
//! nothing. Deviation: the port takes PGI_AuthHandler.IsConnected? as true wherever a graph reads it.
//!
//! Owed (R7, online): the queue behind Confirm (Lobby.queue and EnterPreMatch), the party (never full
//! offline, so PARTY_OFF_CARDS stay as the style leaves them) and the net process behind the Join Lobby card
//! (online.canJoin, false here: the card keeps its disabled look). Owed (the arcade track): an arcade card's run and
//! pre-match (arcadeStart). Each warns once.
//! Not ported: showCrossplayPromptIfUnset and the server prompt on the way in, which ASK_SERVER_AND_CROSSPLAY keeps
//! off, so the crossplay dialog is never made.

use std::any::Any;
use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

use serde_json::{Value, json};

use super::common::{ModalBase, fill_page, palette_entry, text};
use super::controls::bar_buttons::{BarButton, P_LABEL};
use super::controls::buttons::{ButtonMain, DARK, IconButton, WHITE};
use super::controls::drop_menu::DropMenu;
use super::controls::interactor::{
    FocusedCommon, InteractorState, batch_set_context, batch_set_scroll_box, set_focused, set_interactable,
};
use super::controls::parallax_items::{ParallaxItem, ParallaxItems};
use super::controls::{bind, find, ist, prop, quad_color, quad_wobble, wd};
use super::home::Homescreen;
use super::modals::show_modal_generic;
use super::section_bar::SectionBar;
use crate::bundle::{Bundle, Rgba, WData};
use crate::menu::arcade::ARCADE_EXPERT;
use crate::menu::controller::{Controller, Cx, Event, Handler};
use crate::menu::cursor::with_cursor;
use crate::menu::custom_text;
use crate::menu::manager::{Manager, Modal};
use crate::menu::online::OnlineCmd;
use crate::menu::screens::lab_selectors::{GridItemList, Tile, TileData, TileEvent};
use crate::menu::scroll::scroll_list;
use crate::menu::tutorial::{INTRO_TUTORIAL, TUTORIAL_MAP, launch_tutorial};
use crate::menu::view::MenuView;
use crate::menu::widgets::controls::grid_item::{GridItem, GridItemState};
use crate::slate::{HAlign, Mid, VAlign, Vis, WidgetId};
use mvs_protocol::online::QueueMode;
use mvs_protocol::queue_rules::QUEUE_MODES;

/// One console warning per button id for what the port does not do.
pub fn warn_once(id: &str, what: &str) {
    static W: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    let mut w = W.get_or_init(|| Mutex::new(HashSet::new())).lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if w.insert(id.to_string()) {
        eprintln!("menu: {what} ({id}) needs the backend or a match the port does not have");
    }
}

/// The UI_ServerSettingsPrompt save (QAServerRegion2). Deviation: kept for the process's session only.
fn server_region() -> &'static Mutex<Option<String>> {
    static R: OnceLock<Mutex<Option<String>>> = OnceLock::new();
    R.get_or_init(|| Mutex::new(None))
}

/// A brush's resource by its bundle name: a UI material's object path, else a texture's entry (slate.rs resolve).
fn brush(b: &Bundle, name: &str) -> Option<String> {
    b.material_path(name).map(str::to_string).or_else(|| b.textures.get(name).map(|t| t.entry().to_string()))
}

/// An enumerator's name without its enum's prefix.
fn enum_tail(v: &Value) -> &str {
    v.as_str().map_or("", |s| s.rsplit("::").next().unwrap_or(s))
}

fn set_vis(cx: &mut Cx<'_>, name: &str, v: Vis) {
    let w = find(cx, name);
    wd(cx, w).vis = v;
}

fn interactor_in(cx: &Cx<'_>, w: WidgetId) -> WidgetId {
    cx.view.tree().find_in(w, "UI_Interactor")
}

// ---------- WBP_GameModeCard ----------

/// UI_GameModeCardSize: the byte of each enumerator (the asset's Names map); 0 XSmall .. 4 Full.
fn card_size(e: &str) -> Option<usize> {
    match e {
        "NewEnumerator4" => Some(0),
        "NewEnumerator0" => Some(1),
        "NewEnumerator3" => Some(2),
        "NewEnumerator1" => Some(3),
        "NewEnumerator2" => Some(4),
        _ => None,
    }
}
const BG_SCALE: [&str; 5] = ["BGScale_XSmall", "BGScale_Small", "BGScale_Mid", "BGScale_Large", "BGScale_Full"];
const MODE_BOX_W: [f64; 5] = [600.0, 700.0, 700.0, 800.0, 800.0];
pub const CARD_W: [f64; 5] = [550.0, 650.0, 750.0, 900.0, 2350.0];
const CARD_H: [f64; 5] = [800.0, 800.0, 800.0, 800.0, 1000.0];
const CARD_H_UNFOCUSED: [f64; 5] = [800.0, 800.0, 800.0, 800.0, 860.0];
/// Deviation: the build's Normal row (five XSmall cards, 5 x (550 + 10) = 2800 units) overflows ScrollBox_0's 2560.
/// The port's row has four (CO-OP VS AI removed, REMOVED_CARDS): 4 x (550 + 10) = 2240, moved 160 in
/// (ModeStyleSelected's own SetRenderTranslation, as BOTS does) so it sits centred. The Practice row
/// (modeCardsContainerOffline, 2440 units of cards) is centred the same way, where the build leaves it at the
/// translation the last style set (0 or 75: a corner off one edge or the other).
pub const NORMAL_CARD_W: f64 = 550.0;
const NORMAL_ROW_X: f64 = 160.0;
const PRACTICE_ROW_X: f64 = 55.0;

/// EGameMode as the card's switches read it. Unverified: the order is taken from the switches' cases (Solo 0, Teams
/// 1, FreeForAll 2, CoopVsAI 3, Training 4, guild 5); Null takes their default branch.
fn game_mode(e: &str) -> i32 {
    match e {
        "Solo" => 0,
        "Teams" => 1,
        "FreeForAll" => 2,
        "CoopVsAI" => 3,
        "Training" => 4,
        _ => -1,
    }
}
/// SetCardTitle's titles by game mode (@283-614): 1 VS 1, TEAMS, FFA, CO-OP VS AI, The Lab, Guild Dungeons.
const MODE_TITLES: [&str; 6] = [
    "C1413E164DB8DCE5DC2CC19A2E4AEBA5",
    "720C33654366ACE4376C668A91B54234",
    "1789A04E4D313B3F325D3485EB970175",
    "7ECCDD29495ACF160A8075A6A6E9284F",
    "5B67710A4E49F1191D882881BE49BA48",
    "BB0134F34BE80AD9409367A798038041",
];
/// SetCardTitle's overrides (@1291-3693): the title's key, and the subtitle "" where the override sets one.
fn override_title(o: &str) -> Option<(&'static str, Option<&'static str>)> {
    Some(match o {
        "BeginnerTutorial" => ("AD52E96E4755BBAB6C29399627551AF6", None), // Beginner Tutorial
        "AdvancedTutorial" => ("B6B3CE9C4AD0A253910D7B8EB9637D69", None), // Advanced Tutorials
        "CharacterTutorial" => ("D73932DF45CF0638917219B96DA727FD", None), // Character Tutorials
        "CreateLobby" => ("AA1FB30946DAEA27D055589B1824E9B9", None),      // Create Online Lobby
        "JoinLobby" => ("D55C2EE74ADE85593595F5AA556BEDB2", None),        // Join Lobby
        "guilddungeon" => ("E264A904459A4BC582904ABE8CC2278F", None),     // Guild Dungeon
        "Local" => ("3968777043BA4F19D152A49B3FF60F68", Some("")),        // Local Play
        _ => return None,
    })
}
/// SetCardTitle's Casual title (@3969, Silly Queue) and its empty subtitle (@4056).
const CASUAL_TITLE: &str = "95F2CC7145437E299415B0A1BFEBBF98";
/// SetCardTitle's arcade overrides (@2698-3450): the title and subtitle keys.
fn arcade_titles(o: &str) -> Option<(&'static str, &'static str)> {
    Some(match o {
        "ArcadeEasy" => ("E332E75C4858C6F577B7B3A7B194585E", "343B221A43DBCF911B176AB61CC62EE0"), // Easy, 3 Stages - Unlimited Tries
        "ArcadeNormal" => ("B806799F4EB2FFB76314CCBB0A4DE665", "70FDF8DB4DE046A48108EB9FC44FD7B9"), // Medium, 5 Stages - 3 Tries
        "ArcadeHard" => ("F39BDE90481CF97CA8A290859ACF18C5", "78EB00F649C85206E1B6288AE019EA6E"), // Hard, 8 Stages - 1 Try
        _ => return None,
    })
}
/// GameModeSelected's arcade overrides (@52364-52476): the run's EArcadeDifficulty.
fn arcade_difficulty(o: &str) -> Option<u8> {
    match o {
        "arcadeeasy" => Some(1),
        "arcadenormal" => Some(2),
        "arcadehard" => Some(3),
        "arcadeexpert" => Some(ARCADE_EXPERT),
        _ => None,
    }
}
const CHECK_ON: Rgba = [1.0, 0.558341, 0.0, 1.0];
const CHECK_OFF: Rgba = [0.262251, 0.366253, 0.396755, 1.0];
const CARD_TEXT_ON: Rgba = [1.0, 1.0, 1.0, 1.0];
const CARD_TEXT_OFF: Rgba = [0.262251, 0.366253, 0.396755, 1.0];
const CARD_BG: &str = "MI_TextureColorBlend_BG";
/// BackgroundArt's Texture parameter: PreConstruct @4120-4588 (BG Texture Override when set, else by Casual /
/// GameMode), then SetCardTitle's overrides (it runs after, @5183).
const BG_BY_MODE: [&str; 5] =
    ["UI_GameMode_1vs1_BG", "UI_GameMode_2vs2_BG", "MapBG_ScoobyMansion", "MapBG_TreeFort", "MapBG_TrainingRoom"];
fn bg_by_override(o: &str) -> Option<&'static str> {
    Some(match o {
        "BeginnerTutorial" | "CreateLobby" => "UI_ModeSelect_BG_1",
        "AdvancedTutorial" | "JoinLobby" => "UI_ModeSelect_BG_2",
        "guilddungeon" => "ui_icons_guild",
        "ArcadeEasy" => "GameMode_AXEasy_BG",
        "ArcadeNormal" => "GameMode_AXMedium_BG",
        "ArcadeHard" | "Local" | "Casual" => "GameMode_AXHard_BG",
        EXPERT_CARD => "UI_ModeSelect_BG_1",
        _ => return None,
    })
}

/// The texture PreConstruct and SetCardTitle leave on a card's BackgroundArt, by its props (None: Guild Dungeons'
/// GameMode 5).
pub fn card_bg_texture(p: &Value) -> Option<String> {
    let o = p.get("ModeOverride").and_then(Value::as_str).unwrap_or_default();
    if let Some(t) = bg_by_override(o) {
        return Some(t.to_string());
    }
    // (no card of the tree sets BG Texture Override; Casual's UI_GameMode_silly_BG is replaced by SetCardTitle's)
    if let Some(over) = p.get("BG Texture Override").and_then(Value::as_str).filter(|s| !s.is_empty()) {
        let s = over.trim_end_matches('\'');
        return Some(s.rsplit(['\'', '.']).next().unwrap_or(s).to_string());
    }
    let g = game_mode(enum_tail(p.get("GameMode").unwrap_or(&Value::Null)));
    usize::try_from(g).ok().and_then(|i| BG_BY_MODE.get(i)).map(|s| s.to_string())
}

/// (the card, its mode, its override, IsToggle?, and its selection and enabled state after the click: the card is
/// busy while it fires, so the screen reads them here).
#[derive(Clone, Debug)]
pub struct CardSelected {
    pub card: WidgetId,
    pub mode: i32,
    pub mode_override: String,
    pub is_toggle: bool,
    pub selected: bool,
    pub enabled: bool,
}

const C_FOCUS: u32 = 1;
const C_UNFOCUS: u32 = 2;
const C_CLICK: u32 = 3;

/// WBP_GameModeCard. Not ported: WBP_OccluderSlants (Collapsed in the tree; its UpdateSize /
/// UpdateColor change nothing seen), the cursor reads of Select / Deselect and SizeBox_1's SetClipping (the tree's
/// already). A background texture the bundle lacks leaves the material's own (UI_GameMode_2vs2_BG).
#[derive(Debug)]
pub struct GameModeCard {
    /// OnFocusedCard(Card) / OnUnfocusedCard(Card) / OnSelectedCard(Game Mode, Mode Override, IsToggle).
    pub on_focused_card: Event<WidgetId>,
    pub on_unfocused_card: Event<WidgetId>,
    pub on_selected_card: Event<CardSelected>,
    pub is_selected: bool,
    pub is_enabled: bool,
    size_type: usize,
    focus_throttle_timer: f64,
    made_materials: bool,
}

impl GameModeCard {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(GameModeCard {
            on_focused_card: Event::default(),
            on_unfocused_card: Event::default(),
            on_selected_card: Event::default(),
            is_selected: false,
            is_enabled: true,
            size_type: 3,
            focus_throttle_timer: 1.0,
            made_materials: false,
        })
    }

    fn p(cx: &Cx<'_>) -> Value {
        cx.view.tree().props(cx.w).clone()
    }

    fn game_mode(cx: &Cx<'_>) -> i32 {
        game_mode(enum_tail(&prop(cx, "GameMode")))
    }

    fn mode_override(cx: &Cx<'_>) -> String {
        prop(cx, "ModeOverride").as_str().unwrap_or_default().to_string()
    }

    fn is_toggle(cx: &Cx<'_>) -> bool {
        prop(cx, "IsToggle?").as_bool() != Some(false)
    }

    fn mat_scalar(cx: &mut Cx<'_>, image: &str, k: &str, v: f32) {
        let w = find(cx, image);
        if let Some(m) = &mut wd(cx, w).mid {
            m.set_scalar(k, v);
        }
    }

    fn materials(cx: &mut Cx<'_>, bg: f32, char1: f32) {
        Self::mat_scalar(cx, "BackgroundArt", "UseFullColor", bg);
        Self::mat_scalar(cx, "Char1", "UseFullColor", char1);
    }

    /// Set Card Size(Size); width: the port's own narrower card (NORMAL_CARD_W), its title box narrower by as much.
    pub fn set_card_size(&mut self, cx: &mut Cx<'_>, size: usize, width: f64) {
        self.size_type = size;
        let m = find(cx, "ModeSizeBox");
        wd(cx, m).w_override = MODE_BOX_W[size] - (CARD_W[size] - width);
        let b = find(cx, "SizeBox_1");
        let x = wd(cx, b);
        x.w_override = width;
        x.h_override = CARD_H[size];
        self.update_aspect(cx);
    }

    /// UpdateCharTextureAspectRatio.
    fn update_aspect(&self, cx: &mut Cx<'_>) {
        if prop(cx, "MaintainAspectRatio?").as_bool() == Some(false) {
            return;
        }
        let b = find(cx, "SizeBox_1");
        let r = {
            let x = cx.view.tree().widget(b);
            x.h_override / x.w_override
        };
        let ct = find(cx, "CharTextures");
        wd(cx, ct).transform.scale = [r, 1.0];
        let s = prop(cx, BG_SCALE[self.size_type]);
        let sx = s.get("X").and_then(Value::as_f64).unwrap_or(1.0);
        let sy = s.get("Y").and_then(Value::as_f64).unwrap_or(1.0);
        let bg = find(cx, "BackgroundArt");
        wd(cx, bg).transform.scale = [r * sx, sy];
    }

    /// SetCardTitle.
    fn set_card_title(cx: &mut Cx<'_>) {
        let mode = find(cx, "Mode");
        let o = Self::mode_override(cx);
        if o.is_empty() {
            let g = Self::game_mode(cx);
            let key = usize::try_from(g).ok().and_then(|i| MODE_TITLES.get(i));
            let t = key.map(|k| cx.view.game_text(k).to_string()).unwrap_or_default();
            let w = wd(cx, mode);
            w.text = t;
            w.auto_wrap = g > 1;
            return;
        }
        wd(cx, mode).auto_wrap = true;
        let sub = find(cx, "SubtitleText");
        if o == EXPERT_CARD {
            let (_, s) = arcade_titles("ArcadeHard").expect("the Hard card's titles");
            let s = cx.view.game_text(s).to_string();
            wd(cx, mode).text = custom_text::EXPERT.to_string();
            wd(cx, sub).text = s;
            return;
        }
        if let Some((t, s)) = arcade_titles(&o) {
            let (t, s) = (cx.view.game_text(t).to_string(), cx.view.game_text(s).to_string());
            wd(cx, mode).text = t;
            wd(cx, sub).text = s;
            return;
        }
        if o == "Casual" {
            let t = cx.view.game_text(CASUAL_TITLE).to_string();
            wd(cx, mode).text = t;
            wd(cx, sub).text = String::new();
            return;
        }
        let Some((t, s)) = override_title(&o) else { return };
        let t = cx.view.game_text(t).to_string();
        wd(cx, mode).text = t;
        if let Some(s) = s {
            wd(cx, sub).text = s.to_string();
        }
    }

    /// OnFocus / OnUnfocus: UI_Functions.TimeIntervalCheck(focusThrottleTimer, 0.033333) on real time.
    fn throttle(&mut self, cx: &Cx<'_>) -> bool {
        let now = cx.view.now();
        let d = now - self.focus_throttle_timer;
        if !(0.0..0.033333).contains(&d) {
            self.focus_throttle_timer = now;
            return true;
        }
        false
    }

    /// The cursor's parallax of the card's UI_ParallaxItems (Char1, Char2): on Select (@8641 -> @123-1212) item i's
    /// ParallaxMultiplier is 40 - i x 30, on Deselect and RemoveParallax (@7759 -> @2435-2200) 0; per item its
    /// entries in UI Cursor.Parallaxers removed, then UI_ParallaxItems.AddItem. The multiplier is set on a copy: the
    /// UI_ParallaxItems' own list keeps its 0s.
    fn parallax(cx: &mut Cx<'_>, on: bool) {
        let p = find(cx, "UI_ParallaxItems");
        cx.visit::<ParallaxItems, _>(p, |pi, cx| {
            for (i, &item) in pi.items.iter().enumerate() {
                let multiplier = if on { 40 - 30 * i as i32 } else { 0 };
                if let Some(x) = pi.parallaxer(cx, ParallaxItem { multiplier, ..item }) {
                    with_cursor(cx, move |c| c.replace_parallax_item(x));
                }
            }
        });
    }

    /// Select(Wobble).
    pub fn select(&mut self, cx: &mut Cx<'_>, wobble: bool) {
        let me = cx.w;
        cx.emit(self, |c| &c.on_focused_card, &me);
        self.update_checkmark_backing(cx);
        let q = find(cx, "QuadWobbler");
        quad_wobble(cx, q, wobble);
        let b = find(cx, "SizeBox_1");
        wd(cx, b).h_override = 860.0;
        self.update_aspect(cx);
        Self::materials(cx, 1.0, 1.0);
        let mode = find(cx, "Mode");
        wd(cx, mode).shadow_offset = [5.0, 5.0];
        set_vis(cx, "groundDescription", Vis::SelfHitTestInvisible);
        Self::parallax(cx, true);
    }

    /// Deselect(RemoveWobbleOnly).
    pub fn deselect(&mut self, cx: &mut Cx<'_>, wobble_only: bool) {
        let me = cx.w;
        cx.emit(self, |c| &c.on_unfocused_card, &me);
        self.update_checkmark_backing(cx);
        let q = find(cx, "QuadWobbler");
        quad_wobble(cx, q, false);
        if wobble_only {
            // RemoveParallax @10086
            Self::parallax(cx, false);
            return;
        }
        let b = find(cx, "SizeBox_1");
        wd(cx, b).h_override = CARD_H_UNFOCUSED[self.size_type];
        self.update_aspect(cx);
        Self::materials(cx, 0.25, 0.65);
        set_vis(cx, "UI_InputLabel_185", Vis::Collapsed);
        let full = self.size_type == 4;
        let mode = find(cx, "Mode");
        wd(cx, mode).shadow_offset = if full { [5.0, 5.0] } else { [0.0, 0.0] };
        set_vis(cx, "groundDescription", if full { Vis::HitTestInvisible } else { Vis::Collapsed });
        Self::parallax(cx, false);
    }

    /// The interactor's OnClick.
    fn on_click(&mut self, cx: &mut Cx<'_>) {
        let g = find(cx, "SelectedGroup");
        let toggle = Self::is_toggle(cx);
        if toggle {
            wd(cx, g).vis = Vis::SelfHitTestInvisible;
            self.is_selected = !self.is_selected;
            if self.is_selected {
                self.select(cx, true);
                wd(cx, g).opacity = 1.0;
                cx.post("ui_match_select_click");
                for q in ["CheckmarkBacking", "WBP_Quadrilateral"] {
                    let q = find(cx, q);
                    quad_color(cx, q, CHECK_ON);
                }
                Self::materials(cx, 1.0, 1.0);
            } else {
                wd(cx, g).opacity = 0.0;
                cx.post("ui_match_select_unclick");
            }
        } else {
            wd(cx, g).vis = Vis::Collapsed;
        }
        let ev = CardSelected {
            card: cx.w,
            mode: Self::game_mode(cx),
            mode_override: Self::mode_override(cx),
            is_toggle: toggle,
            selected: self.is_selected,
            enabled: self.is_enabled,
        };
        cx.emit(self, |c| &c.on_selected_card, &ev);
    }

    pub fn update_checkmark_backing(&self, cx: &mut Cx<'_>) {
        let c = if self.is_selected && self.is_enabled { CHECK_ON } else { CHECK_OFF };
        for q in ["CheckmarkBacking", "WBP_Quadrilateral"] {
            let q = find(cx, q);
            quad_color(cx, q, c);
        }
    }

    /// ToggleDisable(Enabled) with DisabledCardsAppearance.
    pub fn toggle_disable(&mut self, cx: &mut Cx<'_>, enabled: bool) {
        self.is_enabled = enabled;
        let f = if enabled { 1.0 } else { 0.0 };
        Self::materials(cx, f, f);
        let c = if enabled { CARD_TEXT_ON } else { CARD_TEXT_OFF };
        let mode = find(cx, "Mode");
        wd(cx, mode).color = c;
        let t = text(Some(&prop(cx, "Disabled Text")));
        let msg = find(cx, "DisableMessage");
        let m = wd(cx, msg);
        m.color = c;
        m.vis = if enabled { Vis::Collapsed } else { Vis::HitTestInvisible };
        m.text = t;
        let i = find(cx, "UI_Interactor");
        set_interactable(cx, i, enabled);
    }

    /// Toggle Locked(Locked).
    pub fn toggle_locked(&mut self, cx: &mut Cx<'_>, locked: bool) {
        set_vis(cx, "LockedIndicator", if locked { Vis::SelfHitTestInvisible } else { Vis::Collapsed });
        let t = text(Some(&prop(cx, "Locked Text")));
        let l = find(cx, "labelLockedText");
        wd(cx, l).text = t;
    }

    pub fn set_bots(&mut self, cx: &mut Cx<'_>, b: bool) {
        set_vis(cx, "SubtitleContainer", if b { Vis::SelfHitTestInvisible } else { Vis::Hidden });
    }

    pub fn set_arcade_subtitle(&mut self, cx: &mut Cx<'_>, b: bool) {
        set_vis(cx, "SubtitleContainer", if b { Vis::SelfHitTestInvisible } else { Vis::Hidden });
    }

    pub fn show_experimental_tag(&mut self, cx: &mut Cx<'_>) {
        set_vis(cx, "UI_PM_Experimental", Vis::SelfHitTestInvisible);
    }

    pub fn enter_anim(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        cx.view.play(me, "Enter", false, 1.0);
    }

    /// ManualFocus: the interactor's OnFocus broadcast, then the wobble off. (The card's own handler of the event
    /// runs here at once, as `Cx::emit` hands a controller its own; the others are called.)
    pub fn manual_focus(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        let i = find(cx, "UI_Interactor");
        let handlers = ist(cx, i).map(|s| s.on_focus.handlers().to_vec()).unwrap_or_default();
        for h in handlers {
            if h.owner == me {
                self.handle(cx, h.id, &0i32);
            } else {
                cx.call(h, &0i32);
            }
        }
        let q = find(cx, "QuadWobbler");
        quad_wobble(cx, q, false);
        // RemoveParallax @10334
        Self::parallax(cx, false);
    }
}

impl Controller for GameModeCard {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let i = find(cx, "UI_Interactor");
        bind(cx, i, |s: &mut InteractorState| &mut s.on_focus, C_FOCUS);
        bind(cx, i, |s: &mut InteractorState| &mut s.on_unfocus, C_UNFOCUS);
        bind(cx, i, |s: &mut InteractorState| &mut s.on_click, C_CLICK);
    }

    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        let p = Self::p(cx);
        self.size_type = card_size(enum_tail(p.get("CardSizeType").unwrap_or(&Value::Null))).unwrap_or(3);
        // each Image with its own dynamic instance (GetDynamicMaterial), made once. SetTextureParameterValue on
        // BackgroundArt's: the card's texture when the bundle has it.
        if !self.made_materials {
            self.made_materials = true;
            let b = cx.view.bundle.clone();
            let bg = find(cx, "BackgroundArt");
            if let Some(path) = b.material_path(CARD_BG) {
                let mut m = Mid::of(path);
                if let Some(t) = card_bg_texture(&p).and_then(|t| b.textures.get(&t)) {
                    m.set_texture("Texture", t.entry());
                }
                let x = wd(cx, bg);
                x.tex = Some(path.to_string());
                x.mid = Some(m);
            }
            let c1 = find(cx, "Char1");
            let mc = p.get("MaterialChar").and_then(Value::as_str).unwrap_or_default();
            if let Some(path) = brush(&b, mc) {
                let x = wd(cx, c1);
                x.mid = b.has_material(&path).then(|| Mid::of(&path));
                x.tex = Some(path);
            }
        }
        self.toggle_disable(cx, p.get("IsEnabled").and_then(Value::as_bool) != Some(false));
        let f = |k: &str, d: f64| p.get(k).and_then(Value::as_f64).unwrap_or(d) as f32;
        Self::mat_scalar(cx, "BackgroundArt", "XOffset", f("BG_XOffset", 0.0));
        Self::mat_scalar(cx, "BackgroundArt", "YOffset", f("BG_YOffset", 0.0));
        Self::mat_scalar(cx, "BackgroundArt", "Zoom", f("BG_Zoom", 1.0));
        let desc = find(cx, "groundDescription");
        let d = text(p.get("Description"));
        let full = self.size_type == 4;
        let x = wd(cx, desc);
        x.vis = if full { Vis::HitTestInvisible } else { Vis::Collapsed };
        x.text = d;
        let mode = find(cx, "Mode");
        wd(cx, mode).font_size = p.get("TitleFontSize").and_then(Value::as_f64).unwrap_or(56.0);
        let st = self.size_type;
        self.set_card_size(cx, st, CARD_W[st]);
        Self::set_card_title(cx);
        let toggle = Self::is_toggle(cx);
        set_vis(cx, "SelectedGroup", if toggle { Vis::SelfHitTestInvisible } else { Vis::Collapsed });
        set_vis(cx, "Char2", Vis::Collapsed);
        let tag = find(cx, "RecommendedTag");
        let tt = cx.view.tree().find_in(tag, "TagText");
        wd(cx, tt).text = text(p.get("Recommended Text"));
        if Self::game_mode(cx) == 1 && Self::mode_override(cx) != "Casual" {
            wd(cx, desc).transform.translation = [0.0, 0.0];
        }
        let flag = |k: &str| p.get(k).and_then(Value::as_bool).unwrap_or(false);
        wd(cx, tag).vis = if flag("IsRecommended") { Vis::HitTestInvisible } else { Vis::Collapsed };
        set_vis(
            cx,
            "StagesAvailableTag",
            if flag("IsStagesAvialable") { Vis::HitTestInvisible } else { Vis::Collapsed },
        );
    }

    fn construct(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        wd(cx, me).opacity = 0.0;
        let m = find(cx, "ModeSizeBox");
        wd(cx, m).w_override = MODE_BOX_W[self.size_type];
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, _arg: &dyn Any) {
        match id {
            C_FOCUS => {
                if self.is_enabled && self.throttle(cx) {
                    self.select(cx, true);
                }
            }
            C_UNFOCUS => {
                if self.is_enabled && self.throttle(cx) {
                    let only = self.is_selected && Self::is_toggle(cx);
                    self.deselect(cx, only);
                }
            }
            C_CLICK => self.on_click(cx),
            _ => {}
        }
    }
}

// ---------- UI_SectionBarHorizontalMenusRework2 (a tab of the Style bar: BOTS / NORMAL / RANKED) ----------

const RW2_ON: Rgba = [0.006995, 0.008023, 0.022174, 1.0];
const RW2_BORDER: Rgba = [0.191202, 0.651406, 0.630757, 1.0];
const RW2_BORDER_OFF: Rgba = [0.066667, 0.101961, 0.152941, 1.0];
const RW2_LABEL_OFF: Rgba = [0.066626, 0.104616, 0.155926, 1.0];

const B_FOCUS: u32 = 1;
const B_UNFOCUS: u32 = 2;
const B_INTERACTABLE: u32 = 3;
const B_NOT_INTERACTABLE: u32 = 4;

/// UI_SectionBarHorizontalMenusRework2, from its graphs.
#[derive(Debug, Default)]
pub struct SectionBarHorizontalButton2 {
    is_on: bool,
}

impl SectionBarHorizontalButton2 {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(SectionBarHorizontalButton2::default())
    }

    fn label_color(cx: &mut Cx<'_>, c: Rgba) {
        let l = find(cx, "Label");
        wd(cx, l).color = c;
    }
}

impl BarButton for SectionBarHorizontalButton2 {
    fn init_bar_button(&mut self, cx: &mut Cx<'_>, params: &Value) {
        let me = cx.w;
        cx.view.tree_mut().set_prop(me, "Params", params.clone());
        let l = find(cx, "Label");
        wd(cx, l).text = text(params.get(P_LABEL));
        self.set_on(cx, self.is_on);
    }

    fn set_on(&mut self, cx: &mut Cx<'_>, v: bool) {
        self.is_on = v;
        let s = if v { Vis::SelfHitTestInvisible } else { Vis::Collapsed };
        set_vis(cx, "OnIndic", s);
        set_vis(cx, "OnArrow", s);
        let i = find(cx, "UI_Interactor");
        if !ist(cx, i).is_some_and(|s| s.get_interactable()) {
            return;
        }
        let l = find(cx, "Label");
        let x = wd(cx, l);
        x.font_size = if v { 28.0 } else { 24.0 };
        x.color = if v { RW2_ON } else { WHITE };
    }

    fn is_on(&self) -> bool {
        self.is_on
    }
}

impl Controller for SectionBarHorizontalButton2 {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let i = find(cx, "UI_Interactor");
        bind(cx, i, |s: &mut InteractorState| &mut s.on_focus, B_FOCUS);
        bind(cx, i, |s: &mut InteractorState| &mut s.on_unfocus, B_UNFOCUS);
        bind(cx, i, |s: &mut InteractorState| &mut s.on_interactable, B_INTERACTABLE);
        bind(cx, i, |s: &mut InteractorState| &mut s.on_not_interactable, B_NOT_INTERACTABLE);
    }

    fn construct(&mut self, cx: &mut Cx<'_>) {
        let fc = find(cx, "UI_FocusedCommon");
        let i = find(cx, "UI_Interactor");
        cx.visit::<FocusedCommon, _>(fc, |f, cx| f.set_interactor(cx, i));
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, _arg: &dyn Any) {
        if self.is_on {
            return;
        }
        match id {
            B_FOCUS => Self::label_color(cx, DARK),
            B_UNFOCUS => Self::label_color(cx, WHITE),
            B_INTERACTABLE | B_NOT_INTERACTABLE => {
                let on = id == B_INTERACTABLE;
                let q = find(cx, "QuadBorder");
                quad_color(cx, q, if on { RW2_BORDER } else { RW2_BORDER_OFF });
                Self::label_color(cx, if on { WHITE } else { RW2_LABEL_OFF });
            }
            _ => {}
        }
    }

    fn as_bar_button(&mut self) -> Option<&mut dyn BarButton> {
        Some(self)
    }
}

// ---------- WBP_MatchOptions (the modal `matchoptions`) ----------

/// A card's widget name.
fn c(n: &str) -> String {
    format!("WBP_GameModeCard_{n}")
}

/// (the port's own) The Arcade's Expert card (ARCADE_EXPERT), after Hard: a copy of the Hard card the modal makes
/// (`add_expert_card`), its ModeOverride this, its title the port's own and its subtitle Hard's (the same ladder).
const EXPERT_CARD: &str = "ArcadeExpert";
/// The Expert card's character art: another card's (the character tutorials' Harley), so it is not the Hard card's
/// Shaggy; its background is the Beginner Tutorial card's (`bg_by_override`).
const EXPERT_CHAR: &str = "MI_TextureColorBlend_Chartuts";
/// Deviation: the Arcade row holds four cards, Expert after the build's three (Mid cards, their centres 780 apart in
/// CanvasPanel_0, which four would overflow): every card of the row is an XSmall one (the Normal row's size), their
/// centres ARCADE_GAP apart about the row's middle.
const ARCADE_ROW: [&str; 4] = ["ArcadeEasy", "ArcadeNormal", "ArcadeHard", EXPERT_CARD];
const ARCADE_GAP: f64 = 600.0;
/// UI_GameModeCardSize XSmall (`card_size` 0).
const ARCADE_CARD_SIZE: &str = "UI_GameModeCardSize::NewEnumerator4";
const ALL_CARDS: [&str; 17] = [
    "2v2",
    "FFA",
    "1v1",
    "GuildDungeons",
    "TheLab",
    "Tutorial_beg",
    "Tutorial_adv",
    "Tutorial_chars",
    "Local",
    "CoopVsAI",
    "Casual",
    "Create",
    "Join",
    "ArcadeEasy",
    "ArcadeNormal",
    "ArcadeHard",
    EXPERT_CARD,
];
const ONLINE_CARDS: [&str; 5] = ["2v2", "FFA", "1v1", "CoopVsAI", "Casual"];
/// GetCardsForMode: normal, bots, ranked.
fn mode_cards(id: &str) -> &'static [&'static str] {
    match id {
        "normal" => &["2v2", "FFA", "1v1", "CoopVsAI", "Casual"],
        "bots" => &["2v2", "FFA", "1v1"],
        "ranked" => &["2v2", "1v1"],
        _ => &[],
    }
}
/// The cards the modal opens on: its online tab (the Type bar's start, openSequence).
pub const MATCH_OPTIONS_FIRST_CARDS: [&str; 5] = ONLINE_CARDS;
/// ReAnimIn's list @48168 (the cards it makes unseen).
const REANIM_CARDS: [&str; 10] = [
    "2v2",
    "FFA",
    "1v1",
    "GuildDungeons",
    "TheLab",
    "Tutorial_beg",
    "Tutorial_adv",
    "Tutorial_chars",
    "CoopVsAI",
    "Casual",
];
/// Deviation (the user's call): CO-OP VS AI is not offered; its card stays Collapsed in every style and tab.
const REMOVED_CARDS: [&str; 1] = ["CoopVsAI"];
/// The enter order of ReAnimIn and the open sequence (GetCardsForMode's own order for the lists), less
/// REMOVED_CARDS.
const ENTER_NORMAL: [&str; 4] = ["2v2", "1v1", "Casual", "FFA"];
const ENTER_BOTS: [&str; 3] = ["2v2", "FFA", "1v1"];
/// PreConstruct's tab arrays (a shipping build: no DebugMapSelect; IsConnected? true: CustomTab is Create, Join).
/// "UI_SectionBar_Style" is a widget's own name, the others cards'.
const TABS: [&[&str]; 5] = [
    &["UI_SectionBar_Style", "2v2", "FFA", "1v1", "CoopVsAI", "Casual"],
    &["Create", "Join"],
    &["TheLab", "Tutorial_beg", "Tutorial_adv", "Local"],
    &["GuildDungeons"],
    &["ArcadeEasy", "ArcadeNormal", "ArcadeHard", EXPERT_CARD],
];
/// ChangeType's enter lists per tab.
const TAB_ENTER: [&[&str]; 5] = [
    &[],
    &["Local", "Create", "Join"],
    &["TheLab", "Tutorial_beg", "Tutorial_adv", "Tutorial_chars", "Local"],
    &["GuildDungeons"],
    &["ArcadeEasy", "ArcadeNormal", "ArcadeHard", EXPERT_CARD],
];
/// ModeTypeSelected's switch (case-insensitive).
pub fn type_index(id: &str) -> Option<usize> {
    match id.to_lowercase().as_str() {
        "online" => Some(0),
        "custom" => Some(1),
        "practice" => Some(2),
        "guilddungeon" => Some(3),
        "arcade" => Some(4),
        _ => None,
    }
}
/// PrintSelectedModes: the order and the names (Game.locres keys: 1 VS 1, TEAMS, CO-OP VS AI, FFA, SILLY).
const PRINT_ORDER: [(&str, &str); 5] = [
    ("1v1", "C1413E164DB8DCE5DC2CC19A2E4AEBA5"),
    ("2v2", "720C33654366ACE4376C668A91B54234"),
    ("CoopVsAI", "7ECCDD29495ACF160A8075A6A6E9284F"),
    ("FFA", "1789A04E4D313B3F325D3485EB970175"),
    ("Casual", "B2EE21AE4D716D4BC84E558DB0B5FCC8"),
];
fn style_label(id: &str) -> Option<&'static str> {
    match id {
        "normal" => Some("Normal"),
        "bots" => Some("Bots"),
        "ranked" => Some("Ranked"),
        _ => None,
    }
}
/// The card size of a style (ModeStyleSelected): Normal's XSmall, Bots' and Ranked's Mid; another none.
pub fn style_card_size(id: &str) -> Option<(usize, f64)> {
    match id {
        "normal" => Some((0, NORMAL_CARD_W)),
        "bots" | "ranked" => Some((2, CARD_W[2])),
        _ => None,
    }
}
/// UI_Functions.IsRankedAvailable? reads the season and the account (the backend's). The live game with no backend
/// shows BOTS and NORMAL only: CheckRankedOption's RemoveOption (@65012), the feature off. The ranked path below
/// stays as the graph has it, unreached; were it reached, the account level (15) is taken as not met (Unverified).
const RANKED_FEATURE_ENABLED: bool = false;
const RANKED_LEVEL_MET: bool = false;
/// Deviation: the cards the port runs, per tab; every other card shows the card's own disabled look
/// (ToggleDisable(false): greyed, its "COMING SOON!" Disabled Text) and takes no input. The online tab runs the
/// Normal 1v1, 2v2, FFA and Silly (Casual) queues only (BOTS has no queue in the port); of the tutorials, the
/// Beginner Tutorial and the Advanced Tutorials (Tutorial_chars, the character tutorials, stays off).
const PORTED_CARDS: [&str; 13] = [
    "1v1",
    "2v2",
    "FFA",
    "Casual",
    "Create",
    "Local",
    "TheLab",
    "Tutorial_beg",
    "Tutorial_adv",
    "ArcadeEasy",
    "ArcadeNormal",
    "ArcadeHard",
    EXPERT_CARD,
];
/// @778-1077 / @1248-1457: the cards a full party (GetIsPartyFull) cannot queue. Deviation (the user's call): FFA,
/// the build's third, stays a party's.
const PARTY_OFF_CARDS: [&str; 2] = ["1v1", "CoopVsAI"];
/// Deviation: the Join Lobby card (IsEnabled false in the build's tree, and JoinLobby does nothing) runs in the app
/// (online.canJoin): it opens the custom game browser.
const JOIN_CARD: &str = "Join";
const PORTED_STYLES: [&str; 1] = ["normal"];
/// Deviation: neither the server prompt nor the crossplay dialog opens on the way in.
const ASK_SERVER_AND_CROSSPLAY: bool = false;

/// QUEUE_CARD: a queue mode's card.
fn queue_card(m: QueueMode) -> &'static str {
    match m {
        QueueMode::OneVOne => "1v1",
        QueueMode::TwoVTwo => "2v2",
        QueueMode::Ffa => "FFA",
        QueueMode::Silly => "Casual",
    }
}

/// The confirm's label and whether it takes input, by the count of selected online cards and whether this is a party
/// member's page (party full and not leading: its leader queues for both; the port's own).
pub fn confirm_state(count: usize, member: bool) -> (&'static str, bool) {
    let ok = count > 0 && !member;
    let label = if ok {
        "CONFIRM"
    } else if count > 0 {
        crate::menu::custom_text::PARTY_LEADER_QUEUES
    } else {
        "Choose One or More Modes"
    };
    (label, ok)
}

/// (the port's own; pictures.rs) What the named cards draw, from the modal's tree and the card's class defaults (no
/// card built): the background material, each card's character material and the textures the cards' background
/// instances sample (bundle names).
pub fn match_options_pictures(b: &Bundle, cards: &[&str]) -> Vec<String> {
    let names: Vec<String> = cards.iter().map(|n| c(n)).collect();
    let defaults = b.widgets.get("WBP_GameModeCard").map(|c| c.defaults.clone()).unwrap_or(Value::Null);
    let mut out = vec![CARD_BG.to_string()];
    let mut sampled = Vec::new();
    fn walk(
        w: &WData,
        names: &[String],
        defaults: &Value,
        b: &Bundle,
        out: &mut Vec<String>,
        sampled: &mut Vec<String>,
    ) {
        if names.contains(&w.n) {
            let mut p = defaults.clone();
            if let (Value::Object(m), Value::Object(own)) = (&mut p, &w.p) {
                for (k, v) in own {
                    m.insert(k.clone(), v.clone());
                }
            }
            if let Some(mc) = p.get("MaterialChar").and_then(Value::as_str).filter(|s| !s.is_empty()) {
                out.push(mc.to_string());
            }
            if let Some(t) = card_bg_texture(&p).filter(|t| b.textures.contains_key(t)) {
                sampled.push(t);
            }
        }
        for c in &w.c {
            walk(c, names, defaults, b, out, sampled);
        }
        for c in w.ns.values() {
            walk(c, names, defaults, b, out, sampled);
        }
    }
    if let Some(cls) = b.widgets.get("WBP_MatchOptions") {
        walk(&cls.root, &names, &defaults, b, &mut out, &mut sampled);
    }
    out.extend(sampled);
    out
}

/// WBP_TutorialType's pictures (TutType_Images), which WBP_SubOptionModal's advanced tutorials draw.
pub fn advanced_tutorial_pictures(b: &Bundle) -> Vec<String> {
    let d = b.widgets.get("WBP_TutorialType").map(|c| &c.defaults);
    d.and_then(|d| d.get("TutType_Images")).and_then(Value::as_array).map_or_else(Vec::new, |l| {
        l.iter().filter_map(|e| e.get("Value").and_then(Value::as_str)).map(str::to_string).collect()
    })
}

const M_CALLOUT: u32 = 1;
const M_TYPE_DELAY: u32 = 2;
const M_TYPE_CLICK: u32 = 3;
const M_STYLE_CLICK: u32 = 4;
const M_SCROLL: u32 = 5;
const M_SUBMIT3: u32 = 6;
const M_CARD_FOCUSED: u32 = 7;
const M_CARD_SELECTED: u32 = 8;
const M_CLOSE: u32 = 9;
const M_RANKED_HELP: u32 = 10;
const M_CONFIRM: u32 = 11;
const M_FOCUS_FIRST: u32 = 12;
const M_AFTER_DIALOGS: u32 = 13;
const M_SERVER_CHOSEN: u32 = 14;
/// A stagger's delays: M_STAGGER + list slot x 0x100 + the list's generation (its low byte).
const M_STAGGER: u32 = 0x1000;

/// One list of enterCards: its own loop and Delay node.
#[derive(Debug)]
struct Stagger {
    names: Vec<String>,
    generation: u32,
    next: usize,
}

/// WBP_MatchOptions. Not ported: the FTUE hand-holds (WBP_FTUEGameOptions stays Collapsed), the multi-queue save
/// (prev_modes), NotifyPartyLeaderChangingModes, the guild section (removed by the cvar), ScrollWidgetIntoView after
/// a card's click, the analytics (ActivityStart / ActivityEnd), and MO_HorizontalButton.UpdateQuadShape on the Type
/// bar.
#[derive(Debug, Default)]
pub struct MatchOptions {
    base: ModalBase,
    instructions: String,
    style: String,
    waiting_dialog: bool,
    guild_removed: bool,
    callout: Option<WidgetId>,
    staggers: Vec<Stagger>,
    pending_style: Option<(i32, String)>,
    /// The card firing its OnSelectedCard: busy, so its state comes with the event.
    firing: Option<CardSelected>,
    pub current_focused_card: Option<WidgetId>,
    /// The online view as the screen last followed it (online.onChanged, party.onChange).
    seen_can_join: bool,
    seen_party: Option<mvs_net::service::api::PartyView>,
}

impl MatchOptions {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::<MatchOptions>::default()
    }

    fn card(cx: &Cx<'_>, n: &str) -> WidgetId {
        find(cx, &c(n))
    }

    fn shown(cx: &Cx<'_>, n: &str) -> bool {
        cx.view.tree().widget(Self::card(cx, n)).shown()
    }

    fn with_card<R>(cx: &mut Cx<'_>, n: &str, f: impl FnOnce(&mut GameModeCard, &mut Cx<'_>) -> R) -> Option<R> {
        let w = Self::card(cx, n);
        cx.visit::<GameModeCard, _>(w, f)
    }

    /// The card's (IsSelected, IsEnabled): from the event while it fires.
    fn flags(&self, cx: &mut Cx<'_>, n: &str) -> (bool, bool) {
        let w = Self::card(cx, n);
        if let Some(e) = self.firing.as_ref().filter(|e| e.card == w) {
            return (e.selected, e.enabled);
        }
        cx.visit::<GameModeCard, _>(w, |c, _| (c.is_selected, c.is_enabled)).unwrap_or((false, true))
    }

    fn type_bar(cx: &Cx<'_>) -> WidgetId {
        find(cx, "UI_SectionBar_Type")
    }

    fn style_bar(cx: &Cx<'_>) -> WidgetId {
        find(cx, "UI_SectionBar_Style")
    }

    fn confirm(cx: &Cx<'_>) -> WidgetId {
        find(cx, "Button_Confirm")
    }

    fn scroll_box(cx: &Cx<'_>) -> WidgetId {
        find(cx, "ScrollBox_0")
    }

    fn request_dismiss_self(&mut self, cx: &mut Cx<'_>) {
        self.base.request_dismiss(cx, "", false);
    }

    /// CheckRankedOption (PreConstruct and OnInitialized): without the feature the Style bar's ranked option goes.
    fn check_ranked_option(cx: &mut Cx<'_>) {
        if !RANKED_FEATURE_ENABLED {
            let b = Self::style_bar(cx);
            cx.visit::<SectionBar, _>(b, |b, cx| b.remove_option(cx, "ranked"));
        }
    }

    /// (the port's own) The Arcade's Expert card: a new WBP_GameModeCard after the Hard card in its row, with the Hard
    /// card's properties and slot (its ModeOverride EXPERT_CARD); the row's four cards laid out as ARCADE_ROW says.
    fn add_expert_card(cx: &mut Cx<'_>) {
        let hard = Self::card(cx, "ArcadeHard");
        let Some(row) = cx.view.tree().widget(hard).parent else { return };
        let at = cx.view.tree().widget(row).children.iter().position(|&k| k == hard).map(|i| i + 1);
        let props = cx.view.tree().props(hard).clone();
        let w = MenuView::create(cx, "WBP_GameModeCard");
        if let Value::Object(m) = props {
            for (k, v) in m {
                cx.view.tree_mut().set_prop(w, &k, v);
            }
        }
        cx.view.tree_mut().set_prop(w, "ModeOverride", json!(EXPERT_CARD));
        cx.view.tree_mut().set_prop(w, "MaterialChar", json!(EXPERT_CHAR));
        let h = cx.view.tree().widget(hard).clone();
        let x = wd(cx, w);
        x.name = c(EXPERT_CARD);
        x.pad = h.pad;
        x.fill = h.fill;
        x.h_align = h.h_align;
        x.v_align = h.v_align;
        x.anchors = h.anchors;
        x.offsets = h.offsets;
        x.align = h.align;
        x.auto_size = h.auto_size;
        MenuView::add_child(cx, row, w, at);
        for (i, n) in ARCADE_ROW.iter().enumerate() {
            let w = Self::card(cx, n);
            cx.view.tree_mut().set_prop(w, "CardSizeType", json!(ARCADE_CARD_SIZE));
            #[allow(clippy::cast_precision_loss)] // (four cards)
            let x = (i as f64 - 1.5) * ARCADE_GAP;
            wd(cx, w).offsets.l = x;
        }
    }

    /// SetPracticeEnabled(bEnabled): the Type bar SelfHitTestInvisible, or Hidden (home's ARCADE).
    pub fn set_practice_enabled(cx: &mut Cx<'_>, on: bool) {
        set_vis(cx, "UI_SectionBar_Type", if on { Vis::SelfHitTestInvisible } else { Vis::Hidden });
    }

    fn remove_callout(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        cx.view.manager.modal_shown.remove(Handler { owner: me, id: M_CALLOUT });
        if let Some(w) = self.callout.take() {
            MenuView::discard(cx, w);
        }
    }

    /// @40231 (from PostShow @50667): a new context, SetContext, focus, the confirm state, the start tab, the binds,
    /// then @27999-28524: Tutorial_chars collapsed, the 2v2 card's ManualFocus, Enter, the cards' entry, Focus First
    /// Card.
    fn open_sequence(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        let g = cx.view.manager.new_guid();
        self.set_context(cx, &g);
        let fc = find(cx, "focusContext");
        set_focused(cx, fc, true);
        self.set_multiqueue_confirm_enabled(cx);
        let start = if self.instructions == "Arcade" { "arcade" } else { "online" };
        self.mode_type_selected(cx, start);
        let (tb, sb) = (Self::type_bar(cx), Self::style_bar(cx));
        cx.visit::<SectionBar, _>(tb, |b, _| b.on_click.add(Handler { owner: me, id: M_TYPE_CLICK }));
        cx.visit::<SectionBar, _>(sb, |b, _| b.on_click.add(Handler { owner: me, id: M_STYLE_CLICK }));
        cx.view.manager.on_scroll.add(Handler { owner: me, id: M_SCROLL });
        let scroll = Self::scroll_box(cx);
        cx.view.manager.register_scrollbox(scroll);
        cx.view.manager.on_submit3.add(Handler { owner: me, id: M_SUBMIT3 });
        self.join_card_follows(cx, true);
        // OnPartyChanged @61435 (a party that is full now dismisses the modal): Online (R7), no party offline.
        for n in ALL_CARDS {
            Self::with_card(cx, n, |c, _| {
                c.on_focused_card.add(Handler { owner: me, id: M_CARD_FOCUSED });
                c.on_selected_card.add(Handler { owner: me, id: M_CARD_SELECTED });
            });
        }
        for (name, id) in [("UI_IconButton", M_CLOSE), ("buttonRankedHelp", M_RANKED_HELP)] {
            let w = find(cx, name);
            cx.visit::<IconButton, _>(w, |b, _| b.on_click.add(Handler { owner: me, id }));
        }
        let cb = Self::confirm(cx);
        cx.visit::<ButtonMain, _>(cb, |b, _| b.on_click.add(Handler { owner: me, id: M_CONFIRM }));
        let tc = Self::card(cx, "Tutorial_chars");
        wd(cx, tc).vis = Vis::Collapsed;
        // Deviation: the offline row's slots have no VerticalAlignment (Fill), so a hovered card's taller SizeBox
        // (Select @7913) re-centres the whole row in ScrollBox_0; centred as the online row's slots are, only the
        // hovered card grows.
        let off = find(cx, "modeCardsContainerOffline");
        for k in cx.view.tree().widget(off).children.clone() {
            wd(cx, k).v_align = VAlign::Center;
        }
        Self::with_card(cx, "2v2", |c, cx| c.manual_focus(cx));
        cx.view.play(me, "Enter", false, 1.0);
        self.enter_normal_or_bots(cx);
        // Focus First Card: Delay(0.2), SetFocused(ContextOnly) on the 2v2 card
        cx.view.delay(0.2, Handler { owner: me, id: M_FOCUS_FIRST });
    }

    fn set_context(&mut self, cx: &mut Cx<'_>, ctx: &str) {
        self.base.context = ctx.to_string();
        let mut list: Vec<WidgetId> = ALL_CARDS
            .iter()
            .map(|n| {
                let w = Self::card(cx, n);
                interactor_in(cx, w)
            })
            .collect();
        list.push(find(cx, "focusContext"));
        for n in ["Button_Confirm", "UI_IconButton", "buttonRankedHelp"] {
            let w = find(cx, n);
            list.push(interactor_in(cx, w));
        }
        batch_set_context(cx, &list, ctx);
        for b in [Self::type_bar(cx), Self::style_bar(cx)] {
            cx.visit::<SectionBar, _>(b, |b, cx| b.update_context(cx, ctx, None));
        }
    }

    fn unbind(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        let m = &mut cx.view.manager;
        m.modal_shown.remove(Handler { owner: me, id: M_CALLOUT });
        m.on_scroll.remove(Handler { owner: me, id: M_SCROLL });
        m.on_submit3.remove(Handler { owner: me, id: M_SUBMIT3 });
        let sb = Self::scroll_box(cx);
        cx.view.manager.unregister_scrollbox(sb);
        let id = self.base.modal_id.clone();
        cx.view.release_pictures(&id);
        cx.view.release_pictures(&format!("{id}:tutorials"));
    }

    /// ModeTypeSelected(Index, ID) -> ChangeType.
    fn mode_type_selected(&mut self, cx: &mut Cx<'_>, id: &str) {
        if let Some(t) = type_index(id) {
            self.change_type(cx, t);
        }
    }

    fn tab_widget(cx: &Cx<'_>, n: &str) -> WidgetId {
        if n == "UI_SectionBar_Style" { find(cx, n) } else { Self::card(cx, n) }
    }

    fn set_tab(cx: &mut Cx<'_>, t: usize) {
        for (i, tab) in TABS.iter().enumerate() {
            if i != t {
                for n in *tab {
                    let w = Self::tab_widget(cx, n);
                    wd(cx, w).vis = Vis::Collapsed;
                }
            }
        }
        for n in TABS[t] {
            let w = Self::tab_widget(cx, n);
            wd(cx, w).vis = if REMOVED_CARDS.contains(n) { Vis::Collapsed } else { Vis::SelfHitTestInvisible };
        }
    }

    fn set_callout_vis(&self, cx: &mut Cx<'_>, v: Vis) {
        if let Some(w) = self.callout {
            wd(cx, w).vis = v;
        }
    }

    /// ChangeType(Type): @31352 first the UI_Interactor OnUnfocus(0) broadcast of every card (GetAllCards): each that
    /// takes input runs Deselect and restarts its 0.033 s throttle, which swallows the ManualFocus below in the same
    /// frame (the captures' Practice and Arcade tabs show no card focused).
    fn change_type(&mut self, cx: &mut Cx<'_>, t: usize) {
        for n in ALL_CARDS {
            let w = Self::card(cx, n);
            let i = interactor_in(cx, w);
            if let Some(ev) = ist(cx, i).map(|s| s.on_unfocus.clone()) {
                ev.fire(cx, &0i32);
            }
        }
        let desc = find(cx, "ModeDescriptionText");
        if t == 0 {
            Self::set_tab(cx, 0);
            set_vis(cx, "VerticalBox_0", Vis::SelfHitTestInvisible);
            wd(cx, desc).vis = Vis::Collapsed;
            self.set_callout_vis(cx, Vis::SelfHitTestInvisible);
            let s = self.current_style(cx);
            self.mode_style_selected(cx, 0, &s, true);
            self.re_anim_in(cx);
            return;
        }
        match t {
            1 => {
                Self::with_card(cx, "Local", |c, cx| c.manual_focus(cx));
            }
            2 => {
                Self::with_card(cx, "TheLab", |c, cx| c.manual_focus(cx));
                let off = find(cx, "modeCardsContainerOffline");
                wd(cx, off).transform.translation[0] = PRACTICE_ROW_X;
            }
            4 => {
                Self::with_card(cx, "ArcadeNormal", |c, cx| c.manual_focus(cx));
            }
            _ => {}
        }
        Self::set_tab(cx, t);
        set_vis(cx, "VerticalBox_0", Vis::Hidden);
        wd(cx, desc).vis = Vis::Collapsed;
        self.set_callout_vis(cx, Vis::Collapsed);
        if t == 4 {
            // (its text is the tree's own, the arcade description)
            wd(cx, desc).vis = Vis::SelfHitTestInvisible;
            for n in TABS[4] {
                Self::with_card(cx, n, |c, cx| {
                    c.set_arcade_subtitle(cx, true);
                    c.show_experimental_tag(cx);
                });
            }
        }
        for n in TAB_ENTER[t] {
            let w = Self::card(cx, n);
            wd(cx, w).opacity = 0.0;
            cx.view.stop_all_animations(w);
        }
        self.enter_cards(cx, TAB_ENTER[t]);
    }

    /// The Style bar's selected ID; the port's own fallback while the bar has no buttons: the last style that ran.
    fn current_style(&self, cx: &mut Cx<'_>) -> String {
        let b = Self::style_bar(cx);
        cx.visit::<SectionBar, _>(b, |b, _| (!b.buttons.is_empty()).then(|| b.current_selected_id()))
            .flatten()
            .unwrap_or_else(|| self.style.clone())
    }

    /// ModeStyleSelected(Index, ID, Force).
    fn mode_style_selected(&mut self, cx: &mut Cx<'_>, index: i32, id: &str, force: bool) {
        if !force && id == self.current_style(cx) {
            return;
        }
        self.pending_style = Some((index, id.to_string()));
        let unset = server_region().lock().unwrap_or_else(std::sync::PoisonError::into_inner).is_none();
        if ASK_SERVER_AND_CROSSPLAY && unset {
            let me = cx.w;
            if let Some(p) = show_modal_generic(cx, false, "UI_ServerSettingsPrompt", "serverSettings", "") {
                cx.visit::<ServerSettingsPrompt, _>(p, |s, _| {
                    s.on_preferences_confirmed.add(Handler { owner: me, id: M_SERVER_CHOSEN })
                });
            }
            return;
        }
        self.continue_style(cx, id);
    }

    fn continue_style(&mut self, cx: &mut Cx<'_>, id: &str) {
        self.style = id.to_string();
        self.update_card_visibilities(cx, id);
        if let Some((size, width)) = style_card_size(id) {
            for n in ONLINE_CARDS {
                Self::with_card(cx, n, |c, cx| c.set_card_size(cx, size, width));
            }
        }
        self.update_locked_cards_state(cx, id);
        self.disable_unported(cx, id);
        self.party_cards(cx, id);
        if let Some(l) = style_label(id) {
            let s = find(cx, "SelectedLabel");
            wd(cx, s).text = format!("{l}:");
        }
        let help = find(cx, "rankedHelpContainer");
        let row = find(cx, "modeCardsContainer");
        let off = find(cx, "modeCardsContainerOffline");
        if id == "normal" || id == "bots" {
            let bots = id == "bots";
            for n in ONLINE_CARDS {
                Self::with_card(cx, n, |c, cx| c.set_bots(cx, bots));
            }
            let x = if bots { 75.0 } else { 0.0 };
            wd(cx, row).transform.translation[0] = x;
            wd(cx, off).transform.translation[0] = x;
            if id == "normal" {
                wd(cx, row).transform.translation[0] = NORMAL_ROW_X;
            }
            set_vis(cx, "Button_Confirm", Vis::SelfHitTestInvisible);
            wd(cx, help).vis = Vis::Hidden;
        } else if id == "ranked" {
            for n in ONLINE_CARDS {
                Self::with_card(cx, n, |c, cx| c.set_bots(cx, false));
            }
            wd(cx, row).transform.translation[0] = 0.0;
            set_vis(cx, "Button_Confirm", Vis::SelfHitTestInvisible);
            wd(cx, help).vis = Vis::SelfHitTestInvisible;
        }
        // NORMAL @6463 and RANKED @19520: the crossplay prompt (ASK_SERVER_AND_CROSSPLAY), then @1555 / @3959 wait
        // while a dialog is up; BOTS goes to @1724 at once
        if id == "normal" || id == "ranked" {
            self.after_dialogs(cx);
        } else {
            self.finish_style(cx);
        }
    }

    /// @1555: IsDialogActive -> Delay(0.1) and again (one Delay node: a second start while it waits is not a second
    /// loop), else @1724.
    fn after_dialogs(&mut self, cx: &mut Cx<'_>) {
        if self.waiting_dialog {
            return;
        }
        if !Manager::is_dialog_active(cx.view) {
            return self.finish_style(cx);
        }
        self.waiting_dialog = true;
        let me = cx.w;
        cx.view.delay(0.1, Handler { owner: me, id: M_AFTER_DIALOGS });
    }

    /// @1724.
    fn finish_style(&mut self, cx: &mut Cx<'_>) {
        self.re_anim_in(cx);
        self.print_selected_modes(cx);
        self.set_multiqueue_confirm_enabled(cx);
    }

    /// Update Card Visibilities For Mode(ID): the online cards collapsed, then the mode's shown.
    fn update_card_visibilities(&self, cx: &mut Cx<'_>, id: &str) {
        for n in ONLINE_CARDS.iter().chain(&["GuildDungeons"]) {
            let w = Self::card(cx, n);
            wd(cx, w).vis = Vis::Collapsed;
        }
        for n in mode_cards(id) {
            if !REMOVED_CARDS.contains(n) {
                let w = Self::card(cx, n);
                wd(cx, w).vis = Vis::Visible;
            }
        }
    }

    /// UpdateLockedCardsState.
    fn update_locked_cards_state(&self, cx: &mut Cx<'_>, id: &str) {
        for n in mode_cards("ranked") {
            let w = Self::card(cx, n);
            if id == "ranked" {
                let t = json!({ "CultureInvariantString": "Ranked mode unlocks at Account Level 15" });
                cx.view.tree_mut().set_prop(w, "Locked Text", t);
                cx.visit::<GameModeCard, _>(w, |c, cx| {
                    c.toggle_locked(cx, !RANKED_LEVEL_MET);
                    c.toggle_disable(cx, RANKED_LEVEL_MET);
                    if !RANKED_LEVEL_MET {
                        c.deselect(cx, false);
                        set_vis_opacity(cx, "SelectedGroup", 0.0);
                    }
                });
            } else {
                cx.visit::<GameModeCard, _>(w, |c, cx| {
                    c.toggle_locked(cx, false);
                    c.toggle_disable(cx, true);
                });
            }
        }
    }

    /// The Join Lobby card follows online.canJoin while the screen is up (JOIN_CARD). `first`: on the way in, where
    /// the card still has the tree's disabled look (IsEnabled false) whatever isEnabled says.
    fn join_card_follows(&self, cx: &mut Cx<'_>, first: bool) {
        let can = cx.view.session.online.can_join;
        Self::with_card(cx, JOIN_CARD, |c, cx| {
            if !first && can == c.is_enabled {
                return;
            }
            if can {
                return c.toggle_disable(cx, true);
            }
            c.deselect(cx, false);
            c.toggle_disable(cx, false);
        });
    }

    /// @405-1481 (after the Normal cards' entry; the port's: with each style's cards, after disableUnported, which
    /// keeps an unported card off): PARTY_OFF_CARDS ToggleDisable(not full and not locked), and with a full party
    /// Deselect(false) and TurnOffQueueMode (a selected card's selection off).
    fn party_cards(&mut self, cx: &mut Cx<'_>, style: &str) {
        let full = cx.view.session.online.party_full();
        for n in PARTY_OFF_CARDS {
            let ported = PORTED_CARDS.contains(&n) && PORTED_STYLES.contains(&style);
            Self::with_card(cx, n, |c, cx| {
                if ported {
                    let locked = {
                        let l = find(cx, "LockedIndicator");
                        cx.view.tree().widget(l).shown()
                    };
                    c.toggle_disable(cx, !full && !locked);
                }
                if !full {
                    return;
                }
                c.deselect(cx, false);
                if c.is_selected {
                    c.is_selected = false;
                    set_vis_opacity(cx, "SelectedGroup", 0.0);
                }
            });
        }
        self.set_multiqueue_confirm_enabled(cx);
        self.print_selected_modes(cx);
    }

    /// (the port's own: PORTED_CARDS) a card the port does not run is disabled and loses its selection.
    fn disable_unported(&self, cx: &mut Cx<'_>, style: &str) {
        for n in ALL_CARDS {
            let ported = PORTED_CARDS.contains(&n);
            if ported && !ONLINE_CARDS.contains(&n) {
                continue;
            }
            if ported && PORTED_STYLES.contains(&style) {
                // back on from an unported style's pass (the ranked cards come back with updateLockedCardsState)
                Self::with_card(cx, n, |c, cx| {
                    if !c.is_enabled {
                        c.toggle_disable(cx, true);
                    }
                });
                continue;
            }
            if n == JOIN_CARD && cx.view.session.online.can_join {
                continue;
            }
            Self::with_card(cx, n, |c, cx| {
                if c.is_selected {
                    c.is_selected = false;
                    set_vis_opacity(cx, "SelectedGroup", 0.0);
                }
                c.deselect(cx, false);
                c.toggle_disable(cx, false);
            });
        }
    }

    fn enter_normal_or_bots(&mut self, cx: &mut Cx<'_>) {
        if Self::shown(cx, "Casual") {
            self.enter_cards(cx, &ENTER_NORMAL);
        } else {
            self.enter_cards(cx, &ENTER_BOTS);
        }
    }

    /// ReAnimIn @48145: its cards unseen, then the visible list enters.
    fn re_anim_in(&mut self, cx: &mut Cx<'_>) {
        for n in REANIM_CARDS {
            let w = Self::card(cx, n);
            wd(cx, w).opacity = 0.0;
        }
        self.enter_normal_or_bots(cx);
    }

    /// The loops of EnterAnim calls 0.07 s apart: each list its own loop and Delay node, so two lists' loops run
    /// side by side. Port: the same list's loop started again starts over (Deviation: the build's Delay node ignores
    /// a new start while it waits, and the loop's counter goes on).
    fn enter_cards(&mut self, cx: &mut Cx<'_>, names: &[&str]) {
        let names: Vec<String> = names.iter().map(|s| s.to_string()).collect();
        let slot = match self.staggers.iter().position(|s| s.names == names) {
            Some(i) => i,
            None => {
                self.staggers.push(Stagger { names, generation: 0, next: 0 });
                self.staggers.len() - 1
            }
        };
        let s = &mut self.staggers[slot];
        s.generation = s.generation.wrapping_add(1);
        s.next = 0;
        self.stagger_step(cx, slot);
    }

    fn stagger_step(&mut self, cx: &mut Cx<'_>, slot: usize) {
        let s = &mut self.staggers[slot];
        let Some(n) = s.names.get(s.next).cloned() else { return };
        s.next += 1;
        let id = M_STAGGER + (slot as u32) * 0x100 + (s.generation & 0xff);
        Self::with_card(cx, &n, |c, cx| c.enter_anim(cx));
        let me = cx.w;
        cx.view.delay(0.07, Handler { owner: me, id });
    }

    /// PrintSelectedModes.
    fn print_selected_modes(&self, cx: &mut Cx<'_>) {
        let mut names = Vec::new();
        for (n, key) in PRINT_ORDER {
            if self.flags(cx, n).0 && Self::shown(cx, n) {
                names.push(cx.view.game_text(key).to_string());
            }
        }
        let l = find(cx, "ModesLabel");
        wd(cx, l).text = names.join(", ");
        set_vis(cx, "SelectedLabel", if names.is_empty() { Vis::Collapsed } else { Vis::SelfHitTestInvisible });
    }

    /// The selected, shown and enabled cards of `list`.
    fn picked<'a>(&self, cx: &mut Cx<'_>, list: impl IntoIterator<Item = &'a str>) -> Vec<&'a str> {
        let mut out = Vec::new();
        for n in list {
            let (sel, en) = self.flags(cx, n);
            if sel && en && Self::shown(cx, n) {
                out.push(n);
            }
        }
        out
    }

    /// SetMultiqueueConfirmEnabed.
    fn set_multiqueue_confirm_enabled(&self, cx: &mut Cx<'_>) {
        let count = self.picked(cx, ONLINE_CARDS).len();
        let o = &cx.view.session.online;
        let (label, ok) = confirm_state(count, o.party_full() && !o.party_leading());
        let b = Self::confirm(cx);
        cx.visit::<ButtonMain, _>(b, |b, cx| b.set_label(cx, label));
        let i = interactor_in(cx, b);
        set_interactable(cx, i, ok);
        let l = cx.view.tree().find_in(b, "UI_InputLabel");
        wd(cx, l).vis = if ok { Vis::SelfHitTestInvisible } else { Vis::Collapsed };
    }

    /// FocusedCard(Card) @41926: over the card's tab (PracticeTab when it holds the card, else CustomTab when it
    /// does, else OnlineTab) SlotAsCanvasSlot(card).SetZOrder(1 for the card, 0 for the others). The Custom tab's
    /// cards (Create, Join) are CanvasPanel_0's children, whose slots all have ZOrder 0: there it works, as here.
    /// The Online and Practice cards sit in HorizontalBoxes, so in the build the cast gives None and nothing changes:
    /// for them the port's own deviation, the graph's evident intent, the focused card painted (and hit-tested) after
    /// its siblings so that the grown card is not covered by its neighbours.
    fn focused_card(&mut self, cx: &mut Cx<'_>, card: WidgetId) {
        let name = cx.view.tree().widget(card).name.clone();
        let short = name.strip_prefix("WBP_GameModeCard_").unwrap_or("");
        let tab = if TABS[2].contains(&short) {
            TABS[2]
        } else if TABS[1].contains(&short) {
            TABS[1]
        } else {
            TABS[0]
        };
        for n in tab {
            let w = Self::tab_widget(cx, n);
            if cx.view.tree().user_class(w) == Some("WBP_GameModeCard") {
                wd(cx, w).paint_order = i32::from(w == card);
            }
        }
        self.current_focused_card = Some(card);
    }

    /// GameModeSelected(Mode, Mode Override, IsToggle): every path ends in PrintSelectedModes (push_flow 54012).
    fn game_mode_selected(&mut self, cx: &mut Cx<'_>, e: &CardSelected) {
        if e.is_toggle {
            self.set_multiqueue_confirm_enabled(cx);
            return self.print_selected_modes(cx);
        }
        if e.mode_override.is_empty() {
            if e.mode == 4 {
                return Homescreen::transition_to_training(cx);
            }
            warn_once(&format!("GameMode{}", e.mode), "ActOnMatchParams");
            return self.print_selected_modes(cx);
        }
        cx.post("ui_match_select_gamemode");
        let o = e.mode_override.to_lowercase();
        if o == "createlobby" {
            self.request_dismiss_self(cx);
            // BPI_AddNewCustomGameUI: as the home screen's CUSTOM GAME
            Manager::show_modal(cx, "WBP_CustomGame", "customGame", "", "");
        } else if o == "joinlobby" {
            self.show_join_box(cx);
        } else if let Some(d) = arcade_difficulty(&o) {
            self.arcade_selected(cx, d);
        } else if o == "local" {
            show_modal_generic(cx, false, "UI_LocalPlay", "localPlay", "");
            self.request_dismiss_self(cx);
        } else if o.contains("beg") {
            // @52581-52851 BPI_LaunchTutorial(TutorialActor_Intro, Map_TrophyRoom_Tutorial, false)
            launch_tutorial(cx, INTRO_TUTORIAL, TUTORIAL_MAP, false);
        } else if o.contains("adv") {
            show_modal_generic(cx, false, "WBP_SubOptionModal", "SubOption", "advancedtutorial|");
        } else if o.contains("char") {
            warn_once(&e.mode_override, "Character Tutorials");
        }
        self.print_selected_modes(cx);
    }

    /// An arcade card (@52984 / @54262 / @54287 -> @53050-53711): the match config of the difficulty and
    /// ActOnMatchParams MH@6037, whose arcade branch (@6148) launches the pre-match (LaunchCoopVsAILobby MH@10780 ->
    /// BPI_EnterPreMatch, SetupArcadeRun MH@12093), then RequestDismiss @53711. The run (`ArcadeState::start`) stands
    /// in for the config; the pre-match opens as the PreMatch level (EnterPreMatch's seamless branch over the main
    /// menu hands the program 'PreMatch', which opens the screen on its level outside a queue).
    fn arcade_selected(&mut self, cx: &mut Cx<'_>, difficulty: u8) {
        // (Math.random() * 2^32 >>> 0)
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let seed = (cx.view.manager.random() * 4_294_967_296.0) as u32;
        cx.view.session.arcade.start(difficulty, seed);
        let bundle = cx.view.bundle.clone();
        let setup =
            bundle.menu.as_ref().and_then(|m| cx.view.session.arcade.pre_match_setup(m, &cx.prefs.profile.name));
        let Some(setup) = setup else {
            return warn_once(&format!("arcade{difficulty}"), "Arcade (no run was made)");
        };
        self.request_dismiss_self(cx);
        crate::menu::lobby::with_carry(|c| {
            c.pre_match = Some(Box::new(crate::menu::screens::pre_match::setup::PreMatchStart { setup, lobby: None }));
        });
        Manager::open_level_now(cx, "PreMatch", "Lobby");
    }

    /// (the port's own: JOIN_CARD) The custom game browser over the main menu.
    fn show_join_box(&mut self, cx: &mut Cx<'_>) {
        self.request_dismiss_self(cx);
        Manager::show_modal(cx, "CustomGameBrowser", "customGames", "", "");
    }

    /// Confirm @58609 -> ActOnMatchParams MH@6037: the solo queue's, then EnterPreMatch. Deviation: a queue of 1v1,
    /// 2v2, FFA, Silly or any of them, of the Normal style, in the queue rules' order. A party member's page does not
    /// queue (its leader's does). The modes go to the session (Lobby.queue and lastQueue) and the program's online
    /// client searches them and opens the PreMatch level (EnterPreMatch, PGI@30694-31709).
    /// Deviation: the build's seamless branch (NewPrematchEnter: the main menu's screen fades out and hosts the
    /// pre-match widget) is not ported; the level opens as the arcade's pre-match does.
    fn on_confirm(&mut self, cx: &mut Cx<'_>) {
        let o = &cx.view.session.online;
        if o.party_full() && !o.party_leading() {
            return;
        }
        let picked = self.picked(cx, PRINT_ORDER.iter().map(|p| p.0));
        let modes: Vec<QueueMode> = if self.current_style(cx) != "normal" {
            Vec::new()
        } else {
            QUEUE_MODES.into_iter().filter(|m| picked.contains(&queue_card(*m))).collect()
        };
        if modes.is_empty() {
            return warn_once(
                "Button_Confirm",
                "Confirm (ConfirmMultiQueue) of a mode other than Normal 1v1 / 2v2 / FFA / Silly",
            );
        }
        self.request_dismiss_self(cx);
        cx.view.session.main_menu.queue = Some(modes.clone());
        cx.view.session.main_menu.last_queue = Some(modes.clone());
        cx.online(OnlineCmd::Queue { modes });
    }

    /// online.onChanged -> joinCardFollows, party.onChange (OnPartyChanged @61435): a party that is full now dismisses
    /// the modal, and the cards a full party cannot queue and the confirm follow the party.
    fn follow_online(&mut self, cx: &mut Cx<'_>) {
        let o = &cx.view.session.online;
        let (can, party) = (o.can_join, o.party.clone());
        if can != self.seen_can_join {
            self.seen_can_join = can;
            self.join_card_follows(cx, false);
        }
        if party != self.seen_party {
            self.seen_party = party;
            if cx.view.session.online.party_full() {
                return self.request_dismiss_self(cx);
            }
            let style = self.current_style(cx);
            self.party_cards(cx, &style);
        }
    }

    /// QuickSubmit: the confirm while it shows and takes input.
    fn quick_submit(&mut self, cx: &mut Cx<'_>, player: i32) {
        if player != 0 {
            return;
        }
        let b = Self::confirm(cx);
        let i = interactor_in(cx, b);
        let on = ist(cx, i).is_some_and(|s| s.get_interactable());
        if cx.view.tree().widget(b).shown() && on {
            self.on_confirm(cx);
        }
    }
}

/// A child's opacity by name (the card's SelectedGroup).
fn set_vis_opacity(cx: &mut Cx<'_>, name: &str, o: f64) {
    let w = find(cx, name);
    wd(cx, w).opacity = o;
}

impl Modal for MatchOptions {
    fn pre_show(&mut self, cx: &mut Cx<'_>, id: &str) {
        self.base.pre_show(cx, id);
    }

    fn post_show(&mut self, cx: &mut Cx<'_>, instructions: &str, _extra_data: &str) {
        self.instructions = instructions.to_string();
        // (the port's own; pictures.rs) every card of the five tabs and the advanced tutorials' tiles
        let b = cx.view.bundle.clone();
        let id = self.base.modal_id.clone();
        let cards = match_options_pictures(&b, &ALL_CARDS);
        cx.view.hold_pictures(&id, cards.iter().map(String::as_str));
        let tuts = advanced_tutorial_pictures(&b);
        cx.view.hold_pictures(&format!("{id}:tutorials"), tuts.iter().map(String::as_str));
        self.open_sequence(cx);
        // SetupInitialConfiguration @51046
        self.mode_style_selected(cx, 1, "normal", false);
        let sb = Self::style_bar(cx);
        cx.visit::<SectionBar, _>(sb, |b, cx| b.set_current_index_by_id(cx, "normal"));
        // @50672: the cards' container shows (IsConnected?), the FTUE options do not
        set_vis(cx, "modeCardsContainer", Vis::Visible);
        set_vis(cx, "WBP_FTUEGameOptions", Vis::Collapsed);
        // home's ARCADE @19264: SetPracticeEnabled(false) after its ShowModal-Generic (the caller's, run here)
        if self.instructions == "Arcade" {
            Self::set_practice_enabled(cx, false);
        }
    }

    fn pre_dismiss(&mut self, _cx: &mut Cx<'_>) {}

    fn post_dismiss(&mut self, cx: &mut Cx<'_>) {
        self.unbind(cx);
    }

    fn request_dismiss(&mut self, cx: &mut Cx<'_>, reason: &str, from_cancel: bool) {
        self.base.request_dismiss(cx, reason, from_cancel);
    }

    fn get_modal_context(&self) -> String {
        self.base.get_modal_context()
    }
}

impl Controller for MatchOptions {
    fn as_modal(&mut self) -> Option<&mut dyn Modal> {
        Some(self)
    }

    /// PreConstruct @49010: SelectedLabel Format("{mode}:", Normal) (@50146-50515, Game.locres keys), DebugMapSelect
    /// collapsed.
    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        let l = find(cx, "SelectedLabel");
        let t = cx
            .view
            .game_text("CDC94FB34FC59D8BC2BEDDB0B8C255ED")
            .replace("{mode}", cx.view.game_text("40307D3A4C151424C26AB188EC2891FE"));
        wd(cx, l).text = t;
        set_vis(cx, "DebugMapSelect", Vis::Collapsed);
    }

    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        Self::add_expert_card(cx);
        Self::check_ranked_option(cx);
        let sb = Self::scroll_box(cx);
        let list: Vec<WidgetId> = ONLINE_CARDS
            .iter()
            .map(|n| {
                let w = Self::card(cx, n);
                interactor_in(cx, w)
            })
            .collect();
        batch_set_scroll_box(cx, &list, sb);
        // UI_CalloutTooltip's OnInitialized binds the manager's ModalShownEvent to its RemoveFromParent: the callout
        // goes as soon as this modal is shown (the ShowModal of this very modal fires the event)
        self.callout = cx.view.tree().find_opt(me, "UI_CalloutTooltip");
        cx.view.manager.modal_shown.add(Handler { owner: me, id: M_CALLOUT });
        // Delay(0.1): the Type bar's tab, and for the arcade start Update Card Visibilities For Mode("")
        cx.view.delay(0.1, Handler { owner: me, id: M_TYPE_DELAY });
    }

    /// Construct @39960: the party notify (no party in the port), ToggleDismissable(false) on the callout (its
    /// dismiss button collapsed), InitGuildSection (the cvar removes the guild tab @60728). The rest of the open
    /// sequence (@40231) runs from PostShow.
    fn construct(&mut self, cx: &mut Cx<'_>) {
        let sb = find(cx, "SizeBox_0");
        fill_page(cx, sb);
        if let Some(w) = self.callout {
            let b = cx.view.tree().find_in(w, "Button");
            wd(cx, b).vis = Vis::Collapsed;
        }
        if !self.guild_removed {
            self.guild_removed = true;
            let tb = Self::type_bar(cx);
            cx.visit::<SectionBar, _>(tb, |b, cx| b.remove_option(cx, "guilddungeon"));
        }
    }

    fn destruct(&mut self, cx: &mut Cx<'_>) {
        self.unbind(cx);
    }

    /// Deviation: the 16:9 box follows the page.
    fn tick(&mut self, cx: &mut Cx<'_>, _dt: f64) {
        let sb = find(cx, "SizeBox_0");
        fill_page(cx, sb);
        self.follow_online(cx);
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        match id {
            M_CALLOUT => self.remove_callout(cx),
            M_TYPE_CLICK => {
                if let Some((_, id)) = arg.downcast_ref::<(i32, String)>() {
                    self.mode_type_selected(cx, id);
                }
            }
            M_STYLE_CLICK => {
                if let Some((index, id)) = arg.downcast_ref::<(i32, String)>() {
                    self.mode_style_selected(cx, *index, id, false);
                }
            }
            // OnScroll: ScrollList(ScrollBox_0, Amount x -2)
            M_SCROLL => {
                if let Some(&(player, _, amount)) = arg.downcast_ref::<(i32, i32, f64)>()
                    && player == 0
                {
                    let sb = Self::scroll_box(cx);
                    scroll_list(cx.view.tree_mut(), sb, amount * -2.0);
                }
            }
            M_SUBMIT3 => {
                if let Some(&p) = arg.downcast_ref::<i32>() {
                    self.quick_submit(cx, p);
                }
            }
            M_CARD_FOCUSED => {
                if let Some(&w) = arg.downcast_ref::<WidgetId>() {
                    self.focused_card(cx, w);
                }
            }
            M_CARD_SELECTED => {
                if let Some(e) = arg.downcast_ref::<CardSelected>() {
                    self.firing = Some(e.clone());
                    self.game_mode_selected(cx, e);
                    self.firing = None;
                }
            }
            M_CLOSE => self.request_dismiss_self(cx),
            M_RANKED_HELP => {
                show_modal_generic(cx, false, "UI_RankedExplanationModal", "rankedExplanation", "");
            }
            M_CONFIRM => self.on_confirm(cx),
            _ => {}
        }
    }

    fn resume(&mut self, cx: &mut Cx<'_>, id: u32) {
        match id {
            M_TYPE_DELAY => {
                let arcade = self.instructions == "Arcade";
                let tb = Self::type_bar(cx);
                let tab = if arcade { "arcade" } else { "online" };
                cx.visit::<SectionBar, _>(tb, |b, cx| b.set_current_index_by_id(cx, tab));
                if arcade {
                    self.update_card_visibilities(cx, "");
                }
            }
            M_FOCUS_FIRST => {
                let w = Self::card(cx, "2v2");
                let i = interactor_in(cx, w);
                set_focused(cx, i, true);
            }
            M_AFTER_DIALOGS => {
                self.waiting_dialog = false;
                self.after_dialogs(cx);
            }
            // OnServerSettingsChosen -> @45120 with the last ModeStyleSelected's arguments
            M_SERVER_CHOSEN => {
                if let Some((_, id)) = self.pending_style.clone() {
                    self.continue_style(cx, &id);
                }
            }
            _ if id >= M_STAGGER => {
                let slot = ((id - M_STAGGER) / 0x100) as usize;
                let generation = (id - M_STAGGER) & 0xff;
                if self.staggers.get(slot).is_some_and(|s| s.generation & 0xff == generation) {
                    self.stagger_step(cx, slot);
                }
            }
            _ => {}
        }
    }
}

// ---------- WBP_TutorialType (a tile of the advanced tutorials) ----------

/// UpdateAppearance's switch @284-938 (case-insensitive on the tile's key): the save key of each tile.
pub fn tutorial_save_key(key: &str) -> Option<&'static str> {
    Some(match key.to_lowercase().as_str() {
        "dodging" => "DodgeMeterTutorialComplete",
        "cooldowns" => "CooldownsTutorialComplete",
        "walls" => "WallsTutorialComplete",
        "armor" => "ArmorTutorialComplete",
        "dodge jump" => "DodgeJumpTutorialComplete",
        "aerial combat" => "AerialCombatTutorialComplete",
        "attack decay" => "AttackDecayTutorialComplete",
        "fast fall" => "FastFallTutorialComplete",
        "spike" => "DownSpikeTutorialComplete",
        "dodge attack" => "DodgeAttackTutorialComplete",
        "knockback influence" => "KnockbackInfluenceTutorialComplete",
        "elements" => "ElementsTutorialComplete",
        _ => return None,
    })
}

const T_FOCUS: u32 = 1;
const T_CLICK: u32 = 2;

/// Of UI_GridItemBase what UI_GridItemList uses. UpdateAppearance shows the UI_Attention badge (Type 0: the "!"
/// badge) unless the TutorialData save's TutorialsComplete holds the tile's "<Type>TutorialComplete". Not ported:
/// UI_Attention's Idle animation (the pulse).
#[derive(Debug, Default)]
pub struct TutorialType {
    state: GridItemState,
    data: Option<(String, String)>,
    tut_image: Option<String>,
}

impl TutorialType {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::<TutorialType>::default()
    }
}

impl GridItem for TutorialType {
    fn state(&mut self) -> &mut GridItemState {
        &mut self.state
    }

    fn locked(&self) -> bool {
        false
    }

    fn set_size(&mut self, _cx: &mut Cx<'_>, _x: f64, _y: f64) {}

    fn update_appearance(&mut self, cx: &mut Cx<'_>) {
        let Some((_, text)) = self.data.clone() else { return };
        let t = find(cx, "TutorialType");
        wd(cx, t).text = text;
        if let Some(img) = self.tut_image.clone() {
            let b = cx.view.bundle.clone();
            let i = find(cx, "Image_95");
            wd(cx, i).tex = brush(&b, &img).or(Some(img));
        }
        // @10 BPI_LoadTutorialQuestData, @247 Toggle Visibility(true), the key switch @284-938, Toggle
        // Visibility(false) @1140 when the save has the key.
        // Deviation: the "!" badge stays hidden on every tile.
        let att = find(cx, "UI_Attention");
        let w = cx.view.tree().find_in(att, "MainContainer");
        wd(cx, w).vis = Vis::Collapsed;
    }
}

impl Tile for TutorialType {
    fn interactors(&self, cx: &Cx<'_>) -> Vec<WidgetId> {
        let b = find(cx, "UI_ButtonSecondary");
        vec![interactor_in(cx, b)]
    }

    fn set_data(&mut self, cx: &mut Cx<'_>, data: Option<TileData>) {
        self.data = data.as_ref().and_then(|d| d.tutorial()).map(|(k, t)| (k.to_string(), t.to_string()));
        if let Some((key, _)) = &self.data {
            let images = prop(cx, "TutType_Images");
            self.tut_image = images.as_array().and_then(|l| {
                l.iter()
                    .find(|e| e.get("Key").and_then(Value::as_str) == Some(key.as_str()))
                    .and_then(|e| e.get("Value").and_then(Value::as_str))
                    .map(str::to_string)
            });
        }
        self.update_appearance(cx);
    }
}

impl Controller for TutorialType {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let b = find(cx, "UI_ButtonSecondary");
        let i = interactor_in(cx, b);
        bind(cx, i, |s: &mut InteractorState| &mut s.on_focus, T_FOCUS);
        bind(cx, i, |s: &mut InteractorState| &mut s.on_click, T_CLICK);
        let fc = find(cx, "UI_FocusedCommon");
        cx.visit::<FocusedCommon, _>(fc, |f, cx| f.set_interactor(cx, i));
        let me = cx.w;
        cx.view.play(me, "EnterAnim", false, 1.0);
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        let p = arg.downcast_ref::<i32>().copied().unwrap_or(0);
        match id {
            T_FOCUS => self.state.focus(cx, p),
            T_CLICK => GridItemState::select(self, cx, p),
            _ => {}
        }
    }

    fn resume(&mut self, cx: &mut Cx<'_>, id: u32) {
        GridItemState::resume(self, cx, id);
    }

    fn as_grid_item(&mut self) -> Option<&mut dyn Tile> {
        Some(self)
    }
}

// ---------- WBP_SubOptionModal (the modal `SubOption`) ----------

/// OnItemSelected's switch @2742-3382 (case-insensitive on the tile's key): the TutorialActor class each tile's
/// BPI_LaunchTutorial names (@3472-5848).
pub fn tutorial_actor(key: &str) -> Option<&'static str> {
    Some(match key.to_lowercase().as_str() {
        "armor" => "TutorialActor_Armor",
        "dodge attack" => "TutorialActor_DashAttack",
        "cooldowns" => "TutorialActor_Cooldowns",
        "fast fall" => "TutorialActor_FastFall",
        "spike" => "TutorialActor_DownSpike",
        "dodge jump" => "TutorialActor_DodgeJump",
        "aerial combat" => "TutorialActor_AerialCombat",
        "walls" => "TutorialActor_Walls",
        "attack decay" => "TutorialActor_AttackDecay",
        "dodging" => "TutorialActor_DodgeMeter",
        "knockback influence" => "TutorialActor_KBI",
        "elements" => "TutorialActor_Elements",
        _ => return None,
    })
}

const S_CLOSE: u32 = 1;
const S_SELECTED: u32 = 2;
const S_FOCUS: u32 = 3;
/// WBP_SubOptionModal. Not ported: the analytics. Unverified: the tiles follow TutorialTypes' order in the class
/// defaults as the bundle holds the map.
#[derive(Debug, Default)]
pub struct SubOptionModal {
    base: ModalBase,
    /// Instructions: PostShow's string up to its "|" (@1089-1162), compared case-insensitively.
    kind: String,
}

impl SubOptionModal {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::<SubOptionModal>::default()
    }

    fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str) {
        self.base.context = ctx.to_string();
        let fc = find(cx, "focusContext");
        let back = find(cx, "Btn_Back");
        let bi = interactor_in(cx, back);
        batch_set_context(cx, &[fc, bi], ctx);
        let g = find(cx, "UI_GridList");
        cx.visit::<GridItemList, _>(g, |g, cx| g.update_context(cx, ctx));
    }

    /// OnItemSelected @1944: advancedtutorial -> @2362: the tile's key switched on (@2742), the main menu widget's
    /// BPI_LaunchTutorial(TutorialActor_<type>, Map_TrophyRoom_Tutorial, false). The modal is not dismissed: the
    /// transition takes the menus away. (charactertutorial @2068 casts the data to CharacterData and prints a
    /// string: the port's grid never holds one; a key the switch lacks does nothing, @3396.)
    fn on_item_selected(&self, cx: &mut Cx<'_>, e: &TileEvent) {
        let Some((key, _)) = e.data.as_ref().and_then(|d| d.tutorial()) else { return };
        if self.kind != "advancedtutorial" {
            return;
        }
        if let Some(actor) = tutorial_actor(key) {
            launch_tutorial(cx, actor, TUTORIAL_MAP, false);
        }
    }
}

impl Modal for SubOptionModal {
    fn pre_show(&mut self, cx: &mut Cx<'_>, id: &str) {
        self.base.pre_show(cx, id);
    }

    fn post_show(&mut self, cx: &mut Cx<'_>, instructions: &str, _extra_data: &str) {
        let me = cx.w;
        cx.view.play(me, "IntroAnim", false, 1.5);
        let kind = instructions.split('|').next().unwrap_or_default().to_lowercase();
        // SetTitleName: charactertutorial key C22F703C... (@257), advancedtutorial key B6C88335... (@180)
        let key = if kind == "charactertutorial" {
            "C22F703C49FD63B21D818EB48DB00937"
        } else {
            "B6C8833543D790302DCD07B9224B6B8D"
        };
        let t = cx.view.game_text(key).to_string();
        let h = find(cx, "Header");
        wd(cx, h).text = t;
        let grid = find(cx, "UI_GridList");
        if kind == "advancedtutorial" {
            cx.view.tree_mut().set_prop(grid, "Grid Item Class", json!("WBP_TutorialType_C"));
        }
        self.kind = kind;
        let types = prop(cx, "TutorialTypes");
        let data: Vec<TileData> = types.as_array().map_or_else(Vec::new, |l| {
            l.iter()
                .map(|t| TileData::Tutorial {
                    key: t.get("Key").and_then(Value::as_str).unwrap_or_default().to_string(),
                    text: text(t.get("Value")),
                })
                .collect()
        });
        let b = cx.view.bundle.clone();
        let id = self.base.modal_id.clone();
        let pics = advanced_tutorial_pictures(&b);
        cx.view.hold_pictures(&id, pics.iter().map(String::as_str));
        cx.visit::<GridItemList, _>(grid, |g, cx| {
            g.populate(cx, data);
            g.selected_event.add(Handler { owner: me, id: S_SELECTED });
        });
        let g = cx.view.manager.new_guid();
        self.update_context(cx, &g);
        cx.view.delay(0.0, Handler { owner: me, id: S_FOCUS });
    }

    fn pre_dismiss(&mut self, _cx: &mut Cx<'_>) {}

    fn post_dismiss(&mut self, cx: &mut Cx<'_>) {
        let id = self.base.modal_id.clone();
        cx.view.release_pictures(&id);
    }

    fn request_dismiss(&mut self, cx: &mut Cx<'_>, reason: &str, from_cancel: bool) {
        self.base.request_dismiss(cx, reason, from_cancel);
    }

    fn get_modal_context(&self) -> String {
        self.base.get_modal_context()
    }
}

impl Controller for SubOptionModal {
    fn as_modal(&mut self) -> Option<&mut dyn Modal> {
        Some(self)
    }

    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        let back = find(cx, "Btn_Back");
        cx.visit::<IconButton, _>(back, |b, _| b.on_click.add(Handler { owner: me, id: S_CLOSE }));
        let fc = find(cx, "focusContext");
        bind(cx, fc, |s: &mut InteractorState| &mut s.on_click, S_CLOSE);
    }

    /// PreConstruct: the header in the palette's UI-White (GetPaletteEntry(5): PaletteEntryType 5 is
    /// NewEnumerator3, "UI-White", a CentralPalette row).
    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        let c = palette_entry(cx, 5);
        let h = find(cx, "Header");
        wd(cx, h).color = c;
    }

    /// Deviation: the 16:9 box follows the page.
    fn construct(&mut self, cx: &mut Cx<'_>) {
        let sb = find(cx, "SizeBox_1");
        fill_page(cx, sb);
    }

    fn tick(&mut self, cx: &mut Cx<'_>, _dt: f64) {
        let sb = find(cx, "SizeBox_1");
        fill_page(cx, sb);
    }

    fn destruct(&mut self, cx: &mut Cx<'_>) {
        let id = self.base.modal_id.clone();
        cx.view.release_pictures(&id);
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        match id {
            S_CLOSE => self.base.request_dismiss(cx, "", false),
            S_SELECTED => {
                if let Some((e, _)) = arg.downcast_ref::<(TileEvent, bool)>() {
                    self.on_item_selected(cx, e);
                }
            }
            _ => {}
        }
    }

    fn resume(&mut self, cx: &mut Cx<'_>, id: u32) {
        if id == S_FOCUS {
            let fc = find(cx, "focusContext");
            set_focused(cx, fc, true);
            let g = find(cx, "UI_GridList");
            cx.visit::<GridItemList, _>(g, |g, cx| g.focus_index(cx, 0, 0));
        }
    }
}

// ---------- UI_RankedTierOverview (a tier of the ranked help) ----------

const R_ENTER: u32 = 1;

/// The UI_RankedTierIcon's PreConstruct: Dimensions square, the name hidden unless ShowName.
fn ranked_icon_size(cx: &mut Cx<'_>, icon: WidgetId) {
    let p = cx.view.tree().props(icon).clone();
    let dim = p.get("Dimensions").and_then(Value::as_f64).unwrap_or(128.0);
    let b = cx.view.tree().find_in(icon, "sizeContainer");
    let x = wd(cx, b);
    x.w_override = dim;
    x.h_override = dim;
    let n = cx.view.tree().find_in(icon, "labelName");
    let show = p.get("ShowName").and_then(Value::as_bool).unwrap_or(false);
    wd(cx, n).vis = if show { Vis::SelfHitTestInvisible } else { Vis::Hidden };
}

#[derive(Debug, Default)]
pub struct RankedTierOverview;

impl RankedTierOverview {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(RankedTierOverview)
    }

    /// Enter(Delay): unseen until the Delay, then shown and EnterAnim at 1.25.
    pub fn enter(&mut self, cx: &mut Cx<'_>, delay: f64) {
        let me = cx.w;
        wd(cx, me).vis = Vis::Hidden;
        cx.view.delay(delay, Handler { owner: me, id: R_ENTER });
    }
}

impl Controller for RankedTierOverview {
    /// Its UI_RankedTierIcon's PreConstruct; no custom slug.
    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        let icon = find(cx, "UI_RankedTierIcon");
        ranked_icon_size(cx, icon);
    }

    fn resume(&mut self, cx: &mut Cx<'_>, id: u32) {
        if id == R_ENTER {
            let me = cx.w;
            wd(cx, me).vis = Vis::SelfHitTestInvisible;
            cx.view.play(me, "EnterAnim", false, 1.25);
        }
    }
}

// ---------- UI_RankedExplanationModal (the modal `rankedExplanation`) ----------

const E_EXIT: u32 = 1;

/// The season's leagues are the backend's: PopulateTiers finds none and makes the one tier of an empty slug, which
/// keeps its tree's texts (Set Rank Info returns on an empty slug). The two footer badges have custom slugs, so their
/// PreConstruct runs SetTierInfo on an empty tier: no name, the icon and the stars hidden (UI_RankedTierIcon,
/// handled here: the class is also the profile's badge). Not ported: WBP_EOG_StageGroup's graph (its tree shows).
#[derive(Debug, Default)]
pub struct RankedExplanationModal {
    base: ModalBase,
}

impl RankedExplanationModal {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::<RankedExplanationModal>::default()
    }

    /// PopulateTiers.
    fn populate_tiers(cx: &mut Cx<'_>) {
        let bx = find(cx, "tiersContainer");
        for k in cx.view.tree().widget(bx).children.clone() {
            MenuView::discard(cx, k);
        }
        let w = MenuView::create(cx, "UI_RankedTierOverview");
        let t = cx.view.tree_mut();
        t.set_prop(w, "Rank Tier Slug", json!(""));
        t.set_prop(w, "Root Tier", json!(true));
        t.set_prop(w, "Title Suffix", json!(""));
        t.set_prop(w, "Custom Threshold", json!(""));
        let x = wd(cx, w);
        x.h_align = HAlign::Fill;
        x.v_align = VAlign::Fill;
        x.fill = 1.0;
        MenuView::add_child(cx, bx, w, None);
        for (i, k) in cx.view.tree().widget(bx).children.clone().into_iter().enumerate() {
            cx.visit::<RankedTierOverview, _>(k, |r, cx| r.enter(cx, i as f64 * 0.05));
        }
    }

    /// UpdateAppearance with Display Rewards.
    fn update_appearance(cx: &mut Cx<'_>) {
        if prop(cx, "Display Rewards").as_bool() == Some(false) {
            return;
        }
        set_vis(cx, "textBulletsRight", Vis::SelfHitTestInvisible);
        let m = find(cx, "mainSizebox");
        wd(cx, m).h_override = 1100.0;
        let t = find(cx, "tiersLabel");
        wd(cx, t).transform.translation[1] = 10.0;
        let h = find(cx, "header");
        wd(cx, h).transform.translation[1] = -50.0;
        let b = find(cx, "headerBGSize");
        wd(cx, b).h_override = 250.0;
    }

    fn unbind(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        cx.view.manager.on_submit3.remove(Handler { owner: me, id: E_EXIT });
        let id = self.base.modal_id.clone();
        cx.view.release_pictures(&id);
    }
}

impl Modal for RankedExplanationModal {
    fn pre_show(&mut self, cx: &mut Cx<'_>, id: &str) {
        self.base.pre_show(cx, id);
    }

    fn post_show(&mut self, cx: &mut Cx<'_>, _instructions: &str, _extra_data: &str) {
        let id = self.base.modal_id.clone();
        cx.view.hold_pictures(&id, ["RankedBadge_Unranked", "RankedBadge_DivisionStars1"]);
        let g = cx.view.manager.new_guid();
        self.base.context = g.clone();
        let fi = find(cx, "focusInteractor");
        let cb = find(cx, "buttonClose");
        let ci = interactor_in(cx, cb);
        batch_set_context(cx, &[fi, ci], &g);
        set_focused(cx, fi, true);
    }

    fn pre_dismiss(&mut self, _cx: &mut Cx<'_>) {}

    fn post_dismiss(&mut self, cx: &mut Cx<'_>) {
        self.unbind(cx);
    }

    fn request_dismiss(&mut self, cx: &mut Cx<'_>, reason: &str, from_cancel: bool) {
        self.base.request_dismiss(cx, reason, from_cancel);
    }

    fn get_modal_context(&self) -> String {
        self.base.get_modal_context()
    }
}

impl Controller for RankedExplanationModal {
    fn as_modal(&mut self) -> Option<&mut dyn Modal> {
        Some(self)
    }

    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        cx.view.manager.on_submit3.add(Handler { owner: me, id: E_EXIT });
        let b = find(cx, "buttonClose");
        cx.visit::<ButtonMain, _>(b, |b, _| b.on_click.add(Handler { owner: me, id: E_EXIT }));
        Self::populate_tiers(cx);
        wd(cx, me).opacity = 0.0;
        Self::update_appearance(cx);
        cx.view.play(me, "EnterAnim", false, 1.0);
    }

    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        for n in ["UI_RankedTierIcon", "UI_RankedTierIcon_1"] {
            let icon = find(cx, n);
            ranked_icon_size(cx, icon);
            let p = cx.view.tree().props(icon).clone();
            let set = |k: &str| p.get(k).and_then(Value::as_str).is_some_and(|s| !s.is_empty());
            if set("Custom Tier Slug") && set("Custom Season Slug") {
                let name = cx.view.tree().find_in(icon, "labelName");
                wd(cx, name).text = String::new();
                for k in ["Icon", "Stars"] {
                    let w = cx.view.tree().find_in(icon, k);
                    wd(cx, w).vis = Vis::Hidden;
                }
            }
        }
    }

    fn destruct(&mut self, cx: &mut Cx<'_>) {
        self.unbind(cx);
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, _arg: &dyn Any) {
        if id == E_EXIT {
            self.base.request_dismiss(cx, "", false);
        }
    }
}

// ---------- UI_ServerSettingsPrompt (the modal `serverSettings`) ----------

const P_DROP_CLOSED: u32 = 1;
const P_DROP_CLICK: u32 = 2;
const P_CONFIRM: u32 = 3;
const P_UNFOCUS_DROP: u32 = 4;

/// Confirm keeps the region for the session (server_region; Deviation: the build saves it to QAServerRegion2) and
/// broadcasts OnPreferencesConfirmed. Not an Xbox: the crossplay warning stays collapsed.
#[derive(Debug, Default)]
pub struct ServerSettingsPrompt {
    base: ModalBase,
    pub on_preferences_confirmed: Event<()>,
}

impl ServerSettingsPrompt {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::<ServerSettingsPrompt>::default()
    }

    fn drop_main(cx: &Cx<'_>) -> WidgetId {
        let d = find(cx, "ServerDropMenu");
        cx.view.tree().find_in(d, "mainInteractor")
    }
}

impl Modal for ServerSettingsPrompt {
    fn pre_show(&mut self, cx: &mut Cx<'_>, id: &str) {
        self.base.pre_show(cx, id);
    }

    fn post_show(&mut self, cx: &mut Cx<'_>, _instructions: &str, _extra_data: &str) {
        let me = cx.w;
        let d = find(cx, "ServerDropMenu");
        cx.visit::<DropMenu, _>(d, |d, _| {
            d.on_closed.add(Handler { owner: me, id: P_DROP_CLOSED });
            d.on_click.add(Handler { owner: me, id: P_DROP_CLICK });
        });
        let b = find(cx, "ButtonConfirm");
        cx.visit::<ButtonMain, _>(b, |b, _| b.on_click.add(Handler { owner: me, id: P_CONFIRM }));
        let bi = interactor_in(cx, b);
        set_interactable(cx, bi, false);
        let g = cx.view.manager.new_guid();
        self.base.context = g.clone();
        let fc = find(cx, "focusContext");
        batch_set_context(cx, &[bi, fc], &g);
        cx.visit::<DropMenu, _>(d, |_, cx| DropMenu::update_context(cx, &g, None));
        set_focused(cx, fc, true);
        cx.view.delay(0.2, Handler { owner: me, id: P_UNFOCUS_DROP });
    }

    fn pre_dismiss(&mut self, _cx: &mut Cx<'_>) {}

    fn post_dismiss(&mut self, _cx: &mut Cx<'_>) {}

    fn request_dismiss(&mut self, cx: &mut Cx<'_>, reason: &str, from_cancel: bool) {
        self.base.request_dismiss(cx, reason, from_cancel);
    }

    fn get_modal_context(&self) -> String {
        self.base.get_modal_context()
    }
}

impl Controller for ServerSettingsPrompt {
    fn as_modal(&mut self) -> Option<&mut dyn Modal> {
        Some(self)
    }

    fn construct(&mut self, cx: &mut Cx<'_>) {
        set_vis(cx, "XBoxCrossplayWarning", Vis::Collapsed);
        let sb = find(cx, "SizeBox_18");
        fill_page(cx, sb);
    }

    /// Deviation: the 16:9 box follows the page.
    fn tick(&mut self, cx: &mut Cx<'_>, _dt: f64) {
        let sb = find(cx, "SizeBox_18");
        fill_page(cx, sb);
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, _arg: &dyn Any) {
        match id {
            P_DROP_CLOSED => {
                let m = Self::drop_main(cx);
                set_focused(cx, m, false);
            }
            // the drop menu's OnClick (@885 -> @538): the confirm takes input
            P_DROP_CLICK => {
                let b = find(cx, "ButtonConfirm");
                let bi = interactor_in(cx, b);
                set_interactable(cx, bi, true);
            }
            P_CONFIRM => {
                let d = find(cx, "ServerDropMenu");
                let sel = cx.visit::<DropMenu, _>(d, |d, _| d.current_selected_id.clone());
                *server_region().lock().unwrap_or_else(std::sync::PoisonError::into_inner) = sel;
                cx.emit(self, |c| &c.on_preferences_confirmed, &());
                self.base.request_dismiss(cx, "", false);
            }
            _ => {}
        }
    }

    fn resume(&mut self, cx: &mut Cx<'_>, id: u32) {
        if id == P_UNFOCUS_DROP {
            let m = Self::drop_main(cx);
            set_focused(cx, m, false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn card_sizes_by_enumerator() {
        assert_eq!(card_size("NewEnumerator4"), Some(0));
        assert_eq!(card_size("NewEnumerator2"), Some(4));
        assert_eq!(card_size("Other"), None);
    }

    #[test]
    fn style_steps_the_card_size() {
        assert_eq!(style_card_size("normal"), Some((0, NORMAL_CARD_W)));
        assert_eq!(style_card_size("bots"), Some((2, 750.0)));
        assert_eq!(style_card_size("ranked"), Some((2, 750.0)));
        assert_eq!(style_card_size(""), None);
    }

    #[test]
    fn confirm_label_by_count() {
        assert_eq!(confirm_state(0, false), ("Choose One or More Modes", false));
        assert_eq!(confirm_state(2, false), ("CONFIRM", true));
        assert_eq!(confirm_state(2, true), (crate::menu::custom_text::PARTY_LEADER_QUEUES, false));
    }

    #[test]
    fn type_ids_ignore_case() {
        assert_eq!(type_index("Online"), Some(0));
        assert_eq!(type_index("ARCADE"), Some(4));
        assert_eq!(type_index("guild"), None);
    }

    #[test]
    fn card_background_by_override_then_mode() {
        assert_eq!(card_bg_texture(&json!({ "ModeOverride": "Casual" })).as_deref(), Some("GameMode_AXHard_BG"));
        assert_eq!(card_bg_texture(&json!({ "GameMode": "EGameMode::Teams" })).as_deref(), Some("UI_GameMode_2vs2_BG"));
        assert_eq!(
            card_bg_texture(&json!({ "BG Texture Override": "Texture2D'/Game/UI/X/UI_Foo.UI_Foo'" })).as_deref(),
            Some("UI_Foo")
        );
        assert_eq!(card_bg_texture(&json!({})), None);
    }

    #[test]
    fn tutorial_keys() {
        assert_eq!(tutorial_save_key("Dodge Jump"), Some("DodgeJumpTutorialComplete"));
        assert_eq!(tutorial_actor("KNOCKBACK INFLUENCE"), Some("TutorialActor_KBI"));
        assert_eq!(tutorial_actor("x"), None);
    }
}
