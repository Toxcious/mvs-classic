//! The pre-match screen's child widgets: the input labels, the
//! character tiles, the players' state and info rows, the confirm, the emotes, the skin selector and the mode labels.
//!
//! Not ported (the queue's and the online paths, which `PreMatchSetup` does not carry yet): the queue label and its
//! multiqueue texts, the set score outside a queue set's rematch, Change Mode. The round label (GetRoundLabelText, its
//! call site untraced), the region label, the Muxy vote and the warning dialogue are not ported either (no caller
//! passes one in a custom game).

use std::any::Any;

use serde_json::{Value, json};

use super::setup::PreMatchSetup;
use super::widget_api::session;
use crate::bundle::{Rgba, RosterEntry, SkinInfo};
use crate::menu::arcade::ARCADE_EXPERT;
use crate::menu::controller::{Controller, Cx, Event, Handler};
use crate::menu::custom_text;
use crate::menu::lobby::loadout::BadgeRef;
use crate::menu::manager::Manager;
use crate::menu::screens::lab_selectors::{
    CollectionCharacterItem, GridItemList, SkinTile, T_CLICK, T_FOCUS, Tile, TileData, TileEvent, run_on, run_tile,
    set_color, set_text, set_vis,
};
use crate::menu::stat_trackers::badge_item;
use crate::menu::view::MenuView;
use crate::menu::widgets::arcade_progress::AXProgress;
use crate::menu::widgets::badges::{StatBadge, badge_item_data};
use crate::menu::widgets::common::{self, Quadrilateral, rgba};
use crate::menu::widgets::controls::buttons::{ButtonMain, ButtonSecondary};
use crate::menu::widgets::controls::grid_item::{GridItem, GridItemState};
use crate::menu::widgets::controls::input_label::{InputLabel, InputLabelParams, enum_name, field};
use crate::menu::widgets::controls::interactor::{self, batch_set_context, batch_set_scroll_box};
use crate::menu::widgets::controls::{bind, find, focus_frame, ist, prop, prop_bool, prop_f64, wd};
use crate::slate::{Margin, Mid, Vis, WidgetId};

fn text_of(v: Option<&Value>) -> String {
    common::text(v)
}

fn handler(cx: &Cx<'_>, id: u32) -> Handler {
    Handler { owner: cx.w, id }
}

fn player_of(arg: &dyn Any) -> i32 {
    arg.downcast_ref::<i32>().copied().unwrap_or(0)
}

// ---------- UI_InputLabelSet ----------

/// The Params entry as the label takes it.
fn label_params(p: &Value) -> InputLabelParams {
    let size = field(Some(p), "SizeIconSize");
    let num = |v: Option<&Value>, d: f64| v.and_then(Value::as_f64).unwrap_or(d);
    let action = field(Some(p), "Action");
    let modifier = field(Some(p), "Modifier");
    InputLabelParams {
        action: if action.is_none() { "UISubmit".into() } else { enum_name(action) },
        modifier: if modifier.is_none() { "None".into() } else { enum_name(modifier) },
        label: text_of(field(Some(p), "Label")),
        player_index: num(field(Some(p), "PlayerIndex"), 0.0) as i32,
        automatic_input_type: field(Some(p), "AutomaticInputType").and_then(Value::as_bool).unwrap_or(true),
        fixed_input_type: field(Some(p), "FixedInputType").and_then(Value::as_str).map_or(0, trailing_number),
        tint: field(Some(p), "Tint").map_or([1.0; 4], rgba),
        text_size: num(field(Some(p), "TextFontInfo").and_then(|f| f.get("Size")), 18.0),
        key_size: num(field(Some(p), "KeyFontInfo").and_then(|f| f.get("Size")), 18.0),
        icon_size: [num(size.and_then(|s| s.get("X")), 32.0), num(size.and_then(|s| s.get("Y")), 32.0)],
        custom_icon: field(Some(p), "CustomActionIcon").and_then(Value::as_str).map(str::to_string),
    }
}

/// The number a name ends in (`/(\d+)$/`), 0 when it ends in none.
fn trailing_number(s: &str) -> i32 {
    let digits: String =
        s.chars().rev().take_while(char::is_ascii_digit).collect::<Vec<_>>().into_iter().rev().collect();
    digits.parse().unwrap_or(0)
}

/// OnInitialized -> Populate @1523: the Container's children are one UI_InputLabel per Params entry, each with the
/// set's Tint, the slot's right padding 20 except on the last.
#[derive(Debug, Default)]
pub struct InputLabelSet {
    pub params: Vec<InputLabelParams>,
}

impl InputLabelSet {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::<InputLabelSet>::default()
    }

    pub fn populate(&mut self, cx: &mut Cx<'_>) {
        let bx = find(cx, "Container");
        for c in cx.view.tree().widget(bx).children.clone() {
            MenuView::discard(cx, c);
        }
        let tint = cx.view.tree().props(cx.w).get("Tint").filter(|t| !t.is_null()).map(rgba);
        let n = self.params.len();
        for (i, p) in self.params.clone().into_iter().enumerate() {
            let w = MenuView::create(cx, "UI_InputLabel");
            let p = InputLabelParams { tint: tint.unwrap_or(p.tint), ..p };
            cx.visit::<InputLabel, _>(w, |l, cx| l.update_params(cx, p));
            wd(cx, w).pad = Margin { l: 0.0, t: 0.0, r: if i + 1 < n { 20.0 } else { 0.0 }, b: 0.0 };
            MenuView::add_child(cx, bx, w, None);
        }
    }
}

impl Controller for InputLabelSet {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let list = prop(cx, "Params");
        self.params = list.as_array().map(|a| a.iter().map(label_params).collect()).unwrap_or_default();
        self.populate(cx);
    }
}

// ---------- UI_InputLabelButton ----------

const IB_FOCUS: u32 = 1;
const IB_UNFOCUS: u32 = 2;
const IB_CLICK: u32 = 3;

/// A button of an input label and an icon (the skin strip's page arrows).
#[derive(Debug, Default)]
pub struct InputLabelButton {
    /// OnClick(Player Index).
    pub on_click: Event<i32>,
}

impl InputLabelButton {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::<InputLabelButton>::default()
    }

    /// The button `w`'s interactor.
    pub fn interactor_of(cx: &Cx<'_>, w: WidgetId) -> WidgetId {
        cx.view.tree().find_in(w, "UI_Interactor")
    }

    fn style(cx: &mut Cx<'_>, focused: bool) {
        let c = |cx: &Cx<'_>, k: &str| rgba(&prop(cx, k));
        let bg = c(cx, if focused { "Hover Color BG" } else { "Idle Color BG" });
        let fg = c(cx, if focused { "Hover Color FG" } else { "Idle Color FG" });
        set_color(cx, "bg", bg);
        set_color(cx, "extraIcon", fg);
        // UI_InputLabel "Toggle Dark Color"(Hover Color FG, focused)
        let label = find(cx, "UI_InputLabel");
        let dark = prop(cx, "Hover Color FG");
        cx.view.tree_mut().set_prop(label, "Dark Color", dark);
        cx.view.tree_mut().set_prop(label, "Is Dark", json!(focused));
        cx.visit::<InputLabel, _>(label, |l, cx| l.force_refresh(cx));
    }

    pub fn set_interactable(&mut self, cx: &mut Cx<'_>, b: bool) {
        let me = cx.w;
        cx.view.tree_mut().set_prop(me, "Not Interactable", json!(!b));
        let i = Self::interactor_of(cx, me);
        interactor::set_interactable(cx, i, b);
    }
}

impl Controller for InputLabelButton {
    // @64; OnFocus @726, OnUnfocus @999, OnClick
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        let i = Self::interactor_of(cx, me);
        let fc = find(cx, "UI_FocusedCommon");
        focus_frame(cx, fc, i);
        bind(cx, i, |s| &mut s.on_focus, IB_FOCUS);
        bind(cx, i, |s| &mut s.on_unfocus, IB_UNFOCUS);
        bind(cx, i, |s| &mut s.on_click, IB_CLICK);
        let on = !prop_bool(cx, "Not Interactable");
        self.set_interactable(cx, on);
    }

    // @1190
    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        let fc = find(cx, "UI_FocusedCommon");
        let wobble = prop(cx, "OnFocus Wobble");
        cx.view.tree_mut().set_prop(fc, "IsWobbling", wobble);
        let q = prop(cx, "Input Label Params");
        let q = (!q.is_null()).then_some(q);
        let label = find(cx, "UI_InputLabel");
        let hide = prop_bool(cx, "Force Hide Text");
        let key_color = prop(cx, "Key Text Color");
        cx.view.tree_mut().set_prop(label, "ForceHideText", json!(hide));
        cx.view.tree_mut().set_prop(label, "KeyTextColor", key_color);
        let player = prop_f64(cx, "Player Index", 0.0) as i32;
        let num = |v: Option<&Value>| v.and_then(Value::as_f64);
        let fq = q.as_ref();
        let size = field(fq, "SizeIconSize");
        let action = field(fq, "Action");
        let modifier = field(fq, "Modifier");
        cx.visit::<InputLabel, _>(label, |l, cx| {
            let b = l.params.clone();
            let params = InputLabelParams {
                action: if action.is_none() { "UISubmit".into() } else { enum_name(action) },
                modifier: if modifier.is_none() { "None".into() } else { enum_name(modifier) },
                label: text_of(field(fq, "Label")),
                player_index: player,
                tint: field(fq, "Tint").map_or(b.tint, rgba),
                text_size: num(field(fq, "TextFontInfo").and_then(|f| f.get("Size"))).unwrap_or(b.text_size),
                key_size: num(field(fq, "KeyFontInfo").and_then(|f| f.get("Size"))).unwrap_or(b.key_size),
                icon_size: [
                    num(size.and_then(|s| s.get("X"))).unwrap_or(32.0),
                    num(size.and_then(|s| s.get("Y"))).unwrap_or(32.0),
                ],
                custom_icon: field(fq, "CustomActionIcon").and_then(Value::as_str).map(str::to_string),
                ..b
            };
            l.update_params(cx, params);
        });
        let icon = find(cx, "extraIcon");
        let header = prop(cx, "Header Image").as_str().map(str::to_string).filter(|s| !s.is_empty());
        let flip = prop_bool(cx, "Flip Header Image");
        let x = wd(cx, icon);
        if let Some(h) = &header {
            x.tex = Some(h.clone());
        }
        x.transform.scale = [if flip { -1.0 } else { 1.0 }, 1.0];
        x.vis = if header.is_some() { Vis::SelfHitTestInvisible } else { Vis::Collapsed };
        Self::style(cx, false);
        let on = !prop_bool(cx, "Not Interactable");
        self.set_interactable(cx, on);
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        match id {
            IB_FOCUS => Self::style(cx, true),
            IB_UNFOCUS => Self::style(cx, false),
            IB_CLICK => cx.emit(self, |c| &c.on_click, &player_of(arg)),
            _ => {}
        }
    }
}

