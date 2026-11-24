//! Local Play (UI_LocalPlay): the match set-up and each player's panel and its character and skin steps. The later
//! steps (cosmetics, perks, confirm, ready) are `local_play_steps`, the match settings `quick_match_settings`.
//!
//! A call into a controller that is in hand (busy) is deferred until its method returns (`controller.rs`), so
//! UI_LocalPlay does not read a slot or the grid through a visit from inside an event of theirs: it keeps a shadow
//! of what it reads of each slot (state, team, character, time of the last change) from the slots' events and the
//! list of the characters it gave the grid, and calls them through `run_on`.

use std::any::Any;

use serde_json::json;

use super::common::{ModalBase, fill_page, palette_entry};
use super::controls::buttons::{ButtonMain, IconButton};
use super::controls::input_label::InputLabel;
use super::controls::interactor::{batch_set_context, set_focused, set_interactable};
use super::controls::{find, prop_bool, prop_f64, prop_str, wd};
use super::local_play_bots::{B_ADD, B_REMOVE, B_SETTINGS, BotSeats, seat_of, set_seat};
use super::local_play_steps::{
    GlyphEntries, LPPlayerConfirmReady, LPPlayerCosmetics, LPPlayerPerks, LPPlayerReady, perk_slugs_of,
};
use super::modals::show_modal_generic;
use super::nav::GlobalNav;
use super::perks::{Perk, PerksEquipper, PerksLoadout, perk_pictures};
use crate::bundle::{Rgba, RosterEntry, SkinInfo};
use crate::menu::controller::{Controller, Cx, Event, Handler};
use crate::menu::local_play_match::{
    Device, InputSource, begin_local_play_match, listen_pads, local_player_devices, set_local_player_devices,
    take_pad_events, team_style_alias, toggle_multi_input,
};
use crate::menu::manager::{DialogParams, Manager, Modal, TransitionParams};
use crate::menu::pictures::{roster_pictures, skin_pictures};
use crate::menu::screens::lab_selectors::{GridItemList, LabMapSelector, TileData, TileEvent, run_on, run_tile};
use crate::menu::screens::pre_match::child_widgets::{InputLabelButton, PMSkinSelector};
use crate::menu::session::HOMESCREEN_LOCAL_PLAY;
use crate::menu::widgets::input_binding_modal::InputBindingModal;
use crate::slate::{Vis, WidgetId};

// UI_MatchSetup_PlayerState by value (the enum asset's Names: NewEnumerator8 0, 4 1, 9 2, 0 3, 5 4, 6 5, 1 6, 2 7,
// 7 8, 3 9)
pub const UNAVAILABLE: i32 = 0;
pub const VACANT: i32 = 1;
pub const INVITE: i32 = 2;
pub const CHARACTER: i32 = 3;
pub const SKIN: i32 = 4;
pub const SOFT_LOCKED: i32 = 5;
pub const COSMETICS: i32 = 6;
pub const PERKS: i32 = 7;
pub const CONFIRM_READY: i32 = 8;
pub const READY: i32 = 9;
const STATE_OF: [i32; 10] = [3, 6, 7, 9, 1, 4, 5, 8, 0, 2];

/// The state a "State" property names (an enumerator's tail `NewEnumerator<n>`; any other is the character step).
fn state_of(name: &str) -> i32 {
    name.strip_prefix("NewEnumerator")
        .and_then(|n| n.parse::<usize>().ok())
        .and_then(|n| STATE_OF.get(n).copied())
        .unwrap_or(CHARACTER)
}

/// GetPlayersWithState's Requirement Met (@895): the players a style takes
const REQUIRED: [usize; 4] = [2, 4, 4, 0];

/// UE's Ease (EEasingFunc 6 EaseOut, 7 EaseInOut) at blend exponent e
fn ease_out(a: f64, b: f64, t: f64, e: f64) -> f64 {
    a + (b - a) * (1.0 - (1.0 - t).powf(e))
}

fn ease_in_out(a: f64, b: f64, t: f64, e: f64) -> f64 {
    a + (b - a) * if t < 0.5 { 0.5 * (2.0 * t).powf(e) } else { 1.0 - 0.5 * (2.0 * (1.0 - t)).powf(e) }
}

fn lerp(a: Rgba, b: Rgba, t: f64) -> Rgba {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t, a[3] + (b[3] - a[3]) * t]
}

const CLEAR: Rgba = [0.0, 0.0, 0.0, 0.0];
const WHITE: Rgba = [1.0, 1.0, 1.0, 1.0];
/// characterFG's colour (UI_MatchSetup_Player @12231)
const FG: Rgba = [0.009134, 0.01033, 0.029557, 1.0];

/// GetPaletteEntry of a team index (@11128-11235): 0 Ally, 1 Enemy, 2 Team3, 3 Team4
fn team_color(cx: &Cx<'_>, team: i32) -> Rgba {
    palette_entry(cx, [0, 1, 3, 4][usize::try_from(team).unwrap_or(0).min(3)])
}

/// (the port's own) the owners of the pictures the screen holds: the grid's portraits for its life, the focused
/// fighter's skins (the slot's thumbnail and the skin strip) while it is focused
const PICTURES_LOCAL: &str = "localPlay";
const PICTURES_LOCAL_SKINS: &str = "localPlay:skins";

/// (the port's own) a slot's pictures past the soft-locked step: its skin's Local Play portrait and its fighter's perk
/// icons
fn pictures_local_slot(player: i32) -> String {
    format!("localPlay:slot{player}")
}

/// DefaultStateForPlayer (UI_LocalPlay fn @0-407): player 0 in the character step, the rest vacant, but players 2 and
/// 3 unavailable in 1v1
fn default_state_for_player(player: i32, style: u8) -> i32 {
    if player == 0 {
        CHARACTER
    } else if style == 0 && player >= 2 {
        UNAVAILABLE
    } else {
        VACANT
    }
}

/// Increment Team's WrapInt maximum per team style (UI_MatchSetup_Player @15373-15583: 1v1 1, Teams 1, FFA 3, none 0)
const TEAM_MAX: [i32; 4] = [1, 1, 3, 0];

fn handler(cx: &Cx<'_>, id: u32) -> Handler {
    Handler { owner: cx.w, id }
}

fn style_of(cx: &Cx<'_>) -> u8 {
    cx.prefs.local_play.team_style
}

/// The interactor child of a button.
fn inter(cx: &Cx<'_>, button: WidgetId) -> WidgetId {
    cx.view.tree().find_in(button, "UI_Interactor")
}

/// UI_Functions BatchSetContext and BatchSetPlayerIndex of a list of interactors.
pub(super) fn set_interactors(cx: &mut Cx<'_>, list: &[WidgetId], ctx: &str, player: i32) {
    batch_set_context(cx, list, ctx);
    for &i in list {
        if let Some(s) = cx.view.manager.interactor_mut(i) {
            s.player_index = player;
        }
    }
}

/// UI_InputLabel.SetPlayerIndex @7822: Params.PlayerIndex, then Refresh
pub(super) fn set_label_player(cx: &mut Cx<'_>, label: WidgetId, player: i32) {
    cx.visit::<InputLabel, _>(label, |l, cx| {
        let mut p = l.params.clone();
        p.player_index = player;
        l.update_params(cx, p);
    });
}

/// UI_ButtonMain / UI_ButtonSecondary "Set Input Player Index": the button's input label's player
pub(super) fn set_input_player_index(cx: &mut Cx<'_>, button: WidgetId, player: i32) {
    let label = cx.view.tree().find_in(button, "UI_InputLabel");
    set_label_player(cx, label, player);
}

// ---------- UI_LP_CharacterInfo ----------

#[derive(Debug, Default)]
pub struct LPCharacterInfo;

impl LPCharacterInfo {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(LPCharacterInfo)
    }

    /// Set Data(Character, Skin, Ready) @15: no character leaves an empty name (the TextConst @486 is empty), no skin
    /// an empty variant (@328, visibility unchanged); a default skin's name is Hidden, another's
    /// SelfHitTestInvisible.
    pub fn set_data(&mut self, cx: &mut Cx<'_>, c: Option<&RosterEntry>, skin: Option<&SkinInfo>, ready: bool) {
        let label = find(cx, "characterLabel");
        wd(cx, label).text = c.map(|c| c.display_name.clone()).unwrap_or_default();
        let variant = find(cx, "variantLabel");
        let v = wd(cx, variant);
        match skin {
            Some(s) => {
                v.text = s.display_name.clone();
                v.vis = if s.is_default { Vis::Hidden } else { Vis::SelfHitTestInvisible };
            }
            None => v.text.clear(),
        }
        let icon = find(cx, "readyIcon");
        wd(cx, icon).vis = if ready { Vis::SelfHitTestInvisible } else { Vis::Collapsed };
        let spinner = find(cx, "UI_Spinner");
        wd(cx, spinner).vis = if ready { Vis::Collapsed } else { Vis::SelfHitTestInvisible };
    }
}

impl Controller for LPCharacterInfo {}

// ---------- the state widgets of a slot ----------

/// UI_LP_Player_Vacant: OnInitialized @225 plays IdleAnim with no loop count (0: for ever) and picks the join icon of
/// the platform (PC: ui_icons_controller, the tree's own). The join itself is the slot's (OnKeyPressed).
#[derive(Debug, Default)]
pub struct LPPlayerVacant {
    pub player_index: i32,
}

impl LPPlayerVacant {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::<LPPlayerVacant>::default()
    }

    /// Update Context(Context, Player Index): no interactors.
    pub fn update_context(&mut self, _cx: &mut Cx<'_>, _ctx: &str, player: i32) {
        self.player_index = player;
    }
}

impl Controller for LPPlayerVacant {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        cx.view.play(me, "IdleAnim", false, 1.0);
    }

    /// (the port's own) the view plays an animation once: it is started again when it ends
    fn timers(&mut self, cx: &mut Cx<'_>, _dt: f64) {
        let me = cx.w;
        if !cx.view.is_animation_playing(me, "IdleAnim") {
            cx.view.play(me, "IdleAnim", false, 1.0);
        }
    }
}

/// UI_LP_Player_Unavailable: Set Unavailable Text @225
#[derive(Debug, Default)]
pub struct LPPlayerUnavailable {
    pub player_index: i32,
}

impl LPPlayerUnavailable {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::<LPPlayerUnavailable>::default()
    }

    pub fn update_context(&mut self, _cx: &mut Cx<'_>, _ctx: &str, player: i32) {
        self.player_index = player;
    }

    pub fn set_unavailable_text(&mut self, cx: &mut Cx<'_>, t: &str) {
        let l = find(cx, "unavailableLabel");
        wd(cx, l).text = t.to_string();
    }
}

impl Controller for LPPlayerUnavailable {}

/// UI_LP_Player_Character: Set Character @270 -> Update Appearance @308 (Set Data(Character, none, false)); Update
/// Context also gives the confirm label its player (@220).
#[derive(Debug, Default)]
pub struct LPPlayerCharacter {
    pub player_index: i32,
    pub character: Option<RosterEntry>,
}

impl LPPlayerCharacter {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::<LPPlayerCharacter>::default()
    }

    pub fn update_context(&mut self, cx: &mut Cx<'_>, _ctx: &str, player: i32) {
        self.player_index = player;
        let label = find(cx, "UI_InputLabel_Confirm");
        set_label_player(cx, label, player);
    }

    pub fn set_character(&mut self, cx: &mut Cx<'_>, c: Option<&RosterEntry>) {
        self.character = c.cloned();
        let info = find(cx, "UI_LP_CharacterInfo");
        cx.visit::<LPCharacterInfo, _>(info, |i, cx| i.set_data(cx, c, None, false));
    }
}

impl Controller for LPPlayerCharacter {}

/// UI_LP_Player_SoftLocked: Set Data @225 -> @282 (Set Data(Character, Skin, true))
#[derive(Debug, Default)]
pub struct LPPlayerSoftLocked {
    pub player_index: i32,
}

impl LPPlayerSoftLocked {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::<LPPlayerSoftLocked>::default()
    }

    pub fn update_context(&mut self, _cx: &mut Cx<'_>, _ctx: &str, player: i32) {
        self.player_index = player;
    }

    pub fn set_data(&mut self, cx: &mut Cx<'_>, c: Option<&RosterEntry>, skin: Option<&SkinInfo>) {
        let info = find(cx, "UI_LP_CharacterInfo");
        cx.visit::<LPCharacterInfo, _>(info, |i, cx| i.set_data(cx, c, skin, true));
    }
}

impl Controller for LPPlayerSoftLocked {}

const SK_READY: u32 = 1;
const SK_SELECTED: u32 = 2;

