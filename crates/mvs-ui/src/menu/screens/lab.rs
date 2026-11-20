//! The Lab menu's user widgets: UI_LAB_PauseMenu, UI_LAB_Panel, its four UI_LAB_PlayerPanel and their fighter card.
//! They talk to the running Lab through `cx.lab` (training_config.rs).
//!
//! The perk page (UI_Perks_Equipper in PerksContainer) opens from the rows' edit buttons and perk tiles. The sidebar
//! (WBP_GlobalNav) and the social panel (UI_SocialPanel) are widgets/nav.rs's and widgets/social.rs's: the pause
//! menu hands them its context.

use std::any::Any;

use mvs_protocol::lobby_rules::{LobbyOptions, OptionValue};
use serde_json::json;

use crate::bundle::{MapInfo, Rgba, RosterEntry, SkinInfo};
use crate::menu::controller::{Controller, Cx, Event, Handler};
use crate::menu::custom_text;
use crate::menu::manager::{DialogParams, Manager, NotificationParams, TransitionParams};
use crate::menu::pictures::PICTURES_LAB_PERKS;
use crate::menu::screens::lab_selectors::{
    CLEAR, LabCharacterSelector, LabMapSelector, LabSkinSelector, run_on, set_color, set_opacity, set_text, set_vis,
    skin_selector_data,
};
use crate::menu::screens::move_list::toggle_movelist;
use crate::menu::training_config::{LAB_EXPERT, LabConfig, LabPlayerConfig, empty_lab_player};
use crate::menu::view::MenuView;
use crate::menu::widgets::common::{self, Quadrilateral, fill_page, palette_entry};
use crate::menu::widgets::controls::buttons::{
    ButtonMain, ButtonSecondary, ButtonSecondaryVertical, IconButton, ToggleSwitch,
};
use crate::menu::widgets::controls::drop_menu::DropMenu;
use crate::menu::widgets::controls::interactor::{FocusedCommon, batch_set_context, set_focused, set_interactable};
use crate::menu::widgets::controls::tooltip::close_all_tooltips;
use crate::menu::widgets::controls::{bind, find, prop, prop_f64, quad_color, wd};
use crate::menu::widgets::custom_settings::{CustomGameSettings, LobbyView, OptionRequest, lab_experimental_page};
use crate::menu::widgets::nav::GlobalNav;
use crate::menu::widgets::perks::{
    LoadoutPick, Perk, PerksEquipper, PerksLoadout, PerksSaved, perk_data, perk_list_pictures, perk_pictures,
};
use crate::menu::widgets::social::SocialPanel;
use crate::slate::{Vis, WidgetId};

/// LAB_BOTS: the Lab's bot entries; EBotBehavior n is entry n - 1 (0 a human).
const LAB_BOTS: [&str; 28] = [
    "recovery",
    "fight/veryeasy",
    "fight/easy",
    "fight/medium",
    "fight/hard",
    "idle",
    "jumping",
    "evade/evadeNeutral",
    "evade/evadeBack",
    "evade/evadeForward",
    "evade/evadePostHit",
    "normalAttack/neutral",
    "normalAttack/down",
    "normalAttack/up",
    "normalAttack/forward",
    "specialAttack/neutral",
    "specialAttack/down",
    "specialAttack/up",
    "specialAttack/forward",
    "stun_di/forward",
    "stun_di/back",
    "stun_di/up",
    "stun_di/down",
    "evade_di/forward",
    "evade_di/back",
    "evade_di/up",
    "evade_di/down",
    "patrolling",
];

fn lerp(a: Rgba, b: Rgba, t: f64) -> Rgba {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t, a[3] + (b[3] - a[3]) * t]
}

fn display_name(cx: &Cx<'_>, character: Option<&str>) -> String {
    match character {
        Some(c) => cx.view.fighters().get(c).map_or_else(|| c.to_string(), |f| f.display_name.clone()),
        None => String::new(),
    }
}

/// The palette entry of a team index (UI_LAB_PlayerPanel @5083-5302, the card's SetTeamColor @4012).
fn team_color(cx: &Cx<'_>, team: i32) -> Rgba {
    palette_entry(cx, [0, 1, 3, 4].get(team.max(0) as usize).copied().unwrap_or(0))
}

/// The interactor of a button / toggle child `name` of the context's widget.
fn inter(cx: &Cx<'_>, name: &str) -> WidgetId {
    let w = find(cx, name);
    cx.view.tree().find_in(w, "UI_Interactor")
}

/// A number's text, as String(number) gives it.
fn num_text(v: f64) -> String {
    if v.fract() == 0.0 { format!("{}", v as i64) } else { format!("{v}") }
}

fn local_leads(cx: &Cx<'_>) -> bool {
    cx.lab.as_ref().is_some_and(|s| s.config().leader_account_id == s.me())
}

/// The handler of `b`'s on_click event (a ButtonSecondary, ButtonSecondaryVertical, IconButton, ButtonMain).
fn on_click_of<T: Controller>(cx: &mut Cx<'_>, name: &str, ev: fn(&mut T) -> &mut Event<()>, id: u32) {
    let w = find(cx, name);
    let me = cx.w;
    cx.visit::<T, _>(w, |b, _| ev(b).add(Handler { owner: me, id }));
}

// ---------- UI_PlusMinusVertical ----------

const PM_PLUS: u32 = 1;
const PM_MINUS: u32 = 2;

#[derive(Debug, Default)]
pub struct PlusMinusVertical {
    /// OnValueChanged(value, fraction).
    pub on_value_changed: Event<(f64, f64)>,
    pub value: f64,
}

impl PlusMinusVertical {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::<PlusMinusVertical>::default()
    }

    fn minimum(cx: &Cx<'_>) -> f64 {
        prop_f64(cx, "Minimum", 0.0)
    }

    fn maximum(cx: &Cx<'_>) -> f64 {
        prop_f64(cx, "Maximum", 0.0)
    }

    /// GetInteractors: minus, then plus (the context's widget is the control).
    pub fn interactors(cx: &Cx<'_>) -> Vec<WidgetId> {
        vec![inter(cx, "ButtonMinus"), inter(cx, "ButtonPlus")]
    }

    /// SetValue @287 (not clamped; Modify clamps).
    pub fn set_value(&mut self, cx: &mut Cx<'_>, v: f64) {
        self.value = v;
        set_text(cx, "ValueText", &num_text(v));
        let (lo, hi) = (Self::minimum(cx), Self::maximum(cx));
        cx.emit(self, |c| &c.on_value_changed, &(v, (v + lo) / (hi - lo)));
    }

    fn modify(&mut self, cx: &mut Cx<'_>, a: f64) {
        let (lo, hi) = (Self::minimum(cx), Self::maximum(cx));
        let v = (self.value + a).max(lo).min(hi);
        self.set_value(cx, v);
    }
}

impl Controller for PlusMinusVertical {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        on_click_of::<ButtonSecondary>(cx, "ButtonPlus", |b| &mut b.on_click, PM_PLUS);
        on_click_of::<ButtonSecondary>(cx, "ButtonMinus", |b| &mut b.on_click, PM_MINUS);
    }

    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        let t = common::text(Some(&prop(cx, "Label")));
        set_text(cx, "LabelText", &t);
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, _arg: &dyn Any) {
        let inc = prop_f64(cx, "Increment", 0.0);
        match id {
            PM_PLUS => self.modify(cx, inc),
            PM_MINUS => self.modify(cx, -inc),
            _ => {}
        }
    }
}

// ---------- UI_LAB_PlayerPanel_Character (the fighter card) ----------

const EDIT_QUAD: Rgba = [0.194618, 0.571125, 0.552012, 1.0];
const EDIT_QUAD_FOCUS: Rgba = [0.89627, 0.973446, 0.871367, 1.0];

const CD_FOCUS: u32 = 1;
const CD_UNFOCUS: u32 = 2;
const CD_CLICK: u32 = 3;
const CD_CHARACTER: u32 = 4;
const CD_SKIN: u32 = 5;
const CD_SKIN_CANCEL: u32 = 6;

/// The skin is its SkinData name ('' the default). A Random pick's skin is rolled by the game mode at the spawn
/// (GetMatchPlayerDataInputs @789 RandomSkin), not here: SetCharacter's own roll (@102-225, a random element of
/// Filtered Skins for the invalid Character) draws from no skins.
#[derive(Debug, Default)]
pub struct LabPlayerPanelCharacter {
    /// CharacterSelectedEvent(Character, Skin, bRandom).
    pub character_selected: Event<(Option<String>, String, bool)>,
    character: Option<String>,
    skin: Option<String>,
    team: i32,
}