// ---------- UI_PM_CharacterItem (parent UI_GridItemBase): a tile of the pre-match grid ----------

const PM_ICON_BG_FOCUS: Rgba = [0.783538, 1.0, 0.822786, 1.0];
const PM_ON_FOCUS: u32 = 100;
/// The arcade star's FillColor by the saved value (@3032-3188) and ArcadeNumPlayers' "1" (@3949).
const ARCADE_STAR: [[f32; 4]; 3] =
    [[0.441667, 0.140846, 0.049687, 1.0], [0.462535, 0.681431, 0.720834, 1.0], [0.982251, 0.520996, 0.000304, 1.0]];
const ARCADE_ONE_PLAYER: &str = "82C165DE4C136D80D01ED693EB1B5E30";

/// The tree and the base rules are the Lab tile's (UI_Collection_CharacterItem); the differences: GetAnimations gives
/// EnterAnim (played by the grid's Auto Enter) and SelectAnim for both focus and select (the FlashQuad), Quad_IconBG's
/// focus colour @7472, and the root's OnCharacterItemAdded PM@113511 (UI_Attention hidden, Shows Selected State
/// false). hover0..3 / selected0..3 are never shown (the grid's Ignore Hover Indicators, Shows Selected State false);
/// attentionNew, the free rotation badge and the Muxy count need the backend; the arcade star reads the local record. The portrait is in the bundle, so the
/// LoadingImage state (@4904-4980 until LoadAsset returns @841) is not observable. The lock is the port's own (a
/// fighter that is not ported), as on the Lab tile; the build's (NOT Is Character Owned?) is false offline.
/// Deviation: OnCharacterLockChanged's desaturated states (CanLockCharacter 1 / 2 / 5: Set Saturation(0),
/// partyHoverIndicator, the duplicate tooltip) are not ported: the result is native and the slate does not render a
/// RetainerBox with a general effect material (Mat_UIDesaturator), so the tile draws at saturation 1, the reset
/// branch @10-67.
#[derive(Debug, Default)]
pub struct PMCharacterItem {
    base: CollectionCharacterItem,
}

impl PMCharacterItem {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::<PMCharacterItem>::default()
    }

    /// The arcade block @2109-4248: no "localPlay" modal and IsArcadeModeActive (the screen's set-up: the pre-match
    /// is no local play), then the profile value "Characters.<slug>.arcade_completion_1p" (a solo party:
    /// GetIsPartyFull is read as false) from the local record (StatsArcade stars): 0 Collapsed @2913, else the star in
    /// the value's colour @3748, "1" @4159 and ShowArcadeCompletionStar @4248. The record is read on the tile's data
    /// (the build's GetServerProfileIntValue reads a cache).
    /// Unverified: the value is read as the highest difficulty won with the fighter (1 Easy, 2 Medium, 3 Hard); the
    /// backend wrote it, no client Blueprint does.
    fn show_arcade_completion(cx: &mut Cx<'_>, slug: &str) {
        let value = cx.view.session.stats.arcade().stars.get(slug).copied().unwrap_or(0);
        let star = find(cx, "ArcadeCompletion");
        if value == 0 {
            wd(cx, star).vis = Vis::Collapsed;
            return;
        }
        wd(cx, star).vis = Vis::SelfHitTestInvisible;
        let icon = find(cx, "ArcadeCompletionIcon");
        let c = ARCADE_STAR.get(usize::from(value) - 1).copied().unwrap_or([0.0; 4]);
        if wd(cx, icon).tex.is_some() || wd(cx, icon).mid.is_some() {
            cx.view.tree_mut().dynamic_material(icon).set_vector("FillColor", c);
        }
        let text = cx.view.game_text(ARCADE_ONE_PLAYER).to_string();
        set_text(cx, "ArcadeNumPlayers", &text);
        let me = cx.w;
        cx.view.play(me, "ShowArcadeCompletionStar", false, 1.0);
    }
}

impl GridItem for PMCharacterItem {
    fn state(&mut self) -> &mut GridItemState {
        self.base.state()
    }

    fn locked(&self) -> bool {
        self.base.locked()
    }

    fn set_size(&mut self, cx: &mut Cx<'_>, x: f64, y: f64) {
        self.base.set_size(cx, x, y);
    }

    fn enter_anim(&self) -> Option<&'static str> {
        Some("EnterAnim")
    }

    fn update_appearance(&mut self, cx: &mut Cx<'_>) {
        self.base.update_appearance(cx);
    }
}

impl Tile for PMCharacterItem {
    fn interactors(&self, cx: &Cx<'_>) -> Vec<WidgetId> {
        self.base.interactors(cx)
    }

    // portrait loaded @841-917; OnCharacterLockChanged's reset @29; the arcade block @4296; PM@113586
    fn set_data(&mut self, cx: &mut Cx<'_>, data: Option<TileData>) {
        let slug = match &data {
            Some(TileData::Roster(r)) => Some(r.slug.clone()),
            _ => None,
        };
        self.base.set_data(cx, data);
        set_vis(cx, "Content", Vis::SelfHitTestInvisible);
        set_vis(cx, "LoadingQuad", Vis::Collapsed);
        set_vis(cx, "partyHoverIndicator", Vis::Hidden);
        set_vis(cx, "ArcadeCompletion", Vis::Collapsed);
        if let Some(slug) = slug
            && session().is_some_and(|s| s.arcade.is_some())
        {
            Self::show_arcade_completion(cx, &slug);
        }
        set_vis(cx, "FreeRotationScaleBox", Vis::Collapsed);
        set_vis(cx, "UI_Attention", Vis::Hidden);
        set_vis(cx, "attentionNew", Vis::Hidden);
        set_vis(cx, "MuxyFighterVoteCount", Vis::Hidden);
    }
}

impl Controller for PMCharacterItem {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        self.base.on_initialized(cx);
        for i in self.interactors(cx) {
            bind(cx, i, |s| &mut s.on_focus, PM_ON_FOCUS);
        }
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        let me = cx.w;
        match id {
            PM_ON_FOCUS => {
                let q = find(cx, "Quad_IconBG");
                cx.visit::<Quadrilateral, _>(q, |q, cx| q.update_color(cx, PM_ICON_BG_FOCUS));
            }
            // UI_GridItemBase Focus @5081 / Select @6464: the class's Focus and Select animations (ErrorAnim for a
            // locked tile: its target LockedIndicator2 is not in the tree)
            T_FOCUS => {
                self.base.handle(cx, id, arg);
                cx.view.play(me, "SelectAnim", false, 1.0);
            }
            T_CLICK => {
                if self.base.state().is_entering {
                    return;
                }
                GridItemState::select(self, cx, player_of(arg));
                if !self.locked() {
                    cx.view.play(me, "SelectAnim", false, 1.0);
                }
            }
            _ => self.base.handle(cx, id, arg),
        }
    }

    fn resume(&mut self, cx: &mut Cx<'_>, id: u32) {
        self.base.resume(cx, id);
    }

    fn as_grid_item(&mut self) -> Option<&mut dyn Tile> {
        Some(self)
    }
}

// ---------- UI_PM_StateInfo: the header ----------

/// A row of UI_MatchFlowData's CharacterSelectTimeLimit (GetCharacterSelectTimeLimitStruct(GetMatchFlowID)).
#[derive(Clone, Debug, PartialEq)]
pub struct TimeLimitRow {
    /// StateText: the Game.locres key of the row's text.
    pub state_text: &'static str,
    pub time_limit: f64,
    pub count_up: bool,
    pub warning_threshold: f64,
    pub display_timer: bool,
}