/// UI_LP_Player_Skin: the skin strip and Ready.
#[derive(Debug, Default)]
pub struct LPPlayerSkin {
    pub player_index: i32,
    /// SkinSelectedEvent(Player Index, Skin Data)
    pub skin_selected_event: Event<(i32, SkinInfo)>,
    skin_data: Option<SkinInfo>,
}

impl LPPlayerSkin {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::<LPPlayerSkin>::default()
    }

    fn selector(cx: &Cx<'_>) -> WidgetId {
        find(cx, "UI_PM_SkinSelector")
    }

    fn button_ready(cx: &Cx<'_>) -> WidgetId {
        find(cx, "buttonReady")
    }

    /// Update Context @513 -> @213: the button's interactor, also the strip's Player Index and context
    pub fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str, player: i32) {
        self.player_index = player;
        let b = inter(cx, Self::button_ready(cx));
        set_interactors(cx, &[b], ctx, player);
        let s = Self::selector(cx);
        cx.view.tree_mut().set_prop(s, "Player Index", json!(player));
        let ctx = ctx.to_string();
        cx.visit::<PMSkinSelector, _>(s, |s, cx| s.update_context(cx, &ctx));
    }

    /// Set Character @572: Skin Data is the strip's focused skin after its SetCharacter (the saved or default one)
    pub fn set_character(&mut self, cx: &mut Cx<'_>, c: Option<&RosterEntry>) {
        self.skin_data = None;
        let s = Self::selector(cx);
        self.skin_data = cx
            .visit::<PMSkinSelector, _>(s, |s, cx| {
                s.set_character(cx, c);
                s.focused_skin.clone()
            })
            .flatten();
    }

    /// Focus @906 -> @51: the strip's selected tile (FocusSelected @9476: skinList.FocusIndex), or Ready without one
    pub fn focus(&mut self, cx: &mut Cx<'_>) {
        let s = Self::selector(cx);
        let list = cx.view.tree().find_in(s, "skinList");
        let player = self.player_index;
        let at = cx
            .visit::<GridItemList, _>(list, |l, _| {
                l.items().iter().position(|w| matches!(l.record(*w), Some((Some(TileData::Skin(t)), _)) if t.selected))
            })
            .flatten();
        match at {
            Some(at) => {
                run_on::<GridItemList>(cx, list, move |l, cx| l.focus_index(cx, player, at));
            }
            None => {
                let i = inter(cx, Self::button_ready(cx));
                set_focused(cx, i, false);
            }
        }
    }

    /// Ready @901 -> @755: a skin, and the step on screen
    pub fn ready(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        let Some(skin) = self.skin_data.clone() else { return };
        if !cx.view.visible_in_hierarchy(me) {
            return;
        }
        let ev = (self.player_index, skin);
        cx.emit(self, |c| &c.skin_selected_event, &ev);
    }
}

impl Controller for LPPlayerSkin {
    /// OnInitialized: the button @736 and the strip's SkinSelectedEvent @693 (a locked skin is not taken;
    /// PMSkinSelector fires only for one that is not)
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let h = handler(cx, SK_READY);
        let b = Self::button_ready(cx);
        cx.visit::<ButtonMain, _>(b, |b, _| b.on_click.add(h));
        let h = handler(cx, SK_SELECTED);
        let s = Self::selector(cx);
        cx.visit::<PMSkinSelector, _>(s, |s, _| s.skin_selected_event.add(h));
    }

    /// UI_PM_SkinSelector PreConstruct @8539, its LocalPlaySetting true branch (@8539-9475; this instance's
    /// LocalPlaySetting is true): the four arrows not interactable, the top pair collapsed and the lower pair shown,
    /// a 24 point name, the experimental tag at (0, 275), a 400 x 80 name box.
    /// Deviation: PMSkinSelector ports the false branch only; the branch is applied here, after it (children
    /// first), until that class takes it.
    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        let s = Self::selector(cx);
        let names = ["buttonPrevious", "buttonNext", "buttonPrevious_1", "buttonNext_1"];
        for (i, n) in names.iter().enumerate() {
            let b = cx.view.tree().find_in(s, n);
            cx.visit::<InputLabelButton, _>(b, |b, cx| b.set_interactable(cx, false));
            wd(cx, b).vis = if i < 2 { Vis::Collapsed } else { Vis::SelfHitTestInvisible };
        }
        let name = cx.view.tree().find_in(s, "skinNameLabel");
        wd(cx, name).font_size = 24.0;
        let tag = cx.view.tree().find_in(s, "UI_PM_Experimental");
        wd(cx, tag).transform.translation = [0.0, 275.0];
        let bx = cx.view.tree().find_in(s, "SizeBox_0");
        let x = wd(cx, bx);
        x.w_override = 400.0;
        x.h_override = 80.0;
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        match id {
            SK_READY => self.ready(cx),
            SK_SELECTED => {
                if let Some(s) = arg.downcast_ref::<SkinInfo>() {
                    self.skin_data = Some(s.clone());
                }
            }
            _ => {}
        }
    }
}

// ---------- UI_MatchSetup_Player: a slot ----------

/// UI_MatchSetup_Player Get Selections' outputs
#[derive(Clone, Debug, Default)]
pub struct Selections {
    pub character_slug: String,
    pub skin: Option<SkinInfo>,
    pub perk_slugs: Vec<String>,
    pub ringout_slug: String,
    pub banner_slug: String,
    pub taunt_slugs: Vec<String>,
    pub username: String,
}

const S_KEY: u32 = 1;
const S_SUBMIT3: u32 = 2;
const S_CANCEL: u32 = 3;
const S_DISMISSED: u32 = 4;
const S_SKIN: u32 = 5;
const S_COS_BACK: u32 = 6;
const S_PERKS: u32 = 7;
const S_TEAM_PREV: u32 = 8;
const S_TEAM_NEXT: u32 = 9;
/// Enter's two Delays (@15871 0.1 s, @2641 the column's).
const S_ENTER_HIDE: u32 = 10;
const S_ENTER_PLAY: u32 = 11;

const STEPS: [&str; 9] = [
    "UI_LP_Player_Unavailable",
    "UI_LP_Player_Vacant",
    "UI_LP_Player_Character",
    "UI_LP_Player_Skin",
    "UI_LP_Player_SoftLocked",
    "UI_LP_Player_Cosmetics",
    "UI_LP_Player_Perks",
    "UI_LP_Player_ConfirmReady",
    "UI_LP_Player_Ready",
];

#[derive(Debug)]
pub struct MatchSetupPlayer {
    /// StateChangedEvent(Player Index, State, the time of the change): the change time is the port's own, the
    /// screen's shadow of Last State Change Time.
    pub state_changed_event: Event<(i32, i32, f64)>,
    /// OnConfirmCharacterShortcut(Player Index), TeamChangedEvent(Player Index, Team)
    pub on_confirm_character_shortcut: Event<i32>,
    pub team_changed_event: Event<(i32, i32)>,
    /// AttemptJoinSlotEvent(Player Index)
    pub attempt_join_slot_event: Event<i32>,
    /// PerksChangedEvent(Player Index): the perks step's, passed on (@18284)
    pub perks_changed_event: Event<i32>,
    /// (the port's own) Set Character's change, for the screen's shadow of the slot.
    pub character_changed_event: Event<(i32, Option<RosterEntry>)>,
    pub state: i32,
    pub previous_state: i32,
    pub last_state_change_time: f64,
    pub character: Option<RosterEntry>,
    pub skin: Option<SkinInfo>,
    pub team_index: i32,
    /// Set Ally Equipped Perks (@31511's argument), kept as well as passed to the perk widgets
    pub ally_perks: Vec<Option<Perk>>,
    context: String,
    /// the port's own timeline state: Animate Character Change (0.1 s) and Set State Height (0.45 s), < 0 = idle
    char_change_at: f64,
    height_at: f64,
}