impl LabPlayerPanelCharacter {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::<LabPlayerPanelCharacter>::default()
    }

    fn button(cx: &Cx<'_>) -> WidgetId {
        find(cx, "buttonChangeCharacter")
    }

    /// ShowCharacterSelector @2033.
    fn show_character_selector(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        let extra = self.character.clone().unwrap_or_default();
        let Some(m) = Manager::show_modal(cx, "UI_LAB_CharacterSelector", "labCharacterSelector", "lab", &extra) else {
            return;
        };
        cx.visit::<LabCharacterSelector, _>(m, |s, _| s.selected_event.add(Handler { owner: me, id: CD_CHARACTER }));
    }

    /// OnCharacterSelected @4289.
    fn on_character_selected(&mut self, cx: &mut Cx<'_>, c: &RosterEntry, random: bool) {
        if random {
            self.set_character(cx, None, "", true);
            cx.emit(self, |c| &c.character_selected, &(None, String::new(), true));
            return;
        }
        // @4343: the skin selector of the picked fighter; a cancel there shows the character selector again
        let me = cx.w;
        let f = c.fighter.clone().unwrap_or_default();
        // (the port's own) the slot's skin goes with the fighter when the slot has that fighter: the page ticks it
        let extra = match &self.character {
            Some(have) if *have == f => skin_selector_data(&f, self.skin.as_deref()),
            _ => f.clone(),
        };
        let Some(m) = Manager::show_modal(cx, "UI_LAB_SkinSelector", "labSkinSelector", "lab", &extra) else { return };
        cx.visit::<LabSkinSelector, _>(m, |s, _| {
            s.selected_event.add(Handler { owner: me, id: CD_SKIN });
            s.canceled_event.add(Handler { owner: me, id: CD_SKIN_CANCEL });
        });
    }

    /// OnSkinSelected @2389.
    fn on_skin_selected(&mut self, cx: &mut Cx<'_>, character: &str, skin: &SkinInfo) {
        self.set_character(cx, Some(character.to_string()), &skin.name, false);
        let ev = (self.character.clone(), self.skin.clone().unwrap_or_default(), false);
        cx.emit(self, |c| &c.character_selected, &ev);
    }

    /// SetCharacter @1057: the name follows a change of the character only, so a card that never had a fighter keeps
    /// its designer text for a Random slot. The skin name is collapsed on both branches of its Conditional
    /// Visibility.
    pub fn set_character(&mut self, cx: &mut Cx<'_>, character: Option<String>, skin: &str, random: bool) {
        if character != self.character {
            self.character = character;
            let t = if random { "Random".to_string() } else { display_name(cx, self.character.as_deref()) };
            set_text(cx, "CharacterName", &t);
        }
        if !random && self.skin.as_deref() != Some(skin) {
            self.skin = Some(skin.to_string());
            set_vis(cx, "SkinName", Vis::Collapsed);
        }
        self.update_appearance(cx);
    }

    /// UpdateAppearance @358: the skin's VariantIconMaterial (the default skin's without one), MI_Quad_RandomChar_Lab
    /// without a fighter.
    fn update_appearance(&mut self, cx: &mut Cx<'_>) {
        let tex = match self.character.as_deref().and_then(|c| cx.view.fighters().get(c)) {
            Some(f) => {
                let skin = self.skin.as_deref().unwrap_or("");
                f.skins.iter().find(|s| s.name == skin).map_or_else(|| f.skin.icon.clone(), |s| s.icon.clone())
            }
            None => "MI_Quad_RandomChar_Lab".to_string(),
        };
        let p = find(cx, "characterPortrait");
        wd(cx, p).tex = Some(tex);
    }

    /// SetTeamColor @3766.
    pub fn set_team_color(&mut self, cx: &mut Cx<'_>, team: i32) {
        self.team = team;
        let c = team_color(cx, team);
        set_color(cx, "CharacterTeamBG_Halftone", c);
        set_color(cx, "CharacterTeamBG", lerp(c, CLEAR, 0.6));
    }

    pub fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str) {
        let b = Self::button(cx);
        batch_set_context(cx, &[b], ctx);
    }
}

impl Controller for LabPlayerPanelCharacter {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        // @1779-1965
        let b = Self::button(cx);
        let fc = find(cx, "UI_FocusedCommon");
        cx.visit::<FocusedCommon, _>(fc, |f, cx| f.set_interactor(cx, b));
        let wob = cx.view.tree().find_in(fc, "Wobbler");
        cx.visit::<Quadrilateral, _>(wob, |q, cx| q.set_outline(cx, 0.1));
        bind(cx, b, |s| &mut s.on_focus, CD_FOCUS); // @2628
        bind(cx, b, |s| &mut s.on_unfocus, CD_UNFOCUS); // @3197
        bind(cx, b, |s| &mut s.on_click, CD_CLICK);
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        match id {
            CD_FOCUS => {
                let q = find(cx, "WBP_Quadrilateral_126");
                quad_color(cx, q, EDIT_QUAD_FOCUS);
                set_vis(cx, "GradientOverlay1", Vis::Hidden);
                let c = lerp(team_color(cx, self.team), CLEAR, 0.35);
                set_color(cx, "CharacterTeamBG", c);
            }
            CD_UNFOCUS => {
                let q = find(cx, "WBP_Quadrilateral_126");
                quad_color(cx, q, EDIT_QUAD);
                set_vis(cx, "GradientOverlay1", Vis::SelfHitTestInvisible);
                let c = lerp(team_color(cx, self.team), CLEAR, 0.6);
                set_color(cx, "CharacterTeamBG", c);
            }
            CD_CLICK | CD_SKIN_CANCEL => self.show_character_selector(cx),
            CD_CHARACTER => {
                if let Some((c, random)) = arg.downcast_ref::<(RosterEntry, bool)>() {
                    self.on_character_selected(cx, c, *random);
                }
            }
            CD_SKIN => {
                if let Some((c, s)) = arg.downcast_ref::<(String, SkinInfo)>() {
                    self.on_skin_selected(cx, c, s);
                }
            }
            _ => {}
        }
    }
}

// ---------- UI_LAB_PlayerPanel_Perks ----------

const PK_EDIT: u32 = 1;
const PK_PICK: u32 = 2;

/// The panel's perk row: its UI_Perks_Loadout (the perk tiles, which keep the slot's perks) and its edit button.
#[derive(Debug, Default)]
pub struct LabPlayerPanelPerks {
    /// EditPerksEvent(Perk Slot): -1 from buttonEdit, a tile's index from the loadout.
    pub edit_perks_event: Event<i32>,
}

impl LabPlayerPanelPerks {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::<LabPlayerPanelPerks>::default()
    }

    /// UpdateContext @218-296.
    pub fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str) {
        let l = Self::interactors(cx);
        batch_set_context(cx, &l, ctx);
        let lo = find(cx, "UI_Perks_Loadout");
        let c = ctx.to_string();
        run_on::<PerksLoadout>(cx, lo, move |l, cx| l.update_context(cx, &c, 0));
    }

    /// SetCharacter @39-168: Random clears the loadout and nothing else; another character than the loadout's loads
    /// its saved loadout. (Clear keeps the loadout's Character: X, Random, X leaves the row empty, as the build.)
    /// The saved loadout is the machine's (`remembered_perks`, the one the Lab's first match and the pre-match screen
    /// take): GetCharacterSavedPerks answers the account's loadouts, which the port keeps in the settings.
    pub fn set_character(&mut self, cx: &mut Cx<'_>, character: Option<&str>, random: bool) {
        let lo = find(cx, "UI_Perks_Loadout");
        let c = character.map(str::to_string);
        run_on::<PerksLoadout>(cx, lo, move |l, cx| {
            l.remember = true;
            if random {
                l.clear(cx);
            } else if c != l.character {
                l.set_character(cx, c.as_deref());
            }
        });
    }

    /// SetPerks @397: the loadout takes the ids' data (its own slot order).
    pub fn set_perks(&mut self, cx: &mut Cx<'_>, ids: &[Option<String>]) {
        let data: Vec<Option<Perk>> = ids.iter().map(|id| perk_data(cx.view, id.as_deref())).collect();
        let lo = find(cx, "UI_Perks_Loadout");
        run_on::<PerksLoadout>(cx, lo, move |l, cx| l.set_perks(cx, &data));
    }

    /// GetCurrentPerks: GetEquippedPerks(false, false, false), the four slots, an empty one None.
    /// (the panel's context: the row's widgets are not under it by name, so they are found in the row's own
    /// context, not by visiting the row: its own EditPerksEvent calls this while the row's handler runs.)
    pub fn current_perks(cx: &mut Cx<'_>) -> Vec<Option<String>> {
        let row = find(cx, "UI_LAB_PlayerPanel_Perks");
        let in_row = cx.with(row);
        let lo = find(&in_row, "UI_Perks_Loadout");
        in_row
            .view
            .controller::<PerksLoadout>(lo)
            .map(|l| l.equipped_perks(false).into_iter().map(|p| p.map(|p| p.id)).collect())
            .unwrap_or_else(|| vec![None; PLAYER_PERK_SLOTS])
    }

    /// GetInteractors.
    pub fn interactors(cx: &Cx<'_>) -> Vec<WidgetId> {
        vec![inter(cx, "buttonEdit")]
    }
}

impl Controller for LabPlayerPanelPerks {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        // ComponentDelegateBinding_0: buttonEdit.OnClick @351
        on_click_of::<ButtonSecondary>(cx, "buttonEdit", |b| &mut b.on_click, PK_EDIT);
        // UI_Perks_Loadout.PerkSelectedEvent @447
        let lo = find(cx, "UI_Perks_Loadout");
        let me = cx.w;
        cx.visit::<PerksLoadout, _>(lo, |l, _| l.perk_selected_event.add(Handler { owner: me, id: PK_PICK }));
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        match id {
            PK_EDIT => cx.emit(self, |c| &c.edit_perks_event, &-1),
            PK_PICK => {
                if let Some(&(_, slot, _)) = arg.downcast_ref::<LoadoutPick>() {
                    cx.emit(self, |c| &c.edit_perks_event, &slot);
                }
            }
            _ => {}
        }
    }
}

// ---------- UI_LAB_PlayerPanel ----------

const ADD_ICON: Rgba = [0.194618, 0.651406, 0.630757, 1.0];
const ADD_ICON_OFF: Rgba = [0.341915, 0.341915, 0.341915, 1.0];
const BEHAVIOR_MENUS: [&str; 5] =
    ["menuBehavior", "menuBehaviorDirection", "menuBehaviorEvade", "menuBehaviorDIDirection", "menuBehaviorFight"];
const PLAYER_PERK_SLOTS: usize = 4;
/// (the port's own) The fight menu's Expert id (LAB_EXPERT).
const FIGHT_EXPERT: &str = "expert";

const PP_INVITE_FOCUS: u32 = 1;
const PP_INVITE_UNFOCUS: u32 = 2;
const PP_CARD: u32 = 3;
const PP_PERKS: u32 = 4;
const PP_ADD_BOT: u32 = 5;
const PP_TEAM_UP: u32 = 6;
const PP_TEAM_DOWN: u32 = 7;
const PP_KICK: u32 = 8;
const PP_DAMAGE: u32 = 9;
/// OnClosed of BEHAVIOR_MENUS[i] is PP_MENU_CLOSED + i.
const PP_MENU_CLOSED: u32 = 10;

/// EditPerksEvent(Player Index, Player Panel, Perk Slot, the panel's fighter, its perks).
pub type EditPerks = (i32, WidgetId, i32, Option<String>, Vec<Option<String>>);