/// Unverified: the row at the screen is read as "Rematch" (SetMatchFlowID("Rematch", true) MH@3649, 1.0 s after the
/// native rematch-started event); the leader's lobby also sets "CustomGame" (TimeLimit 0: no timer in state 1, while
/// states 2-4 still use the hard-coded 45 s formula). The native order of the two is not verified.
pub const ROW_REMATCH: TimeLimitRow = TimeLimitRow {
    state_text: STATE_SELECT_FIGHTER,
    time_limit: 45.0,
    count_up: false,
    warning_threshold: 10.0,
    display_timer: true,
};
/// The queue's row: the flow ID stays "Default" (no rematch sets another), TimeLimit 0: no timer.
pub const ROW_DEFAULT: TimeLimitRow = TimeLimitRow {
    state_text: STATE_SELECT_FIGHTER,
    time_limit: 0.0,
    count_up: false,
    warning_threshold: 10.0,
    display_timer: true,
};

/// UI_MatchFlowData's StateText of the Default and Rematch rows ("Select a Fighter").
const STATE_SELECT_FIGHTER: &str = "06A8A93F4C0A4AF1D662B29E6D4E6DD2";

const SI_WAIT_MAP: u32 = 1;

// UpdateMatchInfo's and UpdateArcadeTriesLeft's arcade texts (@8640, @9515, @11346-11561, @17465): Mode: {mode},
// Arcade; Arcade - Easy / Medium / Hard by EArcadeDifficulty; Tries Left:, Tries Left: {Tries}
const MODE_TEXT: &str = "379E05A74F33E66508DD8D9B9BAA7EFA";
const ARCADE_MODE: &str = "073A54424B37C6B8B8ACEDACCD1968F5";
const ARCADE_ROUND_LABEL: [&str; 3] =
    ["FDC14A54445B89024B5ECAA73F106D50", "51F00C654C63D0092D603FA302D7579B", "24AF51BB4A758D0B7DEF5FBC0F66B1B5"];
const TRIES_UNLIMITED: &str = "8E01A7564E427420B5256588B1EAB2AA";
const TRIES_LEFT: &str = "6CB98FE246BDE894C3BB44BD328966D5";
// UpdateMatchInfo's Normal @8576 and Custom @9023 (the {mode} of Mode: {mode})
const NORMAL_MODE: &str = "A627FC844113F803C63F149760BFE962";
const CUSTOM_MODE: &str = "4031761143B66BE4A8EF9399C7745D28";

/// UI_PM_StateInfo.
#[derive(Debug)]
pub struct PMStateInfo {
    pub on_time_expired: Event<()>,
    pub map_reveal_finished: Event<()>,
    pub on_time_updated: Event<f64>,
    pub warning_threshold_event: Event<()>,
    /// GetDisplayPreMatchTimer(GetMatchFlowID): the flow row's flag.
    pub display_timer: bool,
    count_up: bool,
    time_limit: f64,
    start_time: f64,
    expired: bool,
    warning_threshold: f64,
    warning_timer: bool,
    previous_time_left: f64,
    current_state: Option<String>,
    acc: f64,
    set_score: Option<(u32, u32)>,
    ffa: bool,
    clock_text: String,
    /// IsArcadeModeActive (PreMatchSetup.arcade), and UpdateMatchInfo's ArcadeRound and "Arcade Continues Left".
    arcade: bool,
    arcade_round: i32,
    arcade_continues: i32,
    /// The queue's modes (PreMatchSetup.queue): the screen runs the matchmaking path.
    queue: Option<Vec<mvs_protocol::online::QueueMode>>,
}

/// QueueLabel: the multiqueue configs' ToDisplayText @1275 (native). Unverified: read as the mode card's name that
/// WBP_MatchOptions' PrintSelectedModes shows for the same config: the Game.locres keys of WBP_GameModeCard's titles
/// 1 VS 1, TEAMS, FFA, and of PrintSelectedModes' SILLY (the Casual card's own title is "Silly Queue").
fn queue_label_key(q: mvs_protocol::online::QueueMode) -> &'static str {
    use mvs_protocol::online::QueueMode;
    match q {
        QueueMode::OneVOne => "C1413E164DB8DCE5DC2CC19A2E4AEBA5",
        QueueMode::TwoVTwo => "720C33654366ACE4376C668A91B54234",
        QueueMode::Ffa => "1789A04E4D313B3F325D3485EB970175",
        QueueMode::Silly => "B2EE21AE4D716D4BC84E558DB0B5FCC8",
    }
}