impl MatchSetupPlayer {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(MatchSetupPlayer {
            state_changed_event: Event::default(),
            on_confirm_character_shortcut: Event::default(),
            team_changed_event: Event::default(),
            attempt_join_slot_event: Event::default(),
            perks_changed_event: Event::default(),
            character_changed_event: Event::default(),
            state: UNAVAILABLE,
            previous_state: UNAVAILABLE,
            last_state_change_time: 0.0,
            character: None,
            skin: None,
            team_index: 0,
            ally_perks: Vec::new(),
            context: String::new(),
            char_change_at: -1.0,
            height_at: -1.0,
        })
    }

    pub fn player_index(cx: &Cx<'_>) -> i32 {
        prop_f64(cx, "Player Index", 0.0) as i32
    }

    fn can_change_teams(cx: &Cx<'_>) -> bool {
        cx.view.tree().props(cx.w).get("Can Change Teams").and_then(serde_json::Value::as_bool).unwrap_or(true)
    }

    fn step(cx: &Cx<'_>, i: usize) -> WidgetId {
        find(cx, STEPS[i])
    }

    fn team_buttons(cx: &Cx<'_>) -> [WidgetId; 2] {
        [find(cx, "buttonTeamPrevious"), find(cx, "buttonTeamNext")]
    }

    /// OnModalDismissed(Modal ID) @15786: the inputSettings modal closed: Set State(State, true), the slot's step shown
    /// and focused again
    fn on_modal_dismissed(&mut self, cx: &mut Cx<'_>, id: &str) {
        // NotEqual_StriStri: no case
        if id.eq_ignore_ascii_case("inputsettings") {
            let s = self.state;
            self.set_state(cx, s, true);
        }
    }

    /// Update Context: the slot's own buttons and every state widget, with the slot's player
    pub fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str) {
        self.context = ctx.to_string();
        let player = Self::player_index(cx);
        let [tp, tn] = Self::team_buttons(cx);
        let own =
            [inter(cx, tp), inter(cx, tn), inter(cx, find(cx, "buttonMakeLeader")), inter(cx, find(cx, "buttonKick"))];
        set_interactors(cx, &own, ctx, player);
        let s: Vec<WidgetId> = (0..STEPS.len()).map(|i| Self::step(cx, i)).collect();
        cx.visit::<LPPlayerUnavailable, _>(s[0], |c, cx| c.update_context(cx, ctx, player));
        cx.visit::<LPPlayerVacant, _>(s[1], |c, cx| c.update_context(cx, ctx, player));
        cx.visit::<LPPlayerCharacter, _>(s[2], |c, cx| c.update_context(cx, ctx, player));
        cx.visit::<LPPlayerSkin, _>(s[3], |c, cx| c.update_context(cx, ctx, player));
        cx.visit::<LPPlayerSoftLocked, _>(s[4], |c, cx| c.update_context(cx, ctx, player));
        cx.visit::<LPPlayerCosmetics, _>(s[5], |c, cx| c.update_context(cx, ctx, player));
        cx.visit::<LPPlayerPerks, _>(s[6], |c, cx| c.update_context(cx, ctx, player));
        cx.visit::<LPPlayerConfirmReady, _>(s[7], |c, cx| c.update_context(cx, ctx, player));
        cx.visit::<LPPlayerReady, _>(s[8], |c, cx| c.update_context(cx, ctx, player));
    }

    /// Set State @8407: with Force or a new state
    pub fn set_state(&mut self, cx: &mut Cx<'_>, s: i32, force: bool) {
        if !force && s == self.state {
            return;
        }
        // @8927: Last State Change Time only on a new state (a forced call keeps it)
        if s != self.state {
            self.last_state_change_time = cx.now();
        }
        self.previous_state = self.state;
        self.state = s;
        // Show State Widget @15002 (SetActiveWidgetAndFocus: the switcher's child of the state), then Update Appearance
        let sc = find(cx, "stateContainer");
        wd(cx, sc).active = s;
        self.update_appearance(cx);
        let ch = self.character.clone();
        let skin = self.skin.clone();
        let step = |cx: &Cx<'_>, i: usize| Self::step(cx, i);
        match s {
            UNAVAILABLE | VACANT | INVITE => {
                self.set_character(cx, None);
                self.set_skin(cx, None);
            }
            CHARACTER => {
                let w = step(cx, 2);
                cx.visit::<LPPlayerCharacter, _>(w, |c, cx| c.set_character(cx, ch.as_ref()));
                self.set_skin(cx, None);
            }
            SKIN => {
                let w = step(cx, 3);
                cx.visit::<LPPlayerSkin, _>(w, |c, cx| {
                    c.set_character(cx, ch.as_ref());
                    c.focus(cx);
                });
            }
            SOFT_LOCKED => {
                let w = step(cx, 4);
                cx.visit::<LPPlayerSoftLocked, _>(w, |c, cx| c.set_data(cx, ch.as_ref(), skin.as_ref()));
            }
            COSMETICS => {
                // @9724
                let w = step(cx, 5);
                cx.visit::<LPPlayerCosmetics, _>(w, |c, cx| {
                    c.set_character(cx, ch.as_ref());
                    c.focus(cx);
                });
            }
            PERKS => {
                // @9642
                let w = step(cx, 6);
                cx.visit::<LPPlayerPerks, _>(w, |c, cx| {
                    c.set_character(cx, ch.as_ref());
                    c.focus(cx);
                });
            }
            CONFIRM_READY => {
                // @10064
                let perks = self.get_perks(cx);
                let w = step(cx, 7);
                cx.visit::<LPPlayerConfirmReady, _>(w, |c, cx| {
                    c.set_data(cx, ch.as_ref(), skin.as_ref(), &perks);
                    c.reset_name(cx);
                    c.focus(cx);
                });
            }
            READY => {
                // @10232
                let perks = self.get_perks(cx);
                let w = step(cx, 8);
                cx.visit::<LPPlayerReady, _>(w, |c, cx| c.set_data(cx, ch.as_ref(), skin.as_ref(), &perks));
            }
            _ => {}
        }
        self.hold_slot_pictures(cx);
        // Set State Height @16708
        self.height_at = cx.now();
        self.apply_height(cx, 0.0);
        let ev = (Self::player_index(cx), s, self.last_state_change_time);
        cx.emit(self, |c| &c.state_changed_event, &ev);
        let me = cx.w;
        cx.view.play(me, "StateChangeAnim", false, 2.0);
    }

    /// Last State Time Check(Time): more than Time since the last state change
    pub fn last_state_time_check(&self, cx: &Cx<'_>, t: f64) -> bool {
        cx.now() - self.last_state_change_time > t
    }

    /// Set Ally Equipped Perks: UI_LP_Player_Perks' UI_Perks_Equipper, then UI_LP_Player_Ready's and
    /// UI_LP_Player_ConfirmReady's UI_Perks_Loadout, SetAllyEquipped(Perks).
    pub fn set_ally_equipped_perks(&mut self, cx: &mut Cx<'_>, perks: Vec<Option<Perk>>) {
        let ids: Vec<String> = perks.iter().flatten().map(|p| p.id.clone()).collect();
        self.ally_perks = perks;
        let eq = cx.view.tree().find_in(Self::step(cx, 6), "UI_Perks_Equipper");
        let l = ids.clone();
        run_on::<PerksEquipper>(cx, eq, move |e, cx| e.set_ally_equipped(cx, &l));
        for step in [8, 7] {
            let lo = cx.view.tree().find_in(Self::step(cx, step), "UI_Perks_Loadout");
            let l = ids.clone();
            run_on::<PerksLoadout>(cx, lo, move |lo, cx| lo.set_ally_equipped(cx, &l));
        }
    }

    /// Get Perks: the perks step's
    pub fn get_perks(&self, cx: &mut Cx<'_>) -> Vec<Option<Perk>> {
        let w = Self::step(cx, 6);
        cx.visit::<LPPlayerPerks, _>(w, |p, cx| p.get_perks(cx)).unwrap_or_else(|| vec![None; 4])
    }

    /// Get Selections @0-824: what UI_LocalPlay's Save All Settings writes (the character's slug in states 3-9, else
    /// ""; the skin; the perks' slugs; the cosmetics step's slugs; the name row's glyphs). The name is the row's even
    /// for a slot that never reached the confirm step: its five "A"s (UI_GlyphEntry's first glyph), as the build has
    /// it.
    /// Unverified: the "A"s are from the live game.
    pub fn get_selections(&self, cx: &mut Cx<'_>) -> Selections {
        let cw = Self::step(cx, 5);
        let cos = cx.visit::<LPPlayerCosmetics, _>(cw, |c, cx| c.get_equipped_slugs(cx)).unwrap_or_default();
        let perks = self.get_perks(cx);
        let rw = Self::step(cx, 7);
        let glyphs = cx.view.tree().find_in(rw, "UI_GlyphEntries");
        let username = cx.visit::<GlyphEntries, _>(glyphs, |g, _| g.get_glyphs()).unwrap_or_default();
        Selections {
            character_slug: match &self.character {
                Some(c) if (CHARACTER..=READY).contains(&self.state) => c.slug.clone(),
                _ => String::new(),
            },
            skin: self.skin.clone(),
            perk_slugs: perk_slugs_of(&perks),
            ringout_slug: cos.ringout,
            banner_slug: cos.banner,
            taunt_slugs: cos.taunts,
            username,
        }
    }

    /// (the port's own) the slot's pictures past the soft-locked step, let go below it
    fn hold_slot_pictures(&self, cx: &mut Cx<'_>) {
        let id = pictures_local_slot(Self::player_index(cx));
        if self.state < COSMETICS {
            cx.view.release_pictures(&id);
            return;
        }
        let mut names: Vec<String> = self.skin.iter().filter_map(|s| s.local_play_portrait.clone()).collect();
        names.extend(perk_pictures(cx.view, self.character.as_ref().and_then(|c| c.fighter.as_deref())));
        cx.view.hold_pictures(&id, names.iter().map(String::as_str));
    }

    pub fn release_pictures(&self, cx: &mut Cx<'_>) {
        let id = pictures_local_slot(Self::player_index(cx));
        cx.view.release_pictures(&id);
    }

    /// the cosmetics step's CosmeticsStepBackEvent @17917 -> @6589: not unavailable, no dialog, Local Play or the
    /// custom game the latest modal, 0.1 s since the last change; the skin step
    fn cosmetics_step_back(&mut self, cx: &mut Cx<'_>) {
        if self.state == UNAVAILABLE || Manager::is_dialog_active(cx.view) {
            return;
        }
        if !self.modal_check(cx) || !self.last_state_time_check(cx, 0.1) {
            return;
        }
        self.set_state(cx, SKIN, false);
    }

    /// IsModalActive? and the latest modal localPlay or customGame (a screen with no modal passes, @5123)
    fn modal_check(&self, cx: &Cx<'_>) -> bool {
        if !Manager::is_modal_active(cx.view) {
            return true;
        }
        let id = Manager::latest_modal_id(cx.view);
        id == "localPlay" || id == "customGame"
    }

    /// Set Character @11015: the character step, Update Appearance, Animate Character Change
    pub fn set_character(&mut self, cx: &mut Cx<'_>, c: Option<&RosterEntry>) {
        self.character = c.cloned();
        let w = Self::step(cx, 2);
        cx.visit::<LPPlayerCharacter, _>(w, |s, cx| s.set_character(cx, c));
        self.update_appearance(cx);
        self.char_change_at = cx.now();
        self.apply_char_change(cx, 0.0);
        let ev = (Self::player_index(cx), self.character.clone());
        cx.emit(self, |c| &c.character_changed_event, &ev);
    }

    /// Selected Character @16387: a slug other than the saved one is saved (Set-LocalPlay-CharacterForPlayer @16612),
    /// and the perks (4 empty strings, @16328), the skin (null, @16212) and taunts 0..3 ("", @5826 loop) are reset
    pub fn selected_character(&mut self, cx: &mut Cx<'_>, c: &RosterEntry) {
        let i = usize::try_from(Self::player_index(cx)).unwrap_or(0).min(3);
        let p = &mut cx.prefs.local_play.players[i];
        if p.character_slug == c.slug {
            return;
        }
        p.character_slug = c.slug.clone();
        p.perk_slugs = Default::default();
        p.skin_slug.clear();
        p.skin_reference.clear();
        p.taunts = Default::default();
        self.update_appearance(cx);
    }

    /// Set Skin @14030
    pub fn set_skin(&mut self, cx: &mut Cx<'_>, s: Option<SkinInfo>) {
        self.skin = s;
        self.update_appearance(cx);
    }

    /// Set Team Index @15341 / Increment Team: Update Appearance and TeamChangedEvent
    pub fn set_team_index(&mut self, cx: &mut Cx<'_>, t: i32) {
        self.team_index = t;
        self.update_appearance(cx);
        let ev = (Self::player_index(cx), t);
        cx.emit(self, |c| &c.team_changed_event, &ev);
    }

    /// Increment Team @15373: WrapInt(Team Index, Increment, 0, TEAM_MAX[style]) (UI_Functions WrapInt: below 0 -> the
    /// maximum, above it -> 0)
    fn increment_team(&mut self, cx: &mut Cx<'_>, dir: i32) {
        let max = TEAM_MAX[usize::from(style_of(cx)).min(3)];
        let t = self.team_index + dir;
        self.set_team_index(
            cx,
            if t < 0 {
                max
            } else if t > max {
                0
            } else {
                t
            },
        );
    }

    /// OnKeyPressed @17039: any key of this slot's local player while the slot is Unavailable or Vacant asks to join
    /// (the key itself is not read)
    fn on_key_pressed(&mut self, cx: &mut Cx<'_>, player: i32) {
        if player == Self::player_index(cx) && (self.state == UNAVAILABLE || self.state == VACANT) {
            cx.emit(self, |c| &c.attempt_join_slot_event, &player);
        }
    }

    /// Join Slot @17201 (not Xbox: no sign-in) -> @6260: nothing when Unavailable, while a dialog is up, or under a
    /// latest modal other than localPlay / customGame; a Vacant slot goes to the character step (@6242)
    pub fn join_slot(&mut self, cx: &mut Cx<'_>, player: i32) {
        if player != Self::player_index(cx) || self.state == UNAVAILABLE {
            return;
        }
        if Manager::is_dialog_active(cx.view) {
            return;
        }
        if Manager::is_modal_active(cx.view) {
            let id = Manager::latest_modal_id(cx.view);
            if id != "localPlay" && id != "customGame" {
                return;
            }
        }
        if self.state == VACANT {
            self.set_state(cx, CHARACTER, false);
        }
    }

    /// Set Unavailable(Is Unavailable, Reason) @15926
    pub fn set_unavailable(&mut self, cx: &mut Cx<'_>, unavailable: bool, reason: &str) {
        if unavailable {
            self.set_state(cx, UNAVAILABLE, false);
            let w = Self::step(cx, 0);
            cx.visit::<LPPlayerUnavailable, _>(w, |u, cx| u.set_unavailable_text(cx, reason));
        } else {
            self.set_state(cx, VACANT, false);
        }
    }

    /// Handle Match Settings @15326: Update Team Button Visibility
    pub fn handle_match_settings(&mut self, cx: &mut Cx<'_>) {
        self.update_team_button_visibility(cx);
    }

    /// Update Team Button Visibility: the arrows with teams (style 1) in the character and skin steps
    fn update_team_button_visibility(&self, cx: &mut Cx<'_>) {
        let on = style_of(cx) == 1 && (self.state == CHARACTER || self.state == SKIN) && Self::can_change_teams(cx);
        for b in Self::team_buttons(cx) {
            wd(cx, b).vis = if on { Vis::SelfHitTestInvisible } else { Vis::Hidden };
        }
    }

    /// Get Header Text: "{username}: Team {team}" for styles 0-2 (Get Username: Use Usernames is false, so
    /// GetLocalPlayUsername's PlayerNumberShort "P{player}")
    fn header_text(&self, cx: &Cx<'_>) -> String {
        if style_of(cx) <= 2 {
            format!("P{}: Team {}", Self::player_index(cx) + 1, self.team_index + 1)
        } else {
            String::new()
        }
    }

    /// Update Appearance @11108
    pub fn update_appearance(&mut self, cx: &mut Cx<'_>) {
        let team = team_color(cx, self.team_index);
        let s = self.state;
        let header_bg = find(cx, "headerBG");
        wd(cx, header_bg).color = team;
        let cbg = find(cx, "characterBG");
        wd(cx, cbg).color = lerp(team, CLEAR, if s == SOFT_LOCKED || s == READY { 0.65 } else { 0.8 });
        let cfg = find(cx, "characterFG");
        wd(cx, cfg).color = FG;
        let text = self.header_text(cx);
        let hl = find(cx, "headerLabel");
        wd(cx, hl).text = text;
        self.update_team_button_visibility(cx);
        let can = Self::can_change_teams(cx);
        let ph = find(cx, "panelHeader");
        wd(cx, ph).vis = if ((s == VACANT || s == INVITE) && can) || s == UNAVAILABLE {
            Vis::Collapsed
        } else {
            Vis::SelfHitTestInvisible
        };
        let li = find(cx, "leaderIndicator");
        wd(cx, li).vis = Vis::Collapsed; // (Is Leader is never set on this screen)
        // no account: the switcher shows the thumbnail (@12896)
        let sw = find(cx, "CharacterOrProfile");
        wd(cx, sw).active = 0;
        let thumb = find(cx, "characterThumbnail");
        wd(cx, thumb).vis = if [CHARACTER, SOFT_LOCKED, COSMETICS, CONFIRM_READY, READY].contains(&s) {
            Vis::SelfHitTestInvisible
        } else {
            Vis::Hidden
        };
        self.update_thumbnail(cx, thumb);
    }

    /// @13566-14025 and the load callbacks @3133 / @6918: with a skin, in the character to soft-locked steps, the
    /// skin's VariantIconMaterial; without one the default skin's, as a dynamic instance with TopLeft_X 0 and
    /// BottomRight_X 1; then characterFG's OneCoord 0.9 and ZeroCoord 0.15. In 6-9 (@3878, @4043) the skin's
    /// VariantLocalPlayPortraitMaterial, then OneCoord 0.75 and ZeroCoord 0.15.
    /// Unverified: the port's own: a fighter the port does not have has no skins, and its slot keeps the last
    /// picture; so does a skin with no portrait (two in the build).
    fn update_thumbnail(&self, cx: &mut Cx<'_>, thumb: WidgetId) {
        use crate::slate::Mid;
        let Some(c) = &self.character else { return };
        let Some(f) = c.fighter.as_ref().and_then(|f| cx.view.fighters().get(f)) else { return };
        let default_icon =
            f.skins.iter().find(|x| x.is_default).map_or_else(|| f.skin.icon.clone(), |x| x.icon.clone());
        let bundle = cx.view.bundle.clone();
        let fg = find(cx, "characterFG");
        let set_fg = |cx: &mut Cx<'_>, one: f32| {
            let tex = wd(cx, fg).tex.clone();
            let x = wd(cx, fg);
            if x.mid.is_none() {
                x.mid = tex.as_deref().filter(|t| bundle.has_material(t)).map(Mid::of);
            }
            if let Some(m) = &mut x.mid {
                m.set_scalar("OneCoord", one);
                m.set_scalar("ZeroCoord", 0.15);
            }
        };
        if let Some(skin) = &self.skin
            && self.state >= COSMETICS
        {
            let Some(lp) = skin.local_play_portrait.clone() else { return };
            let t = wd(cx, thumb);
            if t.tex.as_deref() != Some(lp.as_str()) {
                t.tex = Some(lp);
                t.mid = None;
            }
            set_fg(cx, 0.75);
            return;
        }
        match &self.skin {
            Some(skin) if (CHARACTER..=SOFT_LOCKED).contains(&self.state) => {
                let t = wd(cx, thumb);
                if t.tex.as_deref() != Some(skin.icon.as_str()) {
                    t.tex = Some(skin.icon.clone());
                    t.mid = None;
                }
            }
            None => {
                let t = wd(cx, thumb);
                // (the icon is the bundle's material name: its instance is of the object path)
                let path = bundle.material_path(&default_icon).unwrap_or(&default_icon).to_string();
                if t.tex.as_deref() != Some(path.as_str()) || (t.mid.is_none() && bundle.has_material(&path)) {
                    t.tex = Some(path.clone());
                    t.mid = bundle.has_material(&path).then(|| {
                        let mut m = Mid::of(&path);
                        m.set_scalar("TopLeft_X", 0.0);
                        m.set_scalar("BottomRight_X", 1.0);
                        m
                    });
                }
            }
            Some(_) => return,
        }
        set_fg(cx, 0.9);
    }

    fn apply_char_change(&mut self, cx: &mut Cx<'_>, t: f64) {
        let team = team_color(cx, self.team_index);
        let thumb = find(cx, "characterThumbnail");
        wd(cx, thumb).color = if t >= 1.0 { WHITE } else { lerp(team, WHITE, t) };
        let bg = find(cx, "background");
        let s = if t >= 1.0 { 1.0 } else { 1.05 + (1.0 - 1.05) * t };
        wd(cx, bg).transform.scale = [s, s];
        if t >= 1.0 {
            self.char_change_at = -1.0;
        }
    }

    fn height_of(&self, cx: &Cx<'_>, s: i32) -> f64 {
        if s == SOFT_LOCKED {
            prop_f64(cx, "SmallHeight", 490.0)
        } else if s >= COSMETICS {
            prop_f64(cx, "FullHeight", 1200.0)
        } else {
            prop_f64(cx, "MidHeight", 560.0)
        }
    }

    fn apply_height(&mut self, cx: &mut Cx<'_>, t: f64) {
        let h = ease_out(self.height_of(cx, self.previous_state), self.height_of(cx, self.state), t, 6.0);
        let b = find(cx, "SizeBox_1");
        wd(cx, b).h_override = h;
        if t >= 1.0 {
            self.height_at = -1.0;
        }
    }

    /// Step Back @10962 -> @4853: no dialog, Local Play the latest modal, 0.1 s since the last change
    fn step_back(&mut self, cx: &mut Cx<'_>, player: i32) {
        if player != Self::player_index(cx) || Manager::is_dialog_active(cx.view) {
            return;
        }
        let id = Manager::latest_modal_id(cx.view);
        if id != "localPlay" && id != "customGame" {
            return;
        }
        if !self.last_state_time_check(cx, 0.1) {
            return;
        }
        match self.state {
            CHARACTER => {
                if Self::player_index(cx) != 0 {
                    self.set_state(cx, VACANT, false);
                }
            }
            SKIN => self.set_state(cx, CHARACTER, false),
            SOFT_LOCKED => self.set_state(cx, SKIN, false),
            PERKS => self.set_state(cx, COSMETICS, false),
            CONFIRM_READY => self.set_state(cx, PERKS, false),
            READY => {
                // only while waiting for the others (Ready.Is Waiting)
                let w = Self::step(cx, 8);
                if cx.visit::<LPPlayerReady, _>(w, |r, _| r.is_waiting).unwrap_or(false) {
                    self.set_state(cx, CONFIRM_READY, false);
                }
            }
            _ => {}
        }
    }

    /// Step Forward @14088 -> @5014: not unavailable, no dialog, Local Play or the custom game the latest modal, 0.1 s
    /// since the last change (@5014-5269: the key that joined the slot steps no further); the character step's
    /// shortcut, the skin step's, cosmetics step's and confirm step's Ready
    fn step_forward(&mut self, cx: &mut Cx<'_>, player: i32) {
        let me = cx.w;
        if player != Self::player_index(cx) || !cx.view.visible_in_hierarchy(me) {
            return;
        }
        if self.state == UNAVAILABLE || Manager::is_dialog_active(cx.view) {
            return;
        }
        if !self.modal_check(cx) || !self.last_state_time_check(cx, 0.1) {
            return;
        }
        match self.state {
            CHARACTER => cx.emit(self, |c| &c.on_confirm_character_shortcut, &player),
            SKIN => {
                let w = Self::step(cx, 3);
                cx.visit::<LPPlayerSkin, _>(w, |s, cx| s.ready(cx));
            }
            COSMETICS => {
                let w = Self::step(cx, 5);
                cx.visit::<LPPlayerCosmetics, _>(w, |s, cx| s.ready(cx));
            }
            CONFIRM_READY => {
                let w = Self::step(cx, 7);
                cx.visit::<LPPlayerConfirmReady, _>(w, |s, cx| s.ready(cx));
            }
            _ => {}
        }
    }
}