/// Deviation: Invite Player, Promote To Leader and the kick dialog of a human slot need other players (no online
/// Lab); the purchase box stays collapsed.
#[derive(Debug, Default)]
pub struct LabPlayerPanel {
    /// PlayerRemovedEvent(panel, its account).
    pub player_removed: Event<(WidgetId, String)>,
    /// EditPerksEvent(Player Index, Player Panel, Perk Slot, the panel's fighter, its perks).
    pub edit_perks_event: Event<EditPerks>,
    pub account_id: String,
    pub is_leader: bool,
    pub is_occupied: bool,
    pub is_bot: bool,
    character: Option<String>,
    skin: String,
    team_index: i32,
    /// The value the config set on the damage control fires its OnValueChanged once (deferred past this panel's own
    /// call): that echo sends nothing back (the build drops a request while it handles the broadcast,
    /// PS.bRespondingToConfigUpdate).
    damage_echo: bool,
    /// The behaviour menu whose OnClosed is in hand and its CurrentSelectedID: that menu is busy while its event
    /// runs here, so its id comes with the event.
    held: Option<(&'static str, String)>,
}

impl LabPlayerPanel {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::<LabPlayerPanel>::default()
    }

    /// The Character variable.
    pub fn fighter(&self) -> Option<&str> {
        self.character.as_deref()
    }

    fn player_index(cx: &Cx<'_>) -> usize {
        prop_f64(cx, "Player Index", 0.0).max(0.0) as usize
    }

    /// The CurrentSelectedID of the panel's drop menu `name`.
    fn menu_id(&self, cx: &mut Cx<'_>, name: &str) -> String {
        if let Some((n, id)) = &self.held
            && *n == name
        {
            return id.clone();
        }
        let m = find(cx, name);
        cx.visit::<DropMenu, _>(m, |m, _| m.current_selected_id.clone()).unwrap_or_default()
    }

    fn set_menu(cx: &mut Cx<'_>, name: &str, id: &str) {
        let m = find(cx, name);
        let id = id.to_string();
        cx.visit::<DropMenu, _>(m, move |m, cx| m.set_active_by_id(cx, &id));
    }

    /// ToggleInviteMenu @3296.
    fn toggle_invite_menu(&mut self, cx: &mut Cx<'_>, visible: bool) {
        let on = visible && self.account_id.is_empty() && local_leads(cx);
        set_opacity(cx, "inviteActionsContainer", if on { 1.0 } else { 0.0 });
        set_opacity(cx, "inviteInfoContainer", if on { 0.0 } else { 1.0 });
    }

    /// SetCharacter @1397 (the row's @1484).
    fn set_character(&mut self, cx: &mut Cx<'_>, character: Option<String>, random: bool) {
        self.character = character.clone();
        let skin = self.skin.clone();
        let card = find(cx, "UI_LAB_PlayerPanel_Character");
        run_on::<LabPlayerPanelCharacter>(cx, card, move |c, cx| c.set_character(cx, character, &skin, random));
        // the row (SetCharacter @39-168; PerksLoadout.SetCharacter @10436 loads the saved loadout, the machine's)
        let row = find(cx, "UI_LAB_PlayerPanel_Perks");
        let c = self.character.clone();
        run_on::<LabPlayerPanelPerks>(cx, row, move |r, cx| r.set_character(cx, c.as_deref(), random));
        self.hold_row_pictures(cx);
        self.update_appearance(cx);
    }

    fn set_skin(&mut self, cx: &mut Cx<'_>, skin: &str) {
        self.skin = skin.to_string();
        let character = self.character.clone();
        let skin = skin.to_string();
        let card = find(cx, "UI_LAB_PlayerPanel_Character");
        run_on::<LabPlayerPanelCharacter>(cx, card, move |c, cx| c.set_character(cx, character, &skin, false));
    }

    /// SetOccupied @2170.
    pub fn set_occupied(&mut self, cx: &mut Cx<'_>, occupied: bool) {
        self.is_occupied = occupied;
        self.toggle_invite_menu(cx, false);
        self.update_appearance(cx);
    }

    /// btn_InviteBot OnClick @8532 -> @6692: the slot becomes "Bot<index>" on the team of its index, with the
    /// panel's fighter (none, i.e. Random, once the panel was filled from an empty slot) and behaviour.
    fn populate_with_bot(&mut self, cx: &mut Cx<'_>) {
        self.toggle_invite_menu(cx, false);
        if !local_leads(cx) {
            return;
        }
        self.is_bot = true;
        let idx = Self::player_index(cx);
        self.account_id = format!("Bot{idx}");
        self.set_occupied(cx, true);
        self.team_index = idx as i32;
        self.update_server_config(cx);
    }

    /// SwitchTeam @6562 (WrapInt over 0..3).
    fn switch_team(&mut self, cx: &mut Cx<'_>, direction: i32) {
        self.team_index = (self.team_index + direction + 4) % 4;
        self.update_appearance(cx);
        self.update_server_config(cx);
    }

    /// GetBotDifficulty @0-3232: the EBotBehavior of the menus' ids (NotEqual_StriStri, any case; EBotBehavior n is
    /// LAB_BOTS[n - 1]). An id no case names takes the switch's fall-through: recovery for the behaviour, the first
    /// choice for a sub menu; a human is 0. (0 for an id not in LAB_BOTS would turn a bot human.)
    fn get_bot_difficulty(&self, cx: &mut Cx<'_>) -> u8 {
        if !self.is_bot {
            return 0;
        }
        let id = self.menu_id(cx, "menuBehavior").to_lowercase();
        let sub = |name: &str, cx: &mut Cx<'_>| self.menu_id(cx, name).to_lowercase();
        // menuBehaviorDirection @1251 / @1639: neutral, down, up, forward
        let direction = |s: String| match s.as_str() {
            "down" => 1,
            "up" => 2,
            "forward" => 3,
            _ => 0,
        };
        // menuBehaviorDIDirection @2052 / @2437: forward, back, up, down
        let di = |s: String| match s.as_str() {
            "back" => 1,
            "up" => 2,
            "down" => 3,
            _ => 0,
        };
        match id.as_str() {
            "jumping" => 7,
            // @838
            "evade" => match sub("menuBehaviorEvade", cx).as_str() {
                "evadeneutral" => 8,
                "evadeback" => 9,
                "evadeforward" => 10,
                _ => 11,
            },
            "normalattack" => 12 + direction(sub("menuBehaviorDirection", cx)),
            "specialattack" => 16 + direction(sub("menuBehaviorDirection", cx)),
            "patrolling" => 28,
            "stun_di" => 20 + di(sub("menuBehaviorDIDirection", cx)),
            "evade_di" => 24 + di(sub("menuBehaviorDIDirection", cx)),
            "idle" => 6,
            // @2847
            "fight" => match sub("menuBehaviorFight", cx).as_str() {
                "easy" => 3,
                "medium" => 4,
                "hard" => 5,
                FIGHT_EXPERT => LAB_EXPERT,
                _ => 2,
            },
            _ => 1,
        }
    }

    /// BuildPlayerTrainingConfig. PlayerName is GetUsername (@42 -> @414): the text of nameLabel as UpdateAppearance
    /// left it, so a bot's is its "{character} (Bot)" / "(Bot)" (GenerateDisplayName), which
    /// TrainGameMode.PopulateMatchPlayerData @709 gives the HUD as the username. Perks: the row's GetCurrentPerks
    /// (@102 / @610).
    pub fn build_player_training_config(&self, cx: &mut Cx<'_>) -> LabPlayerConfig {
        let n = find(cx, "nameLabel");
        let name = wd(cx, n).text.clone();
        let dmg = find(cx, "settingStartingDamage");
        let damage = cx.visit::<PlusMinusVertical, _>(dmg, |p, _| p.value).unwrap_or(0.0);
        let perks = LabPlayerPanelPerks::current_perks(cx);
        LabPlayerConfig {
            account_id: self.account_id.clone(),
            player_name: name,
            character: self.character.clone(),
            skin: if self.character.is_some() { self.skin.clone() } else { String::new() },
            starting_damage: damage,
            bot_difficulty: self.get_bot_difficulty(cx),
            team_index: self.team_index,
            perks,
        }
    }

    fn update_server_config(&mut self, cx: &mut Cx<'_>) {
        if cx.lab.is_none() {
            return;
        }
        let mine = self.build_player_training_config(cx);
        let idx = Self::player_index(cx);
        let Some(s) = cx.lab.as_mut() else { return };
        let mut config: LabConfig = s.config().clone();
        if config.players.len() <= idx {
            config.players.resize_with(idx + 1, empty_lab_player);
        }
        config.players[idx] = mine;
        s.request_config_update(config);
    }

    /// The row's SetPerks @397.
    pub fn set_perks(&mut self, cx: &mut Cx<'_>, perks: &[Option<String>]) {
        let row = find(cx, "UI_LAB_PlayerPanel_Perks");
        let perks = perks.to_vec();
        run_on::<LabPlayerPanelPerks>(cx, row, move |r, cx| r.set_perks(cx, &perks));
        self.hold_row_pictures(cx);
    }

    /// (the port's own) The icons the row's tiles draw, held for as long as the Lab menu is up (its view's
    /// `release_held`), one owner per panel: the page's own hold is let go when it closes.
    fn hold_row_pictures(&mut self, cx: &mut Cx<'_>) {
        let ids = LabPlayerPanelPerks::current_perks(cx);
        let names = perk_list_pictures(cx.view, &ids);
        let owner = format!("{PICTURES_LAB_PERKS}:row{}", Self::player_index(cx));
        cx.view.hold_pictures(&owner, names.iter().map(String::as_str));
    }