impl PMStateInfo {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        let setup: Option<PreMatchSetup> = session();
        Box::new(PMStateInfo {
            on_time_expired: Event::default(),
            map_reveal_finished: Event::default(),
            on_time_updated: Event::default(),
            warning_threshold_event: Event::default(),
            display_timer: true,
            count_up: false,
            time_limit: 0.0,
            start_time: 0.0,
            expired: false,
            warning_threshold: 10.0,
            warning_timer: false,
            previous_time_left: 0.0,
            current_state: None,
            acc: 0.0,
            set_score: setup.as_ref().and_then(|s| s.set_score),
            ffa: setup.as_ref().is_some_and(|s| s.team_style == 2),
            clock_text: String::new(),
            arcade: setup.as_ref().is_some_and(|s| s.arcade.is_some()),
            arcade_round: 0,
            arcade_continues: 0,
            queue: setup.as_ref().and_then(|s| s.net.as_ref()).and_then(|n| n.queue.as_ref()).map(|q| q.modes.clone()),
        })
    }

    /// HandleGlobalNavVisibility(IsVisible) SI@15555: Conditional Visibility(GlobalNavSpacer, IsVisible, Visible,
    /// Collapsed).
    pub fn handle_global_nav_visibility(&mut self, cx: &mut Cx<'_>, visible: bool) {
        set_vis(cx, "GlobalNavSpacer", if visible { Vis::Visible } else { Vis::Collapsed });
    }

    /// @7858, the custom game reading: "Mode: Custom"; PlaylistLabel Collapsed (a shipping build).
    /// Unverified: SizeBox_1's "Collapsed when both texts are empty" @1661-2515 is read as taken (no multiqueue
    /// config, no arcade tries text). A queue set's rematch, on the rematch path with its set score, is a matchmade
    /// match too: not custom.
    /// The arcade @7904-8445: ArcadeRound = bIsRematch ? GetArcadeInfo's round : 1, the continues GetArcadeInfo's or
    /// GetNumArcadeContinues(difficulty) (the run's either way: a new run starts with that number); "Mode: Arcade"
    /// @9515; ScoreContainer SelfHitTestInvisible @10860 (it holds ArcadeHeader), SetScoreBox Collapsed @11001;
    /// ArcadeHeader and guide_1 SelfHitTestInvisible @11089; the header up 39 @11251; ArcadeRoundLabel by the
    /// difficulty @11809; the queue area @1726-2410: WidgetSwitcher_2 on the tries (1), UpdateArcadeTriesLeft,
    /// SizeBox_1 shown (its tries text is never empty).
    pub fn update_match_info(&mut self, cx: &mut Cx<'_>) {
        let run = if self.arcade { cx.view.session.arcade.info().cloned() } else { None };
        if let Some(run) = run {
            self.arcade_round = if run.rematch { run.round } else { 1 };
            self.arcade_continues = run.continues;
            let mode = cx.view.game_text(MODE_TEXT).replace("{mode}", cx.view.game_text(ARCADE_MODE));
            set_text(cx, "ModeLabel", &mode);
            set_vis(cx, "PlaylistLabel", Vis::Collapsed);
            set_vis(cx, "ScoreContainer", Vis::SelfHitTestInvisible);
            set_vis(cx, "StateContainer", Vis::SelfHitTestInvisible);
            set_vis(cx, "SetScoreBox", Vis::Collapsed);
            set_vis(cx, "ArcadeHeader", Vis::SelfHitTestInvisible);
            set_vis(cx, "guide_1", Vis::SelfHitTestInvisible);
            let header = find(cx, "StateInfoHeaderContainer");
            wd(cx, header).transform.translation = [0.0, -39.0];
            // (an Expert run's label is the port's own)
            let label = if run.difficulty == ARCADE_EXPERT {
                custom_text::ARCADE_EXPERT.to_string()
            } else {
                usize::from(run.difficulty)
                    .checked_sub(1)
                    .and_then(|i| ARCADE_ROUND_LABEL.get(i))
                    .map(|k| cx.view.game_text(k).to_string())
                    .unwrap_or_default()
            };
            set_text(cx, "ArcadeRoundLabel", &label);
            let sw = find(cx, "WidgetSwitcher_2");
            wd(cx, sw).active = 1;
            self.update_arcade_tries_left(cx);
            set_vis(cx, "SizeBox_1", Vis::SelfHitTestInvisible);
            return;
        }
        // (a queue set's rematch, on the rematch path with its set score, is a matchmade match too: not custom)
        let normal = self.queue.is_some() || self.set_score.is_some();
        let m = cx.view.game_text(if normal { NORMAL_MODE } else { CUSTOM_MODE }).to_string();
        let mode = cx.view.game_text(MODE_TEXT).replace("{mode}", &m);
        set_text(cx, "ModeLabel", &mode);
        set_vis(cx, "PlaylistLabel", Vis::Collapsed);
        set_vis(cx, "ScoreContainer", Vis::Collapsed);
        set_vis(cx, "StateContainer", Vis::SelfHitTestInvisible);
        set_vis(cx, "ArcadeHeader", Vis::Collapsed);
        set_vis(cx, "guide_1", Vis::Collapsed);
        let header = find(cx, "StateInfoHeaderContainer");
        wd(cx, header).transform.translation = [0.0, 0.0];
        set_vis(cx, "SizeBox_1", if self.queue.is_some() { Vis::SelfHitTestInvisible } else { Vis::Collapsed });
        if let Some(q) = self.queue.clone() {
            let list: Vec<String> = q.into_iter().map(|m| cx.view.game_text(queue_label_key(m)).to_string()).collect();
            set_text(cx, "QueueLabel", &list.join(", "));
        }
    }

    /// SetInfo @3601 (the callers of a custom game pass threshold 10 and no dialogue).
    #[allow(clippy::too_many_arguments)] // the Blueprint function's own parameter list
    pub fn set_info(
        &mut self,
        cx: &mut Cx<'_>,
        state: &str,
        time_limit: f64,
        count_up: bool,
        warning_threshold: f64,
        wrap_header_text: bool,
        extra_info: &str,
    ) {
        self.count_up = count_up;
        self.clock_text.clear();
        set_vis(cx, "timerWarningIcon", Vis::Hidden);
        self.set_state(cx, state, wrap_header_text, extra_info);
        self.warning_threshold = warning_threshold;
        self.time_limit = time_limit.trunc();
        self.start_time = cx.now() - if count_up { self.time_limit } else { 0.0 };
        self.expired = self.time_limit <= 0.0;
        if self.time_limit > 0.0 {
            let me = cx.w;
            cx.view.play_animation(me, "TimerAnim", false, 1.0, 1, false, true); // @4133, bRestoreState
        }
        // (the root follows every SetInfo with UpdateSetScore(bIsRematch, TeamStyle == 2))
        self.update_set_score(cx);
    }

    /// UpdateSetScore @14588, the rematch branch @143: DetermineScore -> BlueSetScoreLabel (yours) and
    /// RedSetScoreLabel (theirs) @428-612, ScoreContainer Collapsed in free for all, else SelfHitTestInvisible @657.
    /// A screen with no set keeps UpdateMatchInfo's Collapsed @10860. The build's retry, Delay(0) @717 while the
    /// selections are not in, has no counterpart: the set is the setup's.
    pub fn update_set_score(&mut self, cx: &mut Cx<'_>) {
        let Some((yours, theirs)) = self.set_score else { return };
        set_text(cx, "BlueSetScoreLabel", &yours.to_string());
        set_text(cx, "RedSetScoreLabel", &theirs.to_string());
        set_vis(cx, "ScoreContainer", if self.ffa { Vis::Collapsed } else { Vis::SelfHitTestInvisible });
    }

    /// The seconds left of the running limit.
    fn remaining(&self, cx: &Cx<'_>) -> f64 {
        (self.time_limit - (cx.now() - self.start_time).ceil()).clamp(0.0, self.time_limit.max(0.0))
    }

    /// SetInfoStruct @18075: the same text keeps the running clock.
    pub fn set_info_struct(&mut self, cx: &mut Cx<'_>, row: &TimeLimitRow) {
        let remaining = self.remaining(cx);
        let text = cx.view.game_text(row.state_text).to_string();
        let same = self.current_state.as_deref() == Some(text.as_str());
        let limit = if same { remaining } else { row.time_limit };
        self.set_info(cx, &text, limit, row.count_up, row.warning_threshold, false, "");
    }

    /// SetState @13683.
    pub fn set_state(&mut self, cx: &mut Cx<'_>, state: &str, wrap_header_text: bool, extra_info: &str) {
        let label = find(cx, "stateLabel");
        let x = wd(cx, label);
        x.text = state.to_uppercase();
        x.auto_wrap = wrap_header_text;
        // Unverified: "else cleared" @13865 is read as the tree's width (620) coming back
        let b = find(cx, "SizeBox_3");
        wd(cx, b).w_override = if wrap_header_text { 420.0 } else { 620.0 };
        set_text(cx, "stateExtraInfoLabel", extra_info);
        set_vis(
            cx,
            "stateExtraInfoLabel",
            if extra_info.is_empty() { Vis::Collapsed } else { Vis::SelfHitTestInvisible },
        );
        if self.current_state.as_deref() != Some(state) {
            let me = cx.w;
            cx.view.play_animation(me, "StateAnim", false, 1.0, 1, false, true); // PlayAnimationForward @14141
        }
        self.current_state = Some(state.to_string());
    }

    /// HideState @17380.
    pub fn hide_state(&mut self, cx: &mut Cx<'_>) {
        set_vis(cx, "stateLabel", Vis::Collapsed);
    }

    /// ShowArcadeLabelHeader @17296: the round label overlay.
    pub fn show_arcade_label_header(&mut self, cx: &mut Cx<'_>) {
        let w = find(cx, "ArcadeHeader");
        wd(cx, w).active = 0;
    }

    /// ShowArcadeProgression @17338: the progress pips.
    pub fn show_arcade_progression(&mut self, cx: &mut Cx<'_>) {
        let w = find(cx, "ArcadeHeader");
        wd(cx, w).active = 1;
    }

    /// UpdateRivalPortrait @17419.
    pub fn update_rival_portrait(&mut self, cx: &mut Cx<'_>, rival_slug: Option<&str>) {
        let p = find(cx, "UI_ArcadeProgress_160");
        cx.visit::<AXProgress, _>(p, |a, cx| a.update_rival_portrait(cx, rival_slug));
    }

    /// UpdateArcadeDetails @18541: in arcade, the pips' UpdateStageNumber @18597, then UpdateArcadeTriesLeft.
    pub fn update_arcade_details(&mut self, cx: &mut Cx<'_>) {
        if !self.arcade {
            return;
        }
        let p = find(cx, "UI_ArcadeProgress_160");
        cx.visit::<AXProgress, _>(p, |a, cx| a.update_stage_number(cx));
        self.update_arcade_tries_left(cx);
    }

    /// UpdateArcadeTriesLeft @17465-18015: unlimited (<= -1) "Tries Left:" and the infinity sign, else "Tries Left: n".
    fn update_arcade_tries_left(&mut self, cx: &mut Cx<'_>) {
        let unlimited = self.arcade_continues <= -1;
        let text = if unlimited {
            cx.view.game_text(TRIES_UNLIMITED).to_string()
        } else {
            cx.view.game_text(TRIES_LEFT).replace("{Tries}", &self.arcade_continues.to_string())
        };
        set_text(cx, "ArcadeTriesLabel", &text);
        set_vis(cx, "InfinityScaleBox", if unlimited { Vis::SelfHitTestInvisible } else { Vis::Collapsed });
    }

    /// ShowMatchmakingSpinner @14316.
    pub fn show_matchmaking_spinner(&mut self, cx: &mut Cx<'_>, b: bool) {
        set_vis(cx, "SpinnerBox", if b { Vis::SelfHitTestInvisible } else { Vis::Collapsed });
        let hide = !b || self.current_state.as_deref().is_none_or(str::is_empty);
        set_vis(cx, "SpacerBalance", if hide { Vis::Collapsed } else { Vis::SelfHitTestInvisible });
    }

    /// SetMapName @14667 (the player controller's BPI_PreMatchTransition(NextMapPath) @14863 is the caller's).
    pub fn set_map_name(&mut self, cx: &mut Cx<'_>, name: &str, experimental: bool) {
        set_text(cx, "MapLabelSide", name);
        // UI_PM_Experimental.ShowExperimentalTag(exp, false)
        set_vis(cx, "UI_PM_Experimental", if experimental { Vis::SelfHitTestInvisible } else { Vis::Hidden });
        let me = cx.w;
        if !cx.view.is_animation_playing(me, "Mapreveal") {
            cx.view.play(me, "Mapreveal", false, 1.0);
        } else if !cx.view.is_animation_playing_forward(me, "Mapreveal") {
            cx.view.reverse_animation(me, "Mapreveal");
            cx.view.set_playback_speed(me, "Mapreveal", 1.0);
        }
        // Delay 1.5 @1012, then wait while it plays @35-88
        cx.view.delay(1.5, handler(cx, SI_WAIT_MAP));
    }

    /// UpdateTimer @4181 (GetDisplayPreMatchWarningTimer(None) is false: the row "None" does not exist).
    fn update_timer(&mut self, cx: &mut Cx<'_>) {
        let running = self.time_limit > 0.0 || self.count_up;
        let show = self.warning_timer || (self.display_timer && running);
        set_vis(cx, "CenterTimerContainer", if show { Vis::Visible } else { Vis::Hidden });
        if !running {
            return;
        }
        if self.count_up {
            // @4781-4965: TimeFormatter(FromSeconds(now - startTime), false, false, false): "M:SS". Set when it
            // changes (the text's layout).
            let text = clock_text((cx.now() - self.start_time).floor().max(0.0) as u64);
            if text != self.clock_text {
                set_text(cx, "timerLabel", &text);
                self.clock_text = text;
            }
            return;
        }
        let remaining = self.remaining(cx);
        set_text(cx, "timerLabel", &format!("{}{}", if remaining < 10.0 { "0" } else { "" }, remaining as i64));
        let decreased = remaining < self.previous_time_left;
        if remaining < self.warning_threshold && decreased {
            self.warning_timer = true;
            cx.post("ui_countdown_tick");
            set_vis(cx, "timerWarningIcon", Vis::Visible);
            let me = cx.w;
            cx.view.play_animation(me, "TimerAnim", false, 1.0, 1, false, true); // @6353, bRestoreState
        }
        if remaining == self.warning_threshold && decreased {
            cx.emit(self, |c| &c.warning_threshold_event, &());
        }
        self.previous_time_left = remaining;
        if remaining <= 0.0 && !self.expired {
            self.expired = true;
            cx.emit(self, |c| &c.on_time_expired, &());
        }
        cx.emit(self, |c| &c.on_time_updated, &remaining);
    }
}