impl Controller for MatchSetupPlayer {
    /// OnInitialized -> Event Bindings: the manager's OnKeyInput (OnKeyPressed @10506), OnSubmit3 (Step Forward) and
    /// OnCancelWithinModal (Step Back); the skin step's SkinSelectedEvent (Set Skin @14064), the cosmetics step's
    /// CosmeticsStepBackEvent (@17917 -> @6589), the perks step's PerksChangedEvent (@18284). Its CosmeticsDoneEvent
    /// (@15253), PerksReadyEvent (@14297) and ReadyEvent (@15870) do nothing here: UI_LocalPlay binds them. The
    /// manager's ModalDismissedEvent -> OnModalDismissed (@10395). Not bound: OnUIPrefsChanged.
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        self.state = state_of(&prop_str(cx, "State"));
        let (k, s3, c, d) =
            (handler(cx, S_KEY), handler(cx, S_SUBMIT3), handler(cx, S_CANCEL), handler(cx, S_DISMISSED));
        let m = &mut cx.view.manager;
        m.on_key_input.add(k);
        m.on_submit3.add(s3);
        m.on_cancel_within_modal.add(c);
        m.modal_dismissed.add(d);
        let h = handler(cx, S_SKIN);
        let w = Self::step(cx, 3);
        cx.visit::<LPPlayerSkin, _>(w, |s, _| s.skin_selected_event.add(h));
        let h = handler(cx, S_COS_BACK);
        let w = Self::step(cx, 5);
        cx.visit::<LPPlayerCosmetics, _>(w, |s, _| s.cosmetics_step_back_event.add(h));
        let h = handler(cx, S_PERKS);
        let w = Self::step(cx, 6);
        cx.visit::<LPPlayerPerks, _>(w, |s, _| s.perks_changed_event.add(h));
        for (n, id) in [(0, S_TEAM_PREV), (1, S_TEAM_NEXT)] {
            let h = handler(cx, id);
            let b = Self::team_buttons(cx)[n];
            cx.visit::<IconButton, _>(b, |b, _| b.on_click.add(h));
        }
        // Enter @10380 -> @15871: Delay 0.1
        cx.view.delay(0.1, handler(cx, S_ENTER_HIDE));
    }

    /// Enter's Delays: @2477 rootContainer's render opacity 0, then Delay((the grid slot's Column + 1) x 0.15)
    /// @2514-2641 (the column UI_LocalPlay's match settings gave the slot by then); @2429 PlayAnimation(EnterAnim)
    /// (rootContainer's render transform and opacity, 0.3 s, no restore).
    fn resume(&mut self, cx: &mut Cx<'_>, id: u32) {
        match id {
            S_ENTER_HIDE => {
                let root = find(cx, "rootContainer");
                wd(cx, root).opacity = 0.0;
                let col = cx.view.tree().widget(cx.w).col;
                cx.view.delay(f64::from(col + 1) * 0.15, handler(cx, S_ENTER_PLAY));
            }
            S_ENTER_PLAY => cx.view.play(cx.w, "EnterAnim", false, 1.0),
            _ => {}
        }
    }

    fn destruct(&mut self, cx: &mut Cx<'_>) {
        let (k, s3, c, d) =
            (handler(cx, S_KEY), handler(cx, S_SUBMIT3), handler(cx, S_CANCEL), handler(cx, S_DISMISSED));
        let m = &mut cx.view.manager;
        m.on_key_input.remove(k);
        m.on_submit3.remove(s3);
        m.on_cancel_within_modal.remove(c);
        m.modal_dismissed.remove(d);
    }

    /// PreConstruct @8176: Set State(State, true); the leader and kick buttons by Use Leader / Allow Kicking (both
    /// false here); Set Player Index
    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        let s = self.state;
        self.set_state(cx, s, true);
        let (ml, kick) = (find(cx, "buttonMakeLeader"), find(cx, "buttonKick"));
        wd(cx, ml).vis = if prop_bool(cx, "Use Leader") { Vis::Visible } else { Vis::Collapsed };
        wd(cx, kick).vis = if prop_bool(cx, "Allow Kicking") { Vis::Visible } else { Vis::Collapsed };
        let ctx = self.context.clone();
        self.update_context(cx, &ctx);
    }

    /// Animate Character Change @14857 -> @7270 (0.1 s, real time): the thumbnail's colour from the team's to white,
    /// the background's scale from 1.05 to 1; Set State Height @1486 (0.45 s, Ease EaseOut exponent 6) from the
    /// previous state's height to this one's
    fn timers(&mut self, cx: &mut Cx<'_>, _dt: f64) {
        let now = cx.now();
        if self.char_change_at >= 0.0 {
            self.apply_char_change(cx, ((now - self.char_change_at) / 0.1).min(1.0));
        }
        if self.height_at >= 0.0 {
            self.apply_height(cx, ((now - self.height_at) / 0.45).min(1.0));
        }
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        match id {
            S_KEY => {
                if let Some((p, _)) = arg.downcast_ref::<(i32, String)>() {
                    self.on_key_pressed(cx, *p);
                }
            }
            S_SUBMIT3 => {
                if let Some(&p) = arg.downcast_ref::<i32>() {
                    self.step_forward(cx, p);
                }
            }
            S_CANCEL => {
                if let Some(&p) = arg.downcast_ref::<i32>() {
                    self.step_back(cx, p);
                }
            }
            S_DISMISSED => {
                if let Some(m) = arg.downcast_ref::<String>() {
                    self.on_modal_dismissed(cx, m);
                }
            }
            S_SKIN => {
                if let Some((_, s)) = arg.downcast_ref::<(i32, SkinInfo)>() {
                    self.set_skin(cx, Some(s.clone()));
                }
            }
            S_COS_BACK => self.cosmetics_step_back(cx),
            S_PERKS => {
                let p = Self::player_index(cx);
                cx.emit(self, |c| &c.perks_changed_event, &p);
            }
            S_TEAM_PREV => self.increment_team(cx, -1),
            S_TEAM_NEXT => self.increment_team(cx, 1),
            _ => {}
        }
    }
}

// ---------- UI_LocalPlay ----------