    /// PopulatePanelFromConfig.
    pub fn populate_panel_from_config(&mut self, cx: &mut Cx<'_>, c: &LabPlayerConfig, leader: &str) {
        self.account_id = c.account_id.clone();
        self.is_leader = c.account_id == leader;
        self.is_occupied = !c.account_id.is_empty();
        self.toggle_invite_menu(cx, c.account_id.is_empty());
        set_text(cx, "nameLabel", &c.player_name); // SetUsername @883
        self.set_character(cx, c.character.clone(), c.character.is_none());
        self.set_skin(cx, &c.skin);
        // @1019 the config's perks over the saved ones SetCharacter loaded; @1073-1161 the row SelfHitTestInvisible
        // with a character, Hidden (its space kept) for Random or no character
        self.set_perks(cx, &c.perks);
        set_vis(
            cx,
            "UI_LAB_PlayerPanel_Perks",
            if c.character.is_some() { Vis::SelfHitTestInvisible } else { Vis::Hidden },
        );
        let dmg = find(cx, "settingStartingDamage");
        self.damage_echo = true;
        let d = c.starting_damage;
        run_on::<PlusMinusVertical>(cx, dmg, move |p, cx| p.set_value(cx, d));
        self.is_bot = c.bot_difficulty != 0;
        // @1363-8469: the menus follow the EBotBehavior (0 reads as recovery). The DI menu's select reads index 0, an
        // empty id, so it is never set from the config. (LAB_EXPERT, the port's own, is the fight menu's Expert.)
        let entry = if c.bot_difficulty == LAB_EXPERT {
            "fight/expert"
        } else {
            (c.bot_difficulty as usize).checked_sub(1).and_then(|i| LAB_BOTS.get(i)).copied().unwrap_or("recovery")
        };
        let (id, sub) = match entry.split_once('/') {
            Some((a, b)) => (a, Some(b)),
            None => (entry, None),
        };
        Self::set_menu(cx, "menuBehavior", id);
        if let Some(sub) = sub {
            match id {
                "normalAttack" | "specialAttack" => Self::set_menu(cx, "menuBehaviorDirection", sub),
                "evade" => Self::set_menu(cx, "menuBehaviorEvade", sub),
                "fight" => Self::set_menu(cx, "menuBehaviorFight", sub),
                _ => {}
            }
        }
        self.team_index = c.team_index;
        self.update_appearance(cx);
    }

    /// UpdateAppearance @3785-6521.
    fn update_appearance(&mut self, cx: &mut Cx<'_>) {
        let local = cx.lab.as_ref().is_some_and(|s| self.account_id == s.me());
        let leads = local_leads(cx);
        // GenerateDisplayName: a bot is named after its fighter
        if self.is_bot {
            let t = match &self.character {
                Some(c) => format!("{} (Bot)", display_name(cx, Some(c))),
                None => "(Bot)".to_string(),
            };
            set_text(cx, "nameLabel", &t);
        }
        self.enable_interactors(cx, local, self.is_bot, leads);
        self.toggle_invite_menu(cx, false);
        set_vis(cx, "emptyContainer", if self.is_occupied { Vis::Hidden } else { Vis::SelfHitTestInvisible });
        set_vis(cx, "contentContainer", if self.is_occupied { Vis::SelfHitTestInvisible } else { Vis::Hidden });
        set_vis(cx, "leaderIndicator", if self.is_leader { Vis::SelfHitTestInvisible } else { Vis::Collapsed });
        set_vis(
            cx,
            "UI_ButtonSecondary",
            if leads && !self.is_leader && !local { Vis::SelfHitTestInvisible } else { Vis::Collapsed },
        );
        for n in ["menuBehavior", "BehaviorText"] {
            set_vis(cx, n, if self.is_bot { Vis::SelfHitTestInvisible } else { Vis::Collapsed });
        }
        set_vis(
            cx,
            "Btn_MakeLeader",
            if self.is_leader || self.is_bot || !leads { Vis::Collapsed } else { Vis::SelfHitTestInvisible },
        );
        set_vis(cx, "MeHeader", Vis::Hidden);
        let kick = find(cx, "UI_ButtonSecondary");
        let icon = if self.is_bot { "ui_icons_x" } else { "ui_icons_kick" }.to_string();
        cx.visit::<ButtonSecondary, _>(kick, |b, cx| b.set_icon(cx, Some(icon)));
        let team = team_color(cx, self.team_index);
        set_color(cx, "teamHeaderBG", team);
        let t = self.team_index;
        let card = find(cx, "UI_LAB_PlayerPanel_Character");
        run_on::<LabPlayerPanelCharacter>(cx, card, move |c, cx| c.set_team_color(cx, t));
        let me_color = palette_entry(cx, 2);
        set_color(cx, "leaderIndicator", if self.is_leader { me_color } else { team });
        set_color(cx, "MeHeader", me_color);
        set_text(cx, "teamLabel", &format!("Team: {}", self.team_index + 1));
    }

    /// EnableInteractors(bIsLocalPlayer, bIsBot, bAmILeader).
    fn enable_interactors(&mut self, cx: &mut Cx<'_>, local: bool, bot: bool, leads: bool) {
        for n in ["UI_ButtonSecondary", "btn_InvitePlayer", "btn_InviteBot", "Btn_MakeLeader"] {
            let i = inter(cx, n);
            set_interactable(cx, i, leads);
        }
        set_color(cx, "addIcon", if leads { ADD_ICON } else { ADD_ICON_OFF });
        set_vis(cx, "addPlayerLabel", if leads { Vis::SelfHitTestInvisible } else { Vis::Collapsed });
        set_opacity(cx, "emptyContainer", if leads { 1.0 } else { 0.5 });
        let edit = local || (bot && leads);
        for n in ["buttonTeamDecrement", "buttonTeamIncrement"] {
            let i = inter(cx, n);
            set_interactable(cx, i, edit);
            set_vis(cx, n, if edit { Vis::Visible } else { Vis::Hidden });
        }
        let card = find(cx, "UI_LAB_PlayerPanel_Character");
        let edit_icon = cx.view.tree().find_in(card, "EditIcon");
        wd(cx, edit_icon).vis = if edit { Vis::Visible } else { Vis::Hidden };
        let b = cx.view.tree().find_in(card, "buttonChangeCharacter");
        set_interactable(cx, b, edit);
        // @1065-1156 the row's buttonEdit shown or hidden with the team buttons; @1706-2231 the row's GetInteractors
        let perks = find(cx, "UI_LAB_PlayerPanel_Perks");
        let be = cx.view.tree().find_in(perks, "buttonEdit");
        wd(cx, be).vis = if edit { Vis::Visible } else { Vis::Hidden };
        let pi =
            cx.visit::<LabPlayerPanelPerks, _>(perks, |_, cx| LabPlayerPanelPerks::interactors(cx)).unwrap_or_default();
        let dmg = find(cx, "settingStartingDamage");
        let di = cx.visit::<PlusMinusVertical, _>(dmg, |_, cx| PlusMinusVertical::interactors(cx)).unwrap_or_default();
        let mut all: Vec<WidgetId> = pi.into_iter().chain(di).collect();
        for n in BEHAVIOR_MENUS {
            let m = find(cx, n);
            all.extend(cx.visit::<DropMenu, _>(m, |_, cx| DropMenu::interactors(cx)).unwrap_or_default());
        }
        for i in all {
            set_interactable(cx, i, edit);
        }
    }

    /// UpdateContext @1554.
    pub fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str) {
        let names = [
            "buttonTeamDecrement",
            "buttonTeamIncrement",
            "UI_ButtonSecondary",
            "btn_InvitePlayer",
            "btn_InviteBot",
            "Btn_MakeLeader",
        ];
        let l: Vec<WidgetId> = names.iter().map(|n| inter(cx, n)).collect();
        batch_set_context(cx, &l, ctx);
        let card = find(cx, "UI_LAB_PlayerPanel_Character");
        let c = ctx.to_string();
        run_on::<LabPlayerPanelCharacter>(cx, card, move |p, cx| p.update_context(cx, &c));
        let dmg = find(cx, "settingStartingDamage");
        let di = cx.visit::<PlusMinusVertical, _>(dmg, |_, cx| PlusMinusVertical::interactors(cx)).unwrap_or_default();
        batch_set_context(cx, &di, ctx);
        for n in BEHAVIOR_MENUS {
            let m = find(cx, n);
            let c = ctx.to_string();
            cx.visit::<DropMenu, _>(m, move |_, cx| DropMenu::update_context(cx, &c, None));
        }
        let perks = find(cx, "UI_LAB_PlayerPanel_Perks");
        let c = ctx.to_string();
        cx.visit::<LabPlayerPanelPerks, _>(perks, move |p, cx| p.update_context(cx, &c)); // @1704
    }
}