/// The count-up clock's "M:SS" (UI_Functions' minutes and seconds branch, the seconds padded below 10).
fn clock_text(t: u64) -> String {
    let s = t % 60;
    format!("{}:{}{}", t / 60, if s < 10 { "0" } else { "" }, s)
}

impl Controller for PMStateInfo {
    // @11931 (bIsCustomGame); not custom and not the arcade @13492: QueueLabel and Switcher_ChangeQueue Visible. Port:
    // Change Mode is not ported, its switcher stays Collapsed; the queue is not ported, QueueLabel Collapsed.
    // The arcade @13168-13491: QueueLabel (and the switcher) Collapsed when ArcadeRound > 1, else Visible (ArcadeRound
    // is still its default 0 here: UpdateMatchInfo has not run), then the event ends: no tick timer (the clock never
    // runs in arcade, so OnTimeExpired never fires), no multiqueue bind and no UpdateMatchInfo (the root's SetState
    // runs it, PM@34317).
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        self.warning_threshold = prop_f64(cx, "warningThreshold", 10.0);
        set_vis(cx, "Switcher_ChangeQueue", Vis::Collapsed);
        if self.arcade {
            set_vis(cx, "QueueLabel", if self.arcade_round > 1 { Vis::Collapsed } else { Vis::Visible });
            // Deviation: no timer on the arcade's fighter select. The build starts
            // no tick timer here, so CenterTimerContainer keeps its tree's state: shown, timerLabel's designer "00".
            set_vis(cx, "CenterTimerContainer", Vis::Hidden);
            return;
        }
        set_vis(cx, "QueueLabel", if self.queue.is_some() { Vis::Visible } else { Vis::Collapsed });
        self.update_match_info(cx);
    }

    // The looping 0.016667 s timer of OnInitialized @12222 (cleared in Destruct): UpdateTimer reads the real time, so
    // one run a frame gives what several would.
    fn timers(&mut self, cx: &mut Cx<'_>, dt: f64) {
        if self.arcade {
            return;
        }
        self.acc += dt;
        if self.acc < 1.0 / 60.0 {
            return;
        }
        self.acc %= 1.0 / 60.0;
        self.update_timer(cx);
    }

    fn resume(&mut self, cx: &mut Cx<'_>, id: u32) {
        if id != SI_WAIT_MAP {
            return;
        }
        let me = cx.w;
        if cx.view.is_animation_playing(me, "Mapreveal") {
            cx.view.delay(0.0, handler(cx, SI_WAIT_MAP));
        } else {
            cx.emit(self, |c| &c.map_reveal_finished, &());
        }
    }
}

// ---------- UI_PM_PlayerInfo: a name plate ----------

const PLATE_GOLD: Rgba = [0.665387, 0.371238, 0.016808, 1.0];
const BADGE_NAMES: [&str; 3] = ["StatBadge", "StatBadge2", "StatBadge3"];
const DEFAULT_BADGE: &str = "stat_tracking_bundle_default";
const PI_SHOW_DIAMOND: u32 = 1;
const PI_BOT: &str = "4831DD4241C0B5DE94EED3BE5C00E0CE";

/// Unverified: IconSizeBox, nameContainer, StatusHolder and characterLabel are Collapsed in the tree and no decoded
/// Blueprint line shows them (SetCharacter sets Icon Visible inside the Collapsed box; SetStatus does not use its
/// text): as decoded the widget draws its stat badges (StatBadgeSizeBox) alone. The values are kept as the build sets
/// them. Not ported: the palette colours of IconOutline / IconBG / the spinner (UpdateColors @7540-7991, inside the
/// Collapsed box), the portrait texture (NewCharacterSelectPortraitTexture), UserPlatform.
#[derive(Debug)]
pub struct PMPlayerInfo {
    pub account_id: String,
    pub character: Option<RosterEntry>,
    diamond: bool,
    /// Stat Tracker Slug 1-3 (SetStatBadgeInfo).
    badge_slugs: [String; 3],
    /// The match config's TeamStyle (GetActiveMatchConfigForContext; -1: none), which the screen sets.
    pub team_style: i32,
}

impl PMPlayerInfo {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(PMPlayerInfo {
            account_id: String::new(),
            character: None,
            diamond: false,
            badge_slugs: Default::default(),
            team_style: -1,
        })
    }

    /// SetPlayerInfo @8033 -> SetUsername @1420 (an empty name: "BOT" @1463).
    pub fn set_player_info(&mut self, cx: &mut Cx<'_>, account_id: &str, username: &str) {
        self.account_id = account_id.to_string();
        let name = if username.is_empty() { cx.view.game_text(PI_BOT).to_string() } else { username.to_string() };
        set_text(cx, "nameLabel", &name);
    }

    /// SetCharacter @1626.
    pub fn set_character(&mut self, cx: &mut Cx<'_>, c: Option<&RosterEntry>) {
        self.character = c.cloned();
        let Some(c) = c else {
            set_vis(cx, "characterLabel", Vis::Collapsed);
            set_vis(cx, "classContainer", Vis::Collapsed);
            return;
        };
        set_text(cx, "characterLabel", &c.display_name.to_uppercase());
        if let Some(icon) = cx.view.class_info(&c.class).map(|i| i.icon.clone()) {
            let w = find(cx, "classIcon");
            wd(cx, w).tex = Some(icon);
        }
        set_vis(cx, "classContainer", Vis::Visible);
        Self::update_colors(cx);
    }

    /// UpdateColors @6591: the gold name backing for playerIndex 0 only.
    fn update_colors(cx: &mut Cx<'_>) {
        let first = prop_f64(cx, "playerIndex", 0.0) as i32 == 0;
        let w = find(cx, "nameContainer");
        wd(cx, w).brush_color = if first { PLATE_GOLD } else { [0.0; 4] };
    }

    /// ForceShowDiamond @8746: a DoOnce, then the 1.5 s timer AnimTimerDelay -> PlayAnimation(ShowAnim) @8794.
    pub fn force_show_diamond(&mut self, cx: &mut Cx<'_>) {
        if self.diamond {
            return;
        }
        self.diamond = true;
        cx.view.delay(1.5, handler(cx, PI_SHOW_DIAMOND));
    }

    /// SetStatBadgeInfo(slug, count x3) @9402: per badge (@9564-10160) an empty slug or the default bundle's
    /// Collapsed @9810, else the bundle found by slug in the catalogue: its equipper data with the count, the badge
    /// SelfHitTestInvisible (a slug the catalogue lacks does nothing).
    pub fn set_stat_badge_info(&mut self, cx: &mut Cx<'_>, badges: &[BadgeRef]) {
        for (i, name) in BADGE_NAMES.iter().enumerate() {
            let b = badges.get(i).cloned().unwrap_or_default();
            self.badge_slugs[i].clone_from(&b.slug);
            let w = find(cx, name);
            if b.slug.is_empty() || b.slug == DEFAULT_BADGE {
                wd(cx, w).vis = Vis::Collapsed;
                continue;
            }
            let item =
                cx.view.bundle.menu.as_ref().and_then(|m| m.cosmetics.stat_tracker.iter().find(|e| e.slug == b.slug));
            let Some(item) = item.map(badge_item) else { continue };
            let d = badge_item_data(cx, &item, b.count);
            cx.visit::<StatBadge, _>(w, |s, cx| s.set_equipper(cx, &d));
            wd(cx, w).vis = Vis::SelfHitTestInvisible;
        }
    }

    /// ForceShowDiamond's tail @3017: each badge Visible unless its slug is empty or the default bundle's
    /// @3022-3518, then the translations: FreeForAll @123-235, else by the side index @3770-6482 (no match config:
    /// index 0; team style 0 the plate's playerIndex, 1 {0, 1, 0, 1}[playerIndex], 3 index 0; an index past 1 moves
    /// none).
    fn place_badges(&mut self, cx: &mut Cx<'_>) {
        for (i, name) in BADGE_NAMES.iter().enumerate() {
            let s = &self.badge_slugs[i];
            let w = find(cx, name);
            wd(cx, w).vis = if s.is_empty() || s == DEFAULT_BADGE { Vis::Collapsed } else { Vis::Visible };
        }
        let at = badge_offsets(self.team_style, prop_f64(cx, "playerIndex", 0.0) as usize);
        for (name, [x, y]) in BADGE_NAMES.iter().zip(at) {
            let w = find(cx, name);
            wd(cx, w).transform.translation = [x, y];
        }
    }

    /// IsOccupied.
    pub fn is_occupied(&self) -> bool {
        self.character.is_some() && !self.account_id.is_empty()
    }
}

/// The three badges' translations by the team style (-1: no match config) and the plate's playerIndex.
fn badge_offsets(style: i32, player_index: usize) -> [[f64; 2]; 3] {
    if style == 2 {
        return [[-40.0, 0.0], [-20.0, 120.0], [0.0, 240.0]];
    }
    let side = match style {
        0 => player_index,
        1 => [0, 1, 0, 1].get(player_index).copied().unwrap_or(0),
        _ => 0,
    };
    match side {
        0 => [[-20.0, 0.0], [0.0, 120.0], [20.0, 240.0]],
        1 => [[20.0, 0.0], [0.0, 120.0], [-20.0, 240.0]],
        _ => [[0.0; 2]; 3],
    }
}