const L_STATE: u32 = 1;
const L_SKIN: u32 = 2;
const L_SHORTCUT: u32 = 3;
const L_JOIN: u32 = 4;
const L_COS_DONE: u32 = 5;
const L_PERKS_READY: u32 = 6;
const L_CONFIRM_READY: u32 = 7;
const L_PERKS_CHANGED: u32 = 8;
const L_CHARACTER: u32 = 9;
const L_TEAM: u32 = 10;
const L_RESET_KEY: u32 = 11;
const L_DISMISSED: u32 = 12;
const L_GRID_FOCUSED: u32 = 13;
const L_GRID_SELECTED: u32 = 14;
const L_GRID_POPULATED: u32 = 15;
const L_SETTINGS_BUTTON: u32 = 16;
const L_MAPS: u32 = 17;
const L_MAP_CANCELED: u32 = 18;
const L_EXIT: u32 = 19;
const L_POPULATE: u32 = 100;
const L_BEGIN_FADE: u32 = 101;
const L_BEGIN_SAVE: u32 = 102;
const L_BOTS_REFRESH: u32 = 103;

/// What the screen reads of a slot (see the module's note).
#[derive(Clone, Debug, Default)]
struct Shadow {
    state: i32,
    team: i32,
    character: Option<RosterEntry>,
    last_change: f64,
}

#[derive(Debug)]
pub struct LocalPlay {
    base: ModalBase,
    /// UI_LP_SetupState: 0 Characters, 1 Customization, 2 Maps; the class default UI_LP_MAX
    setup_state: i32,
    config_style: Option<u8>,
    /// Toggle Characters Mode's timeline (@21235 -> @17088 -> @1685, 0.45 s real time), < 0 = idle
    toggle_at: f64,
    character_mode: bool,
    shadow: [Shadow; 4],
    /// The characters the grid was filled with, in its order (the Random tile after them).
    roster: Vec<RosterEntry>,
    /// The maps OnMapsSelected stored (Maps), for Save All Settings' GetMapSlugs
    maps: Option<Vec<String>>,
    /// A Delay node already pending is not started again (FLatentActionManager keeps one action per node and
    /// target): a second Begin Match before the 1.25 s one has run adds nothing (begin_pending stands in for the two
    /// nodes).
    begin_pending: bool,
    /// (the port's own big matches) The bot seats past the pads' four.
    bots: Option<BotSeats>,
}