impl Controller for LabPlayerPanel {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        // @2247-3280
        let (invite_i, bot_i) = (inter(cx, "btn_InvitePlayer"), inter(cx, "btn_InviteBot"));
        let fc = find(cx, "UI_FocusedCommon");
        cx.visit::<FocusedCommon, _>(fc, |f, cx| {
            f.set_interactor(cx, invite_i);
            f.set_interactor(cx, bot_i);
        });
        for i in [invite_i, bot_i] {
            cx.view.tree_mut().set_prop(i, "NoFriction", json!(true));
        }
        let wob = cx.view.tree().find_in(fc, "Wobbler");
        cx.visit::<Quadrilateral, _>(wob, |q, cx| q.set_outline(cx, 0.02));
        self.set_character(cx, Some("shaggy".to_string()), false);
        self.set_skin(cx, "");
        self.is_leader = Self::player_index(cx) == 0 && !self.is_bot;
        Self::set_menu(cx, "menuBehavior", "recovery");
        // (the port's own) the fight menu's Expert, after Hard
        let fight = find(cx, "menuBehaviorFight");
        DropMenu::offer_own_item(cx, fight, custom_text::EXPERT, FIGHT_EXPERT);
        let me = cx.w;
        for n in ["btn_InvitePlayer", "btn_InviteBot"] {
            let b = find(cx, n);
            cx.visit::<ButtonSecondaryVertical, _>(b, |b, _| {
                b.on_focus.add(Handler { owner: me, id: PP_INVITE_FOCUS });
                b.on_unfocus.add(Handler { owner: me, id: PP_INVITE_UNFOCUS });
            });
        }
        self.toggle_invite_menu(cx, false);
        // the card's CharacterSelectedEvent @7213
        let card = find(cx, "UI_LAB_PlayerPanel_Character");
        cx.visit::<LabPlayerPanelCharacter, _>(card, |c, _| {
            c.character_selected.add(Handler { owner: me, id: PP_CARD })
        });
        // the row's EditPerksEvent @7422
        let perks = find(cx, "UI_LAB_PlayerPanel_Perks");
        cx.visit::<LabPlayerPanelPerks, _>(perks, |p, _| p.edit_perks_event.add(Handler { owner: me, id: PP_PERKS }));
        // btn_InviteBot @8660
        on_click_of::<ButtonSecondaryVertical>(cx, "btn_InviteBot", |b| &mut b.on_click, PP_ADD_BOT);
        // buttonTeamIncrement @6522, buttonTeamDecrement @6542
        on_click_of::<IconButton>(cx, "buttonTeamIncrement", |b| &mut b.on_click, PP_TEAM_UP);
        on_click_of::<IconButton>(cx, "buttonTeamDecrement", |b| &mut b.on_click, PP_TEAM_DOWN);
        // UI_ButtonSecondary (kick) @11774: a bot goes without the dialog
        on_click_of::<ButtonSecondary>(cx, "UI_ButtonSecondary", |b| &mut b.on_click, PP_KICK);
        // settingStartingDamage OnValueChanged @9617
        let dmg = find(cx, "settingStartingDamage");
        cx.visit::<PlusMinusVertical, _>(dmg, |p, _| p.on_value_changed.add(Handler { owner: me, id: PP_DAMAGE }));
        // menuBehavior OnClosed @9632: the sub menu of the new entry starts at its first choice; the sub menus'
        // OnClosed (@11759, @11764, @11811) send the config.
        // Deviation: menuBehaviorDIDirection's OnClosed sends it too; the build binds none to it, so a DI
        // pick went out with the slot's next change.
        for (i, n) in BEHAVIOR_MENUS.iter().enumerate() {
            let m = find(cx, n);
            let id = PP_MENU_CLOSED + i as u32;
            cx.visit::<DropMenu, _>(m, |m, _| m.on_closed.add(Handler { owner: me, id }));
        }
    }

    /// The Visibility bindings (GetVisibilityBehaviorDirection and the like), read every frame. They are bound on the
    /// SizeBoxes around the sub menus (SizeBox, SizeBox_4, SizeBox_2, SizeBox_5), which are the Fill slots of
    /// settingsContainer: a collapsed box takes no share, so the one that shows sits right under menuBehavior.
    fn tick(&mut self, cx: &mut Cx<'_>, _dt: f64) {
        let id = self.menu_id(cx, "menuBehavior");
        let vis = |on: bool| if on { Vis::Visible } else { Vis::Collapsed };
        let rows = [
            ("menuBehaviorDirection", id == "normalAttack" || id == "specialAttack"),
            ("menuBehaviorDIDirection", id == "stun_di" || id == "evade_di"),
            ("menuBehaviorEvade", id == "evade"),
            ("menuBehaviorFight", id == "fight"),
        ];
        for (n, on) in rows {
            let m = find(cx, n);
            if let Some(p) = cx.view.tree().widget(m).parent {
                wd(cx, p).vis = vis(on);
            }
        }
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        match id {
            PP_INVITE_FOCUS => self.toggle_invite_menu(cx, true),
            PP_INVITE_UNFOCUS => self.toggle_invite_menu(cx, false),
            PP_CARD => {
                if let Some((character, skin, random)) = arg.downcast_ref::<(Option<String>, String, bool)>() {
                    self.set_character(cx, character.clone(), *random);
                    self.set_skin(cx, skin);
                    self.update_server_config(cx);
                }
            }
            PP_PERKS => {
                if let Some(&slot) = arg.downcast_ref::<i32>() {
                    let me = cx.w;
                    let perks = LabPlayerPanelPerks::current_perks(cx);
                    let ev = (Self::player_index(cx) as i32, me, slot, self.character.clone(), perks);
                    cx.emit(self, |c| &c.edit_perks_event, &ev);
                    self.update_server_config(cx);
                }
            }
            PP_ADD_BOT => self.populate_with_bot(cx),
            PP_TEAM_UP => self.switch_team(cx, 1),
            PP_TEAM_DOWN => self.switch_team(cx, -1),
            PP_KICK => {
                if self.is_bot {
                    let me = cx.w;
                    let ev = (me, self.account_id.clone());
                    cx.emit(self, |c| &c.player_removed, &ev);
                }
            }
            PP_DAMAGE => {
                if std::mem::take(&mut self.damage_echo) {
                    return;
                }
                self.update_server_config(cx);
            }
            _ if (PP_MENU_CLOSED..PP_MENU_CLOSED + BEHAVIOR_MENUS.len() as u32).contains(&id) => {
                let i = (id - PP_MENU_CLOSED) as usize;
                let selected = arg.downcast_ref::<String>().cloned().unwrap_or_default();
                self.held = Some((BEHAVIOR_MENUS[i], selected));
                if i == 0 {
                    let id = self.menu_id(cx, "menuBehavior").to_lowercase();
                    match id.as_str() {
                        "normalattack" | "specialattack" => Self::set_menu(cx, "menuBehaviorDirection", "neutral"),
                        "evade" => Self::set_menu(cx, "menuBehaviorEvade", "evadePostHit"),
                        "fight" => Self::set_menu(cx, "menuBehaviorFight", "veryeasy"),
                        "evade_di" => Self::set_menu(cx, "menuBehaviorDIDirection", "forward"),
                        _ => {}
                    }
                }
                self.update_server_config(cx);
                self.held = None;
            }
            _ => {}
        }
    }
}

// ---------- UI_LAB_MapInfo (the map tile) ----------

const MAP_QUAD: Rgba = [0.194618, 0.651406, 0.630757, 1.0];
const MAP_QUAD_FOCUS: Rgba = [0.89627, 0.973446, 0.871367, 1.0];
const MAP_BORDER_OFF: Rgba = [0.262251, 0.366253, 0.396755, 1.0];

const MI_CLICK: u32 = 1;
const MI_FOCUS: u32 = 2;
const MI_UNFOCUS: u32 = 3;
const MI_INTERACTABLE: u32 = 4;
const MI_NOT_INTERACTABLE: u32 = 5;
const MI_SELECTED: u32 = 6;

#[derive(Debug, Default)]
pub struct LabMapInfo {
    /// SelectedEvent(Map).
    pub selected_event: Event<MapInfo>,
    selected_map: Option<MapInfo>,
    player_count: i32,
}

impl LabMapInfo {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::<LabMapInfo>::default()
    }

    pub fn interactor(cx: &Cx<'_>) -> WidgetId {
        find(cx, "UI_Interactor")
    }

    pub fn set_player_count(&mut self, n: i32) {
        self.player_count = n;
    }

    /// UI_Interactor OnClick @15: the map selector as the modal "mapSelector", with the rotation of the player count
    /// (EMapRotation 8 LabSmall up to 2 players, 9 LabLarge from 3) and the maps of UI_Functions "Get Offline Soft
    /// Maps"(false): the port is not connected (connected: GetSoftMaps([8, 9])). One map is picked (no multi select).
    fn open_selector(&mut self, cx: &mut Cx<'_>) {
        let Some(m) = Manager::show_modal(cx, "UI_LAB_MapSelector", "mapSelector", "", "") else { return };
        let me = cx.w;
        let maps: Vec<String> = cx.view.offline_maps().to_vec();
        let selected: Vec<String> = self.selected_map.iter().map(|m| m.name.clone()).collect();
        let count = self.player_count;
        cx.visit::<LabMapSelector, _>(m, |s, cx| {
            s.selected_rotation = if count >= 3 { 9 } else { 8 };
            s.init(cx, &maps, &selected, count, false, false);
            // OnMapSelected @1294
            s.selected_event.add(Handler { owner: me, id: MI_SELECTED });
        });
    }

    /// SetMap (also OnInitialized @1090 with the widget's Selected Map, which is none).
    pub fn set_map(&mut self, cx: &mut Cx<'_>, m: &MapInfo) {
        self.selected_map = Some(m.clone());
        cx.view.load_texture(&m.data.texture); // SetBrushFromSoftTexture
        let icon = find(cx, "mapIcon");
        wd(cx, icon).tex = Some(m.data.texture.clone());
        set_text(cx, "mapLabel", &m.data.display_name);
        set_vis(cx, "UI_PM_Experimental", if m.data.experimental { Vis::Visible } else { Vis::Collapsed });
    }

    /// UpdateContext @1011.
    pub fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str) {
        let i = Self::interactor(cx);
        batch_set_context(cx, &[i], ctx);
    }
}

impl Controller for LabMapInfo {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        // Construct @1346
        let i = Self::interactor(cx);
        let fc = find(cx, "UI_FocusedCommon");
        cx.visit::<FocusedCommon, _>(fc, |f, cx| f.set_interactor(cx, i));
        bind(cx, i, |s| &mut s.on_click, MI_CLICK);
        bind(cx, i, |s| &mut s.on_focus, MI_FOCUS); // @1392
        bind(cx, i, |s| &mut s.on_unfocus, MI_UNFOCUS); // @1501
        bind(cx, i, |s| &mut s.on_interactable, MI_INTERACTABLE); // @1752
        bind(cx, i, |s| &mut s.on_not_interactable, MI_NOT_INTERACTABLE); // @1643
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        match id {
            MI_CLICK => self.open_selector(cx),
            MI_FOCUS => {
                set_vis(cx, "QuadBorder", Vis::Hidden);
                let q = find(cx, "WBP_Quadrilateral_126");
                quad_color(cx, q, MAP_QUAD_FOCUS);
            }
            MI_UNFOCUS => {
                set_vis(cx, "QuadBorder", Vis::SelfHitTestInvisible);
                let q = find(cx, "WBP_Quadrilateral_126");
                quad_color(cx, q, MAP_QUAD);
            }
            MI_INTERACTABLE => {
                let q = find(cx, "QuadBorder");
                quad_color(cx, q, MAP_QUAD);
                set_vis(cx, "EditIcon", Vis::SelfHitTestInvisible);
            }
            MI_NOT_INTERACTABLE => {
                let q = find(cx, "QuadBorder");
                quad_color(cx, q, MAP_BORDER_OFF);
                set_vis(cx, "EditIcon", Vis::Collapsed);
            }
            MI_SELECTED => {
                if let Some(m) = arg.downcast_ref::<MapInfo>() {
                    self.set_map(cx, m);
                    cx.emit(self, |c| &c.selected_event, m);
                }
            }
            _ => {}
        }
    }
}

// ---------- UI_LAB_Panel ----------

/// UWidget::IsVisible 1425d63b0.
fn is_visible_vis(v: Vis) -> bool {
    matches!(v, Vis::Visible | Vis::HitTestInvisible | Vis::SelfHitTestInvisible)
}

const PLAYER_PANELS: [&str; 4] =
    ["UI_LAB_PlayerPanel0", "UI_LAB_PlayerPanel1", "UI_LAB_PlayerPanel2", "UI_LAB_PlayerPanel3"];