impl Controller for PMPlayerInfo {
    // OnInitialized @8169, PreConstruct @6576
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        self.set_character(cx, None);
    }

    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        Self::update_colors(cx);
    }

    fn resume(&mut self, cx: &mut Cx<'_>, id: u32) {
        if id != PI_SHOW_DIAMOND {
            return;
        }
        let me = cx.w;
        if !cx.view.is_animation_playing(me, "ShowAnim") {
            cx.view.play(me, "ShowAnim", false, 1.0);
        }
        self.place_badges(cx);
    }
}

// ---------- UI_PM_EditEmotes ----------

const EE_CLICK: u32 = 1;

/// The pre-match's move list button (ShowMoveListP0; the class name is the build's, it edits no emotes): one icon
/// button, its click the screen's (OnClick -> ClickEvent @10). UpdateContext @226 sets the button's context;
/// PreConstruct @34 (Player Index, the empty label's Init) is left as PMEditPerks leaves it.
#[derive(Debug, Default)]
pub struct PMEditEmotes {
    pub click_event: Event<()>,
}

impl PMEditEmotes {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::<PMEditEmotes>::default()
    }

    pub fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str) {
        let b = find(cx, "UI_ButtonSecondary");
        let i = ButtonSecondary::interactor(cx, b);
        if let Some(s) = ist(cx, i) {
            s.context = ctx.to_string();
        }
    }
}

impl Controller for PMEditEmotes {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let b = find(cx, "UI_ButtonSecondary");
        let h = handler(cx, EE_CLICK);
        cx.visit::<ButtonSecondary, _>(b, |b, _| b.on_click.add(h));
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, _arg: &dyn Any) {
        if id == EE_CLICK {
            cx.emit(self, |c| &c.click_event, &());
        }
    }
}

// ---------- UI_PM_GameplayTips ----------

const GT_NEXT: u32 = 1 << 16;
const GT_GEN_MASK: u32 = 0xFFFF;

/// UI_PM_GameplayTips: the matchmaking state's rotating tip. CDO: Current Index -1, Tip Time 10, Auto Play true.
#[derive(Debug)]
pub struct PMGameplayTips {
    /// All Tips: UI_GameplayTipDataTable's Tip texts as Game.locres keys, shuffled once (Load Tips' DoOnce).
    all_tips: Option<Vec<String>>,
    current_index: i32,
    tip_time: f64,
    auto_play: bool,
    /// Next Tip Timer: a pending delay fires only while its generation is current (ClearAndInvalidate bumps it).
    timer_gen: u32,
}

impl PMGameplayTips {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(PMGameplayTips { all_tips: None, current_index: -1, tip_time: 10.0, auto_play: true, timer_gen: 0 })
    }

    /// Load Tips @472-692 (a DoOnce): every row's Tip of UI_GameplayTipDataTable in row order, then Array_Shuffle
    /// (UE 4.26 GenericArray_Shuffle: for i in 0..=last, swap i with RandRange(i, last)).
    /// Unverified: the random stream is the menus' (Manager::random), not the engine's FMath::Rand.
    fn load_tips(&mut self, cx: &mut Cx<'_>) {
        if self.all_tips.is_some() {
            return;
        }
        let mut tips = cx.view.bundle.menu.as_ref().map(|m| m.gameplay_tips.clone()).unwrap_or_default();
        let last = tips.len().saturating_sub(1);
        for i in 0..tips.len() {
            let j = (i + (cx.view.manager.random() * (last - i + 1) as f64).floor() as usize).min(last);
            if i != j {
                tips.swap(i, j);
            }
        }
        self.all_tips = Some(tips);
    }

    /// Show Next Tip @708-1121: Load Tips, clear the timer, Current Index = WrapInt(Current Index, 1, 0, LastIndex),
    /// tipLabel the tip, TipEnterAnim forward, and with Auto Play the next in Tip Time seconds (a one-shot timer).
    /// With no tips (no table in the bundle) the label is left as it is: the build's Array get would read past the
    /// end.
    pub fn show_next_tip(&mut self, cx: &mut Cx<'_>) {
        self.load_tips(cx);
        self.timer_gen = self.timer_gen.wrapping_add(1);
        let tips = self.all_tips.as_deref().unwrap_or_default();
        if tips.is_empty() {
            return;
        }
        let last = tips.len() as i32 - 1;
        let mut i = self.current_index + 1;
        if i < 0 {
            i = last;
        } else if i > last {
            i = 0;
        }
        self.current_index = i;
        let key = tips[i as usize].clone();
        let text = cx.view.game_text(&key).to_string();
        set_text(cx, "tipLabel", &text);
        let me = cx.w;
        cx.view.play(me, "TipEnterAnim", false, 1.0);
        if self.auto_play {
            cx.view.delay(self.tip_time, handler(cx, GT_NEXT | (self.timer_gen & GT_GEN_MASK)));
        }
    }
}

impl Controller for PMGameplayTips {
    // Destruct @1122: ClearAndInvalidateTimerHandle(Next Tip Timer).
    fn destruct(&mut self, _cx: &mut Cx<'_>) {
        self.timer_gen = self.timer_gen.wrapping_add(1);
    }

    fn resume(&mut self, cx: &mut Cx<'_>, id: u32) {
        if id & !GT_GEN_MASK == GT_NEXT && id & GT_GEN_MASK == self.timer_gen & GT_GEN_MASK {
            self.show_next_tip(cx);
        }
    }
}

// ---------- UI_PM_Confirm ----------

const CF_CLICK: u32 = 1;

#[derive(Debug, Default)]
pub struct PMConfirm {
    pub click_event: Event<()>,
}

impl PMConfirm {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::<PMConfirm>::default()
    }

    fn label(cx: &Cx<'_>) -> String {
        text_of(cx.view.tree().props(cx.w).get("Label"))
    }

    fn button(cx: &Cx<'_>) -> WidgetId {
        find(cx, "UI_ButtonMain")
    }

    pub fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str) {
        let b = Self::button(cx);
        let i = cx.view.tree().find_in(b, "UI_Interactor");
        interactor::set_context(cx, i, ctx);
    }

    /// SetCanConfirmState(LockStatus): CanLockCharacter's result (0 can lock, 1 taken, 5 disabled).
    pub fn set_can_confirm_state(&mut self, cx: &mut Cx<'_>, status: i32) {
        let b = Self::button(cx);
        let i = cx.view.tree().find_in(b, "UI_Interactor");
        interactor::set_interactable(cx, i, status == 0);
        let label = match status {
            1 => "CHARACTER TAKEN".to_string(),
            5 => "CHARACTER DISABLED".to_string(),
            _ => Self::label(cx),
        };
        cx.visit::<ButtonMain, _>(b, |b, cx| b.set_label(cx, &label));
    }
}

impl Controller for PMConfirm {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let b = Self::button(cx);
        let h = handler(cx, CF_CLICK);
        cx.visit::<ButtonMain, _>(b, |b, _| b.on_click.add(h));
        // Update FWOTD Time: the strip needs the backend's quest dates: Collapsed, with the paddings of that branch
        // (the tree's own, to the half pixel)
        set_vis(cx, "FWOTD_Strip", Vis::Collapsed);
        set_vis(cx, "FWOTD_TextContainer", Vis::Collapsed);
        wd(cx, b).pad.t = 55.5;
    }

    // PreConstruct @502, Construct @701
    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        let b = Self::button(cx);
        let label = Self::label(cx);
        cx.visit::<ButtonMain, _>(b, |b, cx| b.set_label(cx, &label));
    }

    fn construct(&mut self, cx: &mut Cx<'_>) {
        let b = Self::button(cx);
        let i = cx.view.tree().find_in(b, "UI_Interactor");
        let fc = find(cx, "UI_FocusedCommon_Play");
        focus_frame(cx, fc, i);
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, _arg: &dyn Any) {
        if id == CF_CLICK {
            cx.emit(self, |c| &c.click_event, &());
        }
    }
}

// ---------- UI_PM_SkinSelector: the skin strip ----------

const SK_SWITCH: u32 = 1;
const SK_INTERACTOR_FOCUSED: u32 = 2;
const SK_ITEM_FOCUSED: u32 = 3;
const SK_ITEM_SELECTED: u32 = 4;
const SK_BUTTON: u32 = 10;
const SK_BUTTONS: [&str; 4] = ["buttonPrevious", "buttonNext", "buttonPrevious_1", "buttonNext_1"];

/// Not ported: the tiles' tooltips (@2393-2627) and the "Coming soon!" one of a one-skin character (@3063: its Grid
/// Items[1] is out of range with one skin as decoded), the inventory delegate. IsLocked is false offline (every skin
/// owned): every tile is taken. The skin the strip fires is the tile's SkinInfo; the screen carries its SkinData name.
#[derive(Debug, Default)]
pub struct PMSkinSelector {
    /// SkinFocusedEvent(Player Index, Skin), SkinSelectedEvent(Player Index, Skin, IsLocked).
    pub skin_focused_event: Event<SkinInfo>,
    pub skin_selected_event: Event<SkinInfo>,
    pub character: Option<RosterEntry>,
    pub focused_skin: Option<SkinInfo>,
    pub selected_skin: Option<SkinInfo>,
    skins: Vec<SkinInfo>,
    context: String,
}