impl LocalPlay {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(LocalPlay {
            base: ModalBase::default(),
            setup_state: 3,
            config_style: None,
            toggle_at: -1.0,
            character_mode: false,
            shadow: Default::default(),
            roster: Vec::new(),
            maps: None,
            begin_pending: false,
            bots: None,
        })
    }

    fn slot_w(cx: &Cx<'_>, i: usize) -> WidgetId {
        find(cx, &format!("UI_LP_Player{i}"))
    }

    fn grid(cx: &Cx<'_>) -> WidgetId {
        find(cx, "CharacterGrid")
    }

    fn header(cx: &Cx<'_>) -> WidgetId {
        find(cx, "WBP_MenuHeader")
    }

    /// A call on slot `i`, now or when the slot's own call returns.
    fn slot_do(cx: &mut Cx<'_>, i: usize, f: impl FnOnce(&mut MatchSetupPlayer, &mut Cx<'_>) + 'static) {
        let w = Self::slot_w(cx, i);
        run_on::<MatchSetupPlayer>(cx, w, f);
    }

    /// OnInitialized @20009: Add Bindings, Handle Match Settings
    fn init(&mut self, cx: &mut Cx<'_>) {
        for i in 0..4 {
            let w = Self::slot_w(cx, i);
            if let Some(s) = cx.visit::<MatchSetupPlayer, _>(w, |s, cx| {
                (s.state, s.team_index, s.character.clone(), MatchSetupPlayer::player_index(cx))
            }) {
                self.shadow[i] = Shadow { state: s.0, team: s.1, character: s.2, last_change: 0.0 };
            }
        }
        self.add_bindings(cx);
        let players = find(cx, "playersGrid");
        self.bots = Some(BotSeats::new(cx, players));
        self.handle_match_settings(cx);
    }

    /// (the port's own big matches) The bot seats made again from the preferences, shown in Teams and Free For All
    /// at the character grid's opacity.
    fn refresh_bots(&mut self, cx: &mut Cx<'_>) {
        let Some(mut b) = self.bots.take() else { return };
        let ctx = self.base.context.clone();
        b.refresh(cx, &ctx);
        let g = Self::grid(cx);
        let opacity = cx.view.tree().widget(g).opacity;
        b.show(cx, matches!(style_of(cx), 1 | 2), opacity);
        self.bots = Some(b);
    }

    /// (the port's own big matches) A bot seat's row asked for a change: a new seat (`B_ADD`: its fighter and
    /// skin), a seat's fighter, skin or difficulty (`B_SETTINGS`), a seat removed (`B_REMOVE`, the seats after it
    /// moving up). The preferences hold the seats.
    fn bot_seat_event(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        let seats = &cx.prefs.local_play.players;
        match id {
            B_ADD => {
                let Some((character, skin)) = arg.downcast_ref::<(String, String)>() else { return };
                let i = seats.len().max(mvs_settings::LOCAL_PLAY_PADS);
                if i >= mvs_settings::LOCAL_PLAY_MAX {
                    return;
                }
                let mut seat = mvs_settings::LocalPlayPlayer::seat(i);
                let d = i32::from(seat.bot_difficulty);
                set_seat(cx, &mut seat, character, skin, d);
                let players = &mut cx.prefs.local_play.players;
                while players.len() < i {
                    players.push(mvs_settings::LocalPlayPlayer::seat(players.len()));
                }
                players.push(seat);
            }
            B_SETTINGS => {
                let Some((account, character, difficulty, skin)) = arg.downcast_ref::<(String, String, i32, String)>()
                else {
                    return;
                };
                let Some(i) = seat_of(account).filter(|&i| i < seats.len()) else { return };
                let mut seat = seats[i].clone();
                set_seat(cx, &mut seat, character, skin, *difficulty);
                cx.prefs.local_play.players[i] = seat;
            }
            B_REMOVE => {
                let Some(i) = arg.downcast_ref::<String>().and_then(|a| seat_of(a)).filter(|&i| i < seats.len()) else {
                    return;
                };
                let players = &mut cx.prefs.local_play.players;
                players.remove(i);
                // (a seat's team follows its place: the seats after it take the alternating team of theirs)
                for (k, p) in players.iter_mut().enumerate().skip(i) {
                    p.team_index = mvs_settings::LocalPlayPlayer::seat(k).team_index;
                }
            }
            _ => return,
        }
        let me = cx.w;
        cx.view.delay(0.0, Handler { owner: me, id: L_BOTS_REFRESH });
    }

    /// Add Bindings @24570: each slot's StateChangedEvent, its skin step's SkinSelectedEvent, its confirm shortcut,
    /// its cosmetics step's CosmeticsDoneEvent, its perks step's PerksReadyEvent, its confirm step's ReadyEvent, its
    /// PerksChangedEvent and its AttemptJoinSlotEvent; the manager's OnResetKey; the controller connection event
    /// (@24857; `timers`); the manager's ModalDismissedEvent -> OnModalDismissed (@24701); the
    /// grid's events (bound as components); the Match Settings button. Not bound: the team events (the port's own:
    /// the shadow reads them, and the character events).
    fn add_bindings(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        let h = |id| Handler { owner: me, id };
        for i in 0..4 {
            let slot = Self::slot_w(cx, i);
            cx.visit::<MatchSetupPlayer, _>(slot, |s, _| {
                s.state_changed_event.add(h(L_STATE));
                s.on_confirm_character_shortcut.add(h(L_SHORTCUT));
                s.attempt_join_slot_event.add(h(L_JOIN));
                s.perks_changed_event.add(h(L_PERKS_CHANGED));
                s.character_changed_event.add(h(L_CHARACTER));
                s.team_changed_event.add(h(L_TEAM));
            });
            let sk = cx.view.tree().find_in(slot, "UI_LP_Player_Skin");
            cx.visit::<LPPlayerSkin, _>(sk, |s, _| s.skin_selected_event.add(h(L_SKIN)));
            let cs = cx.view.tree().find_in(slot, "UI_LP_Player_Cosmetics");
            cx.visit::<LPPlayerCosmetics, _>(cs, |s, _| s.cosmetics_done_event.add(h(L_COS_DONE)));
            let pk = cx.view.tree().find_in(slot, "UI_LP_Player_Perks");
            cx.visit::<LPPlayerPerks, _>(pk, |s, _| s.perks_ready_event.add(h(L_PERKS_READY)));
            let cf = cx.view.tree().find_in(slot, "UI_LP_Player_ConfirmReady");
            cx.visit::<LPPlayerConfirmReady, _>(cf, |s, _| s.ready_event.add(h(L_CONFIRM_READY)));
        }
        cx.view.manager.on_reset_key.add(h(L_RESET_KEY));
        cx.view.manager.modal_dismissed.add(h(L_DISMISSED));
        listen_pads(true);
        let g = Self::grid(cx);
        cx.visit::<GridItemList, _>(g, |g, _| {
            g.focused_event.add(h(L_GRID_FOCUSED));
            g.selected_event.add(h(L_GRID_SELECTED));
            g.populated_event.add(h(L_GRID_POPULATED));
        });
        let b = find(cx, "buttonMatchSettings");
        cx.visit::<super::controls::buttons::ButtonSecondary, _>(b, |b, _| b.on_click.add(h(L_SETTINGS_BUTTON)));
    }

    /// Remove Bindings (PreDismiss). (the port's own) OnModalDismissed too, which the build leaves bound to the dead
    /// widget (no remove of it in the graph; no localPlayMatchSettings can close once the screen is gone)
    fn remove_bindings(&mut self, cx: &mut Cx<'_>) {
        let (r, d) = (handler(cx, L_RESET_KEY), handler(cx, L_DISMISSED));
        cx.view.manager.on_reset_key.remove(r);
        cx.view.manager.modal_dismissed.remove(d);
        listen_pads(false);
    }

    /// Handle Match Settings @26142: a new config (none yet, or another team style) resets the slots to the style's
    /// defaults (@6334: Set State(DefaultStateForPlayer, true), Set Team Index(DefaultTeamIndexForPlayer), the grid
    /// column DefaultColumnForMode) and the screen to the character step (@27004); each slot's focus (@12765); the
    /// title and the settings line (@26219)
    fn handle_match_settings(&mut self, cx: &mut Cx<'_>) {
        let style = style_of(cx);
        let changed = self.config_style != Some(style);
        self.config_style = Some(style);
        for i in 0..4usize {
            Self::slot_do(cx, i, |p, cx| p.handle_match_settings(cx));
            if !changed {
                continue;
            }
            let pi = i as i32;
            let team = if style == 1 { [0, 1, 0, 1][i] } else { pi };
            let col = if style == 1 {
                [0, 2, 1, 3][i]
            } else if style == 3 {
                0
            } else {
                pi
            };
            let st = default_state_for_player(pi, style);
            Self::slot_do(cx, i, move |p, cx| {
                p.set_state(cx, st, true);
                p.set_team_index(cx, team);
            });
            let w = Self::slot_w(cx, i);
            wd(cx, w).col = col;
        }
        if changed {
            self.set_state(cx, 0, false);
        }
        for i in 0..4 {
            self.focus_character_for_player(cx, i);
        }
        self.refresh_bots(cx);
        let mode = ["1 vs 1", "Teams", "Free for All"].get(usize::from(style)).copied().unwrap_or("");
        // WBP_MenuHeader.UpdateTitleText (@2528): the PageTitle variable and the text (the header's PreConstruct,
        // which runs after this OnInitialized, reads the variable)
        let title = format!("Local: {mode}");
        let hd = Self::header(cx);
        cx.view.tree_mut().set_prop(hd, "PageTitle", json!({ "SourceString": title }));
        let t = cx.view.tree().find_in(hd, "txt_PageTitle");
        wd(cx, t).text = title;
        let desc = self.settings_description(cx);
        let sl = find(cx, "settingsLabel");
        wd(cx, sl).text = desc;
    }

    /// GetSettingsDescription @770
    fn settings_description(&self, cx: &Cx<'_>) -> String {
        let l = &cx.prefs.local_play;
        let t = if l.time > 0 { format!("{} MIN", l.time / 60) } else { "NO TIME LIMIT".to_string() };
        let h = if l.hazards { "HAZARDS ON" } else { "HAZARDS OFF" };
        match l.team_style {
            0 | 1 => format!("{} RINGOUTS, {t}, {h}", l.ringouts),
            2 => format!("HIGHEST SCORE WINS, {t}, {h}"),
            _ => format!("{t}, {h}"),
        }
    }

    /// Update Context: the slots, the grid, the context interactor, the nav bar
    fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str) {
        self.base.context = ctx.to_string();
        let ci = find(cx, "contextInteractor");
        let ms = inter(cx, find(cx, "buttonMatchSettings"));
        batch_set_context(cx, &[ci, ms], ctx);
        for i in 0..4 {
            let c = ctx.to_string();
            Self::slot_do(cx, i, move |p, cx| p.update_context(cx, &c));
        }
        let g = Self::grid(cx);
        let c = ctx.to_string();
        run_on::<GridItemList>(cx, g, move |g, cx| g.update_context(cx, &c));
        let nav = find(cx, "WBP_GlobalNav_LocalPlay");
        cx.visit::<GlobalNav, _>(nav, |n, cx| n.update_context_global_nav(cx, ctx, true, true));
        if let Some(b) = &self.bots {
            b.update_context(cx, ctx);
        }
    }

    /// Grid Populated @28999: Load Initial State, then ToggleMultiInput(true) @29059
    fn on_grid_populated(&mut self, cx: &mut Cx<'_>) {
        self.load_initial_state(cx);
        toggle_multi_input(true);
    }

    /// Load Initial State (@31506 -> @14622, loop @12816), per slot: Set Character(CharacterItemFromSlug(the saved
    /// slug)); Set State(3, true) for player 0, or a player with a controller (GetControllerAssignedToPlayer @13477
    /// at least 0: a device in the devices table, before ToggleMultiInput makes players 1..3) and a saved character
    /// (@13981); for such a slot in Teams, Set Team Index(the saved team) (@14493). The slugs are cleared at each
    /// start (InitLocalPlayPreferences), so a saved character comes back within the page's life.
    fn load_initial_state(&mut self, cx: &mut Cx<'_>) {
        let devices = local_player_devices();
        let teams = self.config_style == Some(1);
        for i in 0..4usize {
            let saved = cx.prefs.local_play.players[i].clone();
            let c = self.roster.iter().find(|r| r.slug == saved.character_slug).cloned();
            Self::slot_do(cx, i, move |p, cx| p.set_character(cx, c.as_ref()));
            let assigned = devices.get(i).copied().flatten().is_some();
            if !(i == 0 || (assigned && !saved.character_slug.is_empty())) {
                continue;
            }
            let team = i32::from(saved.team_index);
            Self::slot_do(cx, i, move |p, cx| {
                p.set_state(cx, CHARACTER, true);
                if teams {
                    p.set_team_index(cx, team);
                }
            });
        }
    }

    /// FirstAvailableSlot @448: by grid column 0..3, the Player Index of the first slot that is Vacant (state 1), -1
    /// none
    fn first_available_slot(&self, cx: &Cx<'_>) -> i32 {
        for col in 0..=3 {
            for i in 0..4 {
                if cx.view.tree().widget(Self::slot_w(cx, i)).col == col && self.shadow[i].state == VACANT {
                    return i as i32;
                }
            }
        }
        -1
    }

    /// OnAttemptJoinSlot @31882: the pressing player's own slot when it is the first available one; else, with one
    /// past column 0's player (@32261), the two players' controllers swapped (SwapPlayerControllers @1815: the two
    /// entries of the devices table) and the first available slot joined; then Balance Teams On Join. The swap is
    /// not undone when the slot is left.
    fn on_attempt_join_slot(&mut self, cx: &mut Cx<'_>, player: i32) {
        let first = self.first_available_slot(cx);
        if player == first {
            let p = player as usize;
            Self::slot_do(cx, p, move |s, cx| s.join_slot(cx, player));
            // (Join Slot's state change reaches the shadow when the slot's call returns; the joined slot is
            // balanced as it is then)
            self.balance_teams_on_join(cx, p);
            return;
        }
        if first <= 0 {
            return;
        }
        Self::swap_player_controllers(player as usize, first as usize);
        Self::slot_do(cx, first as usize, move |s, cx| s.join_slot(cx, first));
        // @32581: FirstAvailableSlot is evaluated again after Join Slot (the slot is in state 3 by @32489), so the
        // slot balanced is the next Vacant one by column, none when there is none
        self.shadow[first as usize].state = CHARACTER;
        let next = self.first_available_slot(cx);
        if next >= 0 {
            self.balance_teams_on_join(cx, next as usize);
        }
    }

    /// SwapPlayerControllers(A, B) @1060 / @1157: AssignControllerToPlayer(A, B's id), then (B, A's old id) (and a
    /// debug print, left out)
    fn swap_player_controllers(a: usize, b: usize) {
        let mut t = local_player_devices();
        while t.len() <= a.max(b) {
            t.push(None);
        }
        t.swap(a, b);
        set_local_player_devices(t);
    }

    /// Balance Teams On Join (fn @96-911), Teams only: the slots in the character, skin and soft-locked steps counted
    /// per team; more than 2 on the joined slot's team moves it to the other (0 <-> 1, any other team to 0)
    fn balance_teams_on_join(&mut self, cx: &mut Cx<'_>, joined: usize) {
        if style_of(cx) != 1 {
            return;
        }
        let mut count = [0; 4];
        for (i, s) in self.shadow.iter().enumerate() {
            // (a slot just sent to the character step by its join is counted as it will be)
            let st = if i == joined && s.state == VACANT { CHARACTER } else { s.state };
            if st == CHARACTER || st == SKIN || st == SOFT_LOCKED {
                count[usize::try_from(s.team).unwrap_or(0).min(3)] += 1;
            }
        }
        let team = self.shadow[joined].team;
        if count[usize::try_from(team).unwrap_or(0).min(3)] > 2 {
            let t = if team == 0 { 1 } else { 0 };
            Self::slot_do(cx, joined, move |p, cx| p.set_team_index(cx, t));
        }
    }

    /// OnControllerConnectionChanged @29923 (FighterGameInstance's OnControllerConnectionChange, heard before the id
    /// moves): pid = GetPlayerIdForController; the map selector dismissed (@30079), and the input settings modal of
    /// pid (@30175, below); at a disconnect, slot pid when it is not Unavailable or Vacant goes to its
    /// DefaultStateForPlayer and the screen to Set State(0, true) (@30557-31055)
    /// `pid`: the local player that held the pad before the change (read before the table moves).
    fn on_controller_connection_changed(&mut self, cx: &mut Cx<'_>, _pad: usize, connected: bool, pid: Option<usize>) {
        Manager::dismiss_modal(cx, "mapSelector");
        // @30175-30517, at a connect too: the inputSettings modal (a UI_InputBindingModal) of slot pid's player is
        // dismissed (RequestDismiss("", false)). A pad no player holds (pid -1) gets no slot from Array_Get and the
        // Player Index read off None is 0, so player 0's modal goes.
        // Unverified: Accessed None reads the zero value.
        if let Some(m) = cx.view.manager.get_modal("inputSettings") {
            let want = pid.map_or(0, |p| p as i32);
            cx.visit::<InputBindingModal, _>(m, |m, cx| {
                if m.player_index == want {
                    m.request_dismiss(cx, "", false);
                }
            });
        }
        if connected {
            return;
        }
        let Some(pid) = pid.filter(|p| *p < 4) else { return };
        let st = self.shadow[pid].state;
        if st == UNAVAILABLE || st == VACANT {
            return;
        }
        let style = self.config_style.unwrap_or_else(|| style_of(cx));
        let ds = default_state_for_player(pid as i32, style);
        Self::slot_do(cx, pid, move |p, cx| p.set_state(cx, ds, false));
        self.set_state(cx, 0, true);
    }

    /// Set State @19871 -> @7149, then Update Appearance
    fn set_state(&mut self, cx: &mut Cx<'_>, s: i32, force: bool) {
        if !force && s == self.setup_state {
            return;
        }
        self.setup_state = s;
        if s == 0 {
            self.toggle_characters_mode(cx, true);
            for i in 0..4 {
                self.focus_character_for_player(cx, i);
                if self.shadow[i].state > SOFT_LOCKED {
                    Self::slot_do(cx, i, |p, cx| p.set_state(cx, SOFT_LOCKED, false));
                }
            }
        } else if s == 1 {
            self.toggle_characters_mode(cx, false);
            for i in 0..4 {
                if self.shadow[i].state == SOFT_LOCKED {
                    Self::slot_do(cx, i, |p, cx| p.set_state(cx, COSMETICS, false));
                }
            }
        } else if s == 2 {
            self.toggle_characters_mode(cx, false);
            self.show_map_selector(cx);
        }
        self.update_appearance(cx);
    }

    // ---------- the map step and Begin Match ----------

    /// Set State(2) @8782: ShowModal-Generic(UI_LAB_MapSelector, "mapSelector"); SelectedRotation {6,5,7,12}[style]
    /// (@9185: CustomSolos, CustomDuos, CustomFreeForAll, Default; read on the online path only); forced offline (the
    /// port's, as everywhere) the soft maps are Get Offline Soft Maps(LocalPlay true) @275; Init(SoftMaps,
    /// MapSlugsToData(Maps, Get-LocalPlay-Maps), {2,4,4,0}[style], true, true) @724: the saved maps selected, the
    /// player count label, multi select, no dismiss at Accept (LabMapSelector.init); then MultiSelectedEvent ->
    /// OnMapsSelected @865, CanceledEvent -> OnMapSelectionCanceled @929.
    fn show_map_selector(&mut self, cx: &mut Cx<'_>) {
        let style = usize::from(style_of(cx)).min(3);
        let Some(w) = show_modal_generic(cx, false, "UI_LAB_MapSelector", "mapSelector", "") else { return };
        let soft: Vec<String> = cx.view.offline_maps_local().to_vec();
        // MapSlugsToData: the soft maps whose slug was saved
        let saved = cx.prefs.local_play.maps.clone();
        let selected: Vec<String> =
            soft.iter().filter(|n| cx.view.map(n).is_some_and(|m| saved.contains(&m.data.slug))).cloned().collect();
        let (h1, h2) = (handler(cx, L_MAPS), handler(cx, L_MAP_CANCELED));
        cx.visit::<LabMapSelector, _>(w, |m, cx| {
            m.selected_rotation = [6, 5, 7, 12][style];
            m.init(cx, &soft, &selected, REQUIRED[style] as i32, true, true);
            m.multi_selected_event.add(h1);
            m.canceled_event.add(h2);
        });
    }

    /// OnMapSelectionCanceled @28636: the first Ready slot back to ConfirmReady (Set State(8)), the screen to 1
    fn on_map_selection_canceled(&mut self, cx: &mut Cx<'_>) {
        if let Some(i) = (0..4).find(|i| self.shadow[*i].state == READY) {
            Self::slot_do(cx, i, |p, cx| p.set_state(cx, CONFIRM_READY, false));
        }
        self.set_state(cx, 1, false);
    }

    /// Begin Match @28801, with every slot Ready: Remove Bindings, then two Delays from the same moment (the
    /// sequence @28864-28944): 0.1 s -> @2961 Transition UIManager("", type 0, delay 0, force false, instant false,
    /// levelOptions alias): the fade to black that stays black (an empty nextLevel opens and dismisses nothing);
    /// 1.25 s -> @2631 Save All Settings, RequestDismiss("", false), RemoveFromParent, BPI_RemoveMainMenu,
    /// BPI_LocalPlayBegin.
    fn begin_match(&mut self, cx: &mut Cx<'_>) {
        if self.begin_pending || !self.players_with_state(READY, style_of(cx)).1 {
            return;
        }
        self.begin_pending = true;
        self.remove_bindings(cx);
        let me = cx.w;
        cx.view.delay(0.1, Handler { owner: me, id: L_BEGIN_FADE });
        cx.view.delay(1.25, Handler { owner: me, id: L_BEGIN_SAVE });
    }

    /// BPI_LocalPlayBegin (PGI @35771): AddToHomescreenQueue(14), BPI_SetIsLocalPlay(true), "Local Play Begin Match":
    /// LocalPlayFacilitator.BeginMatch, whose map load ends in Transition(level, type 2
    /// InToCountdown, levelOptions alias) @3275.
    /// Deviation: type 2 (from black, UI_Countdown_C added at ZOrder 4000, a 0.3 s fade 1 -> 0, then OpenLevel(level,
    /// "game=" + alias)) is not ported (`Manager::transition` refuses it): a type 3 (Black) with a 0.3 s delay stands
    /// in, the fader black until the level opens at its end (complete_transition: DismissAllModals, then the level),
    /// with no countdown widget over a fade; whether UI_Countdown_C outlives the OpenLevel is open (the match's own
    /// countdown card is the program's).
    fn local_play_begin(&mut self, cx: &mut Cx<'_>) {
        cx.view.session.add_to_homescreen_queue(HOMESCREEN_LOCAL_PLAY, false);
        cx.view.session.set_is_local_play(true);
        let devices = local_player_devices();
        let r = cx.view.manager.random();
        let Some(start) = begin_local_play_match(cx.view, cx.prefs, &devices, &mut || r) else {
            eprintln!("menu: Local Play Begin Match: no map of the selection is in the menu data");
            return;
        };
        let next = if start.level.is_empty() { start.map.clone() } else { start.level.clone() };
        let alias = start.alias;
        // OnLoad @1877: the match for the program when the level is up
        cx.view.session.started = Some(Box::new(start));
        Manager::transition(
            cx,
            TransitionParams {
                next_level: next,
                kind: 3,
                delay: 0.3,
                force: false,
                instant: false,
                level_options: format!("game={alias}"),
            },
        );
    }

    /// Save All Settings @25542 (PreDismiss and the 1.25 s of Begin Match): per slot Set-LocalPlay-SettingsForPlayer
    /// (LocalPlaySettingsFromPlayer, @7121 -> @3306), Set-LocalPlay-Maps(GetMapSlugs(Maps)) @25709, SavePreferences
    /// @25603 (the program saves the settings it holds). (GetMapSlugs of no Maps yet: an empty list, as the build's
    /// unset array.) LocalPlaySettingsFromPlayer (fn @0-859): the slot's Get Selections, the skin's slug and its soft
    /// reference (the SkinData's path, which LocalPlayFacilitator SetPlayerSelections @524 loads for AssignSkin
    /// @641), the slot's Team Index, the taunts one by one
    fn save_all_settings(&mut self, cx: &mut Cx<'_>) {
        for i in 0..4usize {
            let w = Self::slot_w(cx, i);
            let Some((g, team)) = cx.visit::<MatchSetupPlayer, _>(w, |p, cx| (p.get_selections(cx), p.team_index))
            else {
                continue;
            };
            let p = &mut cx.prefs.local_play.players[i];
            p.character_slug = g.character_slug;
            p.skin_slug = g.skin.as_ref().map(|s| s.slug.clone()).unwrap_or_default();
            p.skin_reference = g.skin.as_ref().map(|s| s.path.clone()).unwrap_or_default();
            p.team_index = u8::try_from(team).unwrap_or(0);
            for (k, slot) in p.perk_slugs.iter_mut().enumerate() {
                *slot = g.perk_slugs.get(k).cloned().unwrap_or_default();
            }
            p.ringout_slug = g.ringout_slug;
            p.banner_slug = g.banner_slug;
            for (k, slot) in p.taunts.iter_mut().enumerate() {
                *slot = g.taunt_slugs.get(k).cloned().unwrap_or_default();
            }
            p.username = g.username;
        }
        cx.prefs.local_play.maps = self.maps.clone().unwrap_or_default();
    }

    /// Toggle Characters Mode @21235 (the grid's SetIsEnabled is not ported: the grid is never left on this screen)
    fn toggle_characters_mode(&mut self, cx: &mut Cx<'_>, on: bool) {
        self.character_mode = on;
        self.toggle_at = cx.now();
        self.apply_toggle(cx, 0.0);
    }

    /// @1685-2465: the grid's opacity Ease(0, 1, EaseInOut, 3) and the header's height Ease(70, 700, EaseOut, 3), both
    /// reversed out of characters mode (SelectFloat on Character Mode)
    fn apply_toggle(&mut self, cx: &mut Cx<'_>, t: f64) {
        let a = ease_in_out(0.0, 1.0, t, 3.0);
        let g = Self::grid(cx);
        let opacity = if self.character_mode { a } else { 1.0 - a };
        wd(cx, g).opacity = opacity;
        if let Some(b) = &self.bots {
            b.show(cx, matches!(style_of(cx), 1 | 2), opacity);
        }
        let h = if self.character_mode { ease_out(70.0, 700.0, t, 3.0) } else { ease_out(700.0, 70.0, t, 3.0) };
        // WBP_MenuHeader.UpdateHeaderHeight (@2367): the HeaderHeight variable (the header's PreConstruct, after the
        // first step of OnInitialized, sizes Spacer_StripVert by it), SpacerFillVert's height HeaderHeight +
        // StripWidth
        let hd = Self::header(cx);
        cx.view.tree_mut().set_prop(hd, "HeaderHeight", json!(h));
        let strip = cx.view.tree().props(hd).get("StripWidth").and_then(serde_json::Value::as_f64).unwrap_or(0.0);
        let sp = cx.view.tree().find_in(hd, "SpacerFillVert");
        wd(cx, sp).brush_h = h + strip;
        if t >= 1.0 {
            self.toggle_at = -1.0;
        }
    }

    /// Focus Character For Player @21425
    fn focus_character_for_player(&mut self, cx: &mut Cx<'_>, player: usize) {
        let st = self.shadow[player].state;
        let pi = player as i32;
        Manager::unfocus_all(cx, pi);
        let character = self.shadow[player].character.clone();
        let g = Self::grid(cx);
        run_on::<GridItemList>(cx, g, move |g, cx| {
            g.deselect_all(cx);
            if st == CHARACTER {
                let at =
                    character.as_ref().and_then(|c| g.index_for_data(|d| d.roster().is_some_and(|r| r.slug == c.slug)));
                g.focus_index(cx, pi, at.unwrap_or(player));
            }
        });
        if st == COSMETICS || st == PERKS || st == READY {
            Manager::unfocus_all(cx, pi);
        }
    }

    /// OnPlayerStateChanged @20038
    fn on_player_state_changed(&mut self, cx: &mut Cx<'_>, player: usize, s: i32, at: f64) {
        self.shadow[player].state = s;
        self.shadow[player].last_change = at;
        let style = style_of(cx);
        let all = |this: &Self, state: i32| this.players_with_state(state, style).1;
        let pi = player as i32;
        if s == UNAVAILABLE || s == VACANT {
            Manager::unfocus_all(cx, pi);
        } else if s == CHARACTER {
            self.set_state(cx, 0, false);
            self.focus_character_for_player(cx, player);
        } else if s == SKIN {
            self.set_state(cx, 0, false);
        } else if s == SOFT_LOCKED {
            if all(self, SOFT_LOCKED) && self.are_teams_valid(style) {
                self.set_state(cx, 1, false);
            }
        } else if s == READY && all(self, READY) {
            self.set_state(cx, 2, false);
        }
        self.refresh_character_selected_indicators(cx);
        self.refresh_interactive_state(cx);
        self.update_appearance(cx);
    }

    /// Refresh Character Selected Indicators: a slot's tile is selected for it past the character step
    fn refresh_character_selected_indicators(&mut self, cx: &mut Cx<'_>) {
        let p0 = &self.shadow[0];
        let show = ![VACANT, CHARACTER, UNAVAILABLE].contains(&p0.state);
        let slug = p0.character.as_ref().map(|c| c.slug.clone());
        let g = Self::grid(cx);
        run_on::<GridItemList>(cx, g, move |g, cx| {
            for w in g.items() {
                let is_p0 =
                    matches!(g.record(w), Some((Some(TileData::Roster(r)), _)) if Some(&r.slug) == slug.as_ref());
                let sel = show && is_p0;
                run_tile(cx, w, move |t, cx| {
                    if t.state().selected != sel {
                        t.state().selected = sel;
                        t.update_appearance(cx);
                    }
                });
            }
        });
    }

    /// Refresh Interactive State For Players: the grid answers a player in the character step only; then Toggle
    /// Unavailable Players (@7070: a slot in state 0 gets its reason)
    fn refresh_interactive_state(&mut self, cx: &mut Cx<'_>) {
        let states: Vec<i32> = self.shadow.iter().map(|s| s.state).collect();
        let g = Self::grid(cx);
        run_on::<GridItemList>(cx, g, move |g, cx| {
            for (p, st) in states.iter().enumerate() {
                let on = *st == CHARACTER;
                for w in g.items() {
                    let list = cx
                        .visit_dyn(w, |c, cx| c.as_grid_item().map(|t| t.interactors(cx)))
                        .flatten()
                        .unwrap_or_default();
                    if let Some(&i) = list.get(p) {
                        set_interactable(cx, i, on);
                    }
                }
            }
        });
        for i in 0..4 {
            if self.shadow[i].state == UNAVAILABLE {
                Self::slot_do(cx, i, |p, cx| p.set_unavailable(cx, true, "CHANGE GAME MODES TO ADD MORE SLOTS"));
            }
        }
    }

    /// GetPlayersWithState @895 (Not Equal false): the count and Requirement Met
    fn players_with_state(&self, s: i32, style: u8) -> (usize, bool) {
        let count = self.shadow.iter().filter(|p| p.state == s).count();
        (count, count == REQUIRED[usize::from(style).min(3)])
    }

    /// AreTeamsValid? @1061: the joined slots per team against the style's team size and team count
    fn are_teams_valid(&self, style: u8) -> bool {
        let mut counts = [0usize; 4];
        for p in &self.shadow {
            if ![UNAVAILABLE, VACANT, INVITE].contains(&p.state) {
                counts[usize::try_from(p.team).unwrap_or(0).min(3)] += 1;
            }
        }
        let st = usize::from(style).min(3);
        let size = [1, 2, 1, 0][st];
        counts.iter().filter(|c| **c == size).count() == [2, 2, 4, 0][st]
    }

    /// Update Appearance @27484: the validation warning (Update Validation Warning @31712: Conditional Visibility 4 /
    /// 2)
    fn update_appearance(&mut self, cx: &mut Cx<'_>) {
        let style = style_of(cx);
        let soft = self.players_with_state(SOFT_LOCKED, style);
        let vacant = self.players_with_state(VACANT, style);
        let (mut title, mut description, mut show) = ("", "", false);
        if soft.1 && !self.are_teams_valid(style) {
            (title, description, show) =
                ("TEAM MISMATCH", "Please ensure all players are on correct teams for the game mode.", true);
        } else if soft.0 > 0 && soft.0 + vacant.0 == REQUIRED[usize::from(style).min(3)] && !soft.1 {
            (title, description, show) = (
                "NOT ENOUGH PLAYERS",
                "Please ensure the number of players fits the selected game mode or change your match settings.",
                true,
            );
        }
        let (t, d, w) = (find(cx, "validationTitle"), find(cx, "validationDescription"), find(cx, "validationWarning"));
        wd(cx, t).text = title.to_string();
        wd(cx, d).text = description.to_string();
        wd(cx, w).vis = if show { Vis::SelfHitTestInvisible } else { Vis::Hidden };
        // @16521-16714: every slot's ready card waits until all the style takes are ready
        let waiting = !self.players_with_state(READY, style).1;
        for i in 0..4 {
            let slot = Self::slot_w(cx, i);
            let r = cx.view.tree().find_in(slot, "UI_LP_Player_Ready");
            run_on::<LPPlayerReady>(cx, r, move |r, cx| r.set_waiting(cx, waiting));
        }
    }

    /// the grid's FocusedEvent @25755: Update Character From List(false) @27074 (a CharacterData only: the Random
    /// tile changes nothing). (The port's own: the focused fighter's skins are held, the slot's picture and the
    /// strip.)
    fn on_grid_focused(&mut self, cx: &mut Cx<'_>, ev: &TileEvent) {
        let Some(c) = ev.data.as_ref().and_then(TileData::roster).cloned() else { return };
        let Ok(player) = usize::try_from(ev.player) else { return };
        if player > 3 {
            return;
        }
        let c2 = c.clone();
        Self::slot_do(cx, player, move |p, cx| p.set_character(cx, Some(&c2)));
        let names = skin_pictures(cx.view, c.fighter.as_deref());
        cx.view.hold_pictures(PICTURES_LOCAL_SKINS, names.iter().map(String::as_str));
    }

    /// the grid's SelectedEvent @25789: SetHasFocused(false), UnfocusAll, Update Character From List(true) (Set
    /// Character, then Selected Character @27438: the slug saved), Set State(4).
    /// Deviation: the port's lock: a fighter the port lacks is not picked.
    fn on_grid_selected(&mut self, cx: &mut Cx<'_>, ev: &TileEvent) {
        let Some(c) = ev.data.as_ref().and_then(TileData::roster).cloned() else { return };
        if ev.locked {
            return;
        }
        let Ok(player) = usize::try_from(ev.player) else { return };
        if player > 3 {
            return;
        }
        Manager::unfocus_all(cx, ev.player);
        Self::slot_do(cx, player, move |p, cx| {
            p.set_character(cx, Some(&c));
            p.selected_character(cx, &c);
            p.set_state(cx, SKIN, false);
        });
    }

    /// OnSkinSelected @29102: the skin step done
    fn on_skin_selected(&mut self, cx: &mut Cx<'_>, player: usize) {
        if self.setup_state == 0 {
            Self::slot_do(cx, player, |p, cx| p.set_state(cx, SOFT_LOCKED, false));
        } else if self.setup_state == 1 {
            Self::slot_do(cx, player, |p, cx| p.set_state(cx, COSMETICS, false));
        }
    }

    /// Refresh Team Perks @31511: in Teams, each player of a team (GetPlayersOnTeam: the slots of that team index, any
    /// state) takes its teammates' perks as Set Ally Equipped Perks, the last one's standing (@14701-15822); in 1v1
    /// and FFA every slot an empty list (@16019-16516).
    fn refresh_team_perks(&mut self, cx: &mut Cx<'_>) {
        match style_of(cx) {
            1 => {
                for team in 0..=3 {
                    let on: Vec<usize> = (0..4).filter(|i| self.shadow[*i].team == team).collect();
                    for &a in &on {
                        for &b in &on {
                            if a == b {
                                continue;
                            }
                            Self::slot_do(cx, a, move |pa, cx| {
                                let perks = pa.get_perks(cx);
                                let wb = Self::slot_w(cx, b);
                                run_on::<MatchSetupPlayer>(cx, wb, move |pb, cx| pb.set_ally_equipped_perks(cx, perks));
                            });
                        }
                    }
                }
            }
            0 | 2 => {
                for i in 0..4 {
                    Self::slot_do(cx, i, |p, cx| p.set_ally_equipped_perks(cx, Vec::new()));
                }
            }
            _ => {}
        }
    }

    /// OnConfirmCharacterShortcut @31269: CharacterGrid.SelectIndex(IndexForData(the slot's character))
    fn on_confirm_character_shortcut(&mut self, cx: &mut Cx<'_>, player: usize) {
        let c = self.shadow[player].character.clone();
        let g = Self::grid(cx);
        let pi = player as i32;
        run_on::<GridItemList>(cx, g, move |g, cx| {
            let at = c.as_ref().and_then(|c| g.index_for_data(|d| d.roster().is_some_and(|r| r.slug == c.slug)));
            let Some(w) = at.and_then(|a| g.items().get(a).copied()) else { return };
            run_tile(cx, w, move |t, cx| crate::menu::widgets::controls::grid_item::GridItemState::select(t, cx, pi));
        });
    }

    /// Show Match Settings @25297 -> CanShowMatchSettings? (not the map step, no dialog, not open) ->
    /// ShowModal-Generic(false, UI_QuickMatchSettings, localPlayMatchSettings) (@25330, quick_match_settings.rs)
    fn show_match_settings(&mut self, cx: &mut Cx<'_>) {
        if self.setup_state == 2
            || Manager::is_dialog_active(cx.view)
            || cx.view.manager.get_modal("localPlayMatchSettings").is_some()
        {
            return;
        }
        show_modal_generic(cx, false, "UI_QuickMatchSettings", "localPlayMatchSettings", "");
    }

    /// OnModalDismissed(Modal ID) @26059: the match settings closed (NotEqual_StriStri: no case) -> Handle Match
    /// Settings (the modal saved the LocalPlay preferences at its PreDismiss)
    fn on_modal_dismissed(&mut self, cx: &mut Cx<'_>, id: &str) {
        if id.eq_ignore_ascii_case("localplaymatchsettings") {
            self.handle_match_settings(cx);
        }
    }

    fn exit_or_ask(&mut self, cx: &mut Cx<'_>) {
        let style = style_of(cx);
        if self.players_with_state(VACANT, style).0 + self.players_with_state(UNAVAILABLE, style).0 == 3 {
            self.exit(cx);
            return;
        }
        let d = Manager::show_dialog(
            cx,
            DialogParams {
                title: "Really exit?".into(),
                body: "Are you sure you want to exit local play?".into(),
                button_left_label: "Yes".into(),
                button_right_label: "No".into(),
                ..DialogParams::default()
            },
        );
        let h = handler(cx, L_EXIT);
        if let Some(dialog) = cx.view.dialog(d) {
            dialog.button_left_clicked().add(h);
        }
    }

    /// Exit @19645: ToggleMultiInput(false) @19691 (the devices table), BPI_SetIsLocalPlay(false) @19828,
    /// DismissModal, and the matchmaking handler's EOGDataCache = [] @19595 (the port keeps no end-of-game cache:
    /// nothing to clear)
    fn exit(&mut self, cx: &mut Cx<'_>) {
        toggle_multi_input(false);
        cx.view.session.set_is_local_play(false);
        let id = self.base.modal_id.clone();
        Manager::dismiss_modal(cx, &id);
    }

    /// Controller connection changes the program queued (`pad_connection_changed`), heard in order, each before the
    /// table moves.
    fn hear_pads(&mut self, cx: &mut Cx<'_>) {
        for e in take_pad_events() {
            self.on_controller_connection_changed(cx, e.pad, e.connected, e.player);
        }
    }
}