const LP_CANCEL: u32 = 1;
const LP_RESUME: u32 = 2;
const LP_MOVES: u32 = 3;
const LP_REMOVED: u32 = 4;
const LP_EDIT_PERKS: u32 = 5;
const LP_DISMISSER: u32 = 6;
const LP_TOOLS: u32 = 7;
const LP_MAP: u32 = 8;
const LP_HITSTUN: u32 = 9;
const LP_JUGGLE: u32 = 10;
const LP_HAZARDS: u32 = 11;
const LP_COOLDOWNS: u32 = 12;
const LP_SAME_ATTACK: u32 = 13;
const LP_QUIT_CONFIRM: u32 = 14;
const LP_HIDE_PERKS: u32 = 15;
const LP_PERKS_SAVED: u32 = 16;
const LP_PERKS_EXITED: u32 = 17;
const LP_EXPERIMENTAL: u32 = 18;
const LP_MUTATORS: u32 = 19;

/// (the port's own) The Lab's Experimental Settings page: the mutators (`lab_experimental_page`).
pub const LAB_EXPERIMENTAL_MODAL: &str = "labExperimental";

/// Deviation: Move List and the online session button are not wired; Show Hitstun Combo, Show Juggle Combo, Instant
/// Cooldowns and Hazards Enabled keep their state but the port has no such feature yet.
#[derive(Debug, Default)]
pub struct LabPanel {
    current_perks_player_index: i32,
    quitting: bool,
    /// The page has been opened once (the build's lazy equipper is built then: the dismisser answers from then on).
    equipper_built: bool,
}

impl LabPanel {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::<LabPanel>::default()
    }

    fn toggle_on(cx: &mut Cx<'_>, name: &str) -> bool {
        let t = find(cx, name);
        cx.visit::<ToggleSwitch, _>(t, |t, _| t.is_on).unwrap_or(false)
    }

    fn set_toggle(cx: &mut Cx<'_>, name: &str, on: bool) {
        let t = find(cx, name);
        cx.visit::<ToggleSwitch, _>(t, move |t, cx| t.set_on(cx, on));
    }

    fn panels(cx: &Cx<'_>) -> Vec<WidgetId> {
        PLAYER_PANELS.iter().map(|n| find(cx, n)).collect()
    }

    /// GetPlayerCount: the occupied player panels.
    fn player_count(cx: &mut Cx<'_>) -> i32 {
        Self::panels(cx)
            .into_iter()
            .filter(|&p| cx.visit::<LabPlayerPanel, _>(p, |p, _| p.is_occupied).unwrap_or(false))
            .count() as i32
    }

    fn map_info(cx: &Cx<'_>) -> WidgetId {
        find(cx, "UI_LAB_MapInfo")
    }

    /// Handle Visibility Changed @14331: PS.RequestAuthorityConfig, answered by the config broadcast.
    pub fn shown(&mut self, cx: &mut Cx<'_>) {
        if let Some(s) = cx.lab.as_mut() {
            s.request_authority_config();
        }
        // (the port's own) the Lab Tools switch as its store holds it
        let on = cx.prefs.lab.tools_on;
        Self::set_toggle(cx, "toggleLabTools", on);
        self.poll_config(cx);
    }

    /// TrainingGameState.OnTrainingConfigUpdateFromServer -> HandleServerUpdateConfig.
    fn poll_config(&mut self, cx: &mut Cx<'_>) {
        let Some(c) = cx.lab.as_mut().and_then(|s| s.take_config()) else { return };
        self.handle_server_update_config(cx, &c);
    }

    fn handle_server_update_config(&mut self, cx: &mut Cx<'_>, c: &LabConfig) {
        for (i, p) in Self::panels(cx).into_iter().enumerate() {
            let cfg = c.players.get(i).cloned().unwrap_or_else(empty_lab_player);
            let leader = c.leader_account_id.clone();
            cx.visit::<LabPlayerPanel, _>(p, move |p, cx| p.populate_panel_from_config(cx, &cfg, &leader));
        }
        let map_info = Self::map_info(cx);
        // @488-689: the map and the three match toggles are the leader's
        let leader = cx.lab.as_ref().is_some_and(|s| c.leader_account_id == s.me());
        let mi = LabMapInfo::interactor_of(cx, map_info);
        set_interactable(cx, mi, leader);
        for n in ["toggleNoCooldowns", "toggleSameAttack", "toggleHazardsEnabled"] {
            let i = inter(cx, n);
            set_interactable(cx, i, leader);
        }
        let count = Self::player_count(cx);
        // @12381 SetMap(Server Config.SelectedMap)
        let map = cx.view.map(&c.selected_map);
        cx.visit::<LabMapInfo, _>(map_info, |m, cx| {
            m.set_player_count(count); // @12289
            if let Some(map) = &map {
                m.set_map(cx, map);
            }
        });
        Self::set_toggle(cx, "toggleNoCooldowns", c.instant_cooldowns);
        Self::set_toggle(cx, "toggleSameAttack", c.same_attack_penalty);
        // (the port's own) the Experimental Settings page, while it is up
        if let Some(m) = cx.view.manager.get_modal(LAB_EXPERIMENTAL_MODAL) {
            let view = Self::experimental_view(c, leader);
            cx.visit::<CustomGameSettings, _>(m, |s, cx| s.refresh(cx, Some(view)));
        }
        // @12543-12640: the active match config's AllowHazards
        if let Some(h) = cx.lab.as_mut().map(|s| s.toggles().hazards) {
            Self::set_toggle(cx, "toggleHazardsEnabled", h);
        }
    }

    /// (the port's own) What the Experimental Settings page reads of the Lab: its mutators, the leader's to change.
    fn experimental_view(c: &LabConfig, leader: bool) -> LobbyView {
        let options = LobbyOptions { mutators: c.mutators, ..LobbyOptions::default() };
        LobbyView { leader, team_style: 0, options }
    }

    /// (the port's own) Experimental Settings: the mutators' page over the menu; a change goes to the config at once
    /// and restarts the match at the unpause.
    fn show_experimental(&mut self, cx: &mut Cx<'_>) {
        let Some(s) = cx.lab.as_ref() else { return };
        let view = Self::experimental_view(s.config(), s.config().leader_account_id == s.me());
        let Some(m) = Manager::show_modal(cx, "CustomGameSettings", LAB_EXPERIMENTAL_MODAL, "", "") else { return };
        let me = cx.w;
        cx.visit::<CustomGameSettings, _>(m, |s, cx| {
            s.on_option.add(Handler { owner: me, id: LP_MUTATORS });
            s.bind(cx, view, lab_experimental_page());
        });
    }

    /// SetFocused @6867.
    pub fn set_focused(&mut self, cx: &mut Cx<'_>) {
        let i = inter(cx, "buttonResume");
        set_focused(cx, i, false);
    }

    /// UpdateContext @6757 (GetInteractors).
    pub fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str) {
        let toggles = [
            "toggleHitstunCombo",
            "toggleNoCooldowns",
            "toggleSameAttack",
            "toggleHazardsEnabled",
            "toggleJuggleCombo",
            "toggleHitboxes",
            "toggleHurtboxes",
            "toggleLabTools",
        ];
        let mut l = vec![inter(cx, "buttonResume")];
        l.extend(toggles.iter().map(|n| inter(cx, n)));
        l.push(inter(cx, "Btn_MovesList"));
        l.push(inter(cx, "Btn_Experimental"));
        batch_set_context(cx, &l, ctx);
        for p in Self::panels(cx) {
            let c = ctx.to_string();
            run_on::<LabPlayerPanel>(cx, p, move |p, cx| p.update_context(cx, &c));
        }
        let mi = Self::map_info(cx);
        let c = ctx.to_string();
        cx.visit::<LabMapInfo, _>(mi, move |m, cx| m.update_context(cx, &c)); // @3417
        // buttonPerksDismisser is among GetInteractors (@0); the page takes the menu's context (@3462)
        let d = find(cx, "buttonPerksDismisser");
        batch_set_context(cx, &[d], ctx);
        let eq = find(cx, "UI_Perks_Equipper");
        let c = ctx.to_string();
        run_on::<PerksEquipper>(cx, eq, move |e, cx| e.update_context(cx, &c));
    }

    /// OnEditPerks (-> @10758): the page for the panel's fighter, its slot entered, the panel's perks in it.
    fn on_edit_perks(&mut self, cx: &mut Cx<'_>, ev: &EditPerks) {
        let (player, _panel, slot, fighter, perks) = ev;
        self.current_perks_player_index = *player;
        self.equipper_built = true;
        let eq = find(cx, "UI_Perks_Equipper");
        let (f, slot) = (fighter.clone(), *slot);
        let data: Vec<Option<Perk>> = perks.iter().map(|id| perk_data(cx.view, id.as_deref())).collect();
        run_on::<PerksEquipper>(cx, eq, move |e, cx| {
            // (the machine's loadout, as the panels' rows: a save here is the one the next Lab and queue take)
            e.remember = true;
            e.set_character(cx, f.as_deref()); // @10796 (no ally perks)
            e.enter(cx, slot); // @10872
            e.set_equipped_perks(cx, &data); // @10917 / @10984
        });
        // (the port's own) the page's icons, while it is up
        let names = perk_pictures(cx.view, fighter.as_deref());
        cx.view.hold_pictures(PICTURES_LAB_PERKS, names.iter().map(String::as_str));
        set_vis(cx, "PerksContainer", Vis::SelfHitTestInvisible); // @11029
    }

    /// PerksSavedEvent (-> @15643): the panel's row takes the list @15675; then every panel's config is sent
    /// (BuildPlayerConfigList @14062 -> PS.TrainingModeConfig.Players @14085 -> RequestConfigUpdate @14143), and after
    /// Delay(0.1) @14266 PerksContainer is Hidden @1463.
    fn on_perks_saved(&mut self, cx: &mut Cx<'_>, saved: &PerksSaved) {
        let panels = Self::panels(cx);
        if let Some(&p) = panels.get(usize::try_from(self.current_perks_player_index).unwrap_or(usize::MAX)) {
            let perks = saved.2.clone();
            run_on::<LabPlayerPanel>(cx, p, move |p, cx| p.set_perks(cx, &perks));
        }
        if cx.lab.is_some() {
            let mut players = Vec::new();
            for p in panels {
                if let Some(c) = cx.visit::<LabPlayerPanel, _>(p, |p, cx| p.build_player_training_config(cx)) {
                    players.push(c);
                }
            }
            if let Some(s) = cx.lab.as_mut() {
                let mut c = s.config().clone();
                c.players = players;
                s.request_config_update(c);
            }
        }
        self.delay_hide_perks(cx);
    }

    fn delay_hide_perks(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        cx.view.delay(0.1, Handler { owner: me, id: LP_HIDE_PERKS });
    }

    /// CanExit?: not with a modal over the menu, the perks overlay visible (@171-234, UWidget::IsVisible: Visible,
    /// HitTestInvisible or SelfHitTestInvisible; the page's close leaves it Hidden) or the social panel open
    /// (@239-425: FindOrCreateSocialPanel(false), the newest UI_SocialPanel that is visible in its hierarchy; the
    /// pause menu's is Collapsed without a connection). `quitting` is the port's stand-in for the panel's
    /// SetIsEnabled(false) @13143: nothing of it answers once the level is being left.
    fn can_exit(&self, cx: &mut Cx<'_>) -> bool {
        if self.quitting || Manager::is_modal_active(cx.view) {
            return false;
        }
        let pc = find(cx, "PerksContainer");
        !is_visible_vis(wd(cx, pc).vis)
    }

    /// Quit @12284 (the sidebar's ExitEvent): CanExit?, then the confirm dialog, whose left button is OnQuitConfirm
    /// @12690: the panel is disabled and the level left for the main menu (@13143, @6927; transition type 0, forced).
    /// While matchmaking (bBotMatchActive OR bIsMatchmaking @13250; no bot match in the port) the same dialog's OK is
    /// TrainGameMode Back To Matchmaking @13490 and nothing else: the pre-match screen, never the main menu.
    /// Not in the port: the analytics, the party and custom lobby calls and TrainGameMode's Destroy All Players /
    /// CleanupBeforeSwitchMaps (the match's end does that), and the step 1.0 s after the transition's start (the
    /// Delay @7194 resumes at @1502): every UI_LAB_Panel widget RemoveFromParent, ActivityEnd("training"), the global
    /// UI's BPI_ClearReferences(true) and CollectGarbage. Nothing of it shows: the type 0 fade is opaque after 0.3 s
    /// and the level is opened then.
    pub fn quit(&mut self, cx: &mut Cx<'_>) {
        if !self.can_exit(cx) {
            return;
        }
        let id = Manager::show_dialog(
            cx,
            DialogParams {
                title: "Exit Lab?".into(),
                body: "Are you sure you want to exit the Lab?".into(),
                button_left_label: "OK".into(),
                button_right_label: "Cancel".into(),
                ..DialogParams::default()
            },
        );
        let me = cx.w;
        if let Some(d) = cx.view.dialog(id) {
            d.button_left_clicked().add(Handler { owner: me, id: LP_QUIT_CONFIRM });
        }
    }

    /// buttonResume OnClick @10347 / Resume.
    fn resume(&mut self, cx: &mut Cx<'_>) {
        if self.can_exit(cx)
            && let Some(s) = cx.lab.as_mut()
        {
            s.pause(false);
        }
    }

    /// Writes this panel's toggles to the session, or the toggles' flags to the config (@9556-9889: either toggle
    /// writes both flags and sends the config).
    /// `changed`: the switch whose change this is (its controller out of the view) and its new state.
    fn send_cooldown_flags(&mut self, cx: &mut Cx<'_>, changed: (u32, bool)) {
        let read =
            |cx: &mut Cx<'_>, id: u32, name: &str| if changed.0 == id { changed.1 } else { Self::toggle_on(cx, name) };
        let (cd, sa) = (read(cx, LP_COOLDOWNS, "toggleNoCooldowns"), read(cx, LP_SAME_ATTACK, "toggleSameAttack"));
        let Some(s) = cx.lab.as_mut() else { return };
        let mut c = s.config().clone();
        c.instant_cooldowns = cd;
        c.same_attack_penalty = sa;
        s.request_config_update(c);
    }
}