impl PMSkinSelector {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::<PMSkinSelector>::default()
    }

    fn player_index(cx: &Cx<'_>) -> i32 {
        prop_f64(cx, "Player Index", 0.0) as i32
    }

    fn list(cx: &Cx<'_>) -> WidgetId {
        find(cx, "skinList")
    }

    fn buttons(cx: &Cx<'_>) -> Vec<WidgetId> {
        SK_BUTTONS.iter().map(|n| find(cx, n)).collect()
    }

    fn button_interactors(cx: &Cx<'_>) -> Vec<WidgetId> {
        Self::buttons(cx).into_iter().map(|b| InputLabelButton::interactor_of(cx, b)).collect()
    }

    /// The interactors of the list's tiles.
    fn tile_interactors(cx: &mut Cx<'_>) -> Vec<WidgetId> {
        let list = Self::list(cx);
        let items = cx.visit::<GridItemList, _>(list, |l, _| l.items()).unwrap_or_default();
        items.into_iter().map(|t| cx.view.tree().find_in(t, "UI_Interactor")).collect()
    }

    /// GetSavedSelectedSkin @0-632: on a LocalPlaySetting instance with a skinSlug in the slot's LocalPlay settings,
    /// the skin its skinReference loads (none when it does not: the port's IsDefaultSkin one is not taken); else
    /// GetSelectedSkinForCharacter (the Hydra preferences' skin, else the default).
    fn saved_skin(&self, cx: &Cx<'_>) -> Option<SkinInfo> {
        let c = self.character.as_ref()?;
        if prop_bool(cx, "LocalPlaySetting") {
            let p = usize::try_from(Self::player_index(cx)).ok().and_then(|i| cx.prefs.local_play.players.get(i));
            if let Some(p) = p.filter(|p| !p.skin_slug.is_empty()) {
                // GetSavedSelectedSkin @390 / @603: a Local Play slot with a skin slug takes the skin its
                // skinReference loads, and none (no tile checked; the default plays) when the reference is empty or
                // names no skin of the fighter
                return (!p.skin_reference.is_empty())
                    .then(|| self.skins.iter().find(|s| s.path.eq_ignore_ascii_case(&p.skin_reference)).cloned())
                    .flatten();
            }
        }
        let fighter = c.fighter.as_ref().and_then(|f| cx.view.fighters().get(f));
        let slug = fighter.map(|f| cx.prefs.hydra.character_skin_slug(&f.slug)).unwrap_or_default();
        self.skins
            .iter()
            .find(|s| !slug.is_empty() && s.slug == slug)
            .or_else(|| self.skins.iter().find(|s| s.is_default))
            .cloned()
    }

    /// SetCharacter @467 -> Populate @1654.
    pub fn set_character(&mut self, cx: &mut Cx<'_>, c: Option<&RosterEntry>) {
        self.character = c.cloned();
        self.focused_skin = None;
        self.selected_skin = None;
        self.skins = c
            .and_then(|c| c.fighter.as_ref())
            .and_then(|f| cx.view.fighters().get(f))
            .map(|f| f.skins.clone())
            .unwrap_or_default();
        if c.is_some() {
            let saved = self.saved_skin(cx).map(|k| k.slug);
            let data: Vec<TileData> = self
                .skins
                .iter()
                .map(|s| TileData::Skin(SkinTile { skin: s.clone(), selected: Some(&s.slug) == saved.as_ref() }))
                .collect();
            let list = Self::list(cx);
            cx.visit::<GridItemList, _>(list, |l, cx| l.populate(cx, data));
            let scroll = find(cx, "listScrollbox");
            let items = cx.visit::<GridItemList, _>(list, |l, _| l.items()).unwrap_or_default();
            for (item, skin) in items.into_iter().zip(self.skins.clone()) {
                let i = cx.view.tree().find_in(item, "UI_Interactor");
                batch_set_scroll_box(cx, &[i], scroll);
                cx.view.tree_mut().set_prop(item, "DimsIcon", json!(false));
                if Some(&skin.slug) == saved.as_ref() {
                    self.update_selection(&skin, true);
                }
            }
            self.update_appearance(cx);
        }
        let ctx = self.context.clone();
        self.update_context(cx, &ctx);
    }

    /// UpdateAppearance @524.
    fn update_appearance(&mut self, cx: &mut Cx<'_>) {
        if self.character.is_none() {
            return;
        }
        if let Some(f) = &self.focused_skin {
            set_text(cx, "skinNameLabel", if f.is_default { "Default" } else { &f.display_name });
            // (no skin of the port's bundle data is experimental)
            set_vis(cx, "UI_PM_Experimental", Vis::Hidden);
        }
        let saved = self.saved_skin(cx).map(|s| s.slug);
        let list = Self::list(cx);
        // (run, not visit: a click on a tile calls this while the list emits its selected event)
        run_on::<GridItemList>(cx, list, move |l, cx| l.set_skin_selected(cx, saved.as_deref()));
    }

    /// Update Selection @6766.
    fn update_selection(&mut self, skin: &SkinInfo, selected: bool) {
        self.focused_skin = Some(skin.clone());
        if selected {
            self.selected_skin = Some(skin.clone());
        }
    }

    /// FocusItem @8491.
    fn focus_item(&mut self, cx: &mut Cx<'_>, skin: &SkinInfo) {
        self.update_selection(skin, false);
        cx.emit(self, |c| &c.skin_focused_event, skin);
        self.update_appearance(cx);
    }

    /// CanPaginate? @0-383: the player, visible in the hierarchy, and no modal or the latest modal's id in the
    /// instance's Pagination Allowed Modal IDs (["localPlay"] on UI_LP_Player_Skin's selector, unset on the pre-match
    /// one).
    fn can_paginate(cx: &Cx<'_>, player: i32) -> bool {
        if player != Self::player_index(cx) || !cx.view.visible_in_hierarchy(cx.w) {
            return false;
        }
        if !Manager::is_modal_active(cx.view) {
            return true;
        }
        let latest = Manager::latest_modal_id(cx.view);
        prop(cx, "Pagination Allowed Modal IDs")
            .as_array()
            .is_some_and(|a| a.iter().any(|v| v.as_str() == Some(latest.as_str())))
    }

    /// OnSwitchPage @5838: the page keys focus and select in one step, wrapping.
    fn on_switch_page(&mut self, cx: &mut Cx<'_>, player: i32, dir: i32) {
        if !Self::can_paginate(cx, player) {
            return;
        }
        let n = self.skins.len() as i32;
        if n == 0 {
            return;
        }
        Manager::unfocus_all(cx, player);
        let at = self
            .focused_skin
            .as_ref()
            .and_then(|f| self.skins.iter().position(|s| s.slug == f.slug))
            .map_or(-1, |i| i as i32);
        let index = (((at + dir) % n) + n) % n;
        let list = Self::list(cx);
        let pi = Self::player_index(cx);
        cx.visit::<GridItemList, _>(list, |l, cx| l.focus_index(cx, pi, index as usize));
        let items = cx.visit::<GridItemList, _>(list, |l, _| l.items()).unwrap_or_default();
        if let Some(&item) = items.get(index as usize) {
            // item.focus(player): the interactor's focus above reached the tile; item.select(player) (never locked)
            run_tile(cx, item, move |t, cx| GridItemState::select(t, cx, pi));
        }
    }

    /// OnInteractorFocused @7361: when the focus leaves the strip the preview goes back to the selected skin.
    fn on_interactor_focused(&mut self, cx: &mut Cx<'_>, i: WidgetId) {
        if !cx.view.visible_in_hierarchy(cx.w) {
            return;
        }
        let Some(p) = cx.view.manager.interactor(i).map(|s| s.player_index) else { return };
        if p != Self::player_index(cx) {
            return;
        }
        if Self::tile_interactors(cx).contains(&i) || Self::button_interactors(cx).contains(&i) {
            return;
        }
        let same = self.focused_skin.as_ref().map(|s| &s.slug) == self.selected_skin.as_ref().map(|s| &s.slug);
        if same {
            return;
        }
        if let Some(sel) = self.selected_skin.clone().filter(|s| self.skins.iter().any(|k| k.slug == s.slug)) {
            self.focus_item(cx, &sel);
        }
    }

    /// UpdateContext @7984.
    pub fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str) {
        self.context = ctx.to_string();
        let list: Vec<WidgetId> = Self::button_interactors(cx);
        batch_set_context(cx, &list, ctx);
        let l = Self::list(cx);
        cx.visit::<GridItemList, _>(l, |l, cx| l.update_context(cx, ctx));
    }
}