impl Modal for LocalPlay {
    fn pre_show(&mut self, cx: &mut Cx<'_>, id: &str) {
        self.base.pre_show(cx, id);
    }

    /// PostShow @18157: Update Context(NewGuid), the context interactor's focus, and after Delay(0.25) @15 the grid
    /// filled with Get Characters (every character of the roster; the port's lock keeps one it lacks from a pick)
    fn post_show(&mut self, cx: &mut Cx<'_>, _instructions: &str, _extra_data: &str) {
        let guid = cx.view.manager.new_guid();
        self.update_context(cx, &guid);
        let ci = find(cx, "contextInteractor");
        set_focused(cx, ci, false);
        let names = roster_pictures(cx.view);
        cx.view.hold_pictures(PICTURES_LOCAL, names.iter().map(String::as_str));
        let me = cx.w;
        cx.view.delay(0.25, Handler { owner: me, id: L_POPULATE });
    }

    /// PreDismiss @25503: Save All Settings, Remove Bindings
    fn pre_dismiss(&mut self, cx: &mut Cx<'_>) {
        self.save_all_settings(cx);
        self.remove_bindings(cx);
    }

    fn post_dismiss(&mut self, cx: &mut Cx<'_>) {
        cx.view.release_pictures(PICTURES_LOCAL);
        cx.view.release_pictures(PICTURES_LOCAL_SKINS);
        BotSeats::release(cx);
        for i in 0..4 {
            let w = Self::slot_w(cx, i);
            cx.visit::<MatchSetupPlayer, _>(w, |p, cx| p.release_pictures(cx));
        }
    }