impl LabMapInfo {
    fn interactor_of(cx: &Cx<'_>, w: WidgetId) -> WidgetId {
        cx.view.tree().find_in(w, "UI_Interactor")
    }
}

impl Controller for LabPanel {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        // @7388-8288: no online Lab (IsOnlineLabEnabled) in the port; standalone shows the button's slot
        set_vis(cx, "buttonGoOnline", Vis::Collapsed);
        set_vis(cx, "socialTipContainer", Vis::Collapsed);
        // Set Try in Lab Visibility: every fighter of the port is owned
        set_vis(cx, "buttonOptionalBuy", Vis::Collapsed);
        {
            let m = &mut cx.view.manager;
            m.on_cancel.add(Handler { owner: me, id: LP_CANCEL });
            m.on_submit3.add(Handler { owner: me, id: LP_CANCEL });
        }
        on_click_of::<ButtonMain>(cx, "buttonResume", |b| &mut b.on_click, LP_RESUME);
        // Btn_MovesList OnClick @15747: TrainingGameState.ToggleMovelist
        on_click_of::<ButtonSecondary>(cx, "Btn_MovesList", |b| &mut b.on_click, LP_MOVES);
        // (the port's own) Experimental Settings, a button of Btn_MovesList's kind under it
        let moves = find(cx, "Btn_MovesList");
        let (user, props, parent, at) = {
            let t = cx.view.tree();
            let parent = t.widget(moves).parent;
            let at = parent.and_then(|p| t.widget(p).children.iter().position(|c| *c == moves));
            (t.user_class(moves).map(str::to_string), t.props(moves).clone(), parent, at)
        };
        if let (Some(user), Some(parent)) = (user, parent) {
            let b = MenuView::create(cx, &user);
            {
                let (pad, ha, va, fill) = {
                    let m = cx.view.tree().widget(moves);
                    (m.pad, m.h_align, m.v_align, m.fill)
                };
                let t = cx.view.tree_mut();
                if let Some(o) = props.as_object() {
                    for (k, v) in o {
                        t.set_prop(b, k, v.clone());
                    }
                }
                // (Move List's icon is a list's: none here)
                t.set_prop(b, "icon", serde_json::Value::Null);
                let w = t.widget_mut(b);
                w.name = "Btn_Experimental".into();
                w.pad = pad;
                w.h_align = ha;
                w.v_align = va;
                w.fill = fill;
            }
            MenuView::add_child(cx, parent, b, at.map(|a| a + 1));
            cx.visit::<ButtonSecondary, _>(b, |x, cx| {
                x.init(cx, custom_text::EXPERIMENTAL_BUTTON);
                x.on_click.add(Handler { owner: me, id: LP_EXPERIMENTAL });
            });
        }
        // PlayerRemovedEvent -> OnPlayerRemoved @10040; EditPerksEvent -> OnEditPerks (@5347 / @5429)
        for p in Self::panels(cx) {
            cx.visit::<LabPlayerPanel, _>(p, |p, _| {
                p.player_removed.add(Handler { owner: me, id: LP_REMOVED });
                p.edit_perks_event.add(Handler { owner: me, id: LP_EDIT_PERKS });
            });
        }
        // buttonPerksDismisser OnClick @10720: the page's Ready(false)
        let d = find(cx, "buttonPerksDismisser");
        bind(cx, d, |s| &mut s.on_click, LP_DISMISSER);
        // the page's PerksSavedEvent and PerksEquipperExitedEvent (-> @15638: Delay(0.1), PerksContainer Hidden)
        let eq = find(cx, "UI_Perks_Equipper");
        cx.visit::<PerksEquipper, _>(eq, |e, _| {
            e.perks_saved_event.add(Handler { owner: me, id: LP_PERKS_SAVED });
            e.exited_event.add(Handler { owner: me, id: LP_PERKS_EXITED });
        });
        // (the port's own) The Lab Tools switch, last under the Settings in the layout of toggleHazardsEnabled; its
        // label is typed. The panel's state, not the config's: it is the local player's alone.
        let hazards = find(cx, "toggleHazardsEnabled");
        let tw = MenuView::create(cx, "UI_ToggleSwitchRework");
        {
            let props = cx.view.tree().props(hazards).clone();
            let t = cx.view.tree_mut();
            t.widget_mut(tw).name = "toggleLabTools".into();
            if let Some(o) = props.as_object() {
                for (k, v) in o {
                    t.set_prop(tw, k, v.clone());
                }
            }
            t.set_prop(tw, "LabelText", json!({ "CultureInvariantString": "Lab Tools" }));
            t.set_prop(tw, "Tooltip", serde_json::Value::Null);
            let h = t.widget(hazards);
            let (pad, ha, va, fill, parent) = (h.pad, h.h_align, h.v_align, h.fill, h.parent);
            let w = t.widget_mut(tw);
            w.pad = pad;
            w.h_align = ha;
            w.v_align = va;
            w.fill = fill;
            if let Some(p) = parent {
                MenuView::add_child(cx, p, tw, None);
            }
        }
        let on = cx.prefs.lab.tools_on;
        cx.visit::<ToggleSwitch, _>(tw, |t, cx| {
            t.set_on(cx, on);
            t.on_toggle_changed.add(Handler { owner: me, id: LP_TOOLS });
        });
        // (the port's own) the other settings but Hazards Enabled are on the Lab Tools panel's Settings page:
        // collapsed here
        for n in ["toggleHitstunCombo", "toggleJuggleCombo", "toggleNoCooldowns", "toggleSameAttack"] {
            set_vis(cx, n, Vis::Collapsed);
        }
        let mi = Self::map_info(cx);
        let count = Self::player_count(cx);
        cx.visit::<LabMapInfo, _>(mi, |m, _| {
            m.set_player_count(count); // @8821
            // UI_LAB_MapInfo SelectedEvent @11676: the player state's config takes the map and is sent
            m.selected_event.add(Handler { owner: me, id: LP_MAP });
        });
        let Some(s) = cx.lab.as_mut() else { return };
        // @8985-9350
        let tg = *s.toggles();
        Self::set_toggle(cx, "toggleHitstunCombo", tg.hitstun_combo);
        Self::set_toggle(cx, "toggleJuggleCombo", tg.juggle_combo);
        Self::set_toggle(cx, "toggleHazardsEnabled", tg.hazards);
        for (n, id) in [
            ("toggleHitstunCombo", LP_HITSTUN),
            ("toggleJuggleCombo", LP_JUGGLE),
            ("toggleHazardsEnabled", LP_HAZARDS),
            ("toggleNoCooldowns", LP_COOLDOWNS),
            ("toggleSameAttack", LP_SAME_ATTACK),
        ] {
            let t = find(cx, n);
            cx.visit::<ToggleSwitch, _>(t, |t, _| t.on_toggle_changed.add(Handler { owner: me, id }));
        }
    }

    /// Destruct: the manager's cancel / submit3 end with the widget (the build's bindings do).
    fn destruct(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        let m = &mut cx.view.manager;
        m.on_cancel.remove_owner(me);
        m.on_submit3.remove_owner(me);
    }

    fn tick(&mut self, cx: &mut Cx<'_>, _dt: f64) {
        self.poll_config(cx);
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        match id {
            // OnUICancel @10590, OnUISubmit3 @15633
            LP_CANCEL => {
                if !Manager::is_dialog_active(cx.view) {
                    self.resume(cx);
                }
            }
            LP_RESUME => self.resume(cx),
            LP_MOVES => toggle_movelist(cx),
            LP_EXPERIMENTAL => self.show_experimental(cx),
            LP_MUTATORS => {
                if let Some(&(_, OptionValue::Set(m))) = arg.downcast_ref::<OptionRequest>()
                    && let Some(s) = cx.lab.as_mut()
                {
                    let mut c = s.config().clone();
                    c.mutators = m;
                    s.request_config_update(c);
                }
            }
            LP_REMOVED => {
                if let Some((panel, account)) = arg.downcast_ref::<(WidgetId, String)>() {
                    if let Some(s) = cx.lab.as_mut() {
                        s.request_kick_player(account);
                    }
                    run_on::<LabPlayerPanel>(cx, *panel, |p, cx| p.set_occupied(cx, false));
                }
            }
            LP_EDIT_PERKS => {
                if let Some(ev) = arg.downcast_ref::<EditPerks>() {
                    self.on_edit_perks(cx, ev);
                }
            }
            LP_PERKS_SAVED => {
                if let Some(saved) = arg.downcast_ref::<PerksSaved>() {
                    self.on_perks_saved(cx, saved);
                }
            }
            LP_PERKS_EXITED => self.delay_hide_perks(cx),
            LP_DISMISSER => {
                if self.equipper_built {
                    let eq = find(cx, "UI_Perks_Equipper");
                    run_on::<PerksEquipper>(cx, eq, |e, cx| e.ready(cx, false));
                }
            }
            LP_TOOLS => {
                let Some(&on) = arg.downcast_ref::<bool>() else { return };
                cx.prefs.lab.tools_on = on;
            }
            LP_MAP => {
                if let (Some(m), Some(s)) = (arg.downcast_ref::<MapInfo>(), cx.lab.as_mut()) {
                    let mut c = s.config().clone();
                    c.selected_map = m.name.clone();
                    s.request_config_update(c);
                }
            }
            LP_HITSTUN | LP_JUGGLE | LP_HAZARDS => {
                let Some(&on) = arg.downcast_ref::<bool>() else { return };
                if let Some(s) = cx.lab.as_mut() {
                    let t = s.toggles();
                    match id {
                        LP_HITSTUN => t.hitstun_combo = on,
                        LP_JUGGLE => t.juggle_combo = on,
                        _ => t.hazards = on,
                    }
                }
            }
            LP_COOLDOWNS | LP_SAME_ATTACK => {
                if let Some(&on) = arg.downcast_ref::<bool>() {
                    self.send_cooldown_flags(cx, (id, on));
                }
            }
            // OnQuitConfirm @12690
            LP_QUIT_CONFIRM => {
                self.quitting = true;
                if cx.lab.as_ref().is_some_and(|s| s.is_matchmaking()) {
                    if let Some(s) = cx.lab.as_mut() {
                        s.back_to_matchmaking();
                    }
                    return;
                }
                Manager::transition(
                    cx,
                    TransitionParams {
                        next_level: "New_MainMenu".into(),
                        kind: 0,
                        delay: 0.0,
                        force: true,
                        instant: false,
                        level_options: "Lobby".into(),
                    },
                );
            }
            _ => {}
        }
    }

    fn resume(&mut self, cx: &mut Cx<'_>, id: u32) {
        // PerksContainer Hidden after the page's close, its pictures let go
        if id == LP_HIDE_PERKS {
            set_vis(cx, "PerksContainer", Vis::Hidden);
            cx.view.release_pictures(PICTURES_LAB_PERKS);
        }
    }
}