impl Controller for PMSkinSelector {
    // @5463; the list's events @4335 / @4368; the buttons @7332 / @7303 / @9583 / @9554
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let (sw, fo) = (handler(cx, SK_SWITCH), handler(cx, SK_INTERACTOR_FOCUSED));
        cx.view.manager.on_request_switch_page.add(sw);
        cx.view.manager.on_interactor_focused.add(fo);
        let (hf, hs) = (handler(cx, SK_ITEM_FOCUSED), handler(cx, SK_ITEM_SELECTED));
        let list = Self::list(cx);
        cx.visit::<GridItemList, _>(list, |l, _| {
            l.focused_event.add(hf);
            l.selected_event.add(hs);
        });
        for (i, b) in Self::buttons(cx).into_iter().enumerate() {
            let h = handler(cx, SK_BUTTON + i as u32);
            cx.visit::<InputLabelButton, _>(b, |b, _| b.on_click.add(h));
        }
    }

    fn destruct(&mut self, cx: &mut Cx<'_>) {
        let (sw, fo) = (handler(cx, SK_SWITCH), handler(cx, SK_INTERACTOR_FOCUSED));
        cx.view.manager.on_request_switch_page.remove(sw);
        cx.view.manager.on_interactor_focused.remove(fo);
    }

    // @8539, LocalPlaySetting false: the top arrow pair and a 900 wide name
    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        let b = find(cx, "listSizeBox");
        let w = prop_f64(cx, "List Width", f64::NAN);
        wd(cx, b).max_w = w;
        for (i, b) in Self::buttons(cx).into_iter().enumerate() {
            cx.visit::<InputLabelButton, _>(b, |b, cx| b.set_interactable(cx, true));
            wd(cx, b).vis = if i < 2 { Vis::SelfHitTestInvisible } else { Vis::Collapsed };
        }
        let name = find(cx, "skinNameLabel");
        wd(cx, name).font_size = 28.0;
        let tag = find(cx, "UI_PM_Experimental");
        wd(cx, tag).transform.translation = [0.0, -60.0];
        let bx = find(cx, "SizeBox_0");
        let x = wd(cx, bx);
        x.w_override = 900.0;
        x.h_override = f64::NAN; // (unset)
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        match id {
            SK_SWITCH => {
                if let Some(&(p, d)) = arg.downcast_ref::<(i32, i32)>() {
                    self.on_switch_page(cx, p, d);
                }
            }
            SK_INTERACTOR_FOCUSED => {
                if let Some(&i) = arg.downcast_ref::<WidgetId>() {
                    self.on_interactor_focused(cx, i);
                }
            }
            SK_ITEM_FOCUSED => {
                let Some(ev) = arg.downcast_ref::<TileEvent>() else { return };
                if let Some(s) = ev.data.as_ref().and_then(TileData::skin).map(|s| s.skin.clone()) {
                    self.focus_item(cx, &s);
                }
            }
            // @4368-4732: Update Selection, SkinSelectedEvent, SetCharacterSkin(Character.Slug, Focused Skin Data) and
            // SavePreferences, UpdateAppearance
            SK_ITEM_SELECTED => {
                let Some((ev, _)) = arg.downcast_ref::<(TileEvent, bool)>() else { return };
                let Some(s) = ev.data.as_ref().and_then(TileData::skin).map(|s| s.skin.clone()) else { return };
                if ev.locked {
                    return;
                }
                self.update_selection(&s, true);
                cx.emit(self, |c| &c.skin_selected_event, &s);
                if let (Some(c), Some(f)) = (&self.character, &self.focused_skin) {
                    let r = mvs_settings::AssetRef { slug: f.slug.clone(), path: f.path.clone() };
                    cx.prefs.hydra.set_character_skin(&c.slug, r);
                }
                self.update_appearance(cx);
            }
            n if (SK_BUTTON..SK_BUTTON + 4).contains(&n) => {
                let p = Self::player_index(cx);
                self.on_switch_page(cx, p, if (n - SK_BUTTON) % 2 == 1 { 1 } else { -1 });
            }
            _ => {}
        }
    }
}

// ---------- UI_PM_ModeLabel ----------

/// The "VS" text: UI_DetailedText_VS_PM, a child Blueprint of UI_DetailedText (the countdown card's text) with no tree
/// of its own: the bundle has the parent's tree under the child's class defaults. Offsets: UI_DetailedText (DT@),
/// UI_DetailedText_VS_PM (VS@). PreConstruct VS@10 runs the parent's (DT@10): SetupParameters, the child's override
/// VS@0.
#[derive(Debug, Default)]
pub struct DetailedTextVSPM;

impl DetailedTextVSPM {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(DetailedTextVSPM)
    }

    fn setup_parameters(cx: &mut Cx<'_>) {
        let p = cx.view.tree().props(cx.w).clone();
        let num = |k: &str| p.get(k).and_then(Value::as_f64);
        let flag = |k: &str| p.get(k).and_then(Value::as_bool).unwrap_or(false);
        let main = find(cx, "MainText");
        // The parent's SetupParameters DT@0 over GetTextArray, which the child overrides to [MainText] (VS@0 of
        // GetTextArray): index 0 takes InnerOutlineThickness, InnerOutlineColor, FontSize, LetterSpacing 0 and Text
        // DT@619-1496; BackgroundOutline and DropShadow keep their tree fonts.
        let font_size = num("FontSize").unwrap_or(0.0);
        let text = text_of(p.get("Text"));
        {
            let x = wd(cx, main);
            x.outline = num("InnerOutlineThickness").unwrap_or(0.0);
            x.font_size = font_size;
            x.text = text;
        }
        // DT@1542-2201: the shadow's slot offset Clamp(Round(FontSize * DropShadowThickness / 100), 1, 999) and the
        // two visibilities
        let shadow = find(cx, "DropShadow");
        let off = (font_size * num("DropShadowThickness").unwrap_or(0.0) / 100.0).round().clamp(1.0, 999.0);
        let s = wd(cx, shadow);
        s.offsets.l = off;
        s.offsets.t = off;
        s.vis = if flag("EnableDropShadow") { Vis::Visible } else { Vis::Collapsed };
        let back = find(cx, "BackgroundOutline");
        // The child's own VS@10-968: MainText white; its font PFG BlueScreens_ExtraBoldItalic at the size just set,
        // LetterSpacing -20, with a dynamic M_FontStyling (Color = FontColor) as the font material and another
        // (Color = InnerOutlineColor, Color2 white) as the outline's material, the outline colour white.
        // Deviation: a text with a font material is drawn without its outline here (the slate's material text), so the
        // outline's material instance (1 unit on the mode label) is not drawn.
        let font_mat = crate::slate::resolve(&cx.view.bundle, "M_FontStyling");
        let mut mid = Mid::of(&font_mat);
        if let Some(c) = p.get("FontColor").and_then(|f| f.get("SpecifiedColor")) {
            let c = rgba(c);
            mid.set_vector("Color", [c[0] as f32, c[1] as f32, c[2] as f32, c[3] as f32]);
        }
        let x = wd(cx, main);
        x.color = [1.0; 4];
        x.outline_color = [1.0; 4];
        x.typeface = Some("BlueScreens_ExtraBoldItalic".to_string());
        x.letter_spacing = -20.0;
        x.font_mat = Some(font_mat);
        x.font_mid = Some(mid);
        let b = wd(cx, back);
        b.vis = if flag("EnableBackgroundOutline") { Vis::Visible } else { Vis::Collapsed };
        // VS@1019: BackgroundOutline.SetText("")
        b.text = String::new();
    }
}

impl Controller for DetailedTextVSPM {
    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        Self::setup_parameters(cx);
    }
}

/// "Free For All" is the label's own FFAText.
#[derive(Debug, Default)]
pub struct PMModeLabel {
    pub team_style: i32,
}

impl PMModeLabel {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::<PMModeLabel>::default()
    }

    fn update_mode_label(&self, cx: &mut Cx<'_>) {
        let ffa = self.team_style == 2;
        set_vis(cx, "FFAText", if ffa { Vis::HitTestInvisible } else { Vis::Collapsed });
        set_vis(cx, "UI_DetailedText_VS_PM", if ffa { Vis::Collapsed } else { Vis::HitTestInvisible });
    }

    /// Enter @370 -> @10.
    pub fn enter(&mut self, cx: &mut Cx<'_>) {
        self.update_mode_label(cx);
        let me = cx.w;
        wd(cx, me).vis = Vis::HitTestInvisible;
        let anim = if self.team_style == 2 { "EnterAnim_FFA" } else { "EnterAnim" };
        cx.view.play_animation(me, anim, false, 3.0, 1, false, true); // @264, bRestoreState
    }

    /// Exit @375.
    pub fn exit(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        cx.view.play(me, "ExitAnim", false, 1.0);
    }
}

impl Controller for PMModeLabel {
    fn construct(&mut self, cx: &mut Cx<'_>) {
        self.update_mode_label(cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn count_up_clock_pads_the_seconds() {
        assert_eq!(clock_text(0), "0:00");
        assert_eq!(clock_text(9), "0:09");
        assert_eq!(clock_text(75), "1:15");
    }

    #[test]
    fn badges_sit_by_the_side() {
        assert_eq!(badge_offsets(2, 1)[0], [-40.0, 0.0]);
        assert_eq!(badge_offsets(-1, 3)[2], [20.0, 240.0]);
        assert_eq!(badge_offsets(0, 1)[0], [20.0, 0.0]);
        assert_eq!(badge_offsets(0, 2), [[0.0; 2]; 3]);
        assert_eq!(badge_offsets(1, 2)[0], [-20.0, 0.0]);
    }

    #[test]
    fn label_params_read_the_struct() {
        let p = json!({
            "Action_3_ABC": "EInputAction::GameAttack",
            "FixedInputType_5_X": "NewEnumerator1",
            "Label_1_Y": {"SourceString": "Go"},
            "SizeIconSize_9_Z": {"X": 48.0, "Y": 40.0},
        });
        let l = label_params(&p);
        assert_eq!(l.action, "GameAttack");
        assert_eq!(l.modifier, "None");
        assert_eq!(l.label, "Go");
        assert_eq!(l.fixed_input_type, 1);
        assert_eq!(l.icon_size, [48.0, 40.0]);
        assert!(l.automatic_input_type);
        assert_eq!(l.text_size, 18.0);
    }

    #[test]
    fn rows_match_the_flow_table() {
        assert_eq!(ROW_REMATCH.time_limit, 45.0);
        assert_eq!(ROW_REMATCH.state_text, STATE_SELECT_FIGHTER);
        assert_eq!(ROW_DEFAULT.time_limit, 0.0);
    }
}