    /// RequestDismiss @18328: a cancel from player 0's character step (0.25 s after its last change) or the nav bar's
    /// back arrow leaves at once when nobody else joined, else asks first (OnConfirmExit -> Exit); anything else
    /// dismisses
    fn request_dismiss(&mut self, cx: &mut Cx<'_>, reason: &str, from_cancel: bool) {
        if from_cancel {
            let p0 = &self.shadow[0];
            if p0.state != CHARACTER || cx.now() - p0.last_change <= 0.25 {
                return;
            }
            self.exit_or_ask(cx);
        } else if reason == "globalnavstepback" {
            self.exit_or_ask(cx);
        } else {
            let id = self.base.modal_id.clone();
            Manager::dismiss_modal(cx, &id);
        }
    }

    fn get_modal_context(&self) -> String {
        self.base.get_modal_context()
    }
}

impl Controller for LocalPlay {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        self.init(cx);
    }

    /// Deviation: the 16:9 box follows the page
    fn construct(&mut self, cx: &mut Cx<'_>) {
        let b = find(cx, "SizeBox_1");
        fill_page(cx, b);
    }

    fn tick(&mut self, cx: &mut Cx<'_>, _dt: f64) {
        let b = find(cx, "SizeBox_1");
        fill_page(cx, b);
    }

    fn timers(&mut self, cx: &mut Cx<'_>, _dt: f64) {
        if self.toggle_at >= 0.0 {
            let t = ((cx.now() - self.toggle_at) / 0.45).min(1.0);
            self.apply_toggle(cx, t);
        }
        self.hear_pads(cx);
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        let slot = |p: i32| usize::try_from(p).ok().filter(|p| *p < 4);
        match id {
            L_STATE => {
                if let Some(&(p, s, t)) = arg.downcast_ref::<(i32, i32, f64)>()
                    && let Some(p) = slot(p)
                {
                    self.on_player_state_changed(cx, p, s, t);
                }
            }
            L_CHARACTER => {
                if let Some((p, c)) = arg.downcast_ref::<(i32, Option<RosterEntry>)>()
                    && let Some(p) = slot(*p)
                {
                    self.shadow[p].character = c.clone();
                }
            }
            L_TEAM => {
                if let Some(&(p, t)) = arg.downcast_ref::<(i32, i32)>()
                    && let Some(p) = slot(p)
                {
                    self.shadow[p].team = t;
                }
            }
            L_SKIN => {
                if let Some((p, _)) = arg.downcast_ref::<(i32, SkinInfo)>()
                    && let Some(p) = slot(*p)
                {
                    self.on_skin_selected(cx, p);
                }
            }
            L_SHORTCUT => {
                if let Some(p) = arg.downcast_ref::<i32>().and_then(|p| slot(*p)) {
                    self.on_confirm_character_shortcut(cx, p);
                }
            }
            L_JOIN => {
                if let Some(&p) = arg.downcast_ref::<i32>() {
                    self.on_attempt_join_slot(cx, p);
                }
            }
            // OnCosmeticsDone @29631, OnPerksReady @29485, OnConfirmReady @29777: the slot's next step
            L_COS_DONE | L_PERKS_READY | L_CONFIRM_READY => {
                if let Some(p) = arg.downcast_ref::<i32>().and_then(|p| slot(*p)) {
                    let next = match id {
                        L_COS_DONE => PERKS,
                        L_PERKS_READY => CONFIRM_READY,
                        _ => READY,
                    };
                    Self::slot_do(cx, p, move |s, cx| s.set_state(cx, next, false));
                }
            }
            // OnPerksChanged @31697: Refresh Team Perks
            L_PERKS_CHANGED => self.refresh_team_perks(cx),
            // OnUIReset (player 0) and the button
            L_RESET_KEY => {
                if arg.downcast_ref::<i32>() == Some(&0) {
                    self.show_match_settings(cx);
                }
            }
            L_SETTINGS_BUTTON => self.show_match_settings(cx),
            L_DISMISSED => {
                if let Some(m) = arg.downcast_ref::<String>() {
                    self.on_modal_dismissed(cx, m);
                }
            }
            L_GRID_FOCUSED => {
                if let Some(ev) = arg.downcast_ref::<TileEvent>() {
                    self.on_grid_focused(cx, ev);
                }
            }
            L_GRID_SELECTED => {
                if let Some((ev, _)) = arg.downcast_ref::<(TileEvent, bool)>() {
                    self.on_grid_selected(cx, ev);
                }
            }
            L_GRID_POPULATED => self.on_grid_populated(cx),
            // OnMapsSelected @27032: Maps, then Begin Match
            L_MAPS => {
                if let Some(m) = arg.downcast_ref::<Vec<crate::bundle::MapInfo>>() {
                    self.maps = Some(m.iter().map(|m| m.data.slug.clone()).collect());
                    self.begin_match(cx);
                }
            }
            L_MAP_CANCELED => self.on_map_selection_canceled(cx),
            L_EXIT => self.exit(cx),
            B_ADD | B_SETTINGS | B_REMOVE => self.bot_seat_event(cx, id, arg),
            _ => {}
        }
    }

    fn resume(&mut self, cx: &mut Cx<'_>, id: u32) {
        match id {
            L_POPULATE => {
                self.roster = cx.view.roster().to_vec();
                let data: Vec<TileData> = self.roster.iter().cloned().map(TileData::Roster).collect();
                let g = Self::grid(cx);
                run_on::<GridItemList>(cx, g, move |g, cx| g.populate(cx, data));
            }
            L_BEGIN_FADE => {
                let style = style_of(cx);
                Manager::transition(
                    cx,
                    TransitionParams {
                        next_level: String::new(),
                        kind: 0,
                        delay: 0.0,
                        force: false,
                        instant: false,
                        level_options: team_style_alias(style).to_string(),
                    },
                );
            }
            L_BOTS_REFRESH => self.refresh_bots(cx),
            L_BEGIN_SAVE => {
                self.begin_pending = false;
                self.save_all_settings(cx);
                self.request_dismiss(cx, "", false);
                // (RemoveFromParent: the dismiss took the widget; BPI_RemoveMainMenu: the main menu screen goes as
                // the level opens)
                self.local_play_begin(cx);
            }
            _ => {}
        }
    }

    fn as_modal(&mut self) -> Option<&mut dyn Modal> {
        Some(self)
    }
}

/// The keyboard and mouse as a source of the last menu input, for the app's call sites (`InputSource`).
pub const fn kbm_source() -> InputSource {
    InputSource::Kbm
}

/// The device a table slot holds, for the app's call sites.
pub const fn kbm_device() -> Device {
    Device::Kbm
}