// ---------- UI_LAB_PauseMenu ----------

const PM_DELAYED_INIT: u32 = 1;
/// The sidebar's ExitEvent (BndEvt WBP_GlobalNav ExitEvent @2669).
const PM_NAV_EXIT: u32 = 2;

#[derive(Debug, Default)]
pub struct LabPauseMenu;

impl LabPauseMenu {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(LabPauseMenu)
    }

    fn panel(cx: &Cx<'_>) -> WidgetId {
        find(cx, "UI_LAB_Panel")
    }

    /// The panel's Quit, for the sidebar's ExitEvent (BndEvt WBP_GlobalNav ExitEvent @2669).
    pub fn quit(&mut self, cx: &mut Cx<'_>) {
        let p = Self::panel(cx);
        run_on::<LabPanel>(cx, p, |p, cx| p.quit(cx));
    }

    /// Handle Visibility Changed @902, shown (@1575): the config is asked for. Not ported: the manager timer of 0.5 s
    /// (@2296, Refresh Social Panel Timer; OnTimerEvent @2616 -> @15), which fills the social panel's lists the first
    /// time only (UI_SocialPanel PopulateAllLists(false); the port's lists follow the server's events).
    pub fn shown(&mut self, cx: &mut Cx<'_>) {
        let p = Self::panel(cx);
        cx.visit::<LabPanel, _>(p, |p, cx| p.shown(cx));
        // @2030: by bIsMatchmaking (@1904 / @1967): the back arrow, "Return to Matchmaking" (@1670), else the quit
        // icon, "Return Home" (@1749)
        let (style, tip) = Self::exit_button(cx);
        let nav = find(cx, "WBP_GlobalNav");
        cx.visit::<GlobalNav, _>(nav, |n, cx| n.customize_exit_button(cx, style, tip));
        // @2218 the game instance's ClearAllUserFocus (native; read as Slate's: no widget keeps the keyboard, which
        // here is a text box's, committed as cleared)
        MenuView::clear_keyboard_focus(cx, 3, true);
    }

    /// The exit button's kind and label the sidebar shows (@2030): 1 and "Return to Matchmaking" while the queue
    /// searches, else 0 and "Return Home".
    pub fn exit_button(cx: &Cx<'_>) -> (i32, &'static str) {
        if cx.lab.as_ref().is_some_and(|s| s.is_matchmaking()) {
            (1, "Return to Matchmaking")
        } else {
            (0, "Return Home")
        }
    }

    /// TrainGameMode's match-found chain, its UI (TGM@2254-2817): PF_Post_Global_Event (ui_map_matchfound), then
    /// UIManager.ShowNotification "Match Found!" / "Returning to Character Select", 4 s, not dismissable, no audio of
    /// its own, Timing 0 (Immediate). (Here because the Lab's menu view is the manager that shows it; the chain
    /// itself is the Lab match's.)
    pub fn match_found(&mut self, cx: &mut Cx<'_>) {
        cx.post("ui_map_matchfound");
        Manager::show_notification(
            cx,
            NotificationParams {
                title: "Match Found!".into(),
                body: "Returning to Character Select".into(),
                screen_time: 4.0,
                allow_dismissal: false,
                audio: None,
                ..NotificationParams::default()
            },
        );
    }

    /// Hidden / collapsed (@1128-1534); @1534 the social panel back to its small mode (SetMode(0, false, false)).
    pub fn hidden(&mut self, cx: &mut Cx<'_>) {
        close_all_tooltips(cx);
        Manager::dismiss_all_modals(cx);
        Manager::dismiss_all_context_menus(cx);
        Manager::dismiss_all_dialogs(cx);
        let social = find(cx, "UI_SocialPanel");
        cx.visit::<SocialPanel, _>(social, |p, cx| p.set_mode(cx, 0, false, false));
    }

    /// UpdateContext @124 (GetInteractors @242 is empty: BatchSetContext sets none).
    pub fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str) {
        cx.view.manager.set_root_context_for_level(ctx);
        let nav = find(cx, "WBP_GlobalNav");
        cx.visit::<GlobalNav, _>(nav, |n, cx| n.update_context_global_nav(cx, ctx, true, true));
        // @367
        let social = find(cx, "UI_SocialPanel");
        cx.visit::<SocialPanel, _>(social, |p, cx| p.update_context(cx, ctx));
        let p = Self::panel(cx);
        let c = ctx.to_string();
        run_on::<LabPanel>(cx, p, move |p, cx| p.update_context(cx, &c));
    }
}

impl Controller for LabPauseMenu {
    /// DelayedInit @473 (-> @726-865), once, when the widget is made: the menu takes a new context and Resume
    /// Training the focus (in cursor mode the context alone, UI_Interactor SetFocused). No later show sets either:
    /// the menu opens with the focus where it was left. (The class's Init Delay Timer is never added.)
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let ctx = cx.view.manager.new_guid();
        self.update_context(cx, &ctx);
        let p = Self::panel(cx);
        run_on::<LabPanel>(cx, p, |p, cx| p.set_focused(cx));
        // @598: Conditional Visibility(UI_SocialPanel, IsConnected?, 4, 1): the port is not connected
        set_vis(cx, "UI_SocialPanel", Vis::Collapsed);
        let _ = PM_DELAYED_INIT;
        // BndEvt WBP_GlobalNav ExitEvent @2669
        let me = cx.w;
        let nav = find(cx, "WBP_GlobalNav");
        cx.visit::<GlobalNav, _>(nav, |n, _| {
            n.exit_bound = true;
            n.exit_event.add(Handler { owner: me, id: PM_NAV_EXIT });
        });
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, _arg: &dyn Any) {
        if id == PM_NAV_EXIT {
            self.quit(cx);
        }
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
}
